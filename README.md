# rust-api-service

[![CI](https://github.com/codenlighten/rust-api-service/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/codenlighten/rust-api-service/actions/workflows/ci.yml)

A notes REST API in Rust (axum, tokio, sqlx/SQLite) with argon2 password hashing, signed bearer tokens, per-user data isolation, RFC 9457 problem+json errors and an OpenAPI document. It shows Rust back-end engineering: typed errors, boundary validation, in-process integration tests, graceful shutdown and a reproducible load test.

## Run it

Needs a Rust toolchain (edition 2021, rustc 1.94 or newer, which `sqlx` 0.9 requires; tested with 1.98.1).

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
- **Operable.** Config validated at startup with messages that say what to set; `tracing` logs with a per-request id (optional JSON); SIGTERM and Ctrl-C drain in-flight requests before the pool closes, with a deadline (`SHUTDOWN_TIMEOUT_SECS`, default 10) so a request that never finishes cannot keep the process alive (it then exits with status 1); `/healthz` reads the tables the service needs and answers 503 if it cannot.

## Tests

`cargo test` runs unit tests (config, tokens, hashing, validation) and 22 integration tests that drive the real router in-process with `tower::ServiceExt` over an in-memory SQLite database: auth failures (missing, garbage, wrong scheme, expired, wrong-secret tokens), isolation between two users (read, update, delete), validation, pagination (full walk, exact page boundary, bad parameters), status mapping, an OpenAPI check that the document and the route table agree, and a version guard that the linked SQLite is at least 3.54.0, the release that fixes RUSTSEC-2026-0459 (the guard does not exercise the flaw).

### Deliberate breaks

Each change was made on its own, the full suite run (`cargo test --no-fail-fast`), and the code restored (37 of 37 pass afterwards). Every break failed the tests named here.

| Break | Tests that failed |
|---|---|
| Drop `AND user_id = ?` from `fetch` | `one_user_cannot_read_change_or_delete_anothers_notes`, `a_missing_note_and_someone_elses_note_look_identical` |
| Accept expired tokens (`exp` check removed in `token::verify`) | `expired_token_is_refused`, `expiry_is_enforced_at_the_boundary` |
| Skip password verification (`Hasher::verify` always true) | `login_with_wrong_password_or_unknown_email_is_the_same_401`, and two `password` unit tests |
| Map `Conflict` to 400 | `duplicate_email_is_a_conflict_regardless_of_case` |
| Make the pagination filter `id >=` | `keyset_pagination_walks_every_note_once_in_order` |

The checks that run on every change: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (17 unit tests, 22 integration tests and 2 tests that start the real binary and signal it), `cargo build --release` and `cargo audit`. They passed on a clean build on the host below. The GitHub Actions workflow runs the same commands; `cargo audit` was added on 2026-11-26.

## Load test

`scripts/loadtest.sh [runs] [concurrency] [seconds] [write_percent]` builds in release mode and, for each run, starts the server on a fresh SQLite file, runs the closed-loop generator (`src/bin/loadgen.rs`, over keep-alive connections, after a 2 s warm-up) and prints one line with requests/s and p50/p99 latency.

Measured on 2026-11-26 (10:41 to 10:46 by the host clock):

- **Host:** AMD Ryzen 7 260, 16 threads, frequency governor `performance`. The server and the generator shared the same 8 logical CPUs (`taskset -c 0-7`, `nice 19`); no other benchmark or build job was booked, only light always-on services (load average 0.4 before the first mix, 9 to 10 at the end of each).
- **Software:** rustc 1.98.1, release build, `sqlx` 0.9.1 with the bundled SQLite 3.54.1, 32 concurrent connections, 20 s per run, 5 runs per mix, `errors=0` in all 10 runs.

| Mix | Requests/s (5 runs) | p50 | p99 | Slowest single request |
|---|---|---|---|---|
| Reads only | 19,331 to 19,408 (median 19,383) | 1.64 to 1.65 ms | 1.92 to 1.93 ms | 5.0 to 8.2 ms |
| 20% writes | 23,514 to 23,814 (median 23,702) | 1.28 to 1.30 ms | 2.34 to 2.39 ms | 80 to 130 ms |

What these numbers are not: they come from one host, one run of five, with the generator competing for the same cores as the server, over loopback with no network and a small dataset (each run starts with 200 notes; a mixed run adds roughly 100,000 more as it goes), so they say nothing about behaviour on other hardware or under real traffic. The read-only mix is not faster than the mixed one; I did not profile why. The occasional slow requests in the mixed runs (up to 130 ms here) are consistent with SQLite write stalls, but I did not confirm that.

### What the dependency upgrade cost

On 2026-11-26 I moved from `sqlx` 0.8.6 with the bundled SQLite 3.46.0 to `sqlx` 0.9.1 with 3.54.1, to pick up the fix for [RUSTSEC-2026-0459](https://rustsec.org/advisories/RUSTSEC-2026-0459.html). It also removed the `rsa` crate (RUSTSEC-2023-0071) from the lockfile; only the MySQL driver depended on it, and this service does not enable that feature. Throughput fell, so I compared builds on the same host, alternating them run by run within minutes (each run on its own fresh database):

| Build | Reads, median req/s | Reads p99 | 20% writes, median req/s | 20% writes p99 |
|---|---|---|---|---|
| Before: `sqlx` 0.8.6, SQLite 3.46.0 (5 runs per mix) | 24,404 | 1.55 to 1.56 ms | 25,607 | 4.15 to 4.17 ms |
| After: `sqlx` 0.9.1, SQLite 3.54.1 (5 runs per mix) | 19,262 | 1.91 to 1.94 ms | 23,752 | 2.33 to 2.36 ms |
| `sqlx` 0.9.1 with the old SQLite 3.46.0 (3 runs, reads) | 27,490 | 1.40 ms | not run | not run |

- Reads are about 21% slower and the mixed load about 7% slower; the slow tail of the mixed load improved.
- For reads, the loss goes with `libsqlite3-sys`: keeping the new `sqlx` and putting only that crate back to 0.30.1 gave more throughput than the old build. That swaps the SQLite version and the crate's build script together, so it does not show which of the two is responsible, and the run did not include the mixed load.
- Restoring `SQLITE_ENABLE_MEMORY_MANAGEMENT`, a build flag the newer binding no longer sets, made no difference (2 runs each). I have not profiled where the time goes.
- The old build reproduced the figures published here the day before to within 2%.

I kept the fix. The flaw needs a process to open a database and log file that a less trusted party could write, and this service opens only the database its operator points it at, so a deployment that cannot afford the slowdown has a real choice to weigh.

## Limits

- Writes are not guaranteed to survive a power failure. SQLite runs in WAL mode with `synchronous = NORMAL`, which keeps the database consistent but can lose the most recently acknowledged transactions if the machine loses power (a crash of the process alone does not). `FULL` would close that window at a cost in write speed that I have not measured.
- SQLite with a small pool: one writer at a time, one process, one disk. Not a multi-node design.
- Tokens cannot be revoked and there are no refresh tokens; a token stays valid until it expires. A token for a deleted account would hit a foreign-key error (account deletion is not implemented).
- No rate limiting or login throttling, no email verification, no password reset.
- The OpenAPI document is hand-written (checked against the routes, not generated from types), so response schemas can drift unnoticed.
- Closed-loop load generator: latency percentiles understate what an open-loop arrival pattern would show.
- Timestamps are unix seconds from the system clock.

## What a code review found

An independent AI review of the source (2026-11-26) found four faults. Three were changed, each behaviour change with a test that failed first:

| Fault | Change |
|---|---|
| One unfinished request could stop the process from ever exiting | A deadline after the signal |
| `/healthz` ran `SELECT 1`, which succeeds with the tables gone | It reads them; any failure is 503 |
| Load-test runs shared one database that kept growing, though the text called it tiny | A fresh database per run, text corrected, figures re-measured |
| Acknowledged writes can be lost on power failure | A deliberate setting, now stated under Limits |

Seven deliberate breaks of the new code were each caught by a test.

## Dependencies

axum, tokio, tower-http, sqlx (SQLite), argon2, hmac and sha2 (RustCrypto), serde, tracing, reqwest (load generator only). All are public crates; none of their code is copied here.

## Licence

MIT. See `LICENSE`.
