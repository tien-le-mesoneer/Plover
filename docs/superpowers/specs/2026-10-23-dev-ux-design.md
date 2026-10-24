# Developer UX: look, layout, response, runner, cancel, variables, quick open — design

Status: draft for review · 2026-10-23 (revised after the mockup session)

## Why

Working daily with a large collection (one real collection has 409 requests in 39
folders, run against Local and Dev) shows rough spots across the app:

- **No way out of a slow send.** `UrlBar.tsx` turns Send into a disabled "Sending…"
  button. A hung server means waiting for the timeout.
- **The environment is easy to misread.** `EnvSelector` sits at the far right of the top
  bar (`App.tsx:118`) and silently follows the active tab's collection (`App.tsx:29`)
  without saying which collection it belongs to. `activeEnv` is already per collection
  (`store/workspace.ts:19`); the problem is labelling, not state.
- **`{{var}}` is opaque until you send.** You cannot see a variable's value or where it
  comes from, and a typo only shows up as a literal `{{tokn}}` in the sent request.
- **Finding a request means clicking through folders.**
- **Too many things compete for attention.** Several coloured buttons, a fixed-width
  sidebar (`--sidebar-w: clamp(220px, 24vw, 300px)`), and a response pane with one Copy
  button and nothing else for picking a single value out of a large body.
- **Runner results live in a bottom panel** (`RunPanel` under the editor in `App.tsx`),
  too narrow for 400 rows, with no way to filter to the failures.

The user's rule for the whole design: **not too much information, not too little; you
should be able to tell things apart at a glance; copying the things you copy all day
should be one click.**

## Mockups

Final screens from the brainstorm session are in
[`2026-10-23-ui-mockups/`](2026-10-23-ui-mockups/). They are content fragments (the
brainstorm tool wrapped them in its own frame and CSS), so they render roughly when
opened directly. **This document wins where they differ.** Known differences:

| File | Shows | Differs from this doc |
|---|---|---|
| `request-layout.html` | request editor, callouts 1–8 | the env chip beside Send (rejected, see §2); a large "Copy body" button (now an icon, §5) |
| `env-placement-v2.html` | env options A/B/C, resizable sidebar, icon copy | the user chose **A** |
| `response-v1.html` | response toolbar, value menu, pinned headers | — |
| `runner-v2.html` | runner with status filter | the folder line under every row (now only on duplicate names, §6) |
| `overall-v1.html` | runner as tab (A) vs bottom panel (B) | the user chose **A**; the ▶ menu shown there was rejected (▶ stays as today) |
| `style-light.html` | four light styles | the user chose **1. Swiss Minimal** |

All names in the mockups are invented.

## Goals (user decisions unless marked Claude's call)

1. **Visual style: Swiss Minimal**, light by default, with a matching dark variant.
2. **Environment stays in the top bar**, labelled with its collection. No colours, no
   confirmation.
3. **Layout:** resizable sidebar; Send is the only coloured button; ⌘S saves; a ⋯ menu
   holds the less frequent request actions; ▶ run buttons stay exactly as today.
4. **Response viewer** built for scanning and copying.
5. **Runner as a tab**, filtered by status like Bruno.
6. **Cancel** a send: `Cancel · 3.2s` + Esc, and the server really stops.
7. **Variable hover** with value and source; undefined variables underlined.
8. **Quick open** with Cmd/Ctrl+K.

## Non-goals

- Environment colours, danger levels and a "send to Stg/Prod?" confirmation (proposed,
  rejected by the user with option A).
- A menu on the ▶ run button. "Run with data…" lives in the Runner tab instead (§6).
- Cancel inside collection runs: `RunPanel` already cancels over the WebSocket
  (`crates/server/src/api/runs.rs:137`).
- Editing a variable from the hover popup, go-to-definition.
- Bruno's mock (`{{$randomUUID}}`) and prompt (`{{?Name}}`) variables. Plover does not
  interpolate them today, so they show as undefined, matching what Send does.
- A full command palette (actions, settings). Quick open is requests only.
- User-selectable themes beyond light/dark.

## Anti-goals

- A screen where two things shout at once (more than one filled, coloured button).
- Meaning carried by colour alone (status, method, test result).
- A secret value visible on screen because the user hovered or clicked it.
- A cancel that leaves half-applied state.
- Any font, icon or script loaded from a CDN (the UI must work offline;
  `frontend/scripts/check-no-cdn.mjs`).

## Constraints

- Bruno compatibility: no new fields in `.bru` files. UI state (sidebar width) lives in
  `state.json` (`crates/core/src/app_state.rs`) like the open tabs.
- New endpoints sit behind the session token like `/collections/{id}/send`.
- Accessibility rules in `components/a11y.test.tsx`: every control has an accessible
  name, focus is visible, text contrast ≥ 4.5:1, control borders ≥ 3:1.
- No new frontend dependency for fuzzy search, highlighting or JSONPath, unless the
  builder finds the JSONPath subset below cannot be written in under ~200 lines.

## Design

### 1. Visual style: Swiss Minimal (user decision; token values are Claude's call)

Clean, flat, high-contrast, one accent. Implemented entirely as CSS custom properties in
`frontend/src/styles.css` (today: `--bg`, `--fg`, `--border`, `--accent`, method and
status tokens, with a `prefers-color-scheme: dark` block).

- **Surfaces:** white page (`--bg: #ffffff`), panels a faint grey (`--bg-2: #f6f7f8`),
  black-ish text (`--fg: #111418`), muted text `#5f6670`. Borders 1 px, `#e3e5e8` for
  dividers and `--border-strong` (≥ 3:1) for form controls. Radius ~3 px everywhere. No
  shadows, except one flat 1 px border on popovers and menus.
- **One accent:** green, used **only** for the Send button (and its focus ring). Every
  other button is outlined/neutral. User decision: Send is Tailwind emerald-700 `#047857`
  with white text (5.48:1). It is a different green from GET and 2xx (`#15803d`), so Send
  never reads as a method label or a status. Claude's call: hover `#065f46` (emerald-800,
  7.68:1 with white); dark theme Send `#10b981` (emerald-500) with text `#16181c` (7.01:1;
  white on emerald-500/600 fails 4.5:1), hover `#34d399` (9.25:1).
- **Method colours (light):** GET `#15803d`, POST `#b45309`, PUT `#1d4ed8`, PATCH
  `#7c3aed`, DELETE `#b91c1c` — all ≥ 4.5:1 on white. Methods are always written out
  (`GET`, `POST`), so colour is never the only signal.
- **Status:** 2xx green, 3xx blue, 4xx amber, 5xx and network errors red, always with the
  code and reason text (`404 Not Found`, `ECONNREFUSED`). Test results use ✓ / ✕ / ⊘ plus
  colour.
- **Typography:** IBM Plex Sans for UI, JetBrains Mono for URLs, bodies and code. Both
  are **bundled** with the frontend (woff2 under `frontend/src/assets/fonts/`, OFL
  licences included, `font-display: swap`), never fetched from Google Fonts.
  `check-no-cdn.mjs` gains `fonts.googleapis.com` and `fonts.gstatic.com` in `FORBIDDEN`.
  Base size 13 px for dense panes, 14 px for body text; tabular figures for times and
  sizes.
- **Dark variant (Claude's call):** the same structure on `#16181c` / `#1d2025`, text
  `#e6e8eb`, borders `#30343a`, accent `#22c55e` with dark text, the existing dark method
  colours. Keep `prefers-color-scheme` as the switch (no in-app toggle in this version).
- **Icons:** one stroke icon set (the existing `components/icons.tsx` style), no emoji.

### 2. Environment in the top bar (user decision: option A)

- `EnvSelector` stays where it is in the top bar.
- It now shows its collection: `signing-api · Dev ▾` (collection name muted, environment
  in normal text), so switching tabs visibly switches the label.
- **No colours, no danger levels, no confirmation** before sending to any environment.
- Long names: the collection part truncates first (ellipsis, full text in `title`); the
  environment name never truncates below 12 characters.
- `Edit` stays next to it.

### 3. Layout (user decisions; details Claude's calls)

- **Resizable sidebar.** A 6 px drag handle on the sidebar's right edge
  (`role="separator"`, `aria-orientation="vertical"`, arrow keys move 16 px for keyboard
  users). Min 180 px, max 50% of the window. Double-click resets to the default. Claude's
  call: width is remembered in `state.json` under `ui` (global, not per collection),
  replacing the fixed `--sidebar-w` clamp with a stored value.
- **▶ run buttons** on the collection header and folders: exactly as today (no menu).
- **URL bar row:** method select · URL input · **Send** (the only coloured button) · `⋯`.
- **Save:** ⌘S / Ctrl+S (exists today) and an unsaved dot on the tab. The separate Save
  button moves into the `⋯` menu.
- **`⋯` menu:** Copy as cURL (from the curl/HAR work), Copy resolved URL (variables
  filled in, secrets masked as `***`), Save, Reveal `.bru` file (desktop) / Copy file path
  (web).
- **Quick open** (§9) replaces the sidebar's "Filter requests ⌘P" box as the main way to
  find a request; the sidebar filter stays for narrowing the tree.

### 4. Cancel (user decision; mechanism is Claude's call)

**How cancellation already works in core.** Dropping the `send_one` future
(`crates/core/src/runner.rs:964`) already stops everything that matters:

- the `reqwest` future inside `Session::send` is dropped, which aborts the HTTP request;
- `ScriptEngine::run` holds a `CancelOnDrop` guard
  (`crates/core/src/script/engine.rs:183`); dropping it sets the job's cancel flag, and
  the worker's interrupt handler stops the QuickJS script. Host calls (`bru.sendRequest`,
  `axios`) receive the same flag (`call_host(..., &cancelled)`).

So the server only needs a way to drop that future on request. Core does not change.

**Server.**

- `SendBody` (`crates/server/src/api/dto.rs`) gets `send_id: Option<String>`, a
  client-generated id (UUID). Optional so older clients keep working.
- `AppState` (`crates/server/src/state.rs`) gets a map `send_id → oneshot::Sender<()>`
  (or a `CancellationToken`, builder's choice), removed when the send ends either way.
- `requests::send` runs `send_one` inside `tokio::select!` against the cancel signal.
  When the signal wins, the future is dropped and the handler returns HTTP 409 with
  `code: "cancelled"`. A `send_id` already in the map is rejected with 409
  `code: "duplicate_send"`.
- New route `POST /collections/{id}/send/{send_id}/cancel` → `204`, also for unknown or
  finished ids (cancel is idempotent and racy by nature).

**State after a cancel (C1).** Nothing from the cancelled send is kept: `send_one`
records runtime and environment changes only after `exec.execute(...)` returns
(`runner.rs:1010-1030`), with no `.await` after that. Cookies set by a response that
arrived before the cancel stay in the shared jar; document this in a test, do not roll
it back.

**Frontend.**

- `Tab` (`store/tabs.ts`) gets `sendId?: string` and `sendStartedAt?: number`.
- `useTabs` gets `cancel(key)`: posts the cancel route, aborts the `fetch`
  (`AbortController` through `api().send`), sets `{ sending: false, result: undefined,
  cancelledAfterMs }`.
- While sending, `UrlBar` shows an **enabled** button `Cancel · 3.2s` in the Send slot
  (neutral outline, not green: the action is now "stop"). One decimal under 10 s, whole
  seconds above, `1m 05s` past a minute; updated every 100 ms; `aria-label="Cancel
  request"`; timer text in an `aria-live="off"` span.
- After a cancel the response area shows "Cancelled after 3.2 s" in a neutral banner.
- **Esc (C2)** cancels the active tab's send when the active tab is sending, no `Modal`
  or quick open is open, and the keydown was not already handled (`!e.defaultPrevented`;
  Monaco closes its own widgets first). Listener in the `App.tsx` shortcut effect.

### 5. Response viewer (user decisions; details Claude's calls)

Changes to `components/ResponseViewer.tsx` and `components/response/`.

**Status line:** status chip (§1 colours + text), `312 ms · 1.2 KB`, tests summary
(`✓ 5/5`), then on the right two small icon buttons: **Copy body** (⧉) and **Save to
file** (⤓). Icons only, with `aria-label` and a tooltip; "Copied" feedback in an
`aria-live="polite"` span as today.

**Sub-tabs:** Body · Headers *n* · Request · Tests *n/m* · Console *n*. ("Request" shows
what was actually sent, from the run-history detail types.)

**Body toolbar (one row, only on Body):**

- `Pretty | Raw | Preview` segmented control. Preview for HTML (sandboxed iframe, no
  scripts), images and PDF; disabled with a reason for other types.
- **Search** (⌘F / Ctrl+F while the response pane has focus): Monaco's find widget in
  Pretty/Raw, with a match count `2/3`; Enter / Shift+Enter move.
- **JSONPath filter** (JSON bodies only), like Bruno's response filter: an input such as
  `$.signers[*].email`; the body shows the filtered result; Copy body copies the filtered
  result while a filter is active. Supported subset (Claude's call): `$`, `.key`,
  `['key']`, `[n]`, `[*]`, `..key`. Invalid expressions show the error under the input
  and leave the body unfiltered.

**Value menu (click on a JSON value, or Enter on it with the keyboard):** a small popover
with

- Copy value (strings without quotes),
- Copy path (`$.id`, `$.signers[0].email` — ready for an assert),
- Save as runtime variable… (asks for a name, prefilled from the key, e.g. `docId`; sets
  it through the same path as `bru.setVar` so the next send sees it).

Implemented with Monaco's JSON model: map the click position to a JSON path with a small
tokenizer over the pretty text (builder's choice), no new dependency.

**Large bodies:** arrays and objects with more than 50 entries start collapsed (Monaco
folding). Bodies over 1 MiB render the first 1 MiB with a "Show all (12.4 MB)" button;
the full body is still what Copy and Save use.

**Pinned headers:** a thin row under the body with the response headers developers copy
to look up logs: `x-request-id`, `x-correlation-id`, `traceparent` / `x-trace-id`,
`location` (Claude's call for the list), each with a copy icon. Hidden when none are
present.

**Headers tab:** every row shows a copy icon on hover and on focus; plus "Copy all".

### 6. Runner as a tab (user decisions; details Claude's calls)

Replaces the bottom `RunPanel` in `App.tsx`. Bruno shows the runner the same way.

- **A run opens a tab** in `RequestTabs` (▶ icon + `Run · <folder or collection>`). One
  tab per started run; starting a run from the sidebar ▶ opens a new Run tab and focuses
  it. `store/tabs.ts` gains a tab kind: `kind: "request" | "run"`, a run tab holding a
  `runId` (from `store/runs.ts`) instead of a request draft. Run tabs are not restored
  after a restart (history has them).
- **Header row:** title, `env · time · duration`; on the right: **Previous runs ▾**,
  export ⤓ (HTML/JSON/JUnit, from the data-driven/export design), **Re-run**, and
  **Run with data…** (opens the data-file dialog from the data-driven design).
- **Status filter, Bruno-style:** a segmented control `All 409 | Fail 32 | Pass 318 |
  Skip 59`. The counts live only here (no separate summary chips). Default selection:
  All while running; when a run ends with failures, the selection stays where the user
  left it (Claude's call: do not jump). Plus a name filter input.
- **List:** flat, in run order. Each row: ✓ / ✕ / ⊘, method, request name, and on the
  right `status · time`. The **folder path is shown only when two or more rows in the run
  share the same request name** (muted second line); otherwise hidden.
- **Detail (right side):** request name, status chip, time/size, ↗ "Open request"
  (opens the request in a request tab, keeping this run's result visible in the Run tab).
  The failure reason (failed test / assert / script error / transport error) sits on top
  in a red-bordered box. Then sub-tabs Response · Headers · Request · Tests · Console with
  the same copy affordances as §5.
- **Previous runs ▾:** the 20 most recent runs of this collection from run history,
  each `time · source (app | plv) · pass/fail`; choosing one loads it into this Run tab
  (Claude's call: replace, not open another tab). Details of storage are in the
  run-history design.
- A single request's Send never opens a Run tab and is not saved to history.

### 7. Variable hover and undefined-variable underline (user decisions; endpoint and masking details Claude's calls)

**Server endpoint.** `POST /collections/{id}/vars` with body
`{ item: string | null, draft: HttpRequestDto | null, env: string | null }` (same shape
as `SendBody`, so scratch tabs from the curl/HAR work pass `item: null`). Returns:

```ts
type VarsDto = {
  vars: Array<{
    name: string;
    source: "processEnv" | "runtime" | "request" | "folder" | "environment" | "collection";
    /** Display value; null when masked. Cut to 1 KiB (`truncated`). */
    value: string | null;
    masked: boolean;
    truncated: boolean;
    /** Lower-precedence definitions this one hides, for the "overrides" line. */
    shadows: Array<"runtime" | "request" | "folder" | "environment" | "collection">;
  }>;
  /** Names that no scope defines but some script or `vars:post-response` in the
   *  collection sets, with the request that sets them (first match). */
  setLater: Array<{ name: string; by: string }>;
};
```

- Core: `VarStack` (`crates/core/src/vars.rs:65`) keeps unlabelled layers. Add labels so
  a new `VarStack::entries()` can report the winning source and shadows per name.
  Precedence is the existing `PRECEDENCE` plus `process` on top (`with_process_env`).
  Request vars come from `initial_vars` (`crates/core/src/resolve.rs:95`) using the
  draft's `varsPre` when a draft is given; runtime/environment exactly as `send_one`
  builds them, so hover shows what Send would use before scripts run.
- No scripts run, no request is sent, no file is written.
- `setLater` is a static scan: literal first arguments of `bru.setVar`,
  `bru.setEnvVar`, `bru.setGlobalEnvVar`
  (`/bru\.set(?:Env|Global(?:Env)?)?Var\(\s*["'`]([^"'`]+)["'`]/`) in collection, folder
  and request scripts, plus names in `vars:post-response` blocks. Computed once per
  collection load, refreshed on reload.

**Masking (user decision for the token rule; the rest Claude's call).**

- Masked (`value: null, masked: true`): environment secrets (`vars:secret` names),
  every `process.env` value, and **runtime values that were also sent in an
  `Authorization`, `Proxy-Authorization` or `Cookie` header** during this server session
  (exact value match, values ≥ 8 characters — the same rule as run-history masking; the
  server keeps the set of sent header values in memory per collection, never on disk).
- Shown: other runtime values, request, folder, plain environment and collection
  values, cut to 1 KiB.
- No "reveal" button in this version.

**Frontend.**

- `store/vars.ts`: per tab, the last `VarsDto`. Fetched when a tab opens, when its
  environment changes, after each send or run, after the environment is saved, and
  300 ms after the draft's `varsPre` changes. Underlining uses the cached list, so typing
  in the URL needs no round trip.
- Classification of each `{{name}}` in `lib/varTokens.ts`: `defined` (subtle tint),
  `setLater` (**amber dotted underline**, hover "Not set yet. Set at runtime by
  <request>." — user approved), `undefined` (**red wavy underline**). Path rule as
  `VarStack::lookup` (`a.b[0]`).
- **Where:** URL bar, Params/Headers/Auth value cells (`KeyValueTable.tsx`,
  `AuthTab.tsx`), and the body editor (Monaco in `BodyTab.tsx`). Scripts and tests are
  not decorated.
- **Plain inputs (C8):** a `VarInput` wraps `<input>` with an `aria-hidden` mirror layer
  that draws the underlines; hover hit-tests the mirror's token spans. The input stays a
  normal input (curl paste in `UrlBar` keeps working).
- **Monaco:** decorations for the three classes and a `HoverProvider`.
- **Hover content:** name, value (or "•••• secret"), source ("Environment: Dev",
  "Folder: Signing", "Runtime", …), "overrides collection" when `shadows` is not empty.
  Keyboard: caret inside `{{…}}` in a focused field shows it after 500 ms; Esc hides it.

### 8. (removed) Environment colours and confirmation

Rejected in the mockup session (option A). Kept as a heading so earlier references to
"§3 colours" in review notes resolve to this decision.

### 9. Quick open (user decision; details Claude's calls)

- `components/QuickOpen.tsx`, opened by Cmd/Ctrl+K from the `App.tsx` shortcut effect.
  In Monaco, `CodeEditor.tsx` registers `KeyMod.CtrlCmd | KeyCode.KeyK` to dispatch
  `plover:quick-open`, like `plover:send` today (gives up Monaco's Cmd+K chords, C9).
- Data: request nodes from `useWorkspace().trees` for every open collection. Each row:
  method, request name, folder path, collection name.
- Matching (C10): case-insensitive subsequence over `name` and
  `collection / folder path / name`, scored by contiguous runs, word starts and name
  over path; no dependency. Empty query lists recently opened tabs (this page life).
- Up to 50 rows. Up/Down, Enter opens (`useTabs.open`), Esc closes and returns focus.
  ARIA: `role="dialog"`, `combobox` input, `listbox` results.
- Budget: under 16 ms per keystroke for 2,000 requests.

## Overlap with work in flight and order

| Area | Run history (main) | curl/HAR (`feat/import-curl-har`) | This design |
|---|---|---|---|
| `styles.css` | detail pane styles | dialog styles | all tokens (§1) |
| `UrlBar.tsx` | — | curl paste, Copy as cURL | Cancel, `⋯` menu, `VarInput` |
| `store/tabs.ts` | — | `item: string \| null`, scratch tabs | `sendId`, `cancel()`, `kind: "run"` |
| `requests::send`, `SendBody` | — | `item` optional | `send_id`, cancel route |
| `dto.rs` | run detail DTOs | create/import DTOs | `VarsDto` |
| `App.tsx` | — | "New request" | shortcuts, RunPanel → Run tab |
| `RunPanel.tsx` / `runs.rs` | detail pane, history | — | becomes the Run tab body |
| `ResponseViewer.tsx` | reused in run detail | — | §5 |
| `vars.rs` / `resolve.rs` | masking reads secrets | — | labelled layers |

Order:

1. Run history and curl/HAR are built first, in parallel (plans exist).
2. **After both branches merge:** style tokens and fonts (§1), env label (§2), layout
   (§3), response viewer (§5), Run tab (§6). These touch nearly every component, so
   doing them after the merges avoids conflicts in every UI file.
3. Cancel (§4), then variable hover (§7), building on curl/HAR's `item: string | null`.
4. Quick open (§9) can go any time; it only adds `QuickOpen.tsx` and two shortcut lines.

## Claude's calls (for review)

- **C1** A cancelled send keeps no variable changes, except cookies already stored by
  responses that arrived before the cancel.
- **C2** Esc cancels only when nothing else (modal, palette, Monaco widget, var hover)
  handled it.
- **C3** Swiss Minimal token values (§1): Send hover and dark-theme Send values (Send fill
  `#047857` itself is the user's choice), and the dark variant values.
- **C4** Sidebar width stored globally in `state.json`; min 180 px, max 50%.
- **C5** `⋯` menu contents; Save moves into it (⌘S stays).
- **C6** JSONPath subset; value menu via a tokenizer over the pretty text; collapse
  threshold 50 entries; 1 MiB render cap.
- **C7** Pinned header list (`x-request-id`, `x-correlation-id`, `traceparent`,
  `x-trace-id`, `location`).
- **C8** Plain inputs get a mirror-layer `VarInput`, not a single-line editor.
- **C9** Cmd/Ctrl+K inside Monaco opens quick open.
- **C10** Fuzzy matching is a small in-house scorer.
- **C11** Cancel is HTTP 409 `code: "cancelled"`; duplicate `send_id` is 409
  `duplicate_send`.
- **C12** Run tabs are not restored after restart; choosing a previous run replaces the
  current Run tab's content; the filter does not jump to Fail when a run ends.

## Questions for the user

None open. Answered in the session: amber "set later" underline — keep; mask runtime
values sent in Authorization-style headers — yes; environment placement — option A;
runner — tab; style — Swiss Minimal.

## Left to the builder

- Cancel signal type (`oneshot` vs `CancellationToken`).
- Exact quick-open scoring weights and row layout.
- How `VarStack` carries labels, as long as `lookup`/`interpolate` behaviour and
  performance are unchanged.
- Font subsetting and weights shipped (at least Plex Sans 400/500/600, JetBrains Mono
  400/600).
- Exact spacing scale within the 4 px grid.

## Tests that pin the decisions

- Style: contrast test over the token pairs in both themes (text ≥ 4.5:1, control
  borders ≥ 3:1); `check-no-cdn` fails on `fonts.googleapis.com`; only one element with
  the accent fill on the request screen.
- Env label: shows `<collection> · <env>`, changes when the active tab's collection
  changes; no confirmation modal appears when sending to an env named `Stg`.
- Sidebar: drag and arrow keys resize within bounds; width survives a reload via
  `state.json`.
- Response: Copy body copies the filtered result while a JSONPath filter is active;
  `$.signers[*].email` and `..id` filter correctly, invalid paths show an error; the value
  menu copies `$.signers[0].email` as the path; Save as variable is visible to the next
  send; a 2 MiB body renders the first 1 MiB and copies all of it; pinned headers appear
  only when present.
- Runner tab: starting a run opens a Run tab; filter counts match the report; folder
  line appears only for duplicate names; ↗ opens the request tab; choosing a previous run
  loads it into the same tab.
- Cancel (server): a send to a sleeping local server is cancelled within 200 ms; a
  looping pre-request script is interrupted; runtime vars it set are not visible to the
  next send; cancelling an unknown id returns 204.
- Vars: `/vars` masks a keychain secret, a `.env` value, and a runtime value previously
  sent as `Authorization: Bearer …`; cuts a 2 KiB value to 1 KiB; lists `token` in
  `setLater`; accepts `item: null`. Frontend classifies `{{a}}`, `{{a.b[0]}}`,
  `{{missing}}`, `{{setLater}}`.
- Quick open: "sign doc" ranks "Sign document" above "assign document type"; Enter opens
  the tab; Cmd+K inside Monaco opens it; a11y test covers palette, value menu, separator.
