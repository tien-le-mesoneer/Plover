# Plover UI/UX review (2026-10-23)

**Tóm tắt (đọc 1 phút)**
1. Luồng chính đã chạy tốt: gửi request, xem response/tests/console, chạy collection, sửa environment và secret. Dark mode và màu theo method đã có sẵn.
2. **3 lỗi nặng:** (a) ở cửa sổ rộng 960 px, layout vỡ: tab bị đè lên nhau và trang cuộn ngang; (b) không dùng được bằng bàn phím: sidebar và tab request không focus được bằng Tab, modal không có focus trap và không đóng bằng Esc; (c) Monaco không đổi theme khi macOS đổi sáng/tối, để lại một khối trắng giữa UI tối.
3. **Độ tương phản:** nút Send ở light mode chỉ đạt 3.19:1 (cần 4.5). Viền ô nhập chỉ đạt 1.3–1.6:1 (cần 3:1). Đã đề xuất giá trị token mới ở mục D.
4. **Sửa nhanh (mỗi việc dưới 30 phút):** thêm ⌘↵ để gửi và ⌘W để đóng tab, sắp xếp collection theo thứ tự cố định, vùng bấm của nút icon tối thiểu 24 px, thêm header "Name / Value" cho bảng.
5. **Để ngang Bruno/Postman:** cần thêm ô tìm request, nút copy hoặc format response, kéo để đổi kích thước pane, nhớ tab sau khi reload, và lịch sử request.

> **Update 2026-10-24:** the token proposals in section D were superseded by the Swiss Minimal tokens in `frontend/src/styles.css` (checked by `frontend/src/theme.test.ts`); see `docs/ui-style-report.md`.

Scope: the React frontend served by `plv serve` and by the Tauri window. It was reviewed as a desktop developer tool used with mouse and keyboard, so mobile-only rules were skipped. The method was a source read plus a live walkthrough: release `plv` (`target/release/plv`), the `fixtures/phase3-chain` and `uicheck` collections copied to scratch, localhost mocks, and agent-browser at 1440×900 and 960×700 in light and dark. Contrast ratios were computed from the CSS tokens with the WCAG 2.x formula.

Screenshots are in `/private/tmp/claude-501/-Users-tienle-projects-rea/1bb5dcfd-43a5-447b-a541-d24ed1b9cd89/scratchpad/uiux/shots/` (referred to below as `shots/`).

ui-ux-pro-max results used:
- `--design-system "developer tool API client desktop dark"`:
  - Kept:
    - Style: "Dark Mode (OLED)". It requires text contrast 4.5:1, keyboard access, visible focus and reduced motion.
    - Typography: JetBrains Mono / IBM Plex Sans.
    - The pre-delivery checklist.
  - **Rejected:**
    - The "FAQ/Documentation Landing" pattern, which doesn't fit an app UI.
    - Its Google Fonts import, which conflicts with Plover's offline/no-CDN rule. Fonts would need to be bundled.
- Verified `--domain ux` hits:
  - Keyboard Navigation (High)
  - Focus States (High)
  - Input Labels (High)
  - Loading Indicators (High)
  - Success Feedback (Medium)
- Verified `--stack react` hit: "Use keys properly" (High).
- The query for "confirm before dismissing a modal" returned no verified match after one retry. The Quick Reference rule `sheet-dismiss-confirm` is cited instead.

---

## A. Defects

### Critical / High

**A1. High: the layout breaks at narrow window widths.** Rules: `horizontal-scroll`, `adaptive-navigation`.
- Evidence: `shots/13-narrow-960.png`. At 960×700, the document `scrollWidth` is 1018 px against a 960 px viewport, so the page scrolls sideways.
  - The request sub-tabs need 408 px in a 340 px pane, so "Assert/Tests" draw over the response status.
  - `.response-meta` needs 397/340 px, so "14 ms" and "215 B" wrap and "Console" is cut off.
  - The sidebar is fixed at 280 px and the two panes are fixed at 50/50.
- Where: `frontend/src/styles.css:73` (`.sidebar { width: 280px }`), `:103-107` (`.split`, `.pane`, `.subtabs`), `:137` (`.response-meta`).
- Fix:
  - `.subtabs { overflow-x: auto; scrollbar-width: thin }` and `.subtabs button { flex: none }`.
  - `.response-meta { flex-wrap: wrap; row-gap: 0 }`, and give its numbers `white-space: nowrap`.
  - Below about 1100 px wide, stack the split vertically (`@media (max-width: 1100px) { .split { flex-direction: column } .pane { border-right: none; border-bottom: 1px solid var(--border) } }`), or add a draggable divider (see C3).
  - Make the sidebar resizable or collapsible, with a minimum width of 200 px.

**A2. High: the sidebar tree and the request tabs can't be reached with the keyboard.** Rules: `keyboard-nav`, `focus-states`. Skill: "Keyboard Navigation", High.
- Evidence: `document.querySelectorAll('.node[tabindex]').length === 0` (out of 5 nodes) and `[role=tab][tabindex]` = 0. Tabbing through the page never lands on a request, a folder or a tab, so a request can't be opened without a mouse.
- Running a folder is right-click only (`title` tooltip).
- Where: `frontend/src/components/Sidebar.tsx:68-80` and `:97-103` (clickable `<div>`s); `frontend/src/components/RequestTabs.tsx:11-19` (a `role="tab"` `<div>` without `tabIndex` or arrow keys).
- Fix:
  - Render nodes as `<button>`s, or use the ARIA tree pattern (`role="tree"/"treeitem"`, `aria-expanded`, roving `tabIndex`, ↑↓ to move, →← to expand or collapse, Enter to open).
  - Add a visible "Run folder" item: a hover/focus icon button on folder rows, with right-click kept as a shortcut.
  - Tabs: `tabIndex={active ? 0 : -1}`, ←→ to move between them, Delete or ⌘W to close.

**A3. High: modals don't manage focus and Escape doesn't close them.** Rules: `escape-routes`, `focus-management`, `modal-escape`. Skill: "Focus States", High.
- Evidence from the Environment editor (`shots/09-env-editor-dark-1440.png`):
  - Focus stays on the topbar "Edit" button behind the backdrop after the modal opens.
  - Escape doesn't close the modal.
  - 15 presses of Tab move focus out of the dialog to the tab "Close" button behind it (`inDialog=false`).
  - There is no `aria-modal`.
- The reload-confirm dialog has the same problem.
- Clicking the backdrop discards unsaved environment edits without asking (`sheet-dismiss-confirm`).
- Where: `frontend/src/components/EnvEditor.tsx:50-51`, `frontend/src/App.tsx:86-87`.
- Fix: a small shared `<Modal>` that:
  - adds `aria-modal="true"`,
  - focuses the first field when it opens,
  - traps Tab inside the dialog,
  - closes on Escape,
  - returns focus to the control that opened it,
  - asks before closing on backdrop click or Escape when there are unsaved changes.
  - The native `<dialog>` element with `showModal()` provides most of this.

**A4. High: Monaco keeps the light theme after the system switches to dark.** Rules: `dark-mode-pairing`, `color-dark-mode`.
- Evidence: `shots/03-me-body-dark-1440.png`. After the system switches to dark, the whole app turns dark except the response editor, which stays `vs` (white).
- macOS changes appearance automatically (Auto mode), so users will hit this. After a reload the editor is `vs-dark` with background `#1e1e1e`, which doesn't match the app's `#1b1d22`.
- Where: `frontend/src/editor/CodeEditor.tsx:25,35`: `matchMedia` is read once when the editor is created.
- Fix:
  - Subscribe once to `matchMedia('(prefers-color-scheme: dark)').addEventListener('change', …)` and call `monaco.editor.setTheme(...)`. The theme is global, so a single listener in `editor/monaco.ts` is enough.
  - Define `plover-dark` / `plover-light` themes with `monaco.editor.defineTheme`, using `editor.background` = `--bg` and the line-highlight colour = `--bg-2`.

**A5. High: the Send button fails text contrast in light mode.** Rules: `color-contrast`, `color-accessible-pairs`.
- Evidence: white `--accent-fg` on `--accent #d97706` = **3.19:1**. The label is 13 px, so it needs 4.5:1.
- The same 3.19:1 applies to the 2 px active-tab underline and the unsaved-changes dot against `--bg`. Those meet the 3:1 non-text minimum only just.
- Dark mode passes at 7.85:1.
- Where: `frontend/src/styles.css:8-9`.
- Fix: see D. Either `--accent: #b45309` (white on it = 5.02), or keep `#d97706` with `--accent-fg: #1d2129` (5.06).

**A6. High: key/value inputs use the placeholder as their only label.** Rules: `input-labels`, `form-labels`. Skill: "Input Labels", High.
- Evidence: the tab walk reported the accessible names of the KV inputs as empty (`INPUT||`). The visible placeholders "name"/"value" disappear once a row has content.
- In dark mode the placeholder colour is the browser default `#757575`, which is 3.66:1 on `--bg`.
- In the Assert tab, the placeholder row `res.status / eq 200` looks exactly like a third real assertion (`shots/05-assert-tests-dark-1440.png`).
- Where: `frontend/src/components/request/KeyValueTable.tsx:50-62`.
- Fix:
  - Add a header row (`<thead><th>Name</th><th>Value</th></thead>`, labelled "Assertion" / "Expected" in the Assert tab).
  - Give each input `aria-label={\`${row.name || "new"} name\`}`.
  - Style placeholders with `::placeholder { color: var(--muted); font-style: italic }`.
  - For Assert, show a hint row ("e.g. res.status — eq 200") instead of example placeholders that look like data.

### Medium

**A7. Medium: icon-only controls are below the 24×24 px target size, and some use font glyphs as icons.** Rules: `web-target-size`, `no-emoji-icons`, `icon-style-consistent`.
- Measured hit areas:
  - Run collection: 18×18
  - Reload from disk: 17×18
  - Tab close: 16×18
  - The row remove button is the same size.
- The glyphs are `▶ ⟳ × ⚠ ▸ ▾ ✓ ✗ •` from the system font. `⟳` renders visibly smaller than `▶` (`shots/03-me-body-light-1440.png`), and `⚠` can render as a colour emoji on macOS.
- Where: `frontend/src/styles.css:62` (`button.icon { padding: 0 4px }`); `Sidebar.tsx:29,37,91`; `RequestTabs.tsx:32`; `KeyValueTable.tsx:67`.
- Fix:
  - `button.icon { min-width: 24px; min-height: 24px; display: inline-grid; place-items: center }`.
  - Replace the glyphs with one SVG icon set, for example inline Lucide or Phosphor SVGs (bundled, not from a CDN), at 16 px with a consistent stroke.

**A8. Medium: input and select borders fail non-text contrast.** Rule: `color-accessible-pairs` (WCAG 1.4.11, 3:1 for component boundaries).
- Evidence: `--border` against `--bg` is **1.34:1** in light (`#dcdfe5` on `#fff`) and **1.59:1** in dark (`#3a3f47` on `#1b1d22`). Against `--bg-2` it is 1.25 and 1.45.
- The URL field and the key/value inputs are recognisable only by that border.
- Where: `frontend/src/styles.css:7,27,46-51`.
- Fix: keep `--border` for dividers and add `--border-strong` for form controls: light `#868e9b` (3.30 on bg, 3.08 on bg-2), dark `#717986` (3.84 / 3.50). See D.

**A9. Medium: the response header shows the HTTP status, not whether the request passed.** Rules: `color-not-only`, `error-clarity`.
- Evidence: `shots/12-script-error-dark-1440.png`. A request whose post-response script throws and whose test fails still shows a green **200** as the most prominent signal. The failure appears only as a small red `0/1` superscript on the "Tests" tab and the banner.
- Where: `frontend/src/components/ResponseViewer.tsx:95-99`.
- Fix:
  - Add an outcome chip before the status: "✓ Passed" or "✗ 1 test failed / script error", with an icon and text, not colour alone.
  - Auto-select the Tests tab when a test fails.
  - Show the reason phrase: "200 OK", "404 Not Found".

**A10. Medium: script errors offer no way to fix them.** Rule: `error-recovery`.
- Evidence: the banner reads "post-response script error (request, line 1): TypeError: …", but there is no action.
- Where: `ResponseViewer.tsx:32-36`.
- Fix:
  - Add an "Open script" action that switches the request pane to Script → Post Response and reveals the line (`editor.revealLineInCenter` + `setPosition`).
  - For collection or folder scopes, name the file.

**A11. Medium: it isn't clear which collection the environment selector belongs to.** Rules: `visual-hierarchy`, `navigation-consistency`.
- Evidence: with two collections open, the topbar shows a single `local` selector. It silently changes meaning with the active tab's collection, and the modal title "Environment: local" doesn't name the collection (`shots/09`, `shots/10`).
- Where: `frontend/src/App.tsx:23,83`, `components/EnvSelector.tsx`, `EnvEditor.tsx:52`.
- Fix:
  - Prefix the selector with the collection name ("phase3-chain · local").
  - Or move the env selector into each collection's sidebar header.
  - Title the modal "phase3-chain — Environment: local".

**A12. Medium: sending blocks the response pane and can't be cancelled.** Rules: `loading-states`, `loading-buttons`. Skill: "Loading Indicators", High.
- Evidence: while a request is in flight, the whole response pane becomes the text "Sending…", the previous response disappears, and there is no Cancel. A slow endpoint (30 s timeout) leaves the user stuck.
- Where: `ResponseViewer.tsx:26`, `UrlBar.tsx:50-52`.
- Fix:
  - Keep the last response visible but dimmed, with an elapsed-time counter and a "Cancel" button. The server already supports cancellation for runs; add it for sends.
  - Mark the region `aria-busy`.

**A13. Medium: tabs and expanded folders are lost on reload, and the collection order changes.** Rules: `state-preservation`, `navigation-consistency`.
- Evidence: after reloading the page, every open tab is gone and every folder is collapsed. The sidebar also swapped the order of the two collections (phase3-chain was listed first before the reload, ui-check after).
- Where: `crates/core/src/workspace.rs:233,289` (`HashMap`-backed `list()`); `frontend/src/store/tabs.ts` (not persisted).
- Fix:
  - Sort `list()` by open time or by name.
  - Persist the open tab keys and expanded folders in `localStorage`, scoped per collection root, and restore them on load. Drafts are already guarded by `beforeunload`.

### Low

**A14. Low: the method badge and other secondary text are too small.** Rules: `readable-font-size`, `font-scale`.
- Evidence: `.badge` uses 10 px bold. The base font is 13 px, and h4 uses 12 px uppercase.
- 10 px falls below the 12 px floor in the skill's checklist.
- Where: `styles.css:19,86,111`.
- Fix: use 11 px with `letter-spacing: .02em` for badges, or keep 10 px and add a background pill for legibility. Define a type scale: 11 / 12 / 13 / 15 / 18.

**A15. Low: sub-tab semantics and counts are read awkwardly by screen readers.** Rules: `aria-labels`, `contextual-live-badge-updates`.
- Evidence: accessible names come out as "Assert2", "Tests•" and "Tests3/3".
- The request and response sub-tabs are plain buttons with no `role="tab"` or `aria-selected`.
- Two different buttons are both named "Body" and two are both named "Headers".
- Where: `RequestEditor.tsx:74-84`, `ResponseViewer.tsx:45-70`.
- Fix:
  - Use `role="tablist"` with `role="tab"`, `aria-selected` and `aria-controls`.
  - Give the counts visually-hidden text: "Tests, 3 of 3 passed", "Headers, 4".
  - Label each tablist ("Request sections", "Response sections").

---

## B. Quick wins (each under 30 min)

1. **⌘↵ / Ctrl+↵ sends from anywhere**, including inside Monaco via `editor.addCommand`. **⌘W** closes the active tab with the existing dirty confirm. Extend the handler in `App.tsx:53-62`.
2. Sort `Workspace::list()` deterministically (A13, server half).
3. Set `button.icon` minimum size to 24×24 and centre its content (A7, size half).
4. Add "Name / Value" headers to the key/value table and style placeholders muted and italic (A6, partial).
5. Add Escape-to-close and autofocus to both modals (A3, partial). A full focus trap comes with the shared `<Modal>`.
6. Show the HTTP reason phrase next to the status code (`200 OK`); use a small map or `statusText` from the server (A9, partial).
7. Add a right-aligned **Copy** button in the response meta bar for the body or headers, with a 1.5 s "Copied" confirmation. Skill: "Success Feedback".
8. Upgrade the empty state (`shots/01-empty-light-1440.png`):
   - Put a primary "Open collection…" button in the main area: Browse in desktop, a focused path field in web.
   - Add a one-line explanation of what a Bruno collection is.
   - Add recent paths (C6).
9. Live Monaco theme switching (A4); one listener in `editor/monaco.ts`.
10. Show the run panel's failure messages in `--err` instead of `.muted`, and add column headers: Result, Method, Name, Status, Time · Size, Tests, Message. Where: `RunPanel.tsx:63-81`.

## C. Bigger enhancements and parity ideas

These are features developers expect from Bruno, Postman or Insomnia. They are not defects.

| # | Idea | Why | Effort |
|---|---|---|---|
| C1 | **Request search / quick open** (⌘P or ⌘K): fuzzy search over request names and URLs across collections, plus a filter box above the tree | Large collections (Bruno's testbench has 288 files) can't be browsed by clicking alone | 0.5–1 day |
| C2 | **Response tools**: format/beautify toggle, a search box in the body (Monaco find), word-wrap toggle, a "Save body to file" action, and an image/HTML preview tab | Parity with Bruno's response pane | 1 day |
| C3 | **Resizable panes** (sidebar width, request/response split, run panel height), plus a vertical-split toggle; persist the sizes | Fixes A1 properly and fits small laptop screens | 0.5 day |
| C4 | **Request history** per collection: the last N sends with status, time and timestamp, stored in memory or locally; click to view the response | Bruno and Postman both have it; it helps debugging | 1–2 days |
| C5 | **Run panel ↔ request linking**: click a run row to open that request with the run's response, tests and console | Today the run output can't be explored | 0.5–1 day |
| C6 | **Recent collections**: remembered across launches in the desktop app and listed in the empty state | This is also a Phase 4 gap ("app doesn't remember open collections") | 0.5 day |
| C7 | **Command palette** (⌘K) for "Run folder", "Switch environment", "Open collection", "Reload from disk", "Toggle theme", showing their shortcuts | Exposes actions that are hidden in tooltips or right-click today | 1 day |
| C8 | **Context menu on tree nodes** (run, rename, duplicate, delete, reveal in Finder, copy as cURL). Note: create/rename/delete were deferred in Phase 2 | Expected file management | 2–3 days with the backend |
| C9 | **Copy as cURL / code snippet** from the URL bar | Very common in API clients | 0.5 day (core already resolves the request) |
| C10 | **Bundled UI font** (IBM Plex Sans + JetBrains Mono, self-hosted woff2, about 150 KB) and a theme toggle (System / Light / Dark) in a settings popover | Consistent look between WKWebView and Chromium; the skill's typography pick, served offline | 0.5 day |

## D. Proposed design-token tweaks

Ratios use the WCAG 2.x formula, computed in this review.

```css
:root {
  /* Light */
  --accent: #b45309;          /* white text on it 5.02:1 (was #d97706 → 3.19:1). Alternative: keep #d97706, set --accent-fg #1d2129 → 5.06:1 */
  --accent-fg: #ffffff;
  --border: #dcdfe5;          /* dividers only (decorative) */
  --border-strong: #868e9b;   /* inputs/selects/buttons: 3.30:1 on #fff, 3.08:1 on --bg-2 (was 1.34 / 1.25) */
  --muted: #5f6673;           /* 5.78 on --bg, 4.97 on --bg-3 (was #6b7280: 4.16 on --bg-3 for selected rows) */
  --focus: #2563eb;           /* 5.17:1 on --bg */
  --placeholder: #6b7280;     /* 4.83:1 on --bg */
}
@media (prefers-color-scheme: dark) {
  :root {
    --border: #3a3f47;        /* dividers only */
    --border-strong: #717986; /* 3.84:1 on --bg, 3.50:1 on --bg-2 (was 1.59 / 1.45) */
    --focus: #60a5fa;         /* 6.63:1 on --bg */
    --placeholder: #8b929e;   /* 5.38:1 on --bg (browser default #757575 was 3.66:1) */
  }
}
input, select, textarea { border-color: var(--border-strong); }
::placeholder { color: var(--placeholder); }
:focus-visible { outline: 2px solid var(--focus); outline-offset: 1px; border-radius: 3px; }
button.icon { min-width: 24px; min-height: 24px; display: inline-grid; place-items: center; }
.badge { font-size: 11px; letter-spacing: .02em; }
@media (prefers-reduced-motion: reduce) { * { transition: none !important; animation: none !important; } }
```

- **Focus rings:** today the app relies on the browser default ring (`outline: auto 1px`). It is visible in Chromium (`shots/14-focus-tab6.png`), but it differs in WKWebView and is thin. An explicit 2 px `--focus` ring satisfies `focus-states` / `focus-appearance` in both.
- **Already fine, no change needed:**
  - Method badge colours on `--bg-2`: light 4.59–6.25, dark 5.55–10.03.
  - Status colours: ok, warn and err are each ≥ 5.0 in both themes.
  - Banner text on its tinted background: ≥ 9.8.
  - Dark-mode muted text: ≥ 5.22.

## E. What already works well

- **Clear layout:** sidebar, tabs, URL bar, request and response side by side. Method colours are consistent across the sidebar, tabs, URL bar and run panel. The response meta shows status, time and size right away (`shots/04`).
- **Dark mode:** a full token set, `color-scheme` set correctly, and all text pairs passing AA in dark (D).
- **Test results:** easy to scan. There is a "3/3 passed" headline, ✓/✗ glyphs together with colour (not colour alone), the assertion expressions in monospace, and counts on the tabs (`shots/05`).
- **Safe-by-default flows:**
  - The unsaved-changes dot and the confirm before closing a dirty tab.
  - The `beforeunload` guard.
  - The conflict banner offers a clear choice (Reload / Overwrite) and doesn't overwrite silently (`shots/11`).
  - The env editor explains where secrets live, with per-secret keychain or memory badges (`shots/10`).
- **Already accessible:** icon buttons have `aria-label`s, banners use `role="alert"`/`"status"`, and Enter in the URL field sends.
- **Readable error text:** the script error names the phase, scope and line (`shots/12`). Only a jump-to-line action is missing (A10).

---

## After (2026-10-23)

**Tóm tắt**
1. Đã sửa cả 6 lỗi nặng (A1–A6), lỗi trung bình A9, cùng các quick win ở mục B. Thêm phần sửa A8 (viền ô nhập), A10 (nút "Open script") và A15 (tablist có tên đọc rõ).
2. Ở cửa sổ 960×700 không còn cuộn ngang (`scrollWidth` 960/960), và hai pane xếp dọc khi cửa sổ hẹp hơn 1100 px.
3. Dùng được hoàn toàn bằng bàn phím: Tab vào cây collection, phím mũi tên để đi, Enter để mở, ⌘/Ctrl+Enter để gửi. Modal giữ focus bên trong, đóng bằng Esc và trả focus về chỗ cũ.
4. Độ tương phản: nút Send ở light mode đạt 5.02:1, viền ô nhập 3.30:1 (light) và 3.84:1 (dark). Editor đổi theme ngay khi hệ thống đổi sáng/tối.
5. Mục C (tính năng lớn, ngang Bruno/Postman) để làm sau.

Commits: `872a10c` (core: collection order), `492c56e` (frontend). Verified with the release `plv` on the same scratch collections and localhost mocks, using agent-browser at 1440×900 and 960×700 in light and dark, plus a keyboard-only walkthrough. After-shots are in `/private/tmp/claude-501/-Users-tienle-projects-rea/1bb5dcfd-43a5-447b-a541-d24ed1b9cd89/scratchpad/uiux/after/` (referred to as `after/`). Gate: 284 Rust and 81 frontend tests pass; clippy, typecheck, lint and `check:no-cdn` are clean; `check:console` reports no page errors.

| Item | Status | Evidence |
|---|---|---|
| A1 layout at narrow widths | Fixed (`492c56e`) | At 960×700, `scrollWidth` is 960/960 (was 1018). Below 1100 px the panes stack, sub-tab strips scroll, the response meta wraps, and the sidebar is `clamp(200px, 24vw, 280px)`. Before: `shots/13-narrow-960.png`. After: `after/13-narrow-960-light.png`, `after/13-narrow-960-dark.png`. |
| A2 keyboard access | Fixed | The sidebar is a `role="tree"` per collection with one tab stop. ↑↓ move, →← expand, collapse or go to child/parent, Enter opens or toggles, Ctrl/Cmd+Enter runs a folder, and a visible ▶ appears on hover/focus. Request tabs use a roving tab stop: ←→ switch, Delete closes. The live walk reached the tree by Tab, opened `login` and sent it with ⌘↵ without the mouse (`after/02-login-sent-light-1440.png`). |
| A3 modal focus | Fixed | A shared `Modal` with `aria-modal`. Focus moves to the first control; after 12 presses of Tab it was still inside the dialog. Escape closes and focus returns to "Edit". The environment editor confirms before discarding edits on Escape, a backdrop click or Cancel (`after/09-env-editor-dark-1440.png`). |
| A4 Monaco theme | Fixed | `plover-light` and `plover-dark` themes use the app tokens, and one `matchMedia` listener calls `setTheme`. A live toggle read the editor background as `#ffffff`, then `rgb(27,29,34)`, which is `--bg` (`after/03-body-dark-live-switch-960.png`). |
| A5 Send contrast | Fixed | `--accent: #b45309`: white on it is **5.02:1** (was 3.19). The underline and unsaved-changes dot are also 5.02:1. Dark mode is unchanged at 7.85:1. |
| A6 key/value labels | Fixed | Name/Value column headers; every input is labelled ("Name of accept", "New name"); styled placeholders. The Assert tab uses Assertion/Expected headers and an example hint instead of fake placeholder rows. |
| A7 icon targets | Fixed | Icon buttons are at least 24×24 and use inline SVG icons (`components/icons.tsx`) instead of font glyphs. |
| A8 control borders | Fixed | `--border-strong` is **3.30:1 / 3.08:1** in light (on bg / bg-2) and **3.84:1 / 3.50:1** in dark. `--border` stays for dividers. |
| A9 outcome signal | Fixed | A chip before the status ("✗ Script error", "✗ 1 of 1 checks failed", "✓ 2 checks passed"); the status shows "200 OK"; the Tests tab opens automatically when checks fail. |
| A10 script error action | Fixed | The banner has an "Open script" button that switches to the Script section. Revealing the exact line is still open. |
| A15 tab semantics | Fixed | Request and response sections are `role="tablist"` with arrow keys and full names ("Tests, 0 of 1 passed"). |
| B1 shortcuts | Fixed | ⌘/Ctrl+Enter sends from anywhere, including inside Monaco (`addCommand`) and on a focused tree request. ⌘/Ctrl+W closes the active tab in the desktop app; browsers keep ⌘W for closing the window, so there it is not delivered to the page. |
| B2 collection order | Fixed (`872a10c`) | `Workspace::list` returns open order; covered by the test `list_keeps_open_order`. |
| B3–B10 | Fixed | 24 px icons, Name/Value headers, modal Escape and autofocus, reason phrase, Copy (body or headers, "Copied" for 1.5 s), an empty state with a primary "Open collection…" (`after/01-empty-no-collection-light-1440.png`), live Monaco theme, run-panel column headers with failures in `--err` (`after/08-run-panel-dark-1440.png`). |

**Contrast after** (WCAG 2.x formula):

| Pair | Ratio |
|---|---|
| Light: white on `--accent` (Send) | 5.02 |
| Light: `--muted` on `--bg` | 5.78 |
| Light: `--muted` on `--bg-3` | 4.97 |
| Light: `--placeholder` | 4.83 |
| Light: focus ring | 5.17 |
| Light: error chip | 6.47 |
| Light: OK chip | 5.02 |
| Dark: Send | 7.85 |
| Dark: `--placeholder` | 5.38 |
| Dark: focus ring | 6.63 |
| Dark: error chip | 6.10 |
| Dark: OK chip | 9.68 |
| Dark: `--muted` | 6.64 |

**Still open:**
- A11: the environment selector doesn't name its collection.
- A12: no Cancel and no dimmed previous response while sending.
- A13: tabs and expanded folders aren't persisted across reloads; the collection order is fixed.
- A14: the type scale is only partly done (badges are 11 px).
- A10: jumping to the exact script line.
- All of section C (request search, response tools, resizable panes, history, run-row linking, recent collections, command palette, tree context menu, copy as cURL, bundled fonts and a theme toggle).
- The Modal and the key handling were verified in Chromium; WKWebView (the desktop app) has not been checked visually.
