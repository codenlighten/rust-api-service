# rust-api-service

A notes REST API in Rust (axum, tokio, sqlx/SQLite) with argon2 password hashing, signed bearer tokens, per-user data isolation, RFC 9457 problem+json errors and an OpenAPI document. It shows Rust back-end engineering: typed errors, boundary validation, in-process integration tests, graceful shutdown and a reproducible load test.

## Run it

Needs a Rust toolchain (edition 2021, rustc 1.80 or newer).

```bash
export TOKEN_SECRET=$(openssl rand -hex 32)
cargo run --release
curl -s localhost:3000/healthz        # {"status":"ok"}; API description at /openapi.json
```

Settings are environment variables; see `.env.example` (the service reads the process environment and does not parse `.env`). `cargo test` runs everything without a server.

## What this proves

- **Auth done carefully.** Argon2id hashes; HMAC-SHA256 signed tokens with expiry, verified in constant time; unknown-email and wrong-password logins return the same 401 and cost one hash each.
- **Isolation by construction.** Every note query carries the user id from the verified token. Someone else's note is a 404, indistinguishable from a missing one.
- **One error type.** `ApiError` maps to statuses in one place and renders `application/problem+json`; extractor rejections (bad JSON, bad query, bad path) use it too. Internal errors are logged, never echoed.
- **Boundaries validated.** Email, password, title and body limits, 64 KiB body cap, unknown fields refused, keyset pagination with bounded `limit`.
- **Operable.** Config validated at startup with messages that say what to set; `tracing` logs with a per-request id (optional JSON); SIGTERM and Ctrl-C drain in-flight requests before the pool closes; `/healthz` runs a database query.

## Tests

`cargo test` runs unit tests (config, tokens, hashing, validation) and 20 integration tests that drive the real router in-process with `tower::ServiceExt` over an in-memory SQLite database: auth failures (missing, garbage, wrong scheme, expired, wrong-secret tokens), isolation between two users (read, update, delete), validation, pagination (full walk, exact page boundary, bad parameters), status mapping, and an OpenAPI check that the document and the route table agree.

### Deliberate breaks

Each change was made on its own, the full suite run (`cargo test --no-fail-fast`), and the code restored (37 of 37 pass afterwards). Every break failed the tests named here.

| Break | Tests that failed |
|---|---|
| Drop `AND user_id = ?` from `fetch` | `one_user_cannot_read_change_or_delete_anothers_notes`, `a_missing_note_and_someone_elses_note_look_identical` |
| Accept expired tokens (`exp` check removed in `token::verify`) | `expired_token_is_refused`, `expiry_is_enforced_at_the_boundary` |
| Skip password verification (`Hasher::verify` always true) | `login_with_wrong_password_or_unknown_email_is_the_same_401`, and two `password` unit tests |
| Map `Conflict` to 400 | `duplicate_email_is_a_conflict_regardless_of_case` |
| Make the pagination filter `id >=` | `keyset_pagination_walks_every_note_once_in_order` |

The checks that run on every change: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (17 unit and 20 integration tests) and `cargo build --release`. They passed on a clean build on the host below. The GitHub Actions workflow runs the same commands.

## Load test

`scripts/loadtest.sh [runs] [concurrency] [seconds] [write_percent]` builds in release mode, starts the server on a fresh SQLite file, runs the closed-loop generator (`src/bin/loadgen.rs`, over keep-alive connections, after a 2 s warm-up) several times and prints one line per run with requests/s and p50/p99 latency.

Measured on 2026-11-25 (13:42 to 13:46 by the host clock):

- **Host:** AMD Ryzen 7 260, 16 threads, frequency governor `performance`. The server and the generator shared the same 8 logical CPUs (`taskset -c 0-7`, `nice 19`) with other light jobs on the machine (load average about 1.5 before the run, 8 during it).
- **Software:** rustc 1.98.1, release build, `sqlx` 0.8 with SQLite, 32 concurrent connections, 20 s per run, 5 runs per mix, `errors=0` in all 10 runs.

| Mix | Requests/s (5 runs) | p50 | p99 | Slowest single request |
|---|---|---|---|---|
| Reads only | 24,105 to 24,173 (median 24,136) | 1.32 ms | 1.56 to 1.57 ms | 4.5 to 5.1 ms |
| 20% writes | 25,379 to 25,741 (median 25,525) | 1.13 to 1.14 ms | 4.15 to 4.18 ms | 105 to 430 ms |

What these numbers are not: they come from one host, one run of five, with the generator competing for the same cores as the server, over loopback with no network and a tiny dataset (a few hundred notes), so they say nothing about behaviour on other hardware or under real traffic. The read-only mix is not faster than the mixed one; I did not profile why. The occasional slow requests in the mixed runs (up to 430 ms) are consistent with SQLite write stalls, but I did not confirm that.

## Limits

- SQLite with a small pool: one writer at a time, one process, one disk. Not a multi-node design.
- Tokens cannot be revoked and there are no refresh tokens; a token stays valid until it expires. A token for a deleted account would hit a foreign-key error (account deletion is not implemented).
- No rate limiting or login throttling, no email verification, no password reset.
- The OpenAPI document is hand-written (checked against the routes, not generated from types), so response schemas can drift unnoticed.
- Closed-loop load generator: latency percentiles understate what an open-loop arrival pattern would show.
- Timestamps are unix seconds from the system clock.

## Dependencies

axum, tokio, tower-http, sqlx (SQLite), argon2, hmac and sha2 (RustCrypto), serde, tracing, reqwest (load generator only). All are public crates; none of their code is copied here.

## Licence

MIT. See `LICENSE`.
