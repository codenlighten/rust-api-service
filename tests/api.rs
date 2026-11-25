//! Integration tests: the real router, a real (in-memory) SQLite database, no network.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, HeaderMap, Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use notes_api::app::{now_secs, router, AppState};
use notes_api::password::Hasher;
use notes_api::{db, openapi, token};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tower::ServiceExt;

const SECRET: &[u8] = b"integration-test-secret-0123456789";

struct TestApp {
    router: Router,
    pool: SqlitePool,
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}

impl TestApp {
    async fn new() -> Self {
        let pool = db::connect("sqlite::memory:", 1)
            .await
            .expect("in-memory database");
        let state = AppState {
            pool: pool.clone(),
            hasher: Arc::new(Hasher::cheap_for_tests()),
            token_secret: SECRET.into(),
            token_ttl_secs: 3600,
        };
        Self {
            router: router(state),
            pool,
        }
    }

    async fn raw(
        &self,
        method: Method,
        uri: &str,
        bearer: Option<&str>,
        body: Body,
        ctype: Option<&str>,
    ) -> Reply {
        let mut req = Request::builder().method(method).uri(uri);
        if let Some(t) = bearer {
            req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        if let Some(c) = ctype {
            req = req.header(header::CONTENT_TYPE, c);
        }
        let resp = self
            .router
            .clone()
            .oneshot(req.body(body).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Reply {
            status,
            headers,
            body,
        }
    }

    async fn send(
        &self,
        method: Method,
        uri: &str,
        bearer: Option<&str>,
        json: Option<Value>,
    ) -> Reply {
        match json {
            Some(v) => {
                self.raw(
                    method,
                    uri,
                    bearer,
                    Body::from(v.to_string()),
                    Some("application/json"),
                )
                .await
            }
            None => self.raw(method, uri, bearer, Body::empty(), None).await,
        }
    }

    async fn register(&self, email: &str, password: &str) -> Reply {
        self.send(
            Method::POST,
            "/auth/register",
            None,
            Some(json!({"email": email, "password": password})),
        )
        .await
    }

    /// Registers and logs in, returning the bearer token.
    async fn user(&self, email: &str) -> String {
        assert_eq!(
            self.register(email, "a-good-password").await.status,
            StatusCode::CREATED
        );
        let r = self
            .send(
                Method::POST,
                "/auth/login",
                None,
                Some(json!({"email": email, "password": "a-good-password"})),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK);
        r.body["access_token"].as_str().unwrap().to_string()
    }

    async fn make_note(&self, token: &str, title: &str) -> i64 {
        let r = self
            .send(
                Method::POST,
                "/notes",
                Some(token),
                Some(json!({"title": title, "body": "b"})),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED);
        r.body["id"].as_i64().unwrap()
    }
}

fn assert_problem(r: &Reply, status: StatusCode) {
    assert_eq!(r.status, status, "body: {}", r.body);
    assert_eq!(r.headers[header::CONTENT_TYPE], "application/problem+json");
    assert_eq!(r.body["status"], status.as_u16());
    assert!(r.body["detail"].as_str().is_some_and(|d| !d.is_empty()));
}

// ---------- health, routing, request ids ----------

#[tokio::test]
async fn health_is_ok_and_carries_a_request_id() {
    let app = TestApp::new().await;
    let r = app.send(Method::GET, "/healthz", None, None).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["status"], "ok");
    assert!(r.headers.contains_key("x-request-id"));
}

#[tokio::test]
async fn health_reports_503_when_the_database_is_gone() {
    let app = TestApp::new().await;
    app.pool.close().await;
    let r = app.send(Method::GET, "/healthz", None, None).await;
    assert_problem(&r, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn unknown_route_and_wrong_method_are_problem_json() {
    let app = TestApp::new().await;
    assert_problem(
        &app.send(Method::GET, "/nope", None, None).await,
        StatusCode::NOT_FOUND,
    );
    assert_problem(
        &app.send(Method::PATCH, "/healthz", None, None).await,
        StatusCode::METHOD_NOT_ALLOWED,
    );
}

// ---------- registration and login ----------

#[tokio::test]
async fn register_creates_an_account_and_stores_only_an_argon2_hash() {
    let app = TestApp::new().await;
    let r = app.register("  Ann@Example.com ", "correct horse").await;
    assert_eq!(r.status, StatusCode::CREATED);
    assert_eq!(r.body["email"], "ann@example.com");
    assert!(r.body.get("password").is_none() && r.body.get("password_hash").is_none());

    let stored: String =
        sqlx::query_scalar("SELECT password_hash FROM users WHERE email = 'ann@example.com'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert!(stored.starts_with("$argon2id$"));
    assert!(!stored.contains("correct horse"));
}

#[tokio::test]
async fn duplicate_email_is_a_conflict_regardless_of_case() {
    let app = TestApp::new().await;
    assert_eq!(
        app.register("a@example.com", "password-1").await.status,
        StatusCode::CREATED
    );
    let r = app.register("A@EXAMPLE.COM", "password-2").await;
    assert_problem(&r, StatusCode::CONFLICT);
}

#[tokio::test]
async fn register_validation_reports_every_bad_field() {
    let app = TestApp::new().await;
    let r = app.register("not-an-email", "short").await;
    assert_problem(&r, StatusCode::UNPROCESSABLE_ENTITY);
    let fields: Vec<&str> = r.body["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["field"].as_str().unwrap())
        .collect();
    assert_eq!(fields, ["email", "password"]);
}

#[tokio::test]
async fn malformed_bodies_get_precise_statuses() {
    let app = TestApp::new().await;
    let bad_json = app
        .raw(
            Method::POST,
            "/auth/login",
            None,
            Body::from("{nope"),
            Some("application/json"),
        )
        .await;
    assert_problem(&bad_json, StatusCode::BAD_REQUEST);
    let wrong_type = app
        .raw(
            Method::POST,
            "/auth/login",
            None,
            Body::from("a=b"),
            Some("text/plain"),
        )
        .await;
    assert_problem(&wrong_type, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let unknown_field = app
        .send(
            Method::POST,
            "/auth/login",
            None,
            Some(json!({"email": "a@b.co", "password": "x", "admin": true})),
        )
        .await;
    assert_problem(&unknown_field, StatusCode::UNPROCESSABLE_ENTITY);
    let huge = app
        .raw(
            Method::POST,
            "/auth/login",
            None,
            Body::from(format!(r#"{{"email":"{}"}}"#, "a".repeat(70_000))),
            Some("application/json"),
        )
        .await;
    assert_problem(&huge, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn login_succeeds_with_the_right_password_and_issues_a_verifiable_token() {
    let app = TestApp::new().await;
    let tok = app.user("ok@example.com").await;
    let claims = token::verify(SECRET, &tok, now_secs()).unwrap();
    let id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE email = 'ok@example.com'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(claims.sub, id);
}

#[tokio::test]
async fn login_with_wrong_password_or_unknown_email_is_the_same_401() {
    let app = TestApp::new().await;
    app.user("real@example.com").await;
    let wrong = app
        .send(
            Method::POST,
            "/auth/login",
            None,
            Some(json!({"email": "real@example.com", "password": "not-the-password"})),
        )
        .await;
    let unknown = app
        .send(
            Method::POST,
            "/auth/login",
            None,
            Some(json!({"email": "ghost@example.com", "password": "a-good-password"})),
        )
        .await;
    assert_problem(&wrong, StatusCode::UNAUTHORIZED);
    assert_problem(&unknown, StatusCode::UNAUTHORIZED);
    assert_eq!(
        wrong.body["detail"], unknown.body["detail"],
        "must not reveal which emails exist"
    );
    assert!(wrong.body.get("access_token").is_none());
}

// ---------- authentication on protected routes ----------

#[tokio::test]
async fn protected_routes_refuse_missing_or_bad_credentials() {
    let app = TestApp::new().await;
    let r = app.send(Method::GET, "/notes", None, None).await;
    assert_problem(&r, StatusCode::UNAUTHORIZED);
    assert_eq!(r.headers[header::WWW_AUTHENTICATE], "Bearer");

    assert_problem(
        &app.send(Method::GET, "/notes", Some("garbage"), None).await,
        StatusCode::UNAUTHORIZED,
    );

    let basic = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/notes")
                .header(header::AUTHORIZATION, "Basic abc")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(basic.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn expired_token_is_refused() {
    let app = TestApp::new().await;
    app.user("exp@example.com").await;
    let expired = token::sign(
        SECRET,
        &token::Claims {
            sub: 1,
            exp: now_secs() - 10,
        },
    );
    let r = app.send(Method::GET, "/notes", Some(&expired), None).await;
    assert_problem(&r, StatusCode::UNAUTHORIZED);
    assert!(r.body["detail"].as_str().unwrap().contains("expired"));
}

#[tokio::test]
async fn token_signed_with_another_secret_is_refused() {
    let app = TestApp::new().await;
    app.user("forge@example.com").await;
    let forged = token::sign(
        b"a-different-secret-of-32-bytes-plus",
        &token::Claims {
            sub: 1,
            exp: now_secs() + 3600,
        },
    );
    assert_problem(
        &app.send(Method::GET, "/notes", Some(&forged), None).await,
        StatusCode::UNAUTHORIZED,
    );
}

// ---------- CRUD, validation ----------

#[tokio::test]
async fn note_lifecycle() {
    let app = TestApp::new().await;
    let t = app.user("life@example.com").await;

    let created = app
        .send(
            Method::POST,
            "/notes",
            Some(&t),
            Some(json!({"title": "  First ", "body": "hello"})),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED);
    let id = created.body["id"].as_i64().unwrap();
    assert_eq!(created.headers[header::LOCATION], format!("/notes/{id}"));
    assert_eq!(created.body["title"], "First");

    let got = app
        .send(Method::GET, &format!("/notes/{id}"), Some(&t), None)
        .await;
    assert_eq!(got.body["body"], "hello");

    let put = app
        .send(
            Method::PUT,
            &format!("/notes/{id}"),
            Some(&t),
            Some(json!({"title": "Renamed", "body": "new"})),
        )
        .await;
    assert_eq!(put.status, StatusCode::OK);
    assert_eq!(
        (put.body["title"].as_str(), put.body["body"].as_str()),
        (Some("Renamed"), Some("new"))
    );

    assert_eq!(
        app.send(Method::DELETE, &format!("/notes/{id}"), Some(&t), None)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_problem(
        &app.send(Method::GET, &format!("/notes/{id}"), Some(&t), None)
            .await,
        StatusCode::NOT_FOUND,
    );
    assert_problem(
        &app.send(Method::DELETE, &format!("/notes/{id}"), Some(&t), None)
            .await,
        StatusCode::NOT_FOUND,
    );
}

#[tokio::test]
async fn note_validation_rejects_bad_input() {
    let app = TestApp::new().await;
    let t = app.user("val@example.com").await;
    let empty = app
        .send(
            Method::POST,
            "/notes",
            Some(&t),
            Some(json!({"title": "   "})),
        )
        .await;
    assert_problem(&empty, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(empty.body["errors"][0]["field"], "title");
    let long_body = app
        .send(
            Method::POST,
            "/notes",
            Some(&t),
            Some(json!({"title": "t", "body": "x".repeat(10_001)})),
        )
        .await;
    assert_problem(&long_body, StatusCode::UNPROCESSABLE_ENTITY);
    let non_numeric_id = app.send(Method::GET, "/notes/abc", Some(&t), None).await;
    assert_problem(&non_numeric_id, StatusCode::BAD_REQUEST);
    assert_eq!(
        app.send(Method::GET, "/notes", Some(&t), None).await.body["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

// ---------- isolation ----------

#[tokio::test]
async fn one_user_cannot_read_change_or_delete_anothers_notes() {
    let app = TestApp::new().await;
    let a = app.user("a@example.com").await;
    let b = app.user("b@example.com").await;
    let a_note = app.make_note(&a, "A's secret").await;
    let uri = format!("/notes/{a_note}");

    assert_problem(
        &app.send(Method::GET, &uri, Some(&b), None).await,
        StatusCode::NOT_FOUND,
    );
    assert_problem(
        &app.send(
            Method::PUT,
            &uri,
            Some(&b),
            Some(json!({"title": "hijacked"})),
        )
        .await,
        StatusCode::NOT_FOUND,
    );
    assert_problem(
        &app.send(Method::DELETE, &uri, Some(&b), None).await,
        StatusCode::NOT_FOUND,
    );

    // B's list is empty, A's note is untouched.
    assert_eq!(
        app.send(Method::GET, "/notes", Some(&b), None).await.body["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    let still = app.send(Method::GET, &uri, Some(&a), None).await;
    assert_eq!(still.body["title"], "A's secret");
}

#[tokio::test]
async fn a_missing_note_and_someone_elses_note_look_identical() {
    let app = TestApp::new().await;
    let a = app.user("a2@example.com").await;
    let b = app.user("b2@example.com").await;
    let a_note = app.make_note(&a, "x").await;
    let theirs = app
        .send(Method::GET, &format!("/notes/{a_note}"), Some(&b), None)
        .await;
    let missing = app.send(Method::GET, "/notes/999999", Some(&b), None).await;
    assert_eq!(theirs.body, missing.body);
}

// ---------- pagination ----------

#[tokio::test]
async fn keyset_pagination_walks_every_note_once_in_order() {
    let app = TestApp::new().await;
    let t = app.user("page@example.com").await;
    let other = app.user("page-other@example.com").await;
    let mut created = Vec::new();
    for i in 0..5 {
        created.push(app.make_note(&t, &format!("n{i}")).await);
        app.make_note(&other, "interleaved noise").await;
    }

    let mut seen = Vec::new();
    let mut after = 0;
    let mut pages = 0;
    loop {
        let r = app
            .send(
                Method::GET,
                &format!("/notes?limit=2&after={after}"),
                Some(&t),
                None,
            )
            .await;
        assert_eq!(r.status, StatusCode::OK);
        pages += 1;
        seen.extend(
            r.body["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|n| n["id"].as_i64().unwrap()),
        );
        match r.body["next_after"].as_i64() {
            Some(next) => after = next,
            None => break,
        }
    }
    assert_eq!(seen, created);
    assert_eq!(pages, 3, "5 notes at 2 per page");
}

#[tokio::test]
async fn exact_page_boundary_has_no_phantom_next_page() {
    let app = TestApp::new().await;
    let t = app.user("edge@example.com").await;
    for i in 0..2 {
        app.make_note(&t, &format!("n{i}")).await;
    }
    let r = app
        .send(Method::GET, "/notes?limit=2", Some(&t), None)
        .await;
    assert_eq!(r.body["items"].as_array().unwrap().len(), 2);
    assert!(r.body["next_after"].is_null());
}

#[tokio::test]
async fn bad_pagination_parameters_are_rejected() {
    let app = TestApp::new().await;
    let t = app.user("pp@example.com").await;
    for q in ["limit=0", "limit=101", "after=-1"] {
        assert_problem(
            &app.send(Method::GET, &format!("/notes?{q}"), Some(&t), None)
                .await,
            StatusCode::UNPROCESSABLE_ENTITY,
        );
    }
    assert_problem(
        &app.send(Method::GET, "/notes?limit=abc", Some(&t), None)
            .await,
        StatusCode::BAD_REQUEST,
    );
    assert_problem(
        &app.send(Method::GET, "/notes?bogus=1", Some(&t), None)
            .await,
        StatusCode::BAD_REQUEST,
    );
}

// ---------- OpenAPI ----------

#[tokio::test]
async fn openapi_is_served_and_matches_the_router() {
    let app = TestApp::new().await;
    let served = app.send(Method::GET, "/openapi.json", None, None).await;
    assert_eq!(served.status, StatusCode::OK);
    assert_eq!(served.body["openapi"], "3.1.0");

    let doc = openapi::document();
    let mut documented: Vec<String> = Vec::new();
    for (path, ops) in doc["paths"].as_object().unwrap() {
        for method in ops.as_object().unwrap().keys() {
            documented.push(format!("{} {path}", method.to_uppercase()));
            // Documented operations must exist: anything but 404/405 proves the route is wired.
            let uri = path.replace("{id}", "1");
            let m = Method::from_bytes(method.to_uppercase().as_bytes()).unwrap();
            let r = app.send(m, &uri, None, None).await;
            assert!(
                r.status != StatusCode::NOT_FOUND && r.status != StatusCode::METHOD_NOT_ALLOWED,
                "{method} {path} is documented but not routed"
            );
        }
    }
    documented.sort();
    let mut expected = vec![
        "GET /healthz",
        "GET /openapi.json",
        "POST /auth/register",
        "POST /auth/login",
        "GET /notes",
        "POST /notes",
        "GET /notes/{id}",
        "PUT /notes/{id}",
        "DELETE /notes/{id}",
    ];
    expected.sort();
    assert_eq!(
        documented, expected,
        "the OpenAPI document and the route table drifted apart"
    );
}
