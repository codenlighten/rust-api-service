# IP records

| Date | Covers | Type | Record hash | Signer | Tag |
|---|---|---|---|---|---|
| 2026-11-26 | Commit `6d63969`: a Rust (axum, SQLite) REST API, source, tests and documentation. No authorship claimed in third-party libraries, published standards and the generated lockfile. | software, hash-only | [`cd880646ea6e683cac30c870a4451347a9e68da6365da6c8def4fa8f7b86cb77`](https://iptrust.org/v/cd880646ea6e683cac30c870a4451347a9e68da6365da6c8def4fa8f7b86cb77) | Gregory J. Ward | `ip-2026-11-26` |

A record is evidence that the committed content existed by the time of its anchor and that the signer made
the statement; it does not create or register any right. Anchoring completes some hours after submission.
Files under `third_party/`, where present, are not part of the record.

## Verifying

Verification needs the evidence package for the record, which the author holds and which is not committed
here. With it in `.iptrust/`:

```
git worktree add /tmp/at-tag-rust-api-service ip-2026-11-26
cp -r .iptrust /tmp/at-tag-rust-api-service/
(cd /tmp/at-tag-rust-api-service && iptrust2 verify cd880646ea6e683cac30c870a4451347a9e68da6365da6c8def4fa8f7b86cb77 --fetch-headers)
git worktree remove --force /tmp/at-tag-rust-api-service
```

Expect `verdict: valid` and `content: matched` once both chains have anchored.
