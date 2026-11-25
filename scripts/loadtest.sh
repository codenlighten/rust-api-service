#!/usr/bin/env bash
# Reproducible load test: release build, fresh SQLite file, N runs of the closed-loop generator.
# Usage: scripts/loadtest.sh [runs] [concurrency] [duration_secs] [write_percent]
set -euo pipefail
RUNS=${1:-5}; CONC=${2:-32}; DUR=${3:-20}; WRITES=${4:-0}
cd "$(dirname "$0")/.."
cargo build --release --bins
WORK=$(mktemp -d)
export DATABASE_URL="sqlite://$WORK/load.sqlite?mode=rwc"
export BIND_ADDR=127.0.0.1:3917
export TOKEN_SECRET=load-test-secret-not-for-real-use-0123456789
export RUST_LOG=warn
./target/release/rust-api-service & SERVER=$!
trap 'kill "$SERVER" 2>/dev/null || true; wait "$SERVER" 2>/dev/null || true; rm -rf "$WORK"' EXIT
for _ in $(seq 50); do curl -fs http://127.0.0.1:3917/healthz >/dev/null && break; sleep 0.1; done
echo "# $(rustc --version); $(lscpu | sed -n 's/^Model name: *//p'); $(nproc) logical CPUs"
for i in $(seq "$RUNS"); do
  ./target/release/loadgen --url http://127.0.0.1:3917 --concurrency "$CONC" --duration-secs "$DUR" --write-percent "$WRITES"
done
