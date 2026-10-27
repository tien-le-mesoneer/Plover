# Follow-up gaps: report

**Tóm tắt (tiếng Việt)**

- **Đã làm xong 10 task** của plan `docs/superpowers/plans/2026-10-24-follow-up-gaps.md` (Task 0–9), trên `main`, chưa push.
- **Import HAR từ menu chuột phải:** chuột phải (hoặc Shift+F10) vào folder hay tên collection mở menu *Run folder / Import HAR…*; dialog chọn sẵn folder đó. Nút ▶ giữ nguyên.
- **Token trong body XML/text:** khi import, `<wsse:Password>`, thuộc tính `password="…"`, CDATA và JWT trong text đều thành biến secret (`{{password}}`, `{{password2}}`, `{{bodyToken}}`). Gửi lại thì body giống hệt từng byte với bản capture (đã kiểm tra với mock).
- **Che credential mạnh hơn:** che theo tên (JSON, XML, form, query, cột data) bất kể độ dài hay kiểu số; header `X-Api-Key` và mọi JWT cũng bị che. Số và chuỗi ngắn hơn 4 ký tự chỉ bị che khi đứng cạnh tên của nó.
- **Danh sách biến runtime:** nút `{x}` cạnh environment; secret hiện `••••`, xoá từng biến, Clear all có hỏi trước.
- **Polish:** nút Send/Cancel cùng một ô rộng cố định nên ô URL không co lại (842 px trước và trong lúc gửi); Esc dừng run đang chạy trong tab Run.
- **Kiểm tra:** 588 test Rust, 321 test frontend, clippy, fmt, build, kiểm tra CDN và console đều qua.

## Commits

| Commit | Task |
|---|---|
| `1269eb7` | 0. Interfaces re-checked on `386dbe9`; plan note |
| `f609a95` | 1. `credentials::body_credential_spans` and `jwt_spans` |
| `5c1a8d5` | 2. HAR import moves XML/text body credentials into secrets |
| `1304b1c` | 3. Masker by-name pass, token headers, JWTs; run-history spec amended |
| `fecac23` | 4. Data-file credentials masked by name in rows |
| `e67e831` | 5. `GET/DELETE …/runtime-vars`, `DELETE …/runtime-vars/{name}` |
| `e0e3b3b` | 6. Runtime-variable panel |
| `c55da12` | 7. Folder and collection context menu with Import HAR… |
| `2be22a0` | 8. Send/Cancel slot; Esc stops a run |
| `a6b056d` | Live-check fixes: panel takes focus on open, context-menu item style, singular confirm text |
| this commit | 9. README and this report |

## Gate

Run before every commit with `set -o pipefail`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, frontend `typecheck`, `lint`, `test`, `build`, `check:no-cdn`, a release `plv` build and `check:console`. Last run: 588 Rust tests passed, 0 failed (554 before this plan); 321 frontend tests passed (305 before).

## Live check

Release `plv serve` with a temporary `PLOVER_STATE_DIR`, `PLOVER_KEYCHAIN_SERVICE=dev.plover.test`, a copy of `fixtures/phase3-chain` (plus a `slow/` folder and a data-driven `data/echo.bru`) and a local mock. Driven with agent-browser in dark and light.

1. **Folder menu → Import HAR…** (Shift+F10 on `data`): the dialog opened with *Import into* = `data`. A synthetic SOAP HAR (three entries: two `wsse:Password`, a repeated value, CDATA, an attribute with `&amp;`, a `<secret>` element) planned the secrets `password, password2, password3, password4, secret`. The `.bru` files hold only `{{…}}`; no value was written to the collection or the state dir. Sending the three imported requests: the mock received **byte-identical** bodies for all three.
2. **Data run** (`plv run data/echo.bru --json-file-path rows.json`, row `{"user":"ana","password":1234,"apiKey":"key-LIVE-7777"}`): `--verbose` printed `"password": "***"` and `"apiKey": "***"`. The stored record has `X-Api-Key: ***`, URL `?api_key=***&page=2`, request body `"password": "***"`, the response's JWT `"session": "***"`, and `"count": 1234` kept. The JSON export and the state dir contain none of the key, the JWT or `"password": 1234`.
3. **Runtime variables** after login: `token` shown as `••••`; `docId` (set via the API) shown in clear, removed with its row button; Clear all asked "Clear 1 runtime variable?" and emptied the list. `/vars` then reported `token` as set later by `auth/login.bru` again (what the hover shows).
4. **Slow send:** the URL field was 842 px wide before, during and after; Esc cancelled it (mock: client closed after 1.98 s).
5. **Slow run:** started from the folder menu, Esc (focus on the folder row) → the Run tab shows *cancelled*; mock: client closed after 1.19 s.

Leak search of the state dir and the export for every fixture value: no match. Temp state, the collection copy and the mock were deleted; no `dev.plover.test` keychain item is left.

Screenshots (looked at each): `/private/tmp/claude-501/-Users-tienle-projects-rea/1bb5dcfd-43a5-447b-a541-d24ed1b9cd89/scratchpad/follow-up-shots/` — folder menu (dark, light), Import HAR dialog (dark), runtime variables (dark, light), Clear-all confirm (dark), Cancel running (dark, light), run running (dark), run cancelled (dark, light).

## Deviations from the plan

1. Review Focus 3 and Task 9 used a `pin` column, but `pin` is not in `CREDENTIAL_FIELDS` and the plan forbids new name lists; the tests and the live check use a numeric `password` instead (`pin` stays visible, as designed).
2. Numeric JSON credentials stay as written on HAR import (an existing test pins `"token": 7`), and such a row is not flagged as having secrets; strings, XML and JWTs move to secrets.
3. The Import HAR dialog's row tooltip already said "Has credentials; they become secret variables", so it was not changed.
4. `saved_note_then_panel_shows_var` saves through `api().setRuntimeVar` rather than driving the response value menu; the panel always refetches on open.
5. The clear-during-run test calls `OpenCollection::clear_runtime` while the run waits, not the HTTP route (the route is a one-line call to it and has its own tests).

## Known gaps

1. A numeric or 1–3 character credential that appears away from its name stays visible, e.g. inside a JSON-escaped echo string (`"echo": "{\"password\": 1234}"` in the live check), a URL path or a `console.log`. This is the accepted trade-off.
2. **Keychain prompt:** after rebuilding the release binary mid-check, reading the test-service keychain items made `GET …/environments` wait (macOS asks the new binary for keychain access); the check deleted those items and continued. Real users hit this only when the app binary changes.
3. The collection header's context menu opens only with the mouse (the header is not focusable); folders also have Shift+F10 and the ContextMenu key.
4. A cancelled run's unfinished row still shows `…` in the Run tab (existing behaviour).
5. Windows and Linux CI have not run these commits, and the desktop app was not opened.
