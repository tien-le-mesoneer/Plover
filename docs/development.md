# Development

Toolchains: Rust stable (`rust-toolchain.toml`), Node 22.20.0 (`.tool-versions`, `.nvmrc`), npm.

## Checks

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cd frontend && npm ci && npm run typecheck && npm run lint && npm test
```

The frontend's API types (`frontend/src/api/types.ts`) are generated from the
server DTOs. After changing `crates/server/src/api/dto.rs` or `runs.rs`:

```bash
PLOVER_UPDATE_BINDINGS=1 cargo test -p plover-server --test bindings
```

`cargo test` fails while the committed file is stale.

## UI dev loop

Terminal 1 — the API server on a fixed port, allowing the Vite origin:

```bash
cargo run -p plover-cli -- serve --no-open --port 4000 --dev-origin http://localhost:5173 /path/to/collection
```

Terminal 2 — Vite with hot reload, proxying `/api` (including the WebSocket) to port 4000:

```bash
cd frontend && PLOVER_API=http://127.0.0.1:4000 npm run dev
```

Open `http://localhost:5173/#token=<token>`, using the token from the URL that
`serve` printed. The token lives in this tab's sessionStorage afterwards.

Debug builds read `frontend/dist` from disk at runtime; release builds embed it.

## Release build

```bash
scripts/build-release.sh                                   # → target/release/plv (with web UI)
cargo build --release -p plover-cli --no-default-features  # CLI-only plv, no embedded UI
```

### Release profile (measured 2026-10-23, macOS arm64)

Benchmark: 1000 requests, each with one post-response script and one chai test, against
the local Python mock (`scratchpad/bench/scoll1000`). Wall/CPU are averages of 4
interleaved runs after a warm-up; this machine's wall times vary by about ±20% between
rounds, so compare within one table only.

| Profile | Size | gzip -9 | Wall | CPU |
|---|---|---|---|---|
| default (before) | 21.3 MB (18.0 MB stripped) | 6.9 MB | 2.16 s | 1.97 s |
| `strip` | 18.0 MB | 6.9 MB | – | – |
| `strip` + thin LTO + 1 CGU | 16.4 MB | 6.4 MB | – | – |
| `strip` + fat LTO + 1 CGU | 15.6 MB | 6.2 MB | – | – |
| **`strip` + fat LTO + 1 CGU + `opt-level = "s"` (chosen)** | **12.7 MB** | **4.9 MB** | **2.28 s (+5.6%)** | **2.11 s (+7%)** |
| same + `opt-level = "z"` | 11.3 MB | 4.6 MB | 3.00 s (+39%) | 2.87 s (+46%) |
| chosen profile, CLI-only (`--no-default-features`) | 7.6 MB | 3.5 MB | 2.27 s | 2.10 s |

"–": measured only in a non-interleaved round (thin 1.86 s, fat 1.85 s vs baseline 1.95 s
in that round). `opt-level = "z"` exceeds the 10% budget. `panic` stays `unwind`.

## Browser check (Monaco and page errors)

Unit tests run in jsdom, which cannot create Monaco editors or workers. After
a release build, run the real-browser check (needs the
[`agent-browser`](https://www.npmjs.com/package/agent-browser) CLI on `PATH`;
it is not a project dependency):

```bash
cd frontend && npm run check:console   # or: PLV_BIN=path/to/plv scripts/check-console.sh
```

It serves a one-request collection on localhost, opens the request (body
editor) and sends it to the server itself (response editor), types invalid
JSON and requires a diagnostic from the JSON worker, and fails on any
uncaught page error. Run it after changing `frontend/src/editor/` or upgrading
`monaco-editor`.

## Security model (local only)

- Binds `127.0.0.1` only, random port by default.
- Every `/api` call needs the per-process token (`Authorization: Bearer`, or the
  WebSocket subprotocol `plover.token.<token>`).
- `Host` and `Origin` must be `127.0.0.1:<port>` / `localhost:<port>` (or a
  `--dev-origin`), which blocks cross-site requests and DNS rebinding.
- URLs the app opens itself (desktop window, Open in Browser, `plv serve`'s
  auto-open) carry a single-use launch code (`#code=`, valid 60 s), which the page
  exchanges once via `POST /api/session/exchange` (no token needed; Host/Origin still
  checked; constant-time compare; 20 failures per minute lock exchanges). The URL
  `plv serve` prints keeps `#token=` so it can be copied into a browser.
- `GET /api/events` (WebSocket, token subprotocol) tells the page about native
  actions (collection opened, reload requested).
- Secret environment values go to the OS keychain (service `dev.plover`, account
  `<collection-id>/<env>/<name>`; values over one entry's limit are chunked). A value
  the keychain refuses stays in memory for that secret only, with a warning; with no
  keychain at startup, every secret is memory-only and the UI says so. Never on disk.

## Script sandbox libraries

The libraries scripts can `require` are prebuilt into `crates/core/js/dist/` and
committed, so building Rust never needs Node. After changing
`crates/core/js/libs/` or its dependencies:

```bash
cd crates/core/js && npm ci && npm run build && npm run check
```

## Phase 3 fixture

`fixtures/phase3-chain` is a chained collection (login → token → requests that
use it) with scripts, asserts and tests. Against the localhost mock:

```bash
python3 scripts/phase3-mock.py 18090 &
plv run fixtures/phase3-chain --env local      # 3 passed · Tests 5/5 · Asserts 2/2
```

## Remembered collections

`plv serve` and the desktop app keep open collections, the recent list and per-collection
layout in `state.json` (see the README for its location). Set `PLOVER_STATE_DIR` to a
temp directory when you start them from tests or scripts so they don't touch your real
list; the CLI tests, `npm run check:console` and the CI smoke step already do.
