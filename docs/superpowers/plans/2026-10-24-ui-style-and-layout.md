# UI Style and Layout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Plover gets the Swiss Minimal look (light by default, matching dark), a top-bar environment label that names its collection, a resizable sidebar, a URL bar where Send is the only coloured button, a response viewer built for scanning and copying, and the runner as a tab with a Bruno-style status filter.

**Architecture:** All colours, fonts and radii become semantic CSS custom properties in `frontend/src/styles.css`; components stop using raw colours. Fonts are bundled woff2 files. Small server additions: a global layout state (`GET/PUT /layout`), a resolved-URL endpoint, a runtime-variable setter and a desktop "reveal file" call. The response viewer gains a body toolbar, a JSONPath filter and a value menu written in `frontend/src/lib/` without new dependencies. The bottom `RunPanel` becomes the body of a run tab in `store/tabs.ts`.

**Tech Stack:** React + TypeScript + Zustand + Monaco + Vitest/Testing Library; Rust (axum, serde, ts-rs) for the four small endpoints.

**Spec:** `docs/superpowers/specs/2026-10-23-dev-ux-design.md` §1, §2, §3, §5, §6 (mockups in `docs/superpowers/specs/2026-10-23-ui-mockups/`; the spec wins where they differ).

**Pending user decision — one-line change:** the Send button colour. The plan puts it behind `--color-send` / `--color-on-send` and defaults to **black fill, white text** (`#111418` / `#ffffff` light; `#e6e8eb` / `#16181c` dark). The alternative is green `#15803d`, which is the same colour as `GET` and 2xx. Changing it is the two token lines in Task 1 and the expected values in `theme.test.ts`.

**Starts after:** the run-history work (`main`, plan `docs/superpowers/plans/2026-10-23-run-history.md`) and the curl/HAR work (`feat/import-curl-har`, plan `docs/superpowers/plans/2026-10-23-import-curl-har.md`) are both merged into `main`. The data-driven/export plan (`docs/superpowers/plans/2026-10-23-data-driven-and-export.md`, written separately) also puts controls into the Run tab header; see Task 10.

## Global Constraints

- No new fields in `.bru` files. UI-only state goes to `state.json` through `crates/core/src/app_state.rs`.
- No font, icon or script from a CDN. `frontend/scripts/check-no-cdn.mjs` gains `fonts.googleapis.com` and `fonts.gstatic.com` in `FORBIDDEN`. Fonts ship as woff2 under `frontend/src/assets/fonts/` with their OFL licence files.
- No new frontend dependency for JSONPath, fuzzy search or highlighting.
- Contrast: text ≥ 4.5:1 against its background, control borders and meaningful icons ≥ 3:1, in both themes. Colour is never the only signal (methods are written out, statuses carry text, results carry ✓ / ✕ / ⊘).
- Exactly one filled, coloured button on the request screen: Send (`--color-send`).
- Every icon-only button has an `aria-label` and a tooltip (`title`); copy feedback goes to an `aria-live="polite"` region.
- Secrets are never shown: "Copy resolved URL" masks secret values as `***` with `plover_core::masking::MASK`.
- New endpoints sit behind the session token like `/collections/{id}/send`. After any DTO change: `PLOVER_UPDATE_BINDINGS=1 cargo test -p plover-server --test bindings`, then commit `frontend/src/api/types.ts`. Never edit it by hand.
- Gate before every commit (`set -o pipefail` when piping): `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cd frontend && npm run typecheck && npm run lint && npm test && npm run build`.
- Tests set a temporary `PLOVER_STATE_DIR` and never touch the real state dir or keychain. Fixtures are synthetic (`example.test`, `tok-AAAA…`).
- Conventional Commit subjects, each message ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Do not push.

## Review Focus

1. **A body that is JSON but huge and deeply nested** (2 MiB, 200 levels): Pretty view, the value menu's path mapping and the JSONPath filter must not freeze the tab. Path mapping and filtering are linear, with a depth guard; the 1 MiB render cap applies before tokenizing. Test `jsonPathAt_is_linear_on_deep_input` and `filter_2mib_under_budget` (Task 7, Task 6).
2. **A response whose `content-type` says JSON but whose body is not JSON** (HTML error page from a proxy): the toolbar falls back to Raw, the filter input is disabled with "Body is not valid JSON", and the value menu does nothing. Test `json_content_type_with_html_body` (Task 6).
3. **Two collections open with environments of the same name** (`Dev` in both): the top-bar label always names the active tab's collection, and switching tabs between them changes the collection part even though `Dev` stays. Test `label_follows_collection_with_same_env_name` (Task 2).
4. **A stored sidebar width larger than the current window** (state saved on a 2560 px monitor, opened on a 1280 px laptop): the sidebar clamps to 50% of the window on load and on resize, without rewriting the stored value until the user drags. Test `stored_width_clamps_to_window` (Task 3).
5. **A run with many requests of the same name across folders and some unique ones** (Bruno collections often have several `login`): the folder line appears on every row of a duplicated name and on no other row, and stays correct while the run is still adding rows. Test `folder_line_only_for_duplicates_live` (Task 9).

---

### Task 0: Verify the merged interfaces

No code. Run this before Task 1, on `main` after both merges.

- [ ] **Step 1: Check that these names exist and have these shapes.** Use `codegraph explore` or grep. Update this plan where a name differs before starting.
  - Run history (Tasks 1–8 of its plan): `plover_core::run_record::{RequestDetail, ResponseDetail, BodyDetail, ResultRecord}`, `plover_core::masking::{MASK, Masker, secret_strings}`, `plover_core::report::{json, html, junit}::render`, `RunEventDto::Accepted { run_id, record_id }` and `index` on passed/failed/skipped, `RunMetaDto`, `RunDto`, `RunResultDetailDto`, routes `GET /collections/{id}/runs`, `GET …/runs/{runId}`, `GET …/runs/{runId}/results/{index}`, `DELETE …/runs`, frontend `components/RunResultDetail.tsx` and `components/RunHistoryMenu.tsx` taking a `runId` prop, `useRuns` with `recordId`, `source`, `selected`, `detail`, `select`, `loadHistory`, `openHistory`, `clearHistory`, and `ResponseViewer`'s optional `request?: RequestDetailDto` prop.
  - curl/HAR (Tasks 6–12 of its plan): `Tab.item: string | null`, `TabsState.newScratch`, `saveScratch`, `isDirty`, `applyCurl`, `SendBody.item: Option<String>`, `ApiClient.copyCurl`, `components/request/CopyCurlMenu.tsx`, `components/request/CurlBanner.tsx`, `UrlBar`'s `onCurl` prop, the "New request" and "Import HAR…" entries in `SidebarHeader`.
- [ ] **Step 2: Record the outcome** as a short note at the top of this plan ("verified on <commit>", plus any renamed items) and commit it with the first task.

### Task 1: Swiss Minimal tokens, bundled fonts, contrast test

**Files:**
- Modify: `frontend/src/styles.css` (token block for light and dark; replace raw colours in rules with tokens), `frontend/scripts/check-no-cdn.mjs` (`FORBIDDEN` += two font hosts), `frontend/src/main.tsx` (import `./assets/fonts/fonts.css`)
- Create: `frontend/src/assets/fonts/fonts.css`, woff2 files for IBM Plex Sans 400/500/600 and JetBrains Mono 400/600 (latin + latin-ext subsets), `frontend/src/assets/fonts/OFL-IBMPlexSans.txt`, `frontend/src/assets/fonts/OFL-JetBrainsMono.txt`
- Create: `frontend/src/lib/contrast.ts` (WCAG relative luminance and ratio)
- Test: `frontend/src/theme.test.ts`, `frontend/scripts/check-no-cdn.test.mjs` (or the existing check's test, if there is one)

**Interfaces:**
- Produces, as custom properties on `:root` (light) and inside `@media (prefers-color-scheme: dark)` (dark); these are the only colour names components may use:
  - Surfaces and text: `--bg`, `--bg-2`, `--bg-3`, `--fg`, `--muted`, `--placeholder`, `--border`, `--border-strong`, `--focus`.
  - Send: `--color-send`, `--color-on-send`.
  - Meaning: `--ok`, `--info`, `--warn`, `--err` (status groups 2xx/3xx/4xx/5xx), `--var-defined-bg`.
  - Methods: `--get`, `--post`, `--put`, `--patch`, `--delete`, `--method-other`.
  - Shape and type: `--radius: 3px`, `--font-ui: "IBM Plex Sans", system-ui, sans-serif`, `--font-mono: "JetBrains Mono", ui-monospace, Menlo, monospace`, `--fs-dense: 13px`, `--fs-body: 14px`.
- Light values (spec §1): `--bg #ffffff`, `--bg-2 #f6f7f8`, `--fg #111418`, `--muted #5f6670`, `--border #e3e5e8`, `--color-send #111418`, `--color-on-send #ffffff`, `--get #15803d`, `--post #b45309`, `--put #1d4ed8`, `--patch #7c3aed`, `--delete #b91c1c`, `--ok #15803d`, `--warn #b45309`, `--err #b91c1c`. Dark values (spec §1, Claude's call): `--bg #16181c`, `--bg-2 #1d2025`, `--fg #e6e8eb`, `--border #30343a`, `--color-send #e6e8eb`, `--color-on-send #16181c`; dark method colours are today's dark values. The implementer picks `--border-strong`, `--muted`, `--info`, `--bg-3`, `--placeholder` and `--focus` so that the contrast test passes, and writes each ratio in a comment as the current file does.
- `export function contrast(a: string, b: string): number` in `lib/contrast.ts` (hex in, ratio out).
- `--accent` / `--accent-fg` are removed. Every former use either becomes `--color-send` (Send only) or a neutral outline.
- No `box-shadow` except `--focus` rings; popovers use a 1 px `--border-strong` border.

- [ ] **Step 1: Write the failing tests** in `frontend/src/theme.test.ts`. It reads `styles.css` as text (`?raw` import) and parses the two token blocks.
  - `text_tokens_meet_4_5`: for each theme, `--fg`, `--muted`, `--placeholder`, every method token and `--ok/--info/--warn/--err` against `--bg` and `--bg-2` ≥ 4.5; `--color-on-send` on `--color-send` ≥ 4.5.
  - `control_borders_meet_3`: `--border-strong` and `--focus` against `--bg` and `--bg-2` ≥ 3.
  - `send_defaults_to_black`: light `--color-send` is `#111418` and `--color-on-send` is `#ffffff` (the pending decision; update here if it changes).
  - `no_raw_colours_outside_tokens`: no `#rgb`, `#rrggbb` or `rgb(` in `styles.css` outside the two token blocks.
  - `fonts_are_local`: `fonts.css` has `@font-face` rules whose `src` are relative `url(./…woff2)`, each with `font-display: swap`.
  - In `check-no-cdn`: a built `dist/` containing `fonts.googleapis.com` fails the check.
- [ ] **Step 2: Run** `cd frontend && npm test -- theme` → FAIL.
- [ ] **Step 3: Implement.** Fill the token blocks, move every rule to tokens, add `@font-face` rules, set `body { font-family: var(--font-ui); font-size: var(--fs-body) }` and `code, pre, .mono, .monaco-editor { font-family: var(--font-mono) }`, set Monaco's `fontFamily` in `editor/monaco.ts` from the same stack, and use tabular figures (`font-variant-numeric: tabular-nums`) for times and sizes.
- [ ] **Step 4: Run** the gate, including `npm run build` and the CDN check → PASS. Open the dev server once in light and dark and compare it with `docs/superpowers/specs/2026-10-23-ui-mockups/style-light.html` option 1.
- [ ] **Step 5: Commit** `feat(ui): Swiss Minimal theme tokens and bundled fonts`.

### Task 2: Environment label in the top bar

**Files:**
- Modify: `frontend/src/components/EnvSelector.tsx`, `frontend/src/App.tsx` (pass the collection name), `frontend/src/styles.css`
- Test: `frontend/src/components/ux.test.tsx`, `frontend/src/components/a11y.test.tsx`

**Interfaces:**
- Consumes: `useWorkspace().collections` (name by id), `activeEnv`, `selectEnv`.
- Produces: `EnvSelector({ collectionId })` renders `<span class="env-collection">{name}</span> · <select aria-label="Environment for {name}">…</select>` followed by `Edit`. No colour classes, no danger level, no confirmation.

- [ ] **Step 1: Write the failing tests.**
  - `label_shows_collection_and_env`: with collection `signing-api` and env `Dev` → visible text `signing-api · ` and the select's value `Dev`; the select's accessible name is `Environment for signing-api`.
  - `label_follows_collection_with_same_env_name` (Review Focus 3): collections `a` and `b` both with `Dev`; switching the active tab from `a` to `b` changes the label to `b · Dev`.
  - `no_confirmation_for_stg`: selecting env `Stg` and sending opens no modal (the send is called once).
  - `long_collection_name_truncates_first`: the collection span has `title` with the full name and the CSS class that applies `text-overflow: ellipsis`; the select keeps `min-width: 12ch`.
- [ ] **Step 2: Run** `cd frontend && npm test -- ux a11y` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the frontend gate → PASS.
- [ ] **Step 5: Commit** `feat(ui): label the environment with its collection`.

### Task 3: Resizable sidebar with a remembered width

**Files:**
- Modify: `crates/core/src/app_state.rs` (`PersistedState.layout`), `crates/server/src/api/app_state.rs` and `api/mod.rs` (routes), `crates/server/src/api/dto.rs`, `frontend/src/api/client.ts`, `frontend/src/App.tsx`, `frontend/src/styles.css` (drop the `--sidebar-w` clamp)
- Create: `frontend/src/components/SidebarResizer.tsx`, `frontend/src/store/layout.ts`
- Test: `crates/core/tests/app_state.rs` (or the module's tests), `crates/server/tests/app_state.rs`, `frontend/src/components/layout.test.tsx`

**Interfaces:**
- Produces (core): `pub struct LayoutState { pub sidebar_width: Option<u32> }` (`#[serde(default, rename_all = "camelCase")]`), field `#[serde(default)] pub layout: LayoutState` on `PersistedState`. Old `state.json` files without `layout` load unchanged; `STATE_VERSION` stays.
- Produces (server): `GET /layout` → `LayoutDto { sidebarWidth: number | null }`; `PUT /layout` with the same body → `204`. Values outside `120..=4000` → 400 `bad_layout`.
- Produces (frontend): `ApiClient.layout(): Promise<LayoutDto>`, `ApiClient.saveLayout(l: LayoutDto): Promise<void>`; `useLayout` store `{ sidebarWidth: number | null; setSidebarWidth(px: number | null): void }` that saves 300 ms after the last change; `SidebarResizer` with `role="separator"`, `aria-orientation="vertical"`, `aria-valuenow/min/max`, `aria-label="Resize sidebar"`.
- Rules (spec §3, Claude's call C4): min 180 px, max 50% of the window; arrow keys ±16 px; Home/End to min/max; double-click or Enter resets to `null` (default 260 px); the effective width is `clamp(180, stored ?? 260, 0.5 × window)`.

- [ ] **Step 1: Write the failing tests.**
  - Core: `layout_defaults_and_round_trips` (missing `layout` → `sidebar_width: None`; save and load `Some(320)`).
  - Server: `layout_get_put` and `layout_rejects_out_of_range` (`50` and `5000` → 400 `bad_layout`); the routes need the token.
  - Frontend: `drag_resizes_within_bounds` (pointer events on the separator from 260 to 100 → 180; to 2000 in a 1000 px window → 500), `keys_resize_by_16`, `double_click_resets`, `width_survives_reload` (the store loads the saved value from `api().layout()`), `stored_width_clamps_to_window` (Review Focus 4: stored 1200 in a 1280 px window → effective 640, `saveLayout` not called until the user drags).
- [ ] **Step 2: Run** the core, server and frontend tests → FAIL.
- [ ] **Step 3: Implement.** The sidebar's width is an inline `style={{ width }}`; a window `resize` listener reclamps without saving.
- [ ] **Step 4: Regenerate bindings, run** the gate → PASS.
- [ ] **Step 5: Commit** `feat(ui): resizable sidebar with remembered width`.

### Task 4: URL bar — Send, `⋯` menu, unsaved dot

**Files:**
- Modify: `frontend/src/components/request/UrlBar.tsx`, `frontend/src/components/request/RequestEditor.tsx`, `frontend/src/components/RequestTabs.tsx` (unsaved dot), `frontend/src/components/request/CopyCurlMenu.tsx` (becomes two menu items), `frontend/src/api/client.ts`
- Modify (server): `crates/server/src/api/requests.rs` (resolved URL), `crates/server/src/native.rs` (`NativeHost::reveal`), `crates/server/src/api/native.rs`, `crates/server/src/api/mod.rs`, `crates/server/src/api/dto.rs`, `crates/desktop/src/host.rs`
- Create: `frontend/src/components/request/RequestMenu.tsx`, `frontend/src/components/Menu.tsx` (generic menu button: `aria-haspopup="menu"`, roving focus, Esc returns focus), unless one exists after the merges
- Test: `crates/server/tests/api.rs`, `frontend/src/components/request/request-menu.test.tsx`, `frontend/src/components/a11y.test.tsx`

**Interfaces:**
- Consumes: `ApiClient.copyCurl` and `CopyCurlBody` (curl/HAR), `Masker`/`secret_strings` (run history), `resolve::prepare`/`finalize` as the curl endpoint uses them.
- Produces (server):
  - `POST /collections/{id}/resolved-url` with `ResolvedUrlBody { item: Option<String>, draft: Option<HttpRequestDto>, env: Option<String> }` → `ResolvedUrlDto { url: String, masked: bool }`. Variables are filled in like Copy as cURL "with variables filled in" (no scripts run); every secret value of `env` in the URL is replaced with `***` and `masked` is true when that happened.
  - `NativeHost::reveal(&self, path: &Path) -> HostFuture<'_, Result<(), String>>` (desktop: Tauri opener's reveal-in-folder); `POST /native/reveal` with `{ item: String, collection: String }` resolves the path through `resolve_item_path` and calls it; 404 `not_desktop` in web mode. `CapabilitiesDto` gains `reveal: bool`.
- Produces (frontend):
  - `UrlBar` row: method select · URL input · Send (`class="send"`, the only element using `--color-send`) · `RequestMenu` (`⋯`, `aria-label="More request actions"`). The separate Save button is removed from the row.
  - `RequestMenu` items in order: Copy as cURL, Copy as cURL (variables filled in), Copy resolved URL, Save (⌘S), Reveal .bru file (desktop, when `capabilities.reveal`) or Copy file path (web). Scratch tabs: Save opens `SaveRequestDialog`; the file items are hidden.
  - `RequestTabs`: a dirty tab shows a dot (`aria-label="Unsaved changes"`) instead of nothing.

- [ ] **Step 1: Write the failing tests.**
  - Server: `resolved_url_masks_secrets` (URL `{{host}}/a?k={{apiKey}}`, `apiKey` secret `s3cr3t-value` → `https://example.test/a?k=***`, `masked: true`), `resolved_url_scratch` (`item: null` with a draft), `reveal_in_web_mode_is_404`, `reveal_rejects_escape` (`item: "../x.bru"` → 400 `bad_item`).
  - Frontend: `only_send_is_filled` (exactly one element with class `send` on the request screen; no other button uses the send token — check computed `background-color` against `--color-send` with the test's CSS loaded), `menu_order_and_keyboard` (items as listed; Arrow keys move; Esc closes and focuses `⋯`), `copy_resolved_url_writes_masked_text`, `save_item_saves` (calls the store's `save`), `scratch_menu_hides_file_items`, `dirty_tab_shows_dot`.
- [ ] **Step 2: Run** the server and frontend tests → FAIL.
- [ ] **Step 3: Implement.** Move the two Copy as cURL choices from `CopyCurlMenu` into `RequestMenu` and delete the split button; keep its confirm-before-secret and omitted-parts toast.
- [ ] **Step 4: Regenerate bindings, run** the gate → PASS.
- [ ] **Step 5: Commit** `feat(ui): one coloured Send button and a request actions menu`.

### Task 5: Response status line, icon copy, pinned headers, Headers tab copy

**Files:**
- Modify: `frontend/src/components/ResponseViewer.tsx`, `frontend/src/components/SubTabs.tsx` (counts), `frontend/src/components/icons.tsx` (`CopyIcon`, `DownloadIcon` if missing), `frontend/src/styles.css`
- Create: `frontend/src/components/response/StatusLine.tsx`, `frontend/src/components/response/PinnedHeaders.tsx`, `frontend/src/components/response/HeadersTable.tsx`, `frontend/src/lib/status.ts`, `frontend/src/lib/download.ts`
- Test: `frontend/src/components/response/response.test.tsx`, `frontend/src/lib/status.test.ts`

**Interfaces:**
- Produces:
  - `statusGroup(status: number | null, errorCode?: string): "ok" | "info" | "warn" | "err"` (2xx ok, 1xx/3xx info, 4xx warn, 5xx and transport errors err) and `statusText(status, reason, errorCode): string` (`404 Not Found`, `ECONNREFUSED`).
  - `StatusLine({ result })`: status chip (group token as text colour + text), `312 ms · 1.2 KB` (tabular figures), tests summary `✓ 5/5` or `✕ 3/5`, then icon buttons Copy body (`aria-label="Copy body"`) and Save to file (`aria-label="Save body to file"`); "Copied" in an `aria-live="polite"` span.
  - `downloadBody(name: string, body: ResponseDto): void` — a Blob download with a name from the request name and the content type's extension (`.json`, `.html`, `.txt`, else `.bin`); base64 bodies are decoded to bytes.
  - `PINNED_HEADERS = ["x-request-id", "x-correlation-id", "traceparent", "x-trace-id", "location"]` (Claude's call C7). `PinnedHeaders({ headers })` renders those present, in that order, each with a copy icon (`aria-label="Copy {name}"`); renders nothing when none are present.
  - `HeadersTable({ headers })`: every row shows its copy icon on hover and on focus-within; a "Copy all" button copies `name: value` lines.
  - Sub-tabs in order: Body · Headers *n* · Request · Tests *n/m* · Console *n* (Request appears when the result has a `RequestDetailDto`).

- [ ] **Step 1: Write the failing tests.**
  - `status.test.ts`: each group boundary (199, 200, 299, 300, 399, 400, 499, 500, null with `ECONNREFUSED`) and the text forms.
  - `response.test.tsx`: `copy_body_icon_copies_and_announces`, `save_to_file_names_json` (mock `URL.createObjectURL`), `pinned_headers_only_when_present` (`x-request-id` present → one row with copy; none → no element), `pinned_order_is_fixed`, `headers_copy_on_focus` (tabbing into a row reveals its copy button), `copy_all_headers`, `subtab_order_and_counts`.
- [ ] **Step 2: Run** `cd frontend && npm test -- response status` → FAIL.
- [ ] **Step 3: Implement.** Replace the existing large Copy button.
- [ ] **Step 4: Run** the frontend gate → PASS.
- [ ] **Step 5: Commit** `feat(ui): response status line with icon copy and pinned headers`.

### Task 6: Body toolbar — Pretty/Raw/Preview, search, JSONPath filter

**Files:**
- Create: `frontend/src/components/response/BodyToolbar.tsx`, `frontend/src/components/response/BodyPreview.tsx`, `frontend/src/lib/jsonpath.ts`
- Modify: `frontend/src/components/ResponseViewer.tsx`, `frontend/src/editor/CodeEditor.tsx` (read-only mode with find widget, `onReady(editor)` callback)
- Test: `frontend/src/lib/jsonpath.test.ts`, `frontend/src/components/response/body-toolbar.test.tsx`

**Interfaces:**
- Produces:
  - `type JsonPathResult = { ok: true; values: unknown[] } | { ok: false; error: string; at: number }`.
  - `evaluateJsonPath(doc: unknown, expr: string): JsonPathResult`. Supported subset (C6): `$`, `.key`, `['key']` / `["key"]`, `[n]` (negative counts from the end), `[*]` and `.*`, `..key` (recursive descent). Anything else → `{ ok: false, error: "Unsupported at …", at }`. Depth guard 512; never throws.
  - Filter output: one value → that value pretty-printed; several → a JSON array of them (Bruno's response filter shows the same).
  - `BodyToolbar({ mode, onMode, filter, onFilter, filterError, isJson, previewKind })` with a `Pretty | Raw | Preview` segmented control (`role="radiogroup"`). Preview is enabled for `text/html` (sandboxed `<iframe sandbox="">` with `srcdoc`, no scripts), `image/*` (object URL) and `application/pdf` (object URL in `<iframe>`); otherwise disabled with `title` giving the reason.
  - Search: ⌘F / Ctrl+F while focus is inside the response pane opens Monaco's find widget (`editor.getAction("actions.find").run()`); the widget's own count and Enter/Shift+Enter are used.
  - Copy body copies the filtered result while a filter is active and valid.

- [ ] **Step 1: Write the failing tests.**
  - `jsonpath.test.ts`: `$.signers[*].email` on a three-signer doc → three emails; `..id` finds nested ids in document order; `$['a b']`; `$.list[-1]`; `$.a.*`; invalid `$.a[?(@.x)]` → `ok: false` with `at`; a 200-level nested doc does not throw; `filter_2mib_under_budget` (a generated 2 MiB array filtered by `$[*].id` in under 200 ms in the test environment).
  - `body-toolbar.test.tsx`: `filter_changes_body_and_copy` (with `$.signers[*].email` active, Copy body writes the filtered JSON), `invalid_filter_shows_error_and_keeps_body`, `json_content_type_with_html_body` (Review Focus 2: filter disabled with "Body is not valid JSON", mode falls back to Raw), `preview_html_is_sandboxed` (iframe has `sandbox=""` and no `allow-scripts`), `preview_disabled_for_plain_text`, `cmd_f_opens_find` (mocked editor action called).
- [ ] **Step 2: Run** `cd frontend && npm test -- jsonpath body-toolbar` → FAIL.
- [ ] **Step 3: Implement.** If the subset cannot be written in about 200 lines, stop and report instead of adding a dependency (spec constraint).
- [ ] **Step 4: Run** the frontend gate → PASS.
- [ ] **Step 5: Commit** `feat(ui): response body toolbar with preview, search and JSONPath filter`.

### Task 7: Value menu — copy value, copy path, save as runtime variable

**Files:**
- Create: `frontend/src/lib/jsonPathAt.ts`, `frontend/src/components/response/ValueMenu.tsx`
- Modify: `frontend/src/components/ResponseViewer.tsx`, `frontend/src/editor/CodeEditor.tsx` (click and Enter at a position → callback), `frontend/src/api/client.ts`
- Modify (server/core): `crates/core/src/workspace.rs` (`OpenCollection::set_runtime_var`), `crates/server/src/api/requests.rs` or a new `crates/server/src/api/vars.rs`, `api/mod.rs`, `api/dto.rs`
- Test: `frontend/src/lib/jsonPathAt.test.ts`, `frontend/src/components/response/value-menu.test.tsx`, `crates/server/tests/api.rs`

**Interfaces:**
- Produces (frontend):
  - `jsonPathAt(text: string, offset: number): { path: string; value: unknown; start: number; end: number } | null` — a single linear scan over the pretty-printed text (tracking keys, indexes and string escapes), returning the innermost scalar or container under the offset. Paths use `.key` when the key is an identifier, else `['key']`; indexes `[n]`. `null` outside any value or for non-JSON text.
  - `ValueMenu({ anchor, path, value, collectionId, onClose })` with items: Copy value (strings without quotes, others as JSON), Copy path, Save as runtime variable… (an inline name field prefilled from the last key, e.g. `id` → `id`, `docId` stays `docId`; validation `^[A-Za-z_][\w.-]*$`).
  - `ApiClient.setRuntimeVar(collectionId: string, name: string, value: unknown): Promise<void>`.
- Produces (server/core): `OpenCollection::set_runtime_var(&self, name: &str, value: serde_json::Value)`; `PUT /collections/{id}/runtime-vars/{name}` with `{ value }` → 204; invalid name → 400 `bad_name`. The next send sees it (`bru.getVar`, `{{name}}`), exactly like `bru.setVar`.

- [ ] **Step 1: Write the failing tests.**
  - `jsonPathAt.test.ts`: offsets on `"id"`'s value → `$.id`; inside `signers[0].email` → `$.signers[0].email`; key `"a b"` → `$['a b']`; escaped quotes in strings do not break the scan; offset on whitespace between members → `null`; `jsonPathAt_is_linear_on_deep_input` (Review Focus 1: 200-level nesting, 2 MiB, under 100 ms).
  - `value-menu.test.tsx`: `click_value_opens_menu`, `enter_on_value_opens_menu` (keyboard), `copy_value_strips_quotes`, `copy_path_for_nested`, `save_as_variable_calls_api_with_prefilled_name`, `invalid_name_blocks_save`, `esc_returns_focus_to_editor`.
  - Server: `runtime_var_set_is_seen_by_next_send` (set `docId` → a send to `{{baseUrl}}/d/{{docId}}` hits `/d/<value>`), `runtime_var_bad_name_400`.
- [ ] **Step 2: Run** the tests → FAIL.
- [ ] **Step 3: Implement.** The menu works on the Pretty view of the rendered (possibly capped) text only; with a JSONPath filter active, paths are relative to the filtered document and Copy path is disabled with a `title` explaining why.
- [ ] **Step 4: Regenerate bindings, run** the gate → PASS.
- [ ] **Step 5: Commit** `feat(ui): copy a JSON value or its path, or keep it as a variable`.

### Task 8: Large bodies — collapse and render cap

**Files:**
- Modify: `frontend/src/components/ResponseViewer.tsx`, `frontend/src/editor/CodeEditor.tsx` (fold ranges), `frontend/src/lib/body.ts`
- Test: `frontend/src/components/response/large-body.test.tsx`, `frontend/src/lib/body.test.ts`

**Interfaces:**
- Produces: `RENDER_CAP = 1 << 20`; `renderSlice(text: string): { shown: string; capped: boolean; total: number }`; `foldTargets(text: string, threshold = 50): Array<{ startLine: number; endLine: number }>` (arrays and objects with more than 50 direct entries). The viewer shows "Showing the first 1 MiB of 12.4 MB" with a "Show all" button when capped; Copy body and Save to file always use the full body.

- [ ] **Step 1: Write the failing tests.**
  - `renders_first_mib_and_copies_all` (2 MiB body: editor value length ≤ `1 << 20`, Copy writes all 2 MiB), `show_all_renders_everything`, `big_arrays_start_folded` (`foldTargets` lists a 60-entry array, not a 40-entry one), `capped_text_is_not_reparsed_as_json` (the value menu uses `jsonPathAt` only on the shown slice).
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement.** Apply folding through `editor.trigger("fold", "editor.fold", { selectionLines })` after the model loads.
- [ ] **Step 4: Run** the frontend gate → PASS.
- [ ] **Step 5: Commit** `feat(ui): fold large JSON and cap rendering at 1 MiB`.

### Task 9: Runner as a tab

**Files:**
- Modify: `frontend/src/store/tabs.ts` (`kind`), `frontend/src/store/runs.ts`, `frontend/src/components/RequestTabs.tsx`, `frontend/src/App.tsx` (remove the bottom `RunPanel` and `showRun`; sidebar ▶ opens a run tab), `frontend/src/store/uiState.ts` (never persist run tabs)
- Create: `frontend/src/components/run/RunTab.tsx`, `frontend/src/components/run/RunFilter.tsx`, `frontend/src/components/run/RunList.tsx`, `frontend/src/lib/runRows.ts`
- Delete: `frontend/src/components/RunPanel.tsx` (its row builder moves to `lib/runRows.ts`; `RunResultDetail` and `RunHistoryMenu` are reused as they are)
- Test: `frontend/src/lib/runRows.test.ts`, `frontend/src/components/run/run-tab.test.tsx`, `frontend/src/store/tabs.test.ts`, `frontend/src/components/a11y.test.tsx`

**Interfaces:**
- Consumes: `useRuns` (`start`, `cancel`, `select`, `loadHistory`, `openHistory`), `RunResultDetail({ runId })`, `RunHistoryMenu({ collectionId, onChoose })`, `RunEventDto` with `index`.
- Produces:
  - `Tab` becomes `RequestTab | RunTab` with `kind: "request" | "run"`; `RunTab = { kind: "run"; key: string /* `${collectionId}:run:${runId}` */; collectionId: string; runId: string; label: string }`. Existing request-tab code narrows on `kind === "request"`. `useTabs.openRun(collectionId, runId, label): string` opens and focuses it; `uiState` save filters out run tabs (C12).
  - `buildRows(events: RunEventDto[]): RunRow[]` with `RunRow = { index: number; item: string; name: string; method: string; folder: string; state: "running" | "pass" | "fail" | "skip"; status?: number; ms?: number }` (rows keyed by `index`, never by item).
  - `duplicateNames(rows: RunRow[]): Set<string>` — names that occur on two or more rows; a row shows its folder line only when its name is in the set.
  - `type RunFilterValue = "all" | "fail" | "pass" | "skip"`; `RunFilter({ counts, value, onChange })` renders a `role="radiogroup"` of `All N | Fail N | Pass N | Skip N`; counts are computed from rows (running rows count only in All). Default `all`; it never changes by itself when a run ends (C12). A name filter input sits next to it.
  - `RunTab` layout: header (title `Run · <label>`, `env · time · duration`; right side, in order: `RunHistoryMenu` "Previous runs ▾", an export slot, Re-run, a "Run with data…" slot), filter row, then list (left) and `RunResultDetail` (right). The failure reason box is the first thing in the detail. `↗ Open request` opens the request tab and leaves the Run tab open.
  - Slots: `RunTab` takes `exportSlot?: ReactNode` and `runWithDataSlot?: ReactNode` props, filled by the data-driven/export work (Task 10).
  - Choosing a previous run replaces the run shown in this tab (`openHistory` then retarget `runId`), not a new tab (C12).

- [ ] **Step 1: Write the failing tests.**
  - `runRows.test.ts`: rows by index for repeated items; `duplicateNames` with `login` ×3 and `me` ×1 → `{login}`; `folder_line_only_for_duplicates_live` (Review Focus 5: add rows one event at a time; after the second `login` arrives both `login` rows show folders and `me` never does).
  - `run-tab.test.tsx`: `sidebar_run_opens_and_focuses_run_tab`, `two_runs_two_tabs`, `filter_counts_match_report` (counts equal `RunReportDto` passed/failed/skipped at `finished`), `fail_filter_lists_only_failures`, `filter_does_not_jump_when_run_ends`, `name_filter_narrows`, `failure_reason_on_top`, `open_request_keeps_run_tab`, `previous_run_replaces_content`, `rerun_starts_same_target`, `run_tabs_not_persisted`, `no_bottom_panel` (`App` renders no `RunPanel`).
  - `a11y.test.tsx`: the filter is a radiogroup with names including counts; list rows are focusable with `aria-selected`.
- [ ] **Step 2: Run** `cd frontend && npm test -- run tabs a11y` → FAIL.
- [ ] **Step 3: Implement.** Compare with `docs/superpowers/specs/2026-10-23-ui-mockups/runner-v2.html` (but folder lines only on duplicate names) and `overall-v1.html` option A (▶ buttons unchanged, no menu).
- [ ] **Step 4: Run** the frontend gate and `cargo test --workspace` (embedded UI) → PASS.
- [ ] **Step 5: Commit** `feat(ui): open runs in a tab with a status filter`.

### Task 10: Wire the data-driven/export controls, docs, screenshots, final check

**Files:**
- Modify: `frontend/src/components/run/RunTab.tsx` call site (fill `exportSlot` and `runWithDataSlot` with the components from the data-driven/export work when that work has landed; otherwise leave the slots empty and note it), `README.md` (screenshots, keyboard section), `docs/ui-ux-review.md` (point to the new tokens), `docs/screenshots/*.png`
- Test: the whole gate

- [ ] **Step 1:** If the data-driven/export UI exists on `main`, mount its export menu and "Run with data…" button in the slots and add `run_tab_has_export_and_run_with_data` to `run-tab.test.tsx`. If it does not, skip and say so in the commit message.
- [ ] **Step 2:** Re-capture `docs/screenshots/` in light and dark with the "Demo API" fixture (`fixtures/phase3-chain`), as the earlier screenshot work did.
- [ ] **Step 3:** Run the full gate, then start `plv serve` against the fixture and check by hand: one coloured button, label `Demo API · local`, sidebar drag, value menu, a run tab with Fail filter.
- [ ] **Step 4: Commit** `docs: screenshots and notes for the new look`.

## Self-review

- **Spec coverage.** §1 → Task 1; §2 → Task 2; §3 resizable sidebar → Task 3, URL bar/⋯/Save/unsaved dot → Task 4, ▶ unchanged → Task 9 (no menu); §5 status line/icons/pinned/headers → Task 5, toolbar/preview/search/JSONPath → Task 6, value menu → Task 7, large bodies → Task 8; §6 → Task 9 and Task 10. §4, §7, §9 are in `2026-10-24-dev-ux-cancel-quickopen-vars.md`.
- **Spec tests.** Contrast pairs, CDN check, one accent → Task 1/4. Env label and no Stg confirmation → Task 2. Sidebar bounds and reload → Task 3. Filtered copy, `$.signers[*].email`, `..id`, value path, saved variable, 2 MiB render, pinned headers → Tasks 5–8. Runner tab, counts, duplicate folder line, ↗, previous run → Task 9.
- **Type consistency.** `RunRow`, `RunFilterValue`, `RunTab` are defined in Task 9 only; `ResolvedUrlDto`, `LayoutDto` and `setRuntimeVar` are defined once and used only in their tasks.
- **Proportion.** No bodies are written out except the token values and the JSONPath subset that the spec fixes.
