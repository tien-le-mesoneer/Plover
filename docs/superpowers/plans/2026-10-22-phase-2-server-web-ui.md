# Phase 2 — server + web UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `plv serve` starts a local axum server with an embedded React UI that opens real Bruno collections, edits and saves requests with minimal `.bru` diffs, sends requests, and streams collection runs over WebSocket.

**Architecture:** Collection state, file persistence, and "send one request" live in `plover-core` (`workspace.rs`, `store.rs`). A new `plover-server` library crate is a thin axum shell: a security layer (token, Origin, Host), REST handlers that map core types to wire DTOs, a WebSocket run endpoint that forwards `RunEvent`s, and `rust-embed` static serving. `plover-cli` gains `serve`. The frontend talks to the server only through `frontend/src/api/client.ts`; its wire types are generated from the Rust DTOs with `ts-rs`.

**Tech Stack:** Phase 1 stack + `axum` (ws), `tower-http`, `rust-embed`, `ts-rs`, `sha2`, `webbrowser`, `uuid`; dev: `tokio-tungstenite`, `tower`. Frontend: Node 22.20.0, npm, Vite, React, TypeScript, Zustand, `@monaco-editor/react` + `monaco-editor` (local, no CDN), Vitest.

**Spec:** `docs/PRD.md` (sections 5, 6 · Phase 2). Phase 1 plan for core types: `docs/superpowers/plans/2026-10-22-phase-1-core-cli.md`.

## Global Constraints

- All business logic in `plover-core`. `plover-server` contains routing, auth/Origin/Host checks, DTO mapping, WebSocket framing, static files only.
- Server binds `127.0.0.1` only (no `--host` flag in Phase 2). Port default `0` (random); `--port N` overrides.
- Every `/api/*` request and WebSocket upgrade must carry the session token; static assets do not.
- Token: 32 random bytes, hex-encoded, generated per process start. REST: `Authorization: Bearer <token>`. WebSocket: subprotocol `plover.token.<token>` (browsers cannot set WS headers); the server echoes it.
- Allowed `Origin` and `Host`: exactly `http://127.0.0.1:<port>` / `127.0.0.1:<port>` and `http://localhost:<port>` / `localhost:<port>`, plus each `--dev-origin <url>` given. A missing `Origin` is allowed (non-browser clients); a present but unlisted one → 403.
- Files are written atomically (temp file in the same dir + rename) and only after a content-hash check; never write a block the user did not change (Phase 1 `to_bru` guarantees this).
- Secret environment values entered in the UI are held in server memory only, never written to disk (keychain is Phase 4).
- No outbound network except requests the user sends; the UI must work offline (Monaco bundled locally).
- Gate per task: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`; frontend tasks also `npm run typecheck`, `npm run lint`, `npm test` in `frontend/`.

## Review Focus

1. **Path traversal through item IDs** (`../../etc/passwd`, absolute paths, `%2e%2e`, symlink escaping the root) → 400, nothing read or written outside the collection root. → Task 1 `item_path_rejects_escape`.
2. **File changed on disk after the UI read it** (git checkout, another editor) → save returns 409 with the current content; the file is not overwritten. → Task 1 `save_conflict_does_not_write`.
3. **Cross-site page or DNS rebinding** targeting `127.0.0.1:<port>` → 403 for bad `Origin` or `Host` even before token check. → Task 2 `rejects_foreign_origin`, `rejects_rebound_host`.
4. **Offline use**: Monaco must not fetch from a CDN. → Task 6 `no_cdn_in_bundle` (grep built `dist/` for `cdn.jsdelivr`/`unpkg`).
5. **Browser tab closed mid-run** → the run stops (no further requests sent). → Task 4 `disconnect_cancels_run`.

---

### Task 1: Core workspace + persistence

**Files:**
- Create: `crates/core/src/workspace.rs`, `crates/core/src/store.rs`
- Modify: `crates/core/src/lib.rs`, `crates/core/src/runner.rs` (extract the per-request send path into `send_one`)
- Test: `crates/core/tests/workspace.rs`

**Interfaces:**
- Consumes: `Collection`, `Item`, `RequestRef`, `Environment` (collection.rs); `RequestFile`, `EnvironmentFile`, `HttpRequest`, `KeyValue`; `resolve`, `ResolveContext`; `Session`, `ExecOptions`, `ResponseSummary`; `VarMap`.
- Produces:
  ```rust
  // store.rs
  pub struct ContentHash(pub String);                       // lowercase hex sha256 of file bytes
  pub struct Stored<T> { pub value: T, pub hash: ContentHash }
  pub enum StoreError { Io(std::io::Error), Parse(String), Conflict { current: Stored<String> }, OutsideRoot(String), NotFound(String) }
  pub fn resolve_item_path(root: &Path, item_id: &str) -> Result<PathBuf, StoreError>; // item_id = '/'-separated path relative to root
  pub fn read_request(root: &Path, item_id: &str) -> Result<Stored<HttpRequest>, StoreError>;
  pub fn save_request(root: &Path, item_id: &str, request: &HttpRequest, expected: &ContentHash) -> Result<ContentHash, StoreError>;
  pub fn save_environment(root: &Path, name: &str, vars: &[KeyValue], expected: &ContentHash) -> Result<ContentHash, StoreError>;

  // workspace.rs
  pub struct CollectionId(pub String);                      // first 16 hex chars of sha256(canonical root path)
  pub struct Workspace { /* RwLock<HashMap<CollectionId, OpenCollection>> */ }
  pub struct OpenCollection { pub collection: Collection, pub session: Session, pub runtime: VarMap, pub secrets: BTreeMap<String, VarMap> /* env name → values */ }
  impl Workspace {
      pub fn new(exec: ExecOptions) -> Workspace;
      pub fn open(&self, path: &Path) -> Result<CollectionId, LoadError>; // find_root + load; idempotent
      pub fn reload(&self, id: &CollectionId) -> Result<(), LoadError>;
      pub fn with<R>(&self, id: &CollectionId, f: impl FnOnce(&OpenCollection) -> R) -> Option<R>;
      pub fn with_mut<R>(&self, id: &CollectionId, f: impl FnOnce(&mut OpenCollection) -> R) -> Option<R>;
  }

  // runner.rs
  pub struct SendInput<'a> { pub item_id: &'a str, pub draft: Option<&'a HttpRequest>, pub env: Option<&'a str> }
  pub async fn send_one(open: &OpenCollection, input: SendInput<'_>) -> Result<ResponseSummary, SendError>;
  pub enum SendError { UnknownEnvironment(String), NotFound(String), Resolve(ResolveError), Exec(ExecError) }
  ```
- Rules: `resolve_item_path` rejects absolute IDs, any `..` component, empty components, and any canonicalized path not under the canonicalized root. `save_request` re-reads the file, compares hash (mismatch → `Conflict` with current content), parses into `RequestFile`, replaces `.request`, writes `to_bru()` via temp file + `rename`, returns the new hash; after a successful save the caller reloads that item. A draft in `send_one` is sent instead of the on-disk request but resolved with the same folder/collection scopes as `item_id`. Env secret values from `secrets[env]` are passed as `env_overrides`. `run` keeps its signature and calls the same resolve+send path.

- [ ] **Step 1: Failing tests** (temp copy of `fixtures/bruno-tests/collection`):
  - `open_is_idempotent`: two `open` calls on root and on a nested file return the same `CollectionId`.
  - `item_path_rejects_escape`: `"../x.bru"`, `"/etc/passwd"`, `"a/../../x.bru"`, and a symlink `link -> /tmp` with ID `"link/x.bru"` → `OutsideRoot`.
  - `save_minimal_diff`: read `ping.bru`, change one header value, save → `similar` line diff of file = 1 removed, 1 added; returned hash == sha256 of new bytes.
  - `save_conflict_does_not_write`: read, modify file on disk, save with old hash → `Conflict { current }` whose value equals new disk content; file bytes unchanged by the save.
  - `send_one_uses_draft_and_secrets` (wiremock): draft URL `{{host}}/draft`, env secret `host` set only in `secrets` → mock hit at `/draft`.
- [ ] **Step 2: Run** `cargo test -p plover-core --test workspace` → FAIL. **Step 3: Implement.** **Step 4: Run** `cargo test --workspace` → PASS.
- [ ] **Step 5: Commit** `feat(core): workspace, safe persistence and send_one`

### Task 2: Server crate + security layer

**Files:**
- Create: `crates/server/Cargo.toml` (package `plover-server`), `crates/server/src/lib.rs`, `crates/server/src/security.rs`, `crates/server/src/state.rs`
- Modify: root `Cargo.toml` (members)
- Test: `crates/server/tests/security.rs`

**Interfaces:**
- Consumes: `Workspace` (Task 1).
- Produces:
  ```rust
  pub struct ServerConfig { pub port: u16, pub dev_origins: Vec<String>, pub exec: ExecOptions }
  pub struct AppState { pub workspace: Arc<Workspace>, pub token: Arc<str>, pub allowed: Arc<AllowList> }
  pub struct AllowList { /* origins + hosts */ }
  impl AllowList { pub fn new(port: u16, dev_origins: &[String]) -> AllowList; }
  pub fn router(state: AppState) -> axum::Router;
  pub struct RunningServer { pub addr: SocketAddr, pub token: String, pub url: String /* http://127.0.0.1:<port>/#token=<token> */ }
  pub async fn start(cfg: ServerConfig) -> Result<(RunningServer, impl Future<Output = std::io::Result<()>>), std::io::Error>;
  ```
  `GET /api/health` → `{"ok":true}` (token required).
- Rules: one middleware on all routes checks Host then Origin (→ 403 `{"error":"forbidden_origin"}` / `"forbidden_host"`); a second on `/api/*` checks the token with constant-time compare (→ 401 `{"error":"unauthorized"}`). The token never appears in logs. Error body shape for every API error: `{"error": "<snake_case_code>", "message": "<human text>"}`.

- [ ] **Step 1: Failing tests** (`tower::ServiceExt::oneshot`):
  - `health_requires_token` (401 without, 200 with).
  - `rejects_foreign_origin`: `Origin: https://evil.example` + valid token → 403.
  - `rejects_rebound_host`: `Host: evil.example:PORT` + valid token → 403.
  - `allows_missing_origin_and_dev_origin`.
  - `start_binds_loopback_random_port`: `start` with port 0 → `addr.ip()` is `127.0.0.1`, port ≠ 0, `url` contains `#token=`.
- [ ] **Step 2–4:** run → FAIL, implement, run → PASS.
- [ ] **Step 5: Commit** `feat(server): axum shell with token, origin and host checks`

### Task 3: REST endpoints + DTOs + generated TS types

**Files:**
- Create: `crates/server/src/api/mod.rs`, `crates/server/src/api/dto.rs`, `crates/server/src/api/collections.rs`, `crates/server/src/api/requests.rs`, `crates/server/src/api/environments.rs`
- Test: `crates/server/tests/api.rs`

**Interfaces:**
- Consumes: `Workspace`, `store::*`, `send_one` (Task 1); `router`, `AppState` (Task 2).
- Produces — routes (all JSON, all under `/api`):
  | Method + path | Body → Response |
  |---|---|
  | `POST /collections` | `{path}` → `CollectionSummaryDto {id, name, root}` |
  | `GET /collections` | → `CollectionSummaryDto[]` |
  | `GET /collections/{id}/tree` | → `TreeNodeDto` (`{kind:"folder"\|"request"\|"invalid", id, name, method?, seq?, error?, children?}`) |
  | `POST /collections/{id}/reload` | → `TreeNodeDto` |
  | `GET /collections/{id}/requests?item=<id>` | → `{request: HttpRequestDto, hash}` |
  | `PUT /collections/{id}/requests?item=<id>` | `{request, hash}` → `{hash}`; 409 `{error:"conflict", current:{text, hash}}` |
  | `POST /collections/{id}/send` | `{item, draft?, env?}` → `ResponseDto` |
  | `GET /collections/{id}/environments` | → `EnvironmentDto[]` (`{name, vars: KeyValueDto[], secretNames, hash}`) |
  | `PUT /collections/{id}/environments/{name}` | `{vars, hash}` → `{hash}`; 409 as above |
  | `PUT /collections/{id}/environments/{name}/secrets` | `{values: Record<string,string>}` → 204 (memory only) |
- `ResponseDto { status, headers: [string,string][], elapsedMs, size, bodyText?: string, bodyBase64?: string, truncated: bool }` — `bodyText` when the body is valid UTF-8, else `bodyBase64`; bodies over 10 MiB are cut to 10 MiB with `truncated: true` (size still reports the full length).
- `HttpRequestDto` mirrors `HttpRequest` field-for-field (camelCase, tagged enums `{"mode":"json","text":…}` for body and `{"mode":"bearer","token":…}` for auth). `From`/`TryFrom` conversions live in `dto.rs`.
- All DTOs derive `ts_rs::TS` with `#[ts(export, export_to = "../../frontend/src/api/types/")]`; `cargo test -p plover-server export_bindings` writes the `.ts` files, which are committed.
- Error mapping: `OutsideRoot`→400 `bad_item`, `NotFound`→404, `Parse`→422 `parse_error`, `Conflict`→409, `Resolve`→422 `resolve_error`, `Exec` (transport failure) → 502 `send_failed` with the message. Any HTTP status from the target (incl. 4xx/5xx) is a successful `ResponseDto` with 200.

- [ ] **Step 1: Failing tests** (temp fixture copy, wiremock for send):
  - `open_and_tree`: tree root has ≥ 10 children; folders listed before requests (Phase 1 order).
  - `read_edit_save_minimal_diff` (**phase acceptance**): GET `ping.bru`, change one header, PUT → file diff 1 removed/1 added line.
  - `save_conflict_409`.
  - `send_draft_returns_response`: status, `bodyText`, `size` match the mock.
  - `binary_and_large_body`: 12 MiB binary body → `bodyBase64` present, `truncated == true`, `size == 12 MiB`.
  - `bad_item_400`: `item=../x.bru`.
  - `env_edit_and_secret_memory_only`: PUT secrets, then send uses it; env file on disk does not contain the secret value.
  - `ts_bindings_up_to_date`: regenerate into a temp dir and compare with committed `frontend/src/api/types/` (fails if stale).
- [ ] **Step 2–4:** run → FAIL, implement, run → PASS.
- [ ] **Step 5: Commit** `feat(server): REST API for collections, requests, environments and send`

### Task 4: WebSocket collection runs

**Files:**
- Create: `crates/server/src/api/runs.rs`
- Modify: `crates/core/src/runner.rs` (`RunEvent` gains `Finished { report: RunReport }` emitted last)
- Test: `crates/server/tests/runs.rs`

**Interfaces:**
- Consumes: `runner::run`, `RunEvent`, `RunReport`, `Workspace`.
- Produces: `GET /api/ws` (upgrade, subprotocol `plover.token.<token>`). Client → server: `{"type":"start","runId":"<uuid>","collectionId","item":"<folder or request id, '' = root>","env":null}` and `{"type":"cancel","runId"}`. Server → client: `RunEventDto` = `{"runId", "type":"started"|"passed"|"failed"|"skipped"|"finished", "item", ...fields}` (item IDs, not absolute paths; `elapsedMs`). One active run per socket; a second `start` while running → `{"type":"error","error":"run_in_progress"}`.
- Rules: the run future is dropped on `cancel` or socket close, which aborts in-flight requests. Runtime vars from the run are written back to `OpenCollection.runtime` when it finishes.

- [ ] **Step 1: Failing tests** (real TCP via `start`, `tokio-tungstenite`, wiremock):
  - `ws_requires_token_subprotocol` (handshake fails without it).
  - `streams_events_in_order`: 3 requests → `started,passed` ×3 then `finished` with `passed: 3`.
  - `disconnect_cancels_run`: 3 requests, mock delays each 300 ms; close socket after first `started` → mock receives ≤ 1 request after 1 s.
  - `cancel_message_stops_run`.
- [ ] **Step 2–4:** run → FAIL, implement, run → PASS.
- [ ] **Step 5: Commit** `feat(server): stream collection runs over WebSocket`

### Task 5: Embedded static UI + `plv serve`

**Files:**
- Create: `crates/server/src/assets.rs`
- Modify: `crates/cli/src/main.rs`, `crates/cli/Cargo.toml`
- Test: `crates/server/tests/assets.rs`, `crates/cli/tests/cli.rs`

**Interfaces:**
- Consumes: `start`, `ServerConfig` (Task 2).
- Produces: `#[derive(RustEmbed)] #[folder = "$CARGO_MANIFEST_DIR/../../frontend/dist"] #[allow_missing = true] struct Assets;` Routes: `GET /` and any non-`/api` path → asset or SPA fallback `index.html`; if `dist` is missing, `/` returns 200 HTML "UI not built — run `npm run build` in frontend/". Correct `content-type` by extension; `cache-control: no-cache` for `index.html`, immutable for hashed `assets/*`. CSP header: `default-src 'self'; connect-src 'self' ws://127.0.0.1:* ws://localhost:*; style-src 'self' 'unsafe-inline'; worker-src 'self' blob:` (Monaco needs blob workers and inline styles).
- CLI: `plv serve [PATH] [--port <N>] [--no-open] [--dev-origin <URL>]...`. Prints `plv listening on <url>` (with token fragment), opens it with `webbrowser` unless `--no-open`; if `PATH` given, opens that collection at startup. Ctrl-C shuts down gracefully.

- [ ] **Step 1: Failing tests:** `index_served_without_token`, `spa_fallback`, `api_not_shadowed_by_fallback` (unknown `/api/x` → 404 JSON, not HTML), `csp_header_present`; CLI `serve_prints_url_and_opens_collection` (spawn with `--no-open --port 0 <fixture copy>`, read first stdout line, GET tree with token → 200, kill).
- [ ] **Step 2–4:** run → FAIL, implement, run → PASS.
- [ ] **Step 5: Commit** `feat(cli): plv serve with embedded UI`

### Task 6: Frontend scaffold, ApiClient, offline Monaco

**Files:**
- Create: `.tool-versions` (`nodejs 22.20.0`), `.nvmrc` (`22.20.0`), `frontend/package.json`, `frontend/package-lock.json`, `frontend/tsconfig.json`, `frontend/vite.config.ts`, `frontend/eslint.config.js`, `frontend/index.html`, `frontend/src/main.tsx`, `frontend/src/App.tsx`, `frontend/src/api/client.ts`, `frontend/src/api/token.ts`, `frontend/src/editor/monaco.ts`
- Test: `frontend/src/api/client.test.ts`, `frontend/src/api/token.test.ts`, `frontend/scripts/check-no-cdn.mjs`

**Interfaces:**
- Consumes: generated types in `frontend/src/api/types/` (Task 3); routes (Tasks 3–4).
- Produces:
  ```ts
  // token.ts
  export function takeTokenFromLocation(loc: Location, storage: Storage): string | null; // reads #token=, stores in sessionStorage, clears hash via history.replaceState
  // client.ts — the ONLY module that calls fetch/WebSocket
  export class ApiError extends Error { status: number; code: string; body: unknown }
  export interface ApiClient {
    openCollection(path: string): Promise<CollectionSummaryDto>;
    listCollections(): Promise<CollectionSummaryDto[]>;
    tree(id: string): Promise<TreeNodeDto>; reload(id: string): Promise<TreeNodeDto>;
    readRequest(id: string, item: string): Promise<{ request: HttpRequestDto; hash: string }>;
    saveRequest(id: string, item: string, request: HttpRequestDto, hash: string): Promise<{ hash: string }>;
    send(id: string, item: string, draft: HttpRequestDto | null, env: string | null): Promise<ResponseDto>;
    environments(id: string): Promise<EnvironmentDto[]>;
    saveEnvironment(id: string, name: string, vars: KeyValueDto[], hash: string): Promise<{ hash: string }>;
    setSecrets(id: string, name: string, values: Record<string, string>): Promise<void>;
    startRun(id: string, item: string, env: string | null, onEvent: (e: RunEventDto) => void): { cancel(): void; done: Promise<void> };
  }
  export function createApiClient(baseUrl: string, token: string, deps?: { fetch?: typeof fetch; WebSocket?: typeof WebSocket }): ApiClient;
  ```
- Scripts: `dev` (vite, proxy `/api` incl. ws to `PLOVER_API` env), `build`, `typecheck` (`tsc --noEmit`), `lint` (eslint), `test` (`vitest run`), `check:no-cdn` (fails if `dist/` contains `cdn.jsdelivr.net` or `unpkg.com`). `monaco.ts` calls `loader.config({ monaco })` with the bundled `monaco-editor` and registers Vite `?worker` workers for editor + json.

- [ ] **Step 1: Failing tests:** `token.test.ts` (`#token=abc` → `"abc"`, stored, hash cleared; reload uses storage); `client.test.ts` (Bearer header on every call; 409 → `ApiError{code:"conflict"}`; `startRun` opens WS with subprotocol `plover.token.<t>` and forwards parsed events; `cancel()` sends cancel then closes).
- [ ] **Step 2:** `npm test` → FAIL. **Step 3: Implement.** **Step 4:** `npm run typecheck && npm run lint && npm test && npm run build && npm run check:no-cdn` → all pass (`no_cdn_in_bundle`).
- [ ] **Step 5: Commit** `feat(frontend): vite scaffold, ApiClient and offline Monaco`

### Task 7: Collection sidebar, request tabs and editor

**Files:**
- Create: `frontend/src/store/workspace.ts`, `frontend/src/store/tabs.ts`, `frontend/src/components/Sidebar.tsx`, `frontend/src/components/OpenCollectionDialog.tsx`, `frontend/src/components/RequestTabs.tsx`, `frontend/src/components/request/UrlBar.tsx`, `frontend/src/components/request/KeyValueTable.tsx`, `frontend/src/components/request/AuthTab.tsx`, `frontend/src/components/request/BodyTab.tsx`, `frontend/src/components/EnvSelector.tsx`, `frontend/src/components/EnvEditor.tsx`
- Test: `frontend/src/store/tabs.test.ts`, `frontend/src/store/workspace.test.ts`

**Interfaces:**
- Consumes: `ApiClient` (Task 6).
- Produces (Zustand):
  ```ts
  // tabs.ts
  interface Tab { key: string /* collectionId + ':' + item */; collectionId: string; item: string; saved: HttpRequestDto; draft: HttpRequestDto; hash: string; response?: ResponseDto; sending: boolean }
  interface TabsState { tabs: Tab[]; active: string | null;
    open(collectionId: string, item: string): Promise<void>; close(key: string): void; activate(key: string): void;
    edit(key: string, f: (d: HttpRequestDto) => HttpRequestDto): void; isDirty(key: string): boolean;
    save(key: string): Promise<'saved' | 'conflict'>; send(key: string): Promise<void> }
  // workspace.ts
  interface WorkspaceState { collections: CollectionSummaryDto[]; trees: Record<string, TreeNodeDto>; environments: Record<string, EnvironmentDto[]>; activeEnv: Record<string, string | null>;
    open(path: string): Promise<void>; reload(id: string): Promise<void>; selectEnv(id: string, name: string | null): void }
  ```
- UI rules: sidebar shows folders/requests in server order with method badge; invalid items greyed with error tooltip. Tabs show a dot when dirty; `Ctrl/Cmd+S` saves; closing a dirty tab asks to confirm. Request tabs: Params (query + path), Headers, Auth (none / inherit / basic / bearer; unsupported modes shown read-only with their name), Body (mode select; Monaco for json/text/xml; key-value table for form-urlencoded/multipart). Disabled rows keep a checkbox, matching `.bru` `~`. Save conflict → modal "File changed on disk" with Reload / Overwrite (overwrite re-saves using the returned `current.hash`). Open collection: text input for an absolute path (native picker is Phase 4).

- [ ] **Step 1: Failing tests** (ApiClient mocked): `edit_marks_dirty_and_save_clears`, `save_conflict_returns_conflict_and_keeps_draft`, `open_same_item_twice_reuses_tab`, `close_removes_and_activates_neighbor`, `select_env_per_collection`.
- [ ] **Step 2–4:** `npm test` → FAIL, implement, `npm run typecheck && npm run lint && npm test` → PASS.
- [ ] **Step 5: Commit** `feat(frontend): sidebar, request tabs and editors`

### Task 8: Response viewer + run panel

**Files:**
- Create: `frontend/src/components/ResponseViewer.tsx`, `frontend/src/components/RunPanel.tsx`, `frontend/src/store/runs.ts`, `frontend/src/lib/format.ts`
- Test: `frontend/src/lib/format.test.ts`, `frontend/src/store/runs.test.ts`

**Interfaces:**
- Consumes: `ResponseDto`, `RunEventDto`, `ApiClient.startRun`.
- Produces: `formatSize(bytes: number): string` (`0 B`, `1.2 KB`, `3.4 MB`, base 1024, one decimal); `formatDuration(ms: number): string` (`85 ms`, `1.24 s`); `prettyBody(r: ResponseDto): { language: 'json' | 'xml' | 'html' | 'text' | 'binary'; text: string }` (JSON pretty-printed with 2 spaces when `content-type` contains `json` and it parses; base64 bodies → `binary` with a size note). `runs.ts`: `{ runs: Record<string, RunState>; start(collectionId, item, env): void; cancel(runId): void }`, `RunState { events: RunEventDto[]; status: 'running' | 'done' | 'cancelled'; report?: RunReportDto }`.
- UI rules: response pane shows status (colored by class), time, size, headers table, body in read-only Monaco; `truncated` → banner "Showing first 10 MB". Right-click folder → "Run folder" opens RunPanel with live rows (✓/✗/–, method, name, status, time) and a Cancel button.

- [ ] **Step 1: Failing tests:** format cases above; `prettyBody` for json / invalid json / base64; `runs` accumulates events, `finished` → done with report, `cancel` → cancelled.
- [ ] **Step 2–4:** FAIL → implement → `npm run typecheck && npm run lint && npm test` PASS.
- [ ] **Step 5: Commit** `feat(frontend): response viewer and live collection runs`

### Task 9: Phase gate — embedded build and end-to-end check

**Files:**
- Create: `scripts/build-release.sh` (`npm ci && npm run build` in `frontend/`, then `cargo build --release -p plover-cli`), `docs/development.md` (dev loop: `cargo run -p plover-cli -- serve --no-open --port 4000 --dev-origin http://localhost:5173` + `PLOVER_API=http://127.0.0.1:4000 npm run dev`)
- Modify: `README.md` (create if missing: what it is, `plv run`, `plv serve`)

- [ ] **Step 1:** `scripts/build-release.sh` → `target/release/plv` exists; `strings` on it contains `index.html` content marker.
- [ ] **Step 2:** Full gate: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `npm run typecheck && npm run lint && npm test && npm run check:no-cdn` → all pass.
- [ ] **Step 3: Browser acceptance** (agent-browser skill, localhost only): copy `fixtures/bruno-tests/collection` to a temp dir and `git init` + commit it there; start a local wiremock-equivalent (`python3 -m http.server` is fine) as `host`; run `target/release/plv serve --no-open --port 0 <copy>`; in the browser: open the URL, expand sidebar, open `ping.bru`, change one header value, Cmd+S, Send → response pane shows status/time/size. Then `git -C <copy> diff --stat` → exactly `1 file changed, 1 insertion(+), 1 deletion(-)`. Screenshot saved to the scratchpad.
- [ ] **Step 4: Commit** `chore: release build script and phase 2 docs`

---

## Phase 2 exit checklist

- [ ] Real Bruno collection opens; edit + save produces a 1-line diff (Task 3 test + Task 9 browser check).
- [ ] Token, Origin, Host enforced; WS requires token (Tasks 2, 4).
- [ ] Runs stream and cancel (Task 4, 8).
- [ ] Offline bundle, single binary (Tasks 5, 6, 9).
- [ ] Summary to the user; **stop** for confirmation before Phase 3.

## Deferred (not in Phase 2)

File watching for external edits (conflict check covers safety; manual Reload exists) · creating/renaming/deleting requests and folders · collection-level settings UI · cookies UI · native folder picker and keychain (Phase 4) · scripts/tests (Phase 3).
