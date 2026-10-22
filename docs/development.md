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

## Security model (local only)

- Binds `127.0.0.1` only, random port by default.
- Every `/api` call needs the per-process token (`Authorization: Bearer`, or the
  WebSocket subprotocol `brs.token.<token>`).
- `Host` and `Origin` must be `127.0.0.1:<port>` / `localhost:<port>` (or a
  `--dev-origin`), which blocks cross-site requests and DNS rebinding.
- Secret environment values entered in the UI stay in server memory.
