# bruno-rs — Product Requirements

Status: draft for review · 2026-10-22

## 1. Goal

A Rust API client that opens existing Bruno collections (`.bru` files on disk)
without conversion and saves them without noisy diffs. One core, three shells:

| Shell | What it is |
|---|---|
| CLI (`bru-rs`) | Runs a collection, folder or single request in CI |
| Web | `bru-rs serve` — single binary, embedded UI on `127.0.0.1` |
| Desktop | Tauri 2 window that starts the same server internally |

The UI never sends HTTP itself. Every request runs in the Rust backend, so there
are no CORS limits and full control of headers, cookies, mTLS and proxies.

## 2. Users and core jobs

1. **Developer with Bruno collections in Git** — open, edit, send, commit.
   A save that changes one header must change only that header's block.
2. **CI pipeline** — run a folder against an environment, get a non-zero exit
   code on failure and (Phase 3) JUnit XML.

## 3. Decisions already made

| ID | Decision | Why |
|---|---|---|
| D1 | Variable precedence (highest first): **runtime > request > folder (innermost first) > environment > collection** | Matches Bruno (`bruno-cli/src/runner/interpolate-vars.js:63`), so collections resolve identically. Defined in one constant. |
| D2 | Two-layer `.bru` handling: a **syntax layer** keeps every block's exact source; a **typed layer** maps blocks to the request model. Saving rewrites only blocks whose meaning changed. | Bruno's own parse→serialize normalizes files; byte-exact round-trip needs source retention. |
| D3 | Blocks we do not model (gRPC, WebSocket, OAuth2, `example`, unknown future blocks) are **kept verbatim** and never block loading. | Collections from newer Bruno versions must still open and save cleanly. |
| D4 | A request **fails** on transport error or (Phase 3) a failed test/assert. HTTP 4xx/5xx alone is not a failure. A `.bru` file that cannot be parsed is **skipped with a warning** naming its path and error; it does not change the exit code. | Matches Bruno CLI semantics (`brunoSkippedFiles`, `bruno-cli/src/utils/collection.js:80`). |
| D5 | Binary name `bru-rs` (Bruno's CLI is `bru`; avoid the clash). | — |
| D6 | Reference fixtures: Bruno's own `packages/bruno-tests` (288 `.bru` files, MIT) copied into `fixtures/bruno-tests/` with its license. User collections may be added under `fixtures/user/`. | Real files beat invented samples. |

## 4. Tech stack (fixed — ask before substituting)

Rust stable, Cargo workspace · `tokio`, `axum` (HTTP + WebSocket) · `reqwest`
with `rustls` + `cookie_store` · `rquickjs` (scripts) · `pest` (`.bru` parser) ·
`clap` · `rust-embed` · `serde`, `thiserror`, `tracing` · Frontend: React +
TypeScript + Vite, Monaco, Zustand · Desktop: Tauri 2 · Secrets: `keyring`.

## 5. Architecture

```
crates/core     domain: .bru syntax + model, collection loader, vars,
                resolver, executor, runner, (P3) script engine
crates/cli      thin: args → core::runner → printer
crates/server   thin: axum REST + WebSocket → core   (Phase 2)
crates/desktop  thin: Tauri → starts server          (Phase 4)
frontend/       shared UI, talks only through ApiClient (Phase 2)
```

Rules:
- All business logic lives in `core`. Shells contain argument parsing, transport
  and presentation only.
- The runner emits `RunEvent`s through a callback. The CLI prints them; the
  server streams the same events over WebSocket. One code path.
- Server binds `127.0.0.1` by default, requires a random auth token on every
  request, and rejects unexpected `Origin` headers. Desktop starts the server on
  a random port and hands port + token to the webview.
- Scripts run in `rquickjs` with per-script timeout, memory limit, and no
  filesystem/network access beyond provided APIs.

## 6. Phases

Each phase ends with: `cargo test`, `cargo clippy --all-targets -- -D warnings`,
(from Phase 2) frontend type-check, a summary, then **stop for confirmation**.

### Phase 1 — core + CLI
Scope:
- Request model: method, URL, query + path params, headers, body
  (none / json / text / xml / form-urlencoded / multipart), auth
  (none / inherit / basic / bearer; others preserved, reported unsupported).
- `.bru` syntax + typed layers for request files, `collection.bru`,
  `folder.bru`, `environments/*.bru`; `bruno.json` via serde with unknown
  fields preserved.
- Collection loader: tree ordered by `meta.seq`, then name.
- `{{var}}` interpolation with D1 precedence; nested references resolved;
  cycles terminate; unknown variables left as written.
- Executor: timeout, follow-redirects toggle, TLS-verify toggle, cookie jar per
  session.
- CLI: `bru-rs run <path> [--env <name>] [--env-var K=V]... [--insecure]
  [--timeout-ms N]`. Exit 0 = all passed, 1 = any failed, 2 = usage/load error.

Acceptance:
- Golden test: every fixture `.bru` file parses and writes back byte-identical.
- Editing one field changes only its block (diff test).
- Interpolation precedence unit tests.
- Integration tests against `wiremock`: methods, bodies, auth, redirects toggle,
  cookies persisted across requests in one run.

### Phase 2 — server + web UI
- REST: open collection by path, list tree, read/write request, list/edit
  environments, send request. WebSocket: collection runs streaming
  `RunEvent`s (started / passed / failed / response summary).
- UI: sidebar tree, request tabs, Monaco body editor, headers/query/auth tabs,
  response viewer (status, time, size, headers, pretty JSON), environment
  selector.
- `bru-rs serve` opens the browser at the local URL; frontend embedded with
  `rust-embed`.
- Acceptance: open a real Bruno collection, edit a request, save, `git diff`
  shows only the edited lines.

### Phase 3 — scripts & tests
- `script:pre-request`, `script:post-response`, `tests`, and `assert` blocks.
- `rquickjs` runtime exposing `bru` (getVar/setVar/getEnvVar/setEnvVar),
  `req`, `res`, `test()`, `expect` (bundled chai).
- Test results in UI and CLI; CLI `--reporter-junit <file>` and
  `--reporter-json <file>`.
- Acceptance: a sample collection with chained requests that set/get variables
  passes in both UI and CLI.

### Phase 4 — desktop
- Tauri 2 shell starts the embedded server and loads the UI.
- Native folder picker; secrets in OS keychain via `keyring`.
- Acceptance: builds and runs on macOS, Windows, Linux with the same behavior
  as web.

## 7. Out of scope (design for, do not build)

Multi-user server deployment, GraphQL, gRPC, WebSocket testing, mobile,
OAuth2/AWS/Digest/NTLM auth execution, `process.env` / `.env` interpolation,
proxy and client-certificate config from `bruno.json` (parsed and kept, not
applied until a later phase).

## 8. Open questions (decide before the phase that needs them)

1. Phase 2: Node.js version for the frontend (no version is pinned in the repo
   yet; `asdf` reports none set).
2. Phase 3: Bruno scripts may `require()` npm modules (`moduleWhitelist` in
   `bruno.json`). Sandboxed `rquickjs` cannot; confirm "unsupported, reported
   clearly" is acceptable.
3. Phase 4: Bruno stores secret env values in its own encrypted app storage,
   not in `.bru` files. Our keychain store will not read Bruno's. Confirm.
