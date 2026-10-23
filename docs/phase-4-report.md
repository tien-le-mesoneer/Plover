# Phase 4 report — desktop (2026-10-23)

## Tóm tắt

- Đã xong 8 task Phase 4 trên `main` (chưa push). Gate pass: 268 test Rust, 54 test frontend, clippy sạch.
- macOS: build được `Plover.app` (16.3 MB) và `.dmg` (6.3 MB), smoke test pass, keychain thật lưu/đọc/xoá được qua lần khởi động lại.
- `plv` nhỏ lại từ 21.3 MB → 12.7 MB (chậm hơn ~6%); bản CLI-only 7.6 MB. App tự chạy như CLI khi gọi là `plv`.
- Chưa kiểm chứng: hộp thoại chọn thư mục, các menu, single-instance và ảnh chụp cửa sổ (sandbox không cho điều khiển UI native/chụp màn hình).
- Windows/Linux chỉ kiểm chứng được qua CI (`.github/workflows/ci.yml`) sau khi bạn push.

Plan: `docs/superpowers/plans/2026-10-23-phase-4-desktop.md`. Not pushed.

## Tasks

| Task | Commit | Tests added |
|---|---|---|
| plan doc | 33ae986 | – |
| 1 secret store: OS keychain + memory fallback, `plv run --use-keychain` | 060b1b3 | 7 (core `tests/secrets.rs`) |
| 2 server `NativeHost` + `/api/capabilities`, native routes | 914846d | 6 (server `tests/native.rs`) |
| 3 frontend: Browse…, keychain status, Open in browser | c9f0a5d | 6 (1 client + 5 component) |
| 4 release profile, `embed-ui` feature, CLI-only build | 5c1041a | 1 (runs only in the CLI-only build; in the gate) |
| 5 Tauri 2 shell (`plover-desktop`) | 8a32cad | 3 (lifecycle) |
| 6 one binary: `plv` dispatch + Install Command Line Tool | 6ff86b2 | 3 (dispatch, link rules, candidates) |
| 7 CI workflow (written, not run) | 83449c5 | YAML parse check only |
| 8 acceptance + this report | (this commit) | – |

**Gate** (final, every commit after the Task 2 amend went through it):
`rust passed 268 failed 0` · CLI-only `cargo test -p plover-cli --no-default-features --test cli` ok · `clippy -D warnings` clean · frontend typecheck/lint ok, `Tests 54 passed (54)`, build ok · `no CDN references in 10 files` · `check:console`: `no page errors` (Task 3) · `dist/ is up to date (9 libraries)`.

## Sizes (macOS arm64, release)

| Artifact | Before | After |
|---|---|---|
| `plv` with web UI | 21.3 MB (18.0 MB stripped) | **12.7 MB** |
| `plv` CLI-only (`--no-default-features`) | – | **7.6 MB** |
| `Plover.app` (du) | – | **16.3 MB** (binary 16.2 MB) |
| `Plover_0.1.0_aarch64.dmg` | – | **6.3 MB** |

Profile: `strip`, fat LTO, `codegen-units = 1`, `opt-level = "s"`, `panic = "unwind"`. Full candidate table in `docs/development.md`.

**Benchmark** (1000 requests, each with a post-response script and a chai test; 4 interleaved runs): default profile 2.16 s wall / 1.97 s CPU → chosen 2.28 s / 2.11 s (+5.6% / +7%). `opt-level = "z"` (11.3 MB) was +39% and rejected. Wall times on this machine vary ±20% between rounds.

## macOS verification (this machine)

Verified:
- `tauri build --bundles app,dmg` succeeds; the bundled binary passes `PLOVER_DESKTOP_SMOKE=1` in about 3 s (window created, server answers an authorised request with the window's Host).
- The app binary is the CLI: `plover run …` dispatches to `plv`, as does `PLOVER_AS_CLI=1 plover --version` → `plv 0.1.0`, and a `plv` symlink in a temp `~/.local/bin` → `plv 0.1.0`. The phase-3 chain through the app's CLI against the local mock: `Requests: 3 passed · Tests: 5/5 · Asserts: 2/2`.
- Real macOS Keychain round-trip through `plv serve` (same server and secret store the app uses), under the test service `dev.plover.p4test`: `/api/capabilities` reports `secretBackend: keychain`; a secret set via the API survives a restart (`secretsSet: ['token']`); `security find-generic-password` (without `-g`) finds account `<collection-id>/kc/token`; the value does not appear in any collection file; clearing it removes the entry. No keychain prompts appeared. No test entries are left.
- SIGTERM ends the app promptly.

Not verified (no way to drive native UI in this sandbox):
- Native folder picker, every menu action (Open Collection…, Open in Browser, Reload, Install Command Line Tool… dialogs, Quit and its 3 s deadline), single-instance forwarding, the Browse…/Open in browser buttons inside the window. These paths are covered only by unit/API tests with a fake host.
- What the window looks like: `screencapture` fails with "could not create image from display" (no screen-recording permission).
- Gatekeeper behaviour when opening the unsigned `.dmg` on another Mac.

## Awaiting CI (Windows, Linux)

`.github/workflows/ci.yml` (macos-14, windows-latest, ubuntu-22.04) runs every check, builds the bundles (dmg / NSIS / deb + AppImage), runs the smoke test (Linux under `xvfb-run`) and uploads artifacts. Nothing has run yet; it starts on your next push. `actionlint` is not installed here, so the workflow was only parse-checked. Likely first-run risks: Windows path assumptions in existing tests, AppImage tooling on the runner, Linux Secret Service absence (handled by the memory fallback).

## Deviations from the plan

1. `ServerConfig.secrets` / `.native` are `Option` (None = memory / web mode) and `ServerConfig` derives `Default`; `AppState` gains `with_secrets`, `with_native`, `with_session_url`. `Workspace::new` keeps a memory store; `Workspace::with_secrets` takes any `SecretStore`.
2. `NativeHost` uses boxed futures (`HostFuture`) instead of `#[async_trait]`: no new dependency.
3. keyring's mock store does not persist across entries, so tests use a persistent in-test `CredentialBuilder` through `KeyringStore::with_builder`.
4. Only `PlatformFailure` / `NoStorageAccess` switch to memory; other keychain errors surface as API 500 `secret_store_error`. Secret names come from `vars:secret` plus names set in the session. `PLOVER_KEYCHAIN_SERVICE` overrides the service name (testing aid; CI sets it).
5. `quit_cancels_runs` proves the 3 s deadline with a server that never finishes; in-flight runs end because the process exits. There is no explicit run-cancellation registry.
6. The window is created in code (no `frontendDist`, no config windows); `capabilities/default.json` grants nothing. An Edit menu was added (macOS needs it for copy/paste in the webview).
7. The start-failure dialog is shown non-blocking and exits 1 when dismissed (a blocking dialog on the main thread would deadlock).
8. CLI dispatch also honours `PLOVER_AS_CLI=1`, used by the `.deb` wrapper `/usr/bin/plv` (a copied file can't be a symlink). Windows release builds attach the parent console in CLI mode (compiled only in CI).
9. "Install Command Line Tool…" is on macOS and Linux only; Windows has no CLI-install path yet.
10. Process slips: the Task 2 commit first landed with a failing gate (piping into `tail` hid the exit code); it was fixed and amended before any later commit, and every later commit went through a wrapper that commits only on a passing gate. The Task 5 lifecycle tests were written together with their code, so I never saw them fail first.

## Gaps

1. **Collections aren't remembered** between app launches: you reopen them each time (or pass a path).
2. **Unsigned bundles**: Gatekeeper/SmartScreen warnings; no notarization. Unsigned binaries may also trigger keychain access prompts after an app update (unverified).
3. **Windows**: no `plv` install / `PATH` integration; Windows behaviour overall is unverified until CI runs.
4. **Native UI paths unverified** on any OS (picker, menus, single instance, Quit deadline).
5. Opening a folder via Finder "Open With" or by dropping it on the Dock icon is not handled; there is no auto-update, no real brand icon (placeholder from `crates/desktop/icon.svg`), and no menu localisation.
6. Secrets are keyed by the collection's absolute path, so moving a collection orphans them. Bruno's stored secrets are not read (accepted, P4-3).
