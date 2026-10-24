# Cancel, quick open and variable hover: report

## Tóm tắt (tiếng Việt)

**Đã xong:** cả 3 tính năng trong `docs/superpowers/plans/2026-10-24-dev-ux-cancel-quickopen-vars.md`, 11 commit trên `main` (chưa push).

1. **Cancel:** khi đang gửi, nút Send đổi thành `Cancel · 2.1s`, hoặc bấm `Esc`. Server dừng request ngay. Kiểm tra thật: mock ghi nhận client đóng kết nối sau 2.39 s, đúng lúc bấm Cancel.
2. **Quick open (`⌘/Ctrl+K`):** tìm request theo tên, folder, collection. Không phân biệt hoa thường và dấu (`dang nhap` tìm ra `Đăng nhập`). Hoạt động cả trong Monaco.
3. **Biến `{{name}}`:** nền xanh nhạt = đã có, gạch chấm vàng = script set sau, gạch sóng đỏ = không có. Rê chuột (hoặc để con trỏ trong biến 0.5 s) để xem giá trị và nguồn. Secret, `process.env` và giá trị đã gửi trong header `Authorization`/`Cookie` hiện `•••• secret`.
4. **Kiểm tra:** 554 test Rust, 305 test frontend, clippy, build, console check đều qua trước mỗi commit. Ảnh sáng/tối đã xem từng ảnh.
5. **Còn thiếu:** hover trong body editor chỉ dùng chuột (phím tắt của Monaco bị ⌘K chiếm), chưa chạy CI Windows, chưa thử trong app desktop.

## What changed

| Commit | What |
|---|---|
| `d0e4ada` | Plan: interfaces verified on `a8577f0` |
| `744f050` | `POST /collections/{id}/send/{sendId}/cancel`; `send` races a cancel signal |
| `ef0eb5c` | `Cancel · 3.2s` button, Esc, "Cancelled after …" banner |
| `716b982` | Quick open palette, in-house fuzzy ranking |
| `b0c589d` | Core: labelled variable layers (`VarStack::entries`), `preview_stack`, `scan_set_later` |
| `44d648a` | `POST /collections/{id}/vars` with masking and `setLater` |
| `c2872a4` | Frontend `/vars` cache per tab and token classification |
| `689126b` | `VarInput` mirror underlines and hover card (URL, params, headers, auth, form bodies) |
| `799606b` | Body editor (Monaco) decorations and hover |
| `0eaa974` | Fix found in the live check: a send refreshes every tab of its collection |

## Verification

- Gate before every commit (`set -o pipefail`): `cargo fmt --check`, `clippy -D warnings`,
  `cargo test --workspace` (554 passed at the end), frontend typecheck, lint, vitest
  (305 passed), build, no-CDN check, release build and `check:console` ("no page errors").
- Live check: release `plv serve`, temp `PLOVER_STATE_DIR`, `PLOVER_KEYCHAIN_SERVICE=dev.plover.test`,
  a copy of `fixtures/phase3-chain` plus a slow request and a variables demo, and a local
  Python mock on `127.0.0.1:18090`. No keychain item was written (the secret was only
  declared); temp files removed.
  - Cancel by click and by Esc (keyboard focus on the button): the mock logged
    `client closed the connection after 2.39s` and `2.47s`, matching the banner times.
  - Hover: `{{apiKey}}` → `•••• secret · Environment: local`; `{{token}}` → `Not set yet.
    Set at runtime by auth/login.bru.`; after sending login → `Runtime`; `{{missing}}` →
    `Not defined in any scope.`; same cards in the JSON body editor.
  - `⌘K`, type `usr me` → `GET me · users · phase3-chain`, Enter opens it.
  - No page errors in light or dark.
- Contrast (measured from the tokens; also pinned in `theme.test.ts`):

  | | light | dark |
  |---|---|---|
  | amber dotted underline (`--warn`) on `--bg` | 5.02 | 10.65 |
  | red wavy underline (`--err`) on `--bg` | 6.47 | 6.43 |
  | text on the defined tint (`--fg` on `--var-defined-bg`) | 16.10 | 7.70 |
  | focus ring (`--focus`) on `--bg` | 6.70 | 6.99 |

- Screenshots (not committed): the session scratchpad `dev-ux-shots/` folder —
  `light-hover-*.png`, `light-body-hover.png`, `light-headers.png`, `light-cancel-running.png`,
  `light-cancelled.png`, `light-quickopen*.png`, `dark-hover-*.png`,
  `dark-cancel-running-focus.png`, `dark-cancelled.png`, `dark-quickopen.png`.

## Deviations from the plan

1. The cancel registry holds `Arc<tokio::sync::Notify>` (the server has no `tokio-util`).
2. `preview_stack` lives in `runner.rs` next to `prepare_copy`; folder variables are one
   merged layer, so the card says "Folder" without its path.
3. `/vars` masks a value of any scope that equals a sent credential, not only runtime
   values (stricter than the spec). Process env entries are named `process.env.NAME` and
   always masked; the list carries the server's environment variable names, never values.
4. The Basic auth password field stays a plain password input (a mirror would put the
   text in the page). The banner reads "Cancelled after 2.4s".
5. A finished send refreshes the variables of every tab of its collection (runtime
   variables are per collection); the plan said "its collection's tabs after a run" only.

## Known gaps

1. Body editor hover is mouse only: Monaco's keyboard hover is the `⌘K ⌘I` chord, which
   quick open now takes. Plain fields have the caret path.
2. Windows and Linux CI have not run on these commits; the desktop app was not opened.
3. Quick open's 16 ms budget is measured in jsdom (median of 20 runs over 2,000 items),
   not in a real browser profile.
4. The Cancel button is wider than Send, so the URL field shrinks while a send runs.
5. Run tabs have no Cancel/Esc path from this work (runs keep their own Stop).
