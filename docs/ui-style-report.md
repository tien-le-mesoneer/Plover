# UI style and layout: report (2026-10-24)

**Tóm tắt**
1. Giao diện mới theo kiểu Swiss Minimal: nền trắng (có dark mode tương ứng), viền 1 px, không đổ bóng. **Send là nút có màu duy nhất** (emerald-700 `#047857` chữ trắng; dark: `#10b981` chữ tối). Font IBM Plex Sans và JetBrains Mono được đóng gói sẵn (giấy phép OFL), không tải từ CDN.
2. Thanh trên cùng ghi `Demo API · local`: luôn cho biết environment thuộc collection nào. Sidebar kéo được độ rộng (180 px đến nửa cửa sổ) và nhớ độ rộng sau khi khởi động lại.
3. Thanh URL: method · URL · **Send** · `⋯` (Copy as cURL, Copy resolved URL với secret thành `***`, Save, Reveal .bru / Copy file path). Tab chưa lưu có dấu chấm.
4. Response: dòng trạng thái gọn kèm nút icon copy/lưu file, header quan trọng (`x-request-id`, `traceparent`, `location`) được ghim, Pretty/Raw/Preview, lọc JSONPath, `⌘F`, bấm vào giá trị JSON để copy giá trị, copy path hoặc lưu thành runtime variable. Body trên 1 MiB chỉ hiển thị một phần, nhưng Copy/Save vẫn lấy toàn bộ.
5. Runner thành **tab riêng** với bộ lọc All/Fail/Pass/Skip kiểu Bruno, chi tiết kết quả bên phải (lý do lỗi nằm đầu tiên), Previous runs, Export, Re-run, Run with data…. Panel chạy ở dưới đáy đã bỏ.

## What was built

| Task | Commit | Result |
|---|---|---|
| 0 | `acc6f19` | Interfaces checked on `e46f2bd`; differences written at the top of the plan |
| 1 | `97160b9` | Semantic colour tokens for light and dark, bundled woff2 fonts, WCAG contrast test, CDN check rejects Google Fonts |
| 2 | `b6ae47b` | `<collection> · <env ▾>` in the top bar |
| 3 | `e9353ab` | Resizable sidebar; `GET/PUT /api/layout` stored in `state.json` |
| 4 | `f938430` | Send + `⋯` request menu; `POST /collections/{id}/resolved-url`; `POST /native/reveal`; `capabilities.reveal` |
| 5 | `712c26e` | Status line, icon copy/save, pinned headers, Headers tab copy (row and all) |
| 6 | `7d3392f` | Pretty/Raw/Preview, JSONPath subset (no dependency), `⌘/Ctrl+F` find |
| 7 | `91cebcc` | Value menu (Copy value / Copy path / Save as runtime variable); `PUT /collections/{id}/runtime-vars/{name}` |
| 8 | `2924467` | 1 MiB render cap with Show all; large arrays start folded |
| 9 | `272e300` | Run tab with status filter, name filter, folder line only on duplicate names; `RunPanel` deleted |
| 10 | `docs:` commit after `272e300` | Export / Run with data in the Run tab header (test), README, screenshots, this report |

Measured contrast (WCAG 2, from the tokens; `frontend/src/theme.test.ts` checks every pair):

| Pair | Light | Dark |
|---|---|---|
| `--fg` on `--bg` | 18.47 | 14.48 |
| `--muted` on `--bg-2` | 5.74 | 6.43 |
| `--placeholder` on `--bg-2` | 5.02 | 5.49 |
| Lowest method/status colour on `--bg-2` | 4.68 (GET, POST, 2xx, 4xx) | 5.91 (DELETE, 5xx) |
| `--color-on-send` on `--color-send` / hover | 5.48 / 7.68 | 7.01 / 9.25 |
| `--border-strong` on `--bg` / `--bg-2` | 3.49 / 3.25 | 4.16 / 3.82 |

Accessibility checks: every new icon-only button has `aria-label` and `title`; copy feedback goes to an `aria-live="polite"` region; the menus, the Pretty/Raw/Preview switch, the run filter and the sidebar handle work from the keyboard (arrow keys, Home/End, Esc returns focus); result marks are ✓ / ✕ / ⊘ with screen-reader text; icons come from `components/icons.tsx` (three added: More, Download, External); the existing `prefers-reduced-motion` rule covers the new parts (no new animation).

## Deviations from the plan

1. **Run tabs are a separate list.** `useTabs.runTabs: RunTab[]` shares `active` with request tabs instead of turning `Tab` into a union, so the request-tab code did not change. Run tabs are not in the UI-state snapshot, so they are never remembered.
2. **No slot props.** The data-driven work had landed, so `ExportMenu` and `RunWithDataDialog` are mounted directly in the Run tab header. `RunHistoryMenu` gained `onChoose`, and `RunWithDataDialog` gained `onStarted`, so the tab retargets itself.
3. **Reveal in web mode answers 501 `not_supported`**, like the other native routes, not 404 `not_desktop`. A missing file is 404; a path outside the collection is 400 `bad_item`.
4. **The Body tab is always "Body"**; run details used to call it "Response". The checks summary is an `n/m` chip with ✓ or ✕.
5. **The Monaco theme repeats `--bg` / `--bg-2` as hex** in `editor/monaco.ts`, because Monaco cannot read CSS variables.
6. **Gate.** The full gate ran before every commit. For Task 9, the full gate passed and then a small markup and CSS fix in `RunList` (method badge cell) was checked with the frontend gate plus the embedded-assets test before the commit; no Rust file changed in between.

## Known gaps

1. Preview of images and PDFs, folding, and the 1 MiB cap were checked in jsdom tests only. The fixture has no HTML, image or large responses.
2. Reveal .bru file was checked against a fake host, not in the Tauri app. Clipboard text in the browser was checked through the "Copied" note only (headless Chrome refuses clipboard reads).
3. The Run tab shows the time the tab opened, not the server's start time, for a live run.
4. There is no list of runtime variables yet, so a saved variable only shows up as a short "Saved {{name}}" note (a later plan covers the variables view).
5. The keyboard focus ring was checked in tests and CSS, but not screenshotted.

## Screenshots

README (committed, "Demo API" copy of `fixtures/phase3-chain` against the local mock):
`docs/screenshots/request-dark.png`, `script-dark.png`, `tests-dark.png`, `run-light.png`, `value-menu-light.png`, `run-fail-dark.png`.

Verification set, light and dark, release `plv serve` with a temporary `PLOVER_STATE_DIR` and test keychain service (not committed):
`/private/tmp/claude-501/-Users-tienle-projects-rea/1bb5dcfd-43a5-447b-a541-d24ed1b9cd89/scratchpad/ui-style-shots/` holds `request-{light,dark}`, `value-menu-{light,dark}`, `find-{light,dark}`, `jsonpath-light`, `jsonpath-error-dark`, `request-menu-light`, `run-{light,dark}`, `run-fail-{light,dark}`, `previous-runs-{light,dark}`, `script-dark` and `tests-dark` (all `.png`). The browser reported no page errors during the walkthrough.
