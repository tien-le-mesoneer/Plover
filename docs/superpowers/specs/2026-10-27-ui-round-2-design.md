# UI round 2: collapsible collections, resizable split, request history, value copy

Date: 2026-10-27 · Base: `main` at `60a6ce9` (v0.1.0)

## Intent

After using v0.1.0 day to day, the user asked for seven changes. Four are about **finding
and copying values faster**: copy a variable's value, copy a param's value, and a cleaner
response/request corner. Two are about **space**: collapse a whole collection, and resize
between request and response. The last, request history, lets you **see what you sent
before**, the same way run history works for automated runs.

The UI should get quieter, not busier. Controls nobody needs at a given moment move out of
sight until the pointer or keyboard focus reaches them, and are never out of reach for
keyboard or touch users.

## Goals

1. **Collapse a collection** (the user's answer: "thu gọn collection"). Clicking the
   collection name (with a ▸/▾ caret) folds its whole tree. The state is remembered per
   collection across restarts. Keyboard: Enter or Space on the header toggles it, and the
   header has `aria-expanded`.
2. **Resizable request/response split, side by side or stacked.** A drag handle sits
   between the two panes: arrow keys move it, Home/End go to the limits, and Enter or a
   double-click resets it. A small toggle button switches between side by side
   (left/right) and stacked (top/bottom). The ratio and the layout choice are remembered
   across restarts, following the same pattern as the sidebar width.
3. **Request history** (the user's answer: "every Send, saved"). Every Send from the UI,
   including scratch tabs, is recorded with its request and response. Values are masked the
   same way run history masks them. The last 100 sends are kept, in one list for all
   collections that can be filtered by collection, and the list survives restarts. Clicking
   an entry shows the saved request and response read-only, with **Open request** and
   **Open as new request**.
4. **Copy a variable's value from its hover card** (the user's answer: "hover card of
   {{var}}"). The card gets a copy button that copies **only the value**: no `{{name}}`,
   no scope label.
5. **Copy a param's value** (the user's answer: "copy each param"). Each Query/Path row in
   the Params table shows a copy icon on hover or focus. It copies that param's value with
   variables filled in for the selected environment.
6. **Collection header buttons show on hover.** Run collection, Reload and Close (and
   Retry/Remove on an unavailable collection) appear when the header is hovered or
   contains keyboard focus. They are always visible on devices without hover. The folder ▶
   already works this way.
7. **Copy in the top-right corner.** The response body's Copy (and Save to file next to
   it) sits in the body box's top-right corner and appears on hover or focus. The same
   applies to the request body editor, which gets a Copy request body button.

## Non-goals

- No history of `plv run` sends: those are already in run history.
- No per-request History sub-tab (the user chose a single global list). Filtering by
  collection covers it.
- No search inside history bodies. Filtering by collection is enough for 100 entries.
- No re-send straight from history: **Open as new request**, then Send.
- No grouping of several collections into named groups (the user chose collapse only).

## Anti-goals

- A secret reaching the page, the clipboard or the history file without the user asking.
  Masked values stay masked, and copying a secret value needs an explicit confirmation.
- Hover-only access. Every control that hides until hover must also show on
  `:focus-within`, stay reachable with Tab, and be visible under `@media (hover: none)`.
- Layout jumps. Hidden buttons keep their space (`opacity`/`visibility`), so names don't
  shift when the pointer enters.

## Decisions

Marked **user** where the user decided, **call** where this design decides.

### 1. Collapse a collection

- **call:** The header gets a toggle button wrapping the caret and the name
  (`<button aria-expanded aria-controls={treeId}>`), so the action buttons stay outside it.
  Folding hides the `CollectionTree` and the "No matching requests" line.
- **call:** While the request filter has text, a collapsed collection still shows its
  matches. The filter wins, as it already does for folders. The remembered state is
  unchanged.
- **call:** Storage is a new `collapsed: bool` on `UiState`/`UiStateDto` (default
  `false`), saved by the existing debounced `startUiPersistence`. Older state files read as
  expanded.

### 2. Request/response split and layout toggle

- **user:** A button switches the split between side by side (left/right) and stacked
  (top/bottom). The default is side by side, and the choice is remembered. If the window
  is too narrow for side by side, the panes stack automatically. The saved choice comes
  back when the window widens. This replaces today's fixed "stacked below 1100 px" CSS
  rule.
- **call — storage:** Each orientation keeps its own ratio, because a 30 % width does not
  mean a useful 30 % height. `LayoutState`/`LayoutDto` gain three fields:
  - `splitOrientation: "side" | "stacked" | null`, where `null` = side;
  - `splitPermille: Option<u16>`, the side-by-side ratio;
  - `stackedSplitPermille: Option<u16>`.

  Both ratios are valid 200..=800, and `None` = 500. Integers keep `Eq` on the struct.
- **call — "too narrow":** Side by side needs the `.split` box (measured with
  `ResizeObserver`, not the window) to be at least `2 × 240 + 6` px. That is both panes'
  240 px minimum plus the handle. Below that the panes are stacked whatever the choice,
  and nothing is saved, so the stored choice survives.
- **call — button when forced:** The button shows the **effective** state (stacked,
  `aria-pressed="true"`). It uses `aria-disabled="true"`, not `disabled`, so it stays
  focusable and its tooltip can give the reason: "Too narrow to place side by side".
  Activating it then does nothing.
- **call — placement:** The button sits at the right end of the request pane's section tab
  bar (Params · Headers · … · Body). That bar is always visible, unlike the response
  status line, which only appears after a send. The button is a 24 × 24 px icon-only
  toggle with two SVG icons (two columns or two rows, in the stroke style of `icons.tsx`).
  - `aria-label` is "Stack request and response" when side by side, and "Place request
    and response side by side" when stacked.
  - `aria-pressed` is true when stacked.
  - The tooltip (`title`) repeats the label.
  - It has the standard visible focus ring.
- **call:** Each pane keeps a minimum of 240 px wide (side by side) or 120 px tall
  (stacked), whatever the ratio. Keyboard step: 2 % per arrow key.
- **call:** The new `PaneResizer` copies `SidebarResizer`'s behaviour and ARIA: separator,
  `aria-valuenow` as a percentage, and the title "Drag to resize · double-click to reset".
  Its `aria-orientation` follows the effective layout: `vertical` side by side,
  `horizontal` stacked. Run tabs keep their own layout; the split is only in the request
  editor.

### 3. Request history

- **user:** Every Send is recorded, global list of the last 100, survives restarts.
- **call:** Only UI sends (`POST /collections/{id}/send`). Cancelled sends are not
  recorded, because no response came back and runtime changes were dropped. Sends that
  failed in transport are recorded with their error.
- **call:** Storage reuses run history's types: a `SendRecord` holds a header
  (`id: RunId`, `sentAtMs`, `collectionRoot`, `collectionName`,
  `item: Option<String>` where `None` = scratch, `env`) and one `ResultRecord`. That gives
  the request and response detail, tests, asserts, console and the 1 MiB-per-body cap from
  `RequestDetail`/`ResponseDetail`. Files are `<state_dir>/sends/<id>.json` plus
  `<id>.meta.json` for listing, the same layout as `runs/`. History is global and pruned to
  `KEEP_SENDS = 100` after each save. Without a state directory, history is kept in memory
  only, like run history.
- **call:** Masking is the same `Masker` run history uses: environment secrets, values
  sent in credential headers, and every rule `2026-10-24-follow-up-gaps.md` Task 3 adds
  (header tokens, JWTs, by-name passes) once that lands. A send record is masked before it
  is written. The page only ever receives masked records.
- **call:** Saving never fails the send. A write error shows in the response as a small
  "Not saved to history: …" note and is logged.
- **call:** History is not turned off by `CI` (that switch is for `plv run`).
- **user:** `plv serve` gets **no** `--no-history` flag. The **Clear history** button
  (with confirmation, for all collections or the filtered one) is enough.
- **call — placement:** A **History** button (clock icon) next to New request and Import
  HAR above the sidebar opens a **History tab**. The tab works like run tabs: one instance,
  never remembered across restarts. It has a list on the left (method, name or "Scratch",
  status, duration, relative time, collection name when showing all) and a read-only
  detail on the right. The detail reuses the run-result detail view (Response, Headers,
  Request, Tests, Console). There is a collection filter at the top (`All` + open
  collections, plus closed collections still in history by name) and a Clear history
  button. This matches the Runner tab the user already approved ("list + detail pane").
- **call:** **Open request** opens the saved `.bru` in its collection. It is disabled, with
  a reason in the title, when the item no longer exists, the collection is closed, or the
  entry is a scratch send. **Open as new request** opens a scratch tab with the recorded
  method, URL, headers and body **as sent**: variables are already filled in, and masked
  values show as `***`. A banner says "Values masked in history show as ***; fill them in
  before sending."

### 4. Copy a variable's value

- **user:** Copy only the value.
- **call:** The card stays the same card, but it becomes interactive:
  - It stays open while the pointer is over the input **or** the card, with a 150 ms grace
    period to cross the gap.
  - It gets a 24 × 24 px **Copy value** icon button.
  - Its role changes from `tooltip` (which must not contain buttons) to a non-modal
    popover: a `div` with `role="dialog"`, `aria-label="{{name}}"`, linked from the input
    with `aria-describedby` for the text.
- **call:** Keyboard path: with the caret inside a token, **⌘/Ctrl+Shift+C** copies that
  token's value and announces "Copied value of {{name}}" in a live region. The card shows
  the shortcut in its hint line. In the Monaco body editor, the same shortcut is registered
  with `editor.addAction` ("Copy variable value"). The hover markdown adds "⌘⇧C copies the
  value", because Monaco hovers cannot hold buttons we control.
- **call:** The value is fetched when copying, not taken from the card. A new route,
  `POST /collections/{id}/resolve-text` with `{ item, draft, env, text, reveal }`, resolves
  `text` (`{{name}}` here) in the tab's scope and returns
  `{ text: string | null, containsSecret: boolean }`. When `containsSecret` is true and
  `reveal` is false, `text` is `null`, so the secret never reaches the page. The UI then
  asks with the same confirmation Copy as cURL uses ("This value is a secret. Copy
  anyway?"), and only on yes asks again with `reveal: true` and writes the result to the
  clipboard. Undefined and "set later" variables have no copy button.
- **user:** **Save as variable** in the JSON value menu no longer puts `{{name}}` on the
  clipboard. It only saves the runtime variable and shows the "Saved {{name}}" note.

### 5. Copy a param's value

- **user:** Copy each param.
- **call:** Query and Path rows get a trailing copy button, using the same pattern as
  `HeadersTable`'s `.row-copy`: visible on row hover and `:focus-within`, and always under
  `hover: none`. The button copies the row's value resolved through `resolve-text`
  (variables filled in for the selected environment), with the same secret confirmation as
  item 4. Disabled rows can still be copied. Header and Assert tables don't get it: the
  user asked for params.

### 6. Hover-only header buttons

- **call:** `.collection-header .icon` buttons use `opacity: 0` and become `opacity: 1` on
  `.collection-header:hover`, `:focus-within` and `@media (hover: none)`. They stay in the
  Tab order, unlike the folder ▶, which is `tabIndex=-1` and has Ctrl/Cmd+Enter. Folder ▶
  is unchanged. Retry/Remove on an unavailable collection stay **always visible**: they
  are the only way out of that state.

### 7. Copy in the corner

- **call:** A small `CornerActions` wrapper places icon buttons absolutely in the top-right
  corner of a body box (8 px inset), shown on box hover and `:focus-within`, and always
  under `hover: none`. They sit above the editor's scrollbar gutter. Response: Copy body
  and Save to file move out of `StatusLine` into the body box corner. The "Copied" live
  note stays in the status line so screen readers keep hearing it. Request: the Body tab's
  text editors (JSON, XML, text, GraphQL query) get Copy request body. Form and multipart
  bodies have no corner copy.
- **user:** Copy request body copies the **resolved** text, with variables filled in for
  the selected environment. It goes through `resolve-text`, with the same secret rule as
  the variable card: when a secret is involved, confirm first, and only then fetch with
  `reveal: true`. `resolve-text` therefore accepts up to 1 MiB of text, the body cap.
- **call:** When a JSONPath filter is active, Copy still copies the filtered result, as
  today.

## Interaction with `2026-10-24-follow-up-gaps.md`

Both plans touch `Sidebar.tsx`, the masking code and the top bar:

- Follow-up Task 7 adds a context menu on the collection header and folders. Round-2 items
  1 and 6 change the same header markup. The toggle button and the hidden actions must
  keep the context menu (and Shift+F10) on the header row.
- Follow-up Task 3 widens `Masker`. Send history reuses `Masker`, so it inherits the
  stricter rules for free if the follow-up work lands first.
- Follow-up Task 8 fixes the Send/Cancel width in `UrlBar`. Round 2 does not touch
  `UrlBar`.
- Follow-up Task 6 adds the runtime-variable panel next to the environment label. Round 2
  adds nothing there.

**Recommended order: follow-up gaps first, then round 2.** The follow-up work is mostly
masking (safety), and send history should be born with the stricter masking. Round 2's
Task 0 re-checks `Sidebar.tsx` and `masking.rs` against whatever landed.

## Left to the builder

- Icon drawing for the clock (stroke style of `icons.tsx`).
- Exact relative-time wording in the History list (`2 min ago`, `Yesterday 14:02`).
- Whether `PaneResizer` and `SidebarResizer` share a helper. Only extract one if it removes
  real duplication.
