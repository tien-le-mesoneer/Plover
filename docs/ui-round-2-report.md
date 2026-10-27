# UI round 2: report

**Tóm tắt (tiếng Việt)**

- **Đã làm xong Task 0–10** của plan `docs/superpowers/plans/2026-10-27-ui-round-2.md` trên `main`, chưa push.
- **Thu gọn collection:** bấm tên collection để gập cả cây; trạng thái được nhớ sau khi khởi động lại. Khi lọc, kết quả vẫn hiện trong collection đang gập. Shift+F10 trên tên mở menu như chuột phải.
- **Chia khung request/response:** kéo thanh giữa để đổi tỉ lệ, nút cuối thanh tab để chuyển cạnh nhau ↔ chồng dọc; cả hai được nhớ. Cửa sổ hẹp hơn 486 px tự chồng dọc mà không đổi lựa chọn đã lưu.
- **Copy giá trị:** thẻ biến có nút Copy value và phím `⌘/Ctrl+Shift+C`; param có nút copy từng dòng; body request có nút Copy ở góc. Secret luôn hỏi trước khi copy, server không gửi secret cho trang nếu chưa đồng ý.
- **Gợi ý biến khi gõ `{{`:** trong URL, param, header, auth, form và body (Monaco). Chỉ hiện tên và scope, không bao giờ hiện giá trị.
- **History:** 100 lần gửi gần nhất, che secret trước khi ghi file; lọc theo collection, xem lại, mở request, mở thành request mới, xoá. Đã grep thư mục state: không có giá trị secret nào.
- **Kiểm tra:** 615 test Rust, 379 test frontend, clippy, fmt, build, kiểm tra CDN và console đều qua.

## Commits

| Commit | Task |
|---|---|
| `d6e0d97` | 0. Interfaces verified on `789baad`; plan notes |
| `ad4477f` | 1. Fold a collection; keyboard menu on its name |
| `b907ba6` | 2. Resizable split with side-by-side / stacked toggle |
| `43260ce` | 3. `POST /collections/{id}/resolve-text` |
| `7e42742` | 4. Hover/focus-revealed header buttons, corner Copy/Save, request body copy |
| `090643d` | 5. Copy a variable's or a param's value; Save as variable leaves the clipboard alone |
| `0f05585` | 6. `{{` variable-name autocomplete (inputs and Monaco) |
| `5eaa28c` | 7. `SendHistory` store (core) |
| `d74327b` | 8. Record sends; `GET/DELETE /api/sends`, `GET /api/sends/{id}` |
| `770e7a8` | 9. History tab |
| `3272061` | Live-check fix: Monaco overflow widgets fixed, so the body list is not cut off |
| `381d44b` | Live-check fix: app `.main` rule renamed `.app-main` (it hid the names in Monaco's suggest rows) |
| `9eec81e` | Live-check fix: selected History row on `--bg-2` (green status was 4.31:1 on `--bg-3`) |
| this commit | 10. README and this report |

## Gate

Run before every commit with `set -o pipefail`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --no-fail-fast`, frontend `typecheck`, `lint`, `test`, `build`, `check:no-cdn`, a release `plv` build and `check:console`. Last run: 615 Rust tests passed, 0 failed (588 before this plan); 379 frontend tests passed (321 before).

## Live check

Release `plv serve` with a temporary `PLOVER_STATE_DIR`, `PLOVER_KEYCHAIN_SERVICE=dev.plover.test`, a copy of `fixtures/phase3-chain` (environment `local` with `userId: 42` and a keychain secret `apiKey`; `me.bru` with `?id={{userId}}` and `X-Api-Key: {{apiKey}}`) and `scripts/phase3-mock.py`. Driven with agent-browser in dark and light. Clipboard and `window.confirm` were stubbed in the page to record calls.

1. **Fold:** the name toggle set `aria-expanded="false"` and removed the tree; after a server restart and a real page reload it was still folded.
2. **Split:** dragging the bar gave 333 ‰ side by side; stacked and dragged, 349 ‰. `GET /api/layout` held `splitOrientation: "stacked", splitPermille: 333, stackedSplitPermille: 349`, and both survived a restart.
3. **Narrow window:** at 720 px the split area was 458 px: stacked, toggle `aria-disabled="true"`, title "Too narrow to place side by side", saved choice untouched. Back at 1440 px it was side by side again.
4. **Hidden buttons:** collection header buttons had opacity 0 with no hover or focus, 1 on hover; Tab from the name reached Run collection (opacity 1, focus ring). Shift+F10 on the name opened *Run collection / Import HAR…* with focus on the first item; Esc returned focus to the name. Response corner Copy/Save: opacity 0, 1 on hover; Copy showed "Copied".
5. **Value copy:** `{{host}}` card → clipboard `http://127.0.0.1:18090`, note "Copied value of {{host}}". `{{apiKey}}` card shows `•••• secret`; Copy value asked "This value is a secret. Copy anyway?"; No → nothing copied, "Not copied"; Yes → 21 characters copied (the secret's length). Param `id` row copy → `42`.
6. **Autocomplete by keyboard:** URL `&u={{us` → one option `userId · Environment: local`, input `role=combobox`, `aria-expanded=true`, `aria-activedescendant` set; Enter → `…&u={{userId}}`, caret at the end, no send. Header value `Bearer {{` → apiKey, host, token (`set later`), userId; ↓ moved the active option; Tab → `Bearer {{host}}`. JSON body `"{{` → Monaco list apiKey, host, token, userId with scope; ↓ + Enter → `"{{host}}"`. No value appears in any list.
7. **History:** sends with the secret in the `X-Api-Key` header, the query (`&k={{apiKey}}`) and the JSON body (`"k": "{{apiKey}}"`). The detail showed `X-Api-Key: ***` and `…&k=***`, no editable field; filter by collection; Open request focused `GET me`; Open as new request opened a scratch tab with the URL as sent and the banner "Opened from history. Values that were masked show as ***; replace them before sending."; Clear asked "Clear 5 sends of phase3-chain from history?", No kept the list, Yes emptied it and the folder.
8. **Leak search:** `grep -rF <secret> $PLOVER_STATE_DIR/sends` and the whole state dir: 0 files. The files hold `"X-Api-Key","***"`, `k=***` and `"k": "***"`.

Contrast measured for the new pairs: muted on `--bg-3` 5.30 (light) / 5.60 (dark); focus bar on `--bg-3` 5.77 / 5.60; text on `--bg-3` 15.89 / 11.59; status colours on the selected History row now on `--bg-2` (≥ 4.68, checked in `theme.test.ts`).

Temp state, the collection copy, the mock and the browser session were removed; no `dev.plover.test` keychain item is left.

Screenshots (looked at each): `/private/tmp/claude-501/-Users-tienle-projects-rea/1bb5dcfd-43a5-447b-a541-d24ed1b9cd89/scratchpad/ui-round2-shots/` — header idle, hover, Tab focus and keyboard menu (dark); split with corner Copy/Save (dark, light); stacked (dark, light); narrow forced-stacked (light); variable card (dark, light); secret card (light); param copy (light); `{{` list in the URL (dark, light), a header and the body (light); History (dark, light); Open as new request banner (light).

## Deviations from the plan

1. `AppState.sends` already named the cancel registry, so the store is `AppState.send_history` (`with_send_history`).
2. `VarsDto` has no separate process-env list; autocomplete drops entries with `source: "processEnv"`.
3. `SubTabs` got an `end` slot for the split toggle (outside the tablist), and `useCopy` got `say(note)` so Save as variable can show its note without writing the clipboard.
4. A transport failure is recorded in History (status `error`), and a skipped send is recorded as `skipped`; the plan named only finished sends.
5. Three live-check fixes were committed separately (Monaco widgets clipped, `.main` class clash, History row contrast).

## Known gaps

1. **Keychain prompt:** each release rebuild made the new binary wait for macOS keychain access to the test items created by the previous binary; the check deleted those items and stored the secret again. Real users hit this only when the app binary changes.
2. With a narrow request pane the section tabs scroll sideways ("Assert" and "Tests" partly hidden until scrolled).
3. Opening the variable card by keyboard only shows it; reaching its Copy button needs the pointer or `⌘/Ctrl+Shift+C` (which copies without the card).
4. History records only sends from the UI, not `plv run` or collection runs (those have run history).
5. Windows and Linux CI have not run these commits, and the desktop app was not opened.
