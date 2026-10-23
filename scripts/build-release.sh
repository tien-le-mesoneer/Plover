#!/usr/bin/env bash
# Builds the frontend, then the single Plover binary with the UI embedded.
set -euo pipefail
cd "$(dirname "$0")/.."
(
  cd frontend
  npm ci --no-fund --no-audit
  npm run build
  npm run check:no-cdn
)
cargo build --release -p plover-cli
ls -lh target/release/plv
# CLI-only (no web UI): cargo build --release -p plover-cli --no-default-features
