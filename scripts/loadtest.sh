#!/usr/bin/env bash
# Reproducible load test: release build, fresh SQLite file, N runs of the closed-loop generator.
# Usage: scripts/loadtest.sh [runs] [concurrency] [duration_secs] [write_percent]
set -euo pipefail
RUNS=${1:-5}; CONC=${2:-32}; DUR=${3:-20}; WRITES=${4:-0}
cd "$(dirname "$0")/.."
cargo build --release --bins
export BIND_ADDR=127.0.0.1:3917
export TOKEN_SECRET=load-test-secret-not-for-real-use-0123456789
export RUST_LOG=warn
SERVER=
WORK=
stop() {
  if [ -n "$SERVER" ]; then kill "$SERVER" 2>/dev/null || true; wait "$SERVER" 2>/dev/null || true; SERVER=; fi
  if [ -n "$WORK" ]; then rm -rf "$WORK"; WORK=; fi
}
trap stop EXIT
echo "# $(rustc --version); $(lscpu | sed -n 's/^Model name: *//p'); $(nproc) logical CPUs"
for i in $(seq "$RUNS"); do
  # A fresh database for every run, so that later runs do not measure a bigger one.
  # Something already answering on the port would be measured in place of the server started here.
  if curl -s -o /dev/null --max-time 1 http://127.0.0.1:3917/healthz; then echo "port 3917 is already in use" >&2; exit 1; fi
  WORK=$(mktemp -d)
  DATABASE_URL="sqlite://$WORK/load.sqlite?mode=rwc" ./target/release/rust-api-service & SERVER=$!
  up=
  for _ in $(seq 50); do curl -fs http://127.0.0.1:3917/healthz >/dev/null && { up=1; break; }; sleep 0.1; done
  if [ -z "$up" ] || ! kill -0 "$SERVER" 2>/dev/null; then echo "the server did not start" >&2; exit 1; fi
  ./target/release/loadgen --url http://127.0.0.1:3917 --concurrency "$CONC" --duration-secs "$DUR" --write-percent "$WRITES"
  stop
done
