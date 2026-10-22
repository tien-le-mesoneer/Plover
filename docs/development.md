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
BRS_UPDATE_BINDINGS=1 cargo test -p brs-server --test bindings
```

`cargo test` fails while the committed file is stale.

## UI dev loop

Terminal 1 — the API server on a fixed port, allowing the Vite origin:

```bash
cargo run -p brs-cli -- serve --no-open --port 4000 --dev-origin http://localhost:5173 /path/to/collection
```

Terminal 2 — Vite with hot reload, proxying `/api` (including the WebSocket) to port 4000:

```bash
cd frontend && BRS_API=http://127.0.0.1:4000 npm run dev
```

Open `http://localhost:5173/#token=<token>`, using the token from the URL that
`serve` printed. The token lives in this tab's sessionStorage afterwards.

Debug builds read `frontend/dist` from disk at runtime; release builds embed it.

## Release build

```bash
scripts/build-release.sh   # → target/release/bru-rs
```

## Browser check (Monaco and page errors)

Unit tests run in jsdom, which cannot create Monaco editors or workers. After
a release build, run the real-browser check (needs the
[`agent-browser`](https://www.npmjs.com/package/agent-browser) CLI on `PATH`;
it is not a project dependency):

```bash
cd frontend && npm run check:console   # or: BRU_RS=path/to/bru-rs scripts/check-console.sh
```

It serves a one-request collection on localhost, opens the request (body
editor) and sends it to the server itself (response editor), types invalid
JSON and requires a diagnostic from the JSON worker, and fails on any
uncaught page error. Run it after changing `frontend/src/editor/` or upgrading
`monaco-editor`.

## Security model (local only)

- Binds `127.0.0.1` only, random port by default.
- Every `/api` call needs the per-process token (`Authorization: Bearer`, or the
  WebSocket subprotocol `brs.token.<token>`).
- `Host` and `Origin` must be `127.0.0.1:<port>` / `localhost:<port>` (or a
  `--dev-origin`), which blocks cross-site requests and DNS rebinding.
- Secret environment values entered in the UI stay in server memory.

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
bru-rs run fixtures/phase3-chain --env local      # 3 passed · Tests 5/5 · Asserts 2/2
```

