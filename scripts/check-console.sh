#!/usr/bin/env bash
# Opens the built UI in a real browser, creates Monaco editors (request body
# and response viewer) and fails if the page logged any uncaught errors.
#
# Needs: a built binary (default target/release/plv; override with PLV_BIN)
# and the `agent-browser` CLI on PATH. Localhost only: the request is sent to
# the Plover server itself.
set -euo pipefail
cd "$(dirname "$0")/.."
BIN=${PLV_BIN:-target/release/plv}
command -v agent-browser >/dev/null || { echo "agent-browser not found on PATH" >&2; exit 2; }
[ -x "$BIN" ] || { echo "$BIN not built; run scripts/build-release.sh" >&2; exit 2; }

TMP=$(mktemp -d)
SESSION="plv-console-$$"
cleanup() {
  agent-browser --session "$SESSION" close >/dev/null 2>&1 || true
  if [ -n "${SERVER_PID:-}" ]; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  rm -rf "$TMP"
}
trap cleanup EXIT

PORT=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')
COL="$TMP/collection"
mkdir -p "$COL"
echo '{"version":"1","name":"console-check","type":"collection"}' > "$COL/bruno.json"
cat > "$COL/r.bru" <<BRU
meta {
  name: json request
  type: http
  seq: 1
}

post {
  url: http://127.0.0.1:$PORT/
  body: json
  auth: none
}

body:json {
  {
    "hello": "world"
  }
}
BRU

# Own state dir: the check must not add its temp collection to the real recent list.
PLOVER_STATE_DIR="$TMP/state" "$BIN" serve --no-open --port "$PORT" "$COL" > "$TMP/serve.log" 2>&1 &
SERVER_PID=$!
for _ in $(seq 50); do grep -q "listening on" "$TMP/serve.log" 2>/dev/null && break; sleep 0.1; done
URL=$(sed -n 's/^plv listening on //p' "$TMP/serve.log")
[ -n "$URL" ] || { cat "$TMP/serve.log" >&2; exit 1; }

ab() { agent-browser --session "$SESSION" "$@"; }
ab open "$URL" >/dev/null
ab wait --text "json request" >/dev/null
ab errors --clear >/dev/null
ab eval 'document.querySelector(".node.request").click()' >/dev/null  # tab: body editor (Monaco)
ab wait 1500 >/dev/null
ab eval 'document.querySelector(".urlbar button[type=submit]").click()' >/dev/null  # response viewer
ab wait --text "200" >/dev/null
ab wait 1500 >/dev/null
# Typing invalid JSON must produce a diagnostic from the JSON worker.
ab click '.request-editor .monaco-editor .view-lines' >/dev/null
for key in End x x x; do ab press "$key" >/dev/null; done
ab wait 2000 >/dev/null
SQUIGGLES=$(ab eval 'document.querySelectorAll(".request-editor .squiggly-error").length')
echo "json diagnostics shown: $SQUIGGLES"
if [ "$SQUIGGLES" -lt 1 ]; then
  echo "the JSON worker produced no diagnostics for invalid JSON" >&2
  exit 1
fi
ERRORS=$(ab errors --json)
EDITORS=$(ab eval 'document.querySelectorAll(".monaco-editor").length')
echo "monaco editors on page: $EDITORS"
if [ "$EDITORS" -lt 2 ]; then
  echo "expected 2 Monaco editors (body + response)" >&2
  exit 1
fi
COUNT=$(echo "$ERRORS" | python3 -c 'import json, sys; print(len(json.load(sys.stdin)["data"]["errors"]))')
if [ "$COUNT" != "0" ]; then
  echo "$COUNT page errors:" >&2
  echo "$ERRORS" | head -c 3000 >&2
  echo >&2
  exit 1
fi
echo "no page errors"
