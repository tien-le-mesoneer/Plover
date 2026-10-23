# Phase 4 — desktop Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Tauri 2 desktop app (`Plover.app` / `.msi` / `.AppImage` + `.deb`) that runs the same embedded server and UI as `plv serve`, adds a native folder picker, stores secrets in the OS keychain, can open the session in a browser, and installs the `plv` CLI from inside the app.

**Architecture:** The desktop crate is a thin shell. It starts `plover_server::start` on `127.0.0.1:0` and points its webview at the returned `http://127.0.0.1:<port>/#token=…` URL. Native features reach the UI **through the server API**, never Tauri IPC, which keeps the PRD rule "frontend talks to the backend only via HTTP + WebSocket through ApiClient". The server gains a `NativeHost` trait (folder picker, open URL) that the desktop implements and web mode leaves absent (endpoints return 501). Secrets move behind a core `SecretStore` trait with an OS-keychain implementation (`keyring`), used by both web and desktop servers. The desktop binary also acts as the CLI when invoked as `plv` (one binary, no duplicated size).

**Tech Stack:** Phase 1–3 stack + `tauri` 2, `tauri-build`, `tauri-plugin-dialog`, `tauri-plugin-opener`, `tauri-plugin-single-instance`, `keyring` (v3, platform-native features), Tauri CLI (`@tauri-apps/cli` as a dev dependency in `crates/desktop/`).

**Spec:** `docs/PRD.md` (Phase 4, §8 Q3). Earlier plans in `docs/superpowers/plans/`.

## Decisions (made by the plan; revisit if wrong)

| ID | Decision |
|---|---|
| P4-1 | Native features go through the server API (`NativeHost`), not Tauri IPC. The webview page needs no Tauri capabilities; no `remote.urls` permission is granted. |
| P4-2 | Keychain is used by `plv serve` and the desktop app. `plv run` keeps `--env-var` (CI has no keychain) and gains `--use-keychain` to read stored secrets locally. |
| P4-3 | Keychain entry: service `dev.plover`, account `<collection-root-sha256-16>/<env>/<name>`. Moving a collection folder orphans its secrets (documented). Plover does not read Bruno's stored secrets (PRD §8 Q3: accepted). |
| P4-4 | If the keychain is unavailable (headless Linux without Secret Service, CI), the server falls back to in-memory secrets and the UI says "memory only". Never fall back to disk. |
| P4-5 | One binary: the desktop executable runs the CLI when argv[0]'s file name is `plv` or `plv.exe`, or when the first argument is `run`/`serve`. "Install Command Line Tool" creates that `plv` link. |
| P4-6 | Bundles are unsigned in this phase (macOS Gatekeeper / Windows SmartScreen warnings). Signing and notarization are a documented gap. |
| P4-7 | CI builds and tests on macOS (arm64), Windows (x64), Linux (x64, Ubuntu 22.04 + `libwebkit2gtk-4.1-dev`). The agent writes the workflow but **does not push**; cross-platform acceptance happens when the user pushes. |
| P4-8 | Node stays 22.20.0 (pinned since Phase 2; PRD §8 Q1 resolved). |

## Global Constraints

- Security rules from Phase 2 still hold: loopback bind, token on every API/WS call, Origin/Host allow-list. The desktop window's origin is `http://127.0.0.1:<port>`, already allowed. "Open in Browser" uses the same URL and token.
- Secrets never touch disk outside the OS keychain; no secret values in logs, menus, or crash output.
- No new outbound network at runtime (no auto-updater in this phase).
- Server shutdown is graceful on window close / Quit: in-flight runs are cancelled, the port is released, and the process exits within 3 s.
- Single instance: a second launch focuses the existing window (and opens the collection path if one was passed).
- Gate per task: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, frontend `typecheck/lint/test/check:no-cdn`; `check:console` where UI changed. Build the desktop crate with `cargo tauri build` only in Tasks 5–8 (it is slow).
- Commits: Conventional Commits; never push.

## Review Focus

1. **Keychain unavailable or denied** (user clicks "Deny" on the macOS prompt; Linux without Secret Service) → secrets still work in memory, UI says so, no crash, no disk write. → Task 1 `keychain_failure_falls_back_to_memory`.
2. **Port or server start failure** (server can't bind) → the app shows a native error dialog with the reason and exits non-zero instead of a blank window. → Task 5 `start_failure_shows_dialog` (unit-test the decision function; manual check).
3. **`plv` link already exists / not writable** (`/usr/local/bin` missing or root-owned) → "Install Command Line Tool" falls back to `~/.local/bin` and tells the user if it isn't on `PATH`; never overwrites a non-Plover file. → Task 6 `install_link_rules`.
4. **Quit during a running collection** → run cancelled, no orphaned requests, process exits ≤ 3 s. → Task 5 `quit_cancels_runs`.
5. **Second launch with a path argument** → existing window opens that collection; no second server. → Task 5 `single_instance_forwards_path`.

---

### Task 1: `SecretStore` in core + keychain implementation

**Files:**
- Create: `crates/core/src/secrets.rs`
- Modify: `crates/core/src/workspace.rs` (secrets come from the store), `crates/core/Cargo.toml` (`keyring` with `apple-native`, `windows-native`, `sync-secret-service`, `crypto-rust`; dev: mock builder)
- Test: `crates/core/tests/secrets.rs`

**Interfaces:**
- Produces:
  ```rust
  pub trait SecretStore: Send + Sync {
      fn get(&self, key: &SecretKey) -> Result<Option<String>, SecretError>;
      fn set(&self, key: &SecretKey, value: &str) -> Result<(), SecretError>;
      fn delete(&self, key: &SecretKey) -> Result<(), SecretError>;
      fn kind(&self) -> SecretBackend;                 // Keychain | Memory
  }
  pub struct SecretKey { pub collection: CollectionId, pub env: String, pub name: String }
  pub enum SecretBackend { Keychain, Memory }
  pub struct KeyringStore;                           // service "dev.plover", account "<id>/<env>/<name>"
  pub struct MemoryStore { /* Mutex<HashMap> */ }
  pub struct FallbackStore { primary: Box<dyn SecretStore>, memory: MemoryStore } // switches to memory on the first platform error, logs once (no values)
  pub fn default_store() -> Arc<dyn SecretStore>;   // FallbackStore(KeyringStore, MemoryStore)
  ```
- `Workspace::new` takes `Arc<dyn SecretStore>`. Secret names come from `vars:secret` in the env file; values from the store. Existing merge semantics (`null` clears) map to `delete`.

- [ ] **Step 1: Failing tests** (keyring mock credential builder for keychain behaviour; a failing fake for fallback): `set_get_delete_roundtrip`, `keys_isolated_per_collection_and_env`, `keychain_failure_falls_back_to_memory` (fake returns `PlatformFailure` → values still readable, `kind() == Memory`), `secrets_set_lists_names_only`, `send_uses_stored_secret`.
- [ ] **Step 2–4:** FAIL → implement → PASS.
- [ ] **Step 5: Commit** `feat(core): secret store with OS keychain and memory fallback`

### Task 2: Server `NativeHost` + capabilities endpoint

**Files:**
- Create: `crates/server/src/native.rs`, `crates/server/src/api/native.rs`
- Modify: `crates/server/src/lib.rs` (`ServerConfig` gains `native: Option<Arc<dyn NativeHost>>`, `secrets: Arc<dyn SecretStore>`), DTOs + regenerated `frontend/src/api/types.ts`
- Test: `crates/server/tests/native.rs`

**Interfaces:**
- Produces:
  ```rust
  #[async_trait] pub trait NativeHost: Send + Sync {
      async fn pick_folder(&self, title: &str) -> Option<PathBuf>;   // None = cancelled
      async fn open_external(&self, url: &str) -> Result<(), String>;
  }
  ```
  Routes: `GET /api/capabilities` → `CapabilitiesDto { desktop: bool, nativePicker: bool, openExternal: bool, secretBackend: "keychain" | "memory" }`; `POST /api/native/pick-folder` → `{ path: string | null }` (501 `not_supported` without a host); `POST /api/native/open-browser` → 204 (opens the session URL incl. token; 501 without a host). Picking a folder does **not** open it; the UI then calls `POST /collections`.
- [ ] **Step 1: Failing tests:** `capabilities_web_mode`, `capabilities_desktop_mode` (fake host), `pick_folder_501_without_host`, `pick_folder_returns_path_or_null`, `native_routes_require_token`.
- [ ] **Step 2–4:** FAIL → implement → PASS.
- [ ] **Step 5: Commit** `feat(server): native host hooks and capabilities endpoint`

### Task 3: Frontend — Browse, keychain status, open in browser

**Files:**
- Modify: `frontend/src/api/client.ts` (`capabilities()`, `pickFolder()`, `openInBrowser()`), `frontend/src/components/OpenCollectionDialog.tsx` (or sidebar open form), `frontend/src/components/EnvEditor.tsx`, `frontend/src/store/workspace.ts`
- Test: `frontend/src/api/client.test.ts`, `frontend/src/components/*.test.tsx`

- UI rules: when `nativePicker` is true show **Browse…** next to the path field (cancel = no change); the path field stays for web mode. EnvEditor secrets header shows "Stored in OS keychain" or "Memory only — values are lost when Plover quits". When `desktop` is true, a toolbar item "Open in browser".
- [ ] **Step 1: Failing tests:** client calls for the three endpoints; Browse button only when capable; cancel keeps path; status text per backend.
- [ ] **Step 2–4:** FAIL → implement → PASS (incl. `check:console`).
- [ ] **Step 5: Commit** `feat(frontend): native folder picker, keychain status, open in browser`

### Task 4: Release profile, size budget, CLI-only build

**Files:**
- Modify: root `Cargo.toml` (`[profile.release]`), `crates/server/Cargo.toml` + `crates/cli/Cargo.toml` (feature `embed-ui`, default on), `crates/server/src/assets.rs` (no-UI build serves the "UI not built" page and the API), `scripts/build-release.sh`, `README.md`
- Test: `crates/cli/tests/cli.rs` (`--no-default-features` build still runs `plv run`)

- Measure each candidate on `plv` (release, macOS arm64) and pick the smallest that keeps the scripted benchmark within 10 % of today (`…/scratchpad/bench`, 1000 requests with scripts, baseline 2.39 s): `strip = true`; `lto = "thin"` vs `"fat"`; `codegen-units = 1`; `opt-level = 3` vs `"s"`. Keep `panic = "unwind"` (script workers rely on unwinding). Record a table (size, stripped size, gzip size, benchmark wall time) in `docs/development.md`.
- [ ] **Step 1:** measure baseline; **Step 2:** apply and measure candidates; **Step 3:** commit chosen profile + feature `build: size-tuned release profile and CLI-only build`.

### Task 5: Tauri desktop shell

**Files:**
- Create: `crates/desktop/Cargo.toml` (package `plover-desktop`, bin `plover`), `crates/desktop/build.rs`, `crates/desktop/tauri.conf.json` (productName `Plover`, identifier `dev.plover.app`, no `frontendDist` dev server; window loads the server URL at runtime), `crates/desktop/capabilities/default.json` (empty permission set for the remote page), `crates/desktop/icons/` (generated by `tauri icon` from `crates/desktop/icon.svg` — a simple placeholder plover glyph), `crates/desktop/src/main.rs`, `crates/desktop/src/host.rs` (`NativeHost` via `tauri-plugin-dialog` + `tauri-plugin-opener`), `crates/desktop/src/menu.rs`, `crates/desktop/src/lifecycle.rs`, `crates/desktop/package.json` (`@tauri-apps/cli`)
- Modify: root `Cargo.toml` members
- Test: `crates/desktop/src/lifecycle.rs` unit tests

**Interfaces / rules:**
- Startup: build `ServerConfig { port: 0, native: Some(host), secrets: default_store(), … }`, `start()`, then create the main window with `WebviewUrl::External(url)`. If `start` fails → native error dialog ("Plover could not start its local server: <reason>") and exit code 1.
- Menu (macOS app menu + File menu; Windows/Linux File menu): Open Collection… (`Cmd/Ctrl+O`, native picker → `workspace.open`), Open in Browser, Reload Window (`Cmd/Ctrl+R`), Install Command Line Tool… (Task 6), Quit (`Cmd/Ctrl+Q`).
- Lifecycle: on Quit/last window close → trigger the server shutdown future, cancel runs, wait ≤ 3 s, exit. `tauri-plugin-single-instance`: second launch focuses the window and forwards a path argument.
- Smoke mode: env `PLOVER_DESKTOP_SMOKE=1` starts the server, opens the window, waits until `/api/health` answers through the webview's origin rules, then exits 0 (for CI).
- [ ] **Step 1: Failing tests:** `start_failure_shows_dialog` (pure decision fn), `quit_cancels_runs` (lifecycle fn with a fake run), `single_instance_forwards_path` (argument parsing fn).
- [ ] **Step 2–4:** FAIL → implement → PASS; `cargo tauri build --bundles app,dmg` on macOS succeeds; `PLOVER_DESKTOP_SMOKE=1 …/Plover.app/Contents/MacOS/plover` exits 0.
- [ ] **Step 5: Commit** `feat(desktop): Tauri 2 shell around the embedded server`

### Task 6: One binary — `plv` from the app

**Files:**
- Create: `crates/desktop/src/cli_link.rs`
- Modify: `crates/desktop/src/main.rs` (dispatch), `crates/cli/src/lib.rs` (expose `pub fn main_with_args(args) -> ExitCode` so desktop reuses the CLI; `crates/cli/src/main.rs` becomes a 3-line wrapper), `crates/desktop/tauri.conf.json` (Linux `.deb`: `/usr/bin/plv` symlink; Windows NSIS: option to add install dir to `PATH` if Tauri supports it, else documented gap)
- Test: `crates/desktop/src/cli_link.rs` unit tests, `crates/cli/tests/cli.rs`

- Dispatch rule (P4-5). "Install Command Line Tool…" (macOS/Linux): target `/usr/local/bin/plv` if the directory exists and is writable, else `~/.local/bin/plv` (create dir); if a file exists there and is not a link to this app's executable → refuse with a dialog naming the file; on success show where it was installed and whether that dir is on `PATH`. No privilege escalation prompts in this phase.
- [ ] **Step 1: Failing tests:** `dispatch_rules` (argv[0] `plv`, `plv.exe`, first arg `run`/`serve`, otherwise GUI); `install_link_rules` (writable target, fallback, existing foreign file refused, existing own link replaced, PATH hint).
- [ ] **Step 2–4:** FAIL → implement → PASS; manual: link created in a temp `HOME`, `plv --version` through the link prints `plv 0.1.0`.
- [ ] **Step 5: Commit** `feat(desktop): plv CLI inside the app and command line tool installer`

### Task 7: CI workflow (written, not pushed)

**Files:**
- Create: `.github/workflows/ci.yml` (matrix `macos-14`, `windows-latest`, `ubuntu-22.04`: Node 22.20.0, Rust stable, Linux deps `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev`; steps: frontend `npm ci` + typecheck/lint/test/build + check:no-cdn, `crates/core/js` `npm ci && npm run check`, `cargo test --workspace`, `cargo clippy … -D warnings`, `cargo tauri build` for that OS, `PLOVER_DESKTOP_SMOKE=1` run under `xvfb-run` on Linux, upload bundles as artifacts), `.github/workflows/README.md` (what runs where)
- Test: validate YAML with `actionlint` if installed; otherwise a YAML parse check.

- [ ] **Step 1:** write workflow; **Step 2:** lint; **Step 3:** commit `ci: build, test and bundle on macOS, Windows and Linux`.

### Task 8: Acceptance on macOS + size report

- [ ] **Step 1:** Fresh `scripts/build-release.sh` + `cargo tauri build` → record sizes: `plv` (with UI, CLI-only), `Plover.app` (du), `.dmg`.
- [ ] **Step 2: Manual run** (binary inside the `.app`, launched directly — `open` is blocked by the sandbox): open the fixture collection via File → Open Collection… (if the native dialog cannot be driven, call `/api/native/pick-folder` path through a test host and note it), send a request, set a secret, quit, relaunch → secret still set (keychain) and `security find-generic-password -s dev.plover` lists the entry without printing the value (`-g` not used). Open in Browser → note the URL opened (blocked by sandbox is acceptable; record the attempted URL without the token). Screenshots with `screencapture` if permitted.
- [ ] **Step 3:** Full gate; commit `docs: phase 4 report` with `docs/phase-4-report.md`: sizes table, what was verified on macOS, what awaits CI on Windows/Linux, gaps.

---

## Phase 4 exit checklist

- [ ] macOS: `.app` + `.dmg` build, smoke passes, keychain round-trip, native picker, open in browser, CLI link (Task 8).
- [ ] Windows/Linux: workflow ready; results pending the user's push.
- [ ] Size table and gaps in `docs/phase-4-report.md`; **stop**.

## Known gaps accepted for Phase 4

Code signing / notarization, auto-update, Windows `PATH` integration if NSIS hooks are insufficient, menu localisation, a real app icon/brand, reading Bruno's stored secrets.
