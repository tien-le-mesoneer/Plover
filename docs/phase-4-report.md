# Phase 4 report — desktop (2026-10-23)

## Tóm tắt

- Phase 4 và cả 13 mục review vòng 2 (11 lỗi + 2 nit) đã xong trên `main`, chưa push. Gate pass: 283 test Rust, 65 test frontend, clippy sạch, kiểm tra console sạch.
- Đã sửa 12/13 mục. Mục 11 (socket single-instance ở `/tmp` trên macOS) ghi thành gap: plugin không cho đổi đường dẫn.
- Secret giờ ghi nhận nơi lưu theo từng secret: keychain từ chối thì chỉ secret đó vào RAM, kèm cảnh báo và badge trên UI. Giá trị dài được chia nhỏ để lưu trên Windows.
- URL app tự mở không còn chứa token (dùng mã một lần). Mở collection bằng menu không còn reload trang. Quit huỷ thật các run đang chạy.
- Windows/Linux chỉ kiểm chứng qua CI sau khi bạn push. Bản checkout `autocrlf=true` đã được chứng minh pass trên macOS.

Plan: `docs/superpowers/plans/2026-10-23-phase-4-desktop.md`. Not pushed.

## Tasks

| Task | Commit | Tests added |
|---|---|---|
| plan doc | 33ae986 | – |
| 1 secret store, `plv run --use-keychain` | 060b1b3 (reworked in 914a2e9) | 7 → 12 |
| 2 server `NativeHost` + `/api/capabilities` | 914846d | 6 |
| 3 frontend: Browse…, keychain status, Open in browser | c9f0a5d | 6 |
| 4 release profile, `embed-ui`, CLI-only build | 5c1041a | 1 |
| 5 Tauri 2 shell | 8a32cad | 3 |
| 6 one binary: `plv` dispatch + CLI installer | 6ff86b2 | 3 |
| 7 CI workflow (written, not run) | 83449c5 | YAML parse only |
| 8 acceptance + report | 7b74ffd | – |
| review round 2 (below) | 77dea74, 914a2e9, 3125abe, this commit | see below |

**Gate (final):** `rust passed 283 failed 0` · CLI-only tests ok · `clippy -D warnings` clean · typecheck/lint ok · `Tests 65 passed (65)` · `no CDN references in 10 files` · `check:console`: 2 editors, JSON diagnostics shown, `no page errors` · `dist/ is up to date (9 libraries)`.

## Review round 2 — findings

| # | Finding | Status | Commit |
|---|---|---|---|
| 1 | Windows `autocrlf=true` breaks bundle check + 6 byte-comparing tests; path test assumes `/` | Fixed: `.gitattributes` (`* text=auto eol=lf`, images binary); renormalize was a no-op (no CR in the repo; the CRLF tests use inline strings); the path test compares `/`-joined components. Proof: fresh `git -c core.autocrlf=true clone --local` → 0 CR files, `node check.mjs` up to date, `cargo test --workspace` 268/268 (at that commit). The separator fix itself is only provable on Windows (CI). | 77dea74 |
| 2 | UI said "keychain" while secrets went to memory | Fixed: per-secret backend; `secretsSet` is `[{name, backend}]`; the editor badges each secret, warns when any is memory-only, and refreshes capabilities after every save | 914a2e9 |
| 3 | One keychain error moved the whole process to memory; Clear left the keychain value | Fixed: `SecretVault` probes once at startup (unavailable → memory only, reported up front); a refused value stays in memory for that secret only, with a warning; reads check memory first; Clear deletes from both and reports a keychain failure. Tests cover -128 / -25293 / -25308, existing entries and clear semantics. | 914a2e9 |
| 4 | Windows 1280-unit limit → 500 | Fixed: values over the limit are split into `<account>#n` chunks behind a header entry (4 KB PEM test with a 1280-unit fake); if chunking fails, that value stays in memory with a warning, leaving no partial chunks | 914a2e9 |
| 5 | Open Collection / second launch reloaded the window (drafts lost) | Fixed: `GET /api/events` WebSocket; `collection_opened` adds and selects the collection without a reload; Reload Window sends `reload_requested` and the page asks before discarding unsaved edits (plus `beforeunload` in browser tabs) | 3125abe |
| 6 | CLI link pointed at the AppImage mount / translocated app | Fixed: links `$APPIMAGE`; refuses `/AppTranslocation/` and `/Volumes/…` with "Move Plover to Applications first" | 3125abe |
| 7 | Non-UTF-8 argument aborted | Fixed: `args_os` everywhere (the plugin already used it) | 3125abe |
| 8 | `--version`/`-h` opened the GUI | Fixed: `--version`, `-V`, `--help`, `-h` dispatch to the CLI before single-instance | 3125abe |
| 9 | Smoke/lifecycle tests could pass vacuously | Fixed: smoke waits for `POST /api/client-ready` from the mounted page (which first exchanged its launch code) and skips single-instance. The server runs on its own runtime and Quit shuts it down: `quit_cancels_runs` drives a real WebSocket run against a localhost mock and asserts nothing is sent after Quit (fails when the shutdown is removed — mutation-checked). The start-failure test binds a taken port. Finding: axum's graceful shutdown never covered upgraded WebSockets, so the old design let runs continue until exit. | 3125abe |
| 10 | Open in Browser put the token in the URL/history | Fixed: single-use launch codes (`#code=`, 60 s, constant-time compare, 20 failures/min → 429) exchanged via `POST /api/session/exchange`; used by the window, Open in Browser and `plv serve`'s auto-open. The URL `plv serve` prints keeps `#token=` (documented). | 3125abe |
| 11 | macOS single-instance socket is a fixed shared `/tmp` path | **Gap**: `tauri-plugin-single-instance` 2.5.4 derives `/tmp/<identifier>_si.sock` in `socket_path()` with no option; a per-user path would mean replacing the plugin with our own IPC. Smoke mode no longer uses single-instance. | – |
| nit | `Cargo.toml` said "within 3%" | Fixed: states +5.6% wall / +7% CPU | 3125abe |
| nit | `--use-keychain` without `--env` ignored | Fixed: clap `requires = "env"` → usage error, exit 2 | 3125abe |

## Sizes (macOS arm64, release)

| Artifact | Before Phase 4 | Now |
|---|---|---|
| `plv` with web UI | 21.3 MB | 12.8 MB |
| `plv` CLI-only | – | 7.6 MB |
| `Plover.app` (du) | – | 16.4 MB |
| `.dmg` | – | 6.3 MB |

Benchmark (1000 scripted requests, 4 interleaved runs): default profile 2.16 s / 1.97 s CPU → chosen profile 2.28 s / 2.11 s. Not re-measured after round 2; the changes are outside the request path.

## macOS verification (this machine)

Verified:
- `tauri build --bundles app,dmg`; the bundled binary passes `PLOVER_DESKTOP_SMOKE=1`, which proves the page loaded, exchanged its launch code, reported ready, and the server answered an authorised request.
- The app binary acts as `plv` (`run`, `--version`, `-h`, a `plv` link, `PLOVER_AS_CLI=1`). The phase-3 chain passes through it (3/3, 5/5, 2/2).
- Real Keychain round-trip through `plv serve` with test service `dev.plover.p4test`: no warnings, backend `keychain`, survives a restart, not in collection files, removed on clear, no leftover entries.
- The CRLF-clone proof described in finding 1.

Not verified (no native UI automation or screen capture here): the folder picker, menu actions, the reload confirmation inside the window, single-instance forwarding, the window's look, and Gatekeeper on another Mac.

## Awaiting CI (Windows, Linux)

`.github/workflows/ci.yml` runs every check, builds the bundles, runs the smoke test (Linux under `xvfb-run`) and uploads artifacts on your next push. `actionlint` is not installed here.

## Deviations from the plan

1. `ServerConfig.secrets` / `.native` are `Option`; `AppState` has builder methods; the secret API is the concrete `SecretVault` over a `Keychain` trait (round 2 replaced `SecretStore`, `MemoryStore` and `FallbackStore`).
2. `NativeHost` uses boxed futures instead of `#[async_trait]`.
3. keyring's mock store doesn't persist across entries, so tests use a persistent in-test `CredentialBuilder` with programmable faults.
4. The server runs on its own Tokio runtime so Quit can cancel in-flight runs.
5. The window is created in code; capabilities grant nothing; an Edit menu was added for macOS copy/paste.
6. CLI dispatch also honours `PLOVER_AS_CLI=1` (used by the `.deb` wrapper); Windows attaches the parent console.
7. The CLI installer is on macOS and Linux only.
8. Process slips:
   - The first Task 2 commit landed with a failing gate, because a pipe hid the exit code. I amended it before any later commit, and every later commit went through the gate wrapper.
   - The first version of the Task 5 lifecycle tests was written together with its code.
   - Round 2, item by item:
     - Item 1 relied on the reviewer's failing log plus the clone proof after the fix.
     - The tests for items 2–5, 9 and 10 first failed to compile against the new API; the frontend tests for items 2, 5 and 10 failed at run time.
     - The tests for items 6–8 and the nit failed at run time against stubs.

## Gaps

1. Collections aren't remembered between app launches.
2. Unsigned bundles (Gatekeeper/SmartScreen warnings). Unsigned binaries may also trigger keychain prompts after an update (unverified).
3. Windows: no `plv` install or PATH integration, and Windows overall is unverified until CI runs.
4. The macOS single-instance socket is shared in `/tmp` (finding 11).
5. Native UI paths are unverified on any OS.
6. No Finder "Open With" or Dock-drop handling, no auto-update, the icon is a placeholder, and the menu isn't localised.
7. Secrets are keyed by the collection's absolute path, so moving a collection orphans them; Bruno's stored secrets are not read.
