#!/usr/bin/env bash
# Builds the frontend, then the single bru-rs binary with the UI embedded.
set -euo pipefail
cd "$(dirname "$0")/.."
(
  cd frontend
  npm ci --no-fund --no-audit
  npm run build
  npm run check:no-cdn
)
cargo build --release -p brs-cli
ls -lh target/release/bru-rs
