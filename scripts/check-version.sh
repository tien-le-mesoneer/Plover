#!/usr/bin/env bash
# Checks that every crate and the desktop config carry the same version, and that it
# matches the tag when one is given: scripts/check-version.sh v0.1.0
set -euo pipefail
cd "$(dirname "$0")/.."
version() { sed -nE "s/^version = \"([^\"]+)\"/\1/p" "$1" | head -1; }
expected=$(version crates/core/Cargo.toml)
status=0
for f in crates/cli/Cargo.toml crates/desktop/Cargo.toml; do
  v=$(version "$f")
  [ "$v" = "$expected" ] || { echo "$f has $v, crates/core has $expected"; status=1; }
done
tauri=$(sed -nE 's/^  "version": "([^"]+)",/\1/p' crates/desktop/tauri.conf.json)
[ "$tauri" = "$expected" ] || { echo "tauri.conf.json has $tauri, crates/core has $expected"; status=1; }
if [ $# -gt 0 ] && [ "${1#v}" != "$expected" ]; then
  echo "tag $1 does not match version $expected"
  status=1
fi
[ $status -eq 0 ] && echo "version $expected"
exit $status
