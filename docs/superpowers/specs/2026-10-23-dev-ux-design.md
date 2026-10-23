# Developer UX: cancel, environment context, variable hover, quick open — design

Status: draft for review · 2026-10-23

## Why

Working daily with a large collection (one real collection has 409 requests in 39
folders, run against Local and Dev) shows five rough spots in the request editor:

- **No way out of a slow send.** `UrlBar.tsx` turns Send into a disabled "Sending…"
  button. A hung server means waiting for the timeout.
- **The environment is easy to miss.** `EnvSelector` sits at the far right of the top bar
  (`App.tsx:118`) and silently follows the active tab's collection
  (`App.tsx:29`). It does not say which collection it belongs to. Switching tabs can
  switch the target from Local to Dev without the user noticing. `activeEnv` is already
  per collection (`store/workspace.ts:19`); the problem is visibility, not state.
- **Nothing tells you an environment is dangerous.** Local, Dev and Stg look the same.
- **`{{var}}` is opaque until you send.** You cannot see a variable's value or where it
  comes from, and a typo only shows up as a literal `{{tokn}}` in the sent request.
- **Finding a request means clicking through folders.** There is a sidebar filter, but
  no keyboard way to jump to a request.

## Goals (user decisions)

1. **Cancel.** While a request is sending, Send becomes `Cancel · 3.2s` with a live
   timer. Esc also cancels. The server really stops the work: the HTTP request in
   flight and any running pre-request or post-response script, not just the UI wait.
2. **Environment next to the URL bar**, shown as `<Collection> · <Env> ▾`, so the target
   is visible at the moment you press Send. The top-bar selector goes away.
3. **Environment colours.** Local green, Dev amber. Environments whose name contains
   `stg` or `prod` are red and ask for confirmation before the first send to them.
4. **Variable hover.** Hovering `{{var}}` shows its value and where it comes from
   (runtime, request, folder, environment, collection, `process.env`), following
   Bruno's precedence. Undefined variables are underlined red while typing.
5. **Quick open.** Cmd+K (Ctrl+K on Windows/Linux) opens a palette that fuzzy-searches
   requests across all open collections and opens the chosen one in a tab.

## Non-goals

- Cancel inside collection runs. `RunPanel` already cancels over the WebSocket
  (`crates/server/src/api/runs.rs:137`).
- Editing a variable from the hover popup, "add to scope", go-to-definition (Bruno has
  these; later).
- Bruno's mock variables (`{{$randomUUID}}`) and prompt variables (`{{?Name}}`). Plover
  does not interpolate them today, so they show as undefined, which matches what Send
  does.
- A full command palette (actions, settings). Quick open is requests only.
- User-chosen environment colours (see Claude's calls, C3).

## Anti-goals

- A secret value visible on screen because the user hovered over it.
- A cancel that leaves half-applied state: some script variables changed, others not.
- Cmd+K stealing a shortcut the user relies on without a way to get it back in the
  same place (see C9 on Monaco).
- A confirmation so frequent that people click through it without reading.

## Constraints

- Bruno compatibility: no new fields in `.bru` files. Colours and "confirmed" state live
  outside collection files.
- Same security posture as the rest of the server: new endpoints sit behind the session
  token like `/collections/{id}/send`.
- Accessibility rules already in `components/a11y.test.tsx`: colour is never the only
  signal, every control has an accessible name, focus is visible.
- No new frontend dependency for fuzzy search or highlighting.

## Design

### 1. Cancel (user decision; mechanism is Claude's call)

**How cancellation already works in core.** Dropping the `send_one` future
(`crates/core/src/runner.rs:964`) already stops everything that matters:

- the `reqwest` future inside `Session::send` is dropped, which aborts the HTTP request;
- `ScriptEngine::run` holds a `CancelOnDrop` guard
  (`crates/core/src/script/engine.rs:183`); dropping it sets the job's cancel flag, and
  the worker's interrupt handler stops the QuickJS script. Host calls (`bru.sendRequest`,
  `axios`) receive the same flag (`call_host(..., &cancelled)`).

So the server only needs a way to drop that future on request. Core does not change.

**Server.**

- `SendBody` (`crates/server/src/api/dto.rs`) gets `send_id: Option<String>`, a client
  generated id (UUID). Optional so older clients keep working.
- `AppState` (`crates/server/src/state.rs`) gets a map `send_id → oneshot::Sender<()>`
  (or a `CancellationToken`, builder's choice), removed when the send ends either way.
- `requests::send` runs `send_one` inside `tokio::select!` against the cancel signal.
  When the signal wins, the `send_one` future is dropped and the handler returns an
  `ApiError` with HTTP 409 and `code: "cancelled"` (Claude's call; the frontend's
  `ApiError.code` already distinguishes `conflict` the same way).
- A `send_id` already in the map is rejected with 409 `code: "duplicate_send"`; ids are
  scoped to the server session, not the collection.
- New route `POST /collections/{id}/send/{send_id}/cancel` → `204`. Unknown or finished
  id → `204` too (cancel is idempotent and racy by nature).
- Also cancel when the client disconnects: the frontend aborts its `fetch` as well, but
  hyper does not reliably notice a closed HTTP/1 connection mid-handler, so the explicit
  cancel route is the primary path.

**State after a cancel (Claude's call, C1).** Nothing from the cancelled send is kept:
no runtime variables, no environment changes, no cookies recorded after the drop point.
This falls out of the code: `send_one` records runtime and environment changes only after
`exec.execute(...)` returns (`runner.rs:1010-1030`), and there is no `.await` between
that and the end of the function. The doc states it so tests pin it.

Cookies set by a response that arrived before the cancel (e.g. a pre-request
`bru.sendRequest` that finished) stay in the session jar, because the jar is shared and
updated as responses arrive. Builder: document this in the test, do not try to roll it
back.

**Frontend.**

- `Tab` (`store/tabs.ts`) gets `sendId?: string` and `sendStartedAt?: number`.
- `useTabs` gets `cancel(key)`: posts the cancel route, aborts the `fetch`
  (`AbortController` passed through `api().send`), and sets
  `{ sending: false, result: undefined, cancelledAfterMs }`.
- `UrlBar` shows, while `sending`, a button `Cancel · 3.2s` (one decimal under 10 s,
  whole seconds above, minutes as `1m 05s`), updated every 100 ms. The button is enabled
  (today it is disabled while sending), has `aria-label="Cancel request"`, and its
  timer text is in an `aria-live="off"` span so screen readers are not spammed.
- After a cancel the response area shows "Cancelled after 3.2 s" in a neutral banner,
  not an error banner.
- **Esc (Claude's call, C2)** cancels the active tab's send when all of these hold: the
  active tab is sending, no `Modal` or the quick-open palette is open, and the keydown
  was not already handled (`!e.defaultPrevented`). Monaco handles Esc for its own
  widgets (suggest, find) and prevents default, so Esc there closes the widget first.
  Listener on `window`, bubbling phase, in the same effect as the other shortcuts in
  `App.tsx`.

### 2. Environment context next to the URL bar (user decision; placement details are Claude's calls)

- New component `EnvChip` (replaces `EnvSelector`; `EnvEditor` stays). It renders a
  button `Demo API · Dev ▾` and a menu with: each environment, "No environment", and
  "Edit <env>…" (opens `EnvEditor` as today).
- **Placement:** in `UrlBar`, left of the method select, on the same row. The chip
  shows the tab's own collection (`tab.collectionId`), never "whatever is active".
- The top-bar `EnvSelector` in `App.tsx` is removed.
- **Sidebar (Claude's call, C4).** Each collection header in `Sidebar.tsx:88` also shows
  its environment as a small coloured chip (same `EnvChip`, compact variant). Reason:
  runs started from the sidebar (`onRun` → `startRun(id, item, activeEnv[id])`) use that
  environment, and with the top-bar selector gone there would be no visible env when no
  tab is open.
- Long names: the collection part truncates first (ellipsis, full text in `title`);
  the environment name never truncates below 12 characters.

### 3. Environment colours and confirmation (user decision for the colours named; the rule is Claude's call)

**Classification (Claude's call, C3).** Automatic from the name only; not user
overridable in this version. Lower-case the name, then the first matching rule wins:

| Level | Name contains any of | Colour token |
|---|---|---|
| `danger` | `prod`, `prd`, `live`, `stg`, `stag`, `uat` | `--err` |
| `caution` | `dev`, `test`, `qa`, `int`, `sandbox` | `--warn` |
| `safe` | `local`, `localhost` | `--ok` |
| `neutral` | anything else, and "No environment" | `--muted` |

`danger` is checked first so `preprod`, `prod-local` and `staging` are red. Substring
match, so `Dev2` and `dev-eu` are amber. Lives in one pure function
`envLevel(name: string): "danger" | "caution" | "safe" | "neutral"` in
`frontend/src/lib/envLevel.ts` with a table test.

- The chip shows the colour as a left border and a dot, plus the name in text; `danger`
  also shows a warning icon, so the level is never colour-only.
- The URL bar row gets a 2 px top border in the level colour for `danger` only. Amber and
  green do not tint the editor (keeps the "red means stop" signal strong).

**Confirmation (Claude's calls, C5).**

- Before the first send to a `danger` environment in a collection, a `Modal` asks:
  title "Send to Stg?", body `<Collection> · <Env>` and the resolved host of the URL
  (from the variable endpoint in section 4; if it cannot be resolved, show the raw URL).
  Buttons: "Cancel" (initial focus) and "Send to <Env>". Cmd/Ctrl+Enter inside the
  modal does nothing, so the send shortcut muscle memory cannot confirm it.
- The same confirmation appears before starting a run (sidebar ▶) against a `danger`
  environment, with "Run on <Env>".
- The gate sits in one place every send path goes through: a `confirmTarget` step in
  front of `useTabs.send` and `useRuns.start` (store level), so the Send button,
  Enter in the URL bar, Cmd/Ctrl+Enter in `App.tsx` and `plover:send` from Monaco are
  all covered. It is not in `UrlBar`.
- "Confirmed" is remembered per `(collectionId, envName)` in frontend memory for the
  life of the page. Not persisted; a reload or app restart asks again. Changing the
  environment away and back does not ask again within the same page life.
- Sends from the CLI are never affected.

### 4. Variable hover and undefined-variable underline (user decision; masking and the endpoint are Claude's calls)

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

- Core: `VarStack` (`crates/core/src/vars.rs:65`) keeps unlabelled layers. Add a
  labelled constructor or a parallel `Vec<Scope>` so a new
  `VarStack::entries() -> Vec<(name, Scope or ProcessEnv, &Value, shadows)>` can report
  the winning source per name. Precedence is the existing `PRECEDENCE` plus `process`
  on top (`with_process_env`). The request's variables come from `initial_vars`
  (`crates/core/src/resolve.rs:95`) using the draft's `varsPre` when a draft is given,
  and runtime/environment from the open collection exactly as `send_one` builds them
  (`RunVars::start` + `open.env_overrides(env)` + `open.runtime()`), so hover shows what
  Send would use before scripts run.
- No scripts run, no request is sent, no file is written.
- `setLater` is a static scan (C7): literal first arguments of `bru.setVar`,
  `bru.setEnvVar`, `bru.setGlobalEnvVar`, `bru.runner.setVar`-style calls
  (`/bru\.set(?:Env|Global(?:Env)?)?Var\(\s*["'`]([^"'`]+)["'`]/`) in collection,
  folder and request scripts, plus names in `vars:post-response` blocks. Computed once per
  collection load and refreshed on reload.

**Masking (Claude's call, C6).**

- Masked (`value: null, masked: true`): environment secrets (`vars:secret` names,
  values from the keychain or memory), and every `process.env` value (usually from
  `.env`, which usually holds credentials).
- Shown: runtime, request, folder, plain environment and collection values. Runtime
  values can be tokens set by a login script; they are shown because Bruno shows them
  and the user needs them for debugging, but cut to 1 KiB.
- No "reveal" button in this version. The environment editor is where secrets are
  managed.

**Frontend.**

- `store/vars.ts`: per tab, the last `VarsDto`. Fetched when a tab opens, when its
  environment changes, after each send or run finishes (runtime vars change), after the
  environment is saved, and 300 ms after the draft's `varsPre` changes. Not on every
  keystroke elsewhere: underlining uses the cached list, so typing in the URL needs no
  round trip.
- Classification of each `{{name}}` in text, in `lib/varTokens.ts`: `defined` (normal,
  subtle tint), `setLater` (amber dotted underline, hover says "Not set yet. Set at
  runtime by <request>."), `undefined` (red wavy underline). Name resolution uses the
  same lodash-style path rule as `VarStack::lookup` (`a.b[0]` resolves when `a` is
  defined and the path exists in its value).
- **Where:** URL bar, Params/Headers/Auth value cells (`KeyValueTable.tsx`,
  `AuthTab.tsx`), and the body editor (Monaco in `BodyTab.tsx`). Scripts and tests are
  not decorated (they are JS, not templates).
- **Plain inputs (C8):** a `VarInput` component wraps `<input>` with an `aria-hidden`
  mirror layer behind transparent-coloured text that draws the underlines; hover is found
  by hit-testing the mirror's token spans on `mousemove`. The input itself stays a normal
  input (paste handling for curl in `UrlBar` keeps working).
- **Monaco:** decorations for the three classes and a `HoverProvider` with the same
  content.
- **Hover content:** name, value (or "•••• secret"), source badge ("Environment: Dev",
  "Folder: Signing", "Runtime", …), and "overrides collection" when `shadows` is not
  empty. Keyboard: when the caret is inside a `{{…}}` in a focused field, the same info
  shows after 500 ms, and Esc hides it (Esc then does not cancel a send in that keypress).

### 5. Quick open (user decision; details are Claude's calls)

- `components/QuickOpen.tsx`, opened by Cmd/Ctrl+K from the `App.tsx` shortcut effect.
  In Monaco, `CodeEditor.tsx` registers `KeyMod.CtrlCmd | KeyCode.KeyK` to dispatch
  `plover:quick-open`, like `plover:send` today. This gives up Monaco's Cmd+K chords
  (e.g. Cmd+K Cmd+C) inside Plover (C9).
- Data: request nodes from `useWorkspace().trees` for every open collection (folders
  are not listed). Each row: method badge, request name, folder path, collection name.
- Matching (C10): case-insensitive subsequence match over `name` and
  `collection / folder path / name`, scored by contiguous runs, word starts and name
  over path; no dependency. Empty query lists recently opened tabs first (this page
  life), then nothing else.
- Up to 50 rows rendered. Up/Down move, Enter opens (`useTabs.open`), Esc closes and
  returns focus. Matched characters are bold. ARIA: `role="dialog"` with a
  `combobox` input and `listbox` results (`aria-activedescendant`).
- Performance budget: under 16 ms per keystroke for 2,000 requests on a laptop
  (index built once per tree change).

## Overlap with work in flight and recommended order

| Area | Run history (main) | curl/HAR (`feat/import-curl-har`) | This design |
|---|---|---|---|
| `UrlBar.tsx` | — | curl paste handler, Copy as cURL | env chip, Cancel button, `VarInput` |
| `store/tabs.ts` | — | `item: string \| null`, scratch tabs | `sendId`, `cancel()` |
| `requests::send`, `SendBody` | — | `item` optional | `send_id`, cancel route |
| `dto.rs` | run detail DTOs | create/import DTOs | `VarsDto` |
| `App.tsx` | — | "New request" | shortcuts, top-bar selector removed |
| `Sidebar.tsx` | history entry? | import menu | env chip, run confirmation |
| `RunPanel.tsx` / `runs.rs` | detail pane, history | — | — |
| `vars.rs` / `resolve.rs` | masking reads secrets | — | labelled layers |

Recommended order:

1. **Quick open** (item 5) any time; it only adds `QuickOpen.tsx` and two shortcut
   lines.
2. **After curl/HAR is merged:** environment chip + colours + confirmation (2, 3), then
   Cancel (1). Both edit `UrlBar.tsx`, `tabs.ts` and the send path that curl/HAR also
   edits; building on its `item: string | null` avoids a second migration.
3. **Variable hover** (4) last. It is the largest, and its endpoint must accept the
   scratch-tab shape (`item: null`) from curl/HAR.

Run history overlaps only in `dto.rs` and possibly `Sidebar.tsx`; plain merges.

## Claude's calls (for review)

- **C1** A cancelled send keeps no variable changes (atomic), except cookies already
  stored by responses that arrived before the cancel.
- **C2** Esc cancels only when nothing else (modal, palette, Monaco widget, var hover)
  handled it.
- **C3** Colour level comes from the name only, by the table above; not overridable.
- **C4** The env chip also appears on each sidebar collection header, because sidebar
  runs use it.
- **C5** Red-env confirmation: once per collection and env per page life, also before
  runs; Cancel has initial focus; not persisted.
- **C6** Hover masks environment secrets and all `process.env` values; shows runtime
  values (cut to 1 KiB); no reveal button.
- **C7** Undefined names that a script or `vars:post-response` sets get an amber
  "set later" underline instead of red, from a static scan.
- **C8** Plain inputs get a mirror-layer `VarInput`, not a single-line Monaco/CodeMirror.
- **C9** Cmd/Ctrl+K inside Monaco opens quick open, giving up Monaco's Cmd+K chords.
- **C10** Fuzzy matching is a small in-house scorer, no dependency.

- **C11** Cancel is reported as HTTP 409 `code: "cancelled"`; duplicate `send_id` is
  409 `duplicate_send`.
- **C12** The red-env confirmation is enforced at store level (`confirmTarget`), so
  every send and run path hits it.

Run history saves only runner runs, not single sends
(`2026-10-23-run-history-design.md`, non-goals), so a cancelled single send writes
nothing to history.

## Questions for the user

1. C7 adds an amber "set later" state that the user did not ask for: in chained flows
   (login → use token) most `{{token}}`-style variables are undefined until a script
   runs, and plain red would mark them all as errors. Keep amber, or strictly red for
   anything undefined right now?
2. C6 shows runtime values (often tokens set by a login script) in plain text on hover.
   Mask runtime values that look like they came from an `Authorization`-style header
   too, or show them as Bruno does?

## Left to the builder

- Cancel signal type (`oneshot` vs `CancellationToken`).
- Debounce intervals other than those stated; timer update rate within 50–200 ms.
- Exact scoring weights in quick open, and its row layout.
- Styling of underlines and chips within the existing tokens in `styles.css`
  (`--ok`, `--warn`, `--err`, `--muted`), checked in both themes.
- How `VarStack` carries labels (parallel vec, tagged layer struct), as long as
  `lookup`/`interpolate` behaviour and performance are unchanged.

## Tests that pin the decisions

- Server: a send to a local test server that sleeps is cancelled through the cancel
  route; the handler returns "cancelled" within 200 ms; a pre-request script in an
  infinite loop is interrupted the same way; runtime variables set by that pre-request
  script before the cancel are not visible to the next send (C1); cancelling an unknown
  id returns 204.
- Core: `VarStack::entries` reports the winning scope and shadows for a name defined in
  runtime, folder and collection; `process.env` wins over all; a draft's `varsPre`
  replaces the file's.
- Server: `/vars` masks a keychain secret and a `.env` value, cuts a 2 KiB runtime value
  to 1 KiB with `truncated: true`, lists `token` in `setLater` when a post-response
  script calls `bru.setVar("token", …)`, and accepts `item: null`.
- Frontend: `envLevel` table test (`Local`, `localhost`, `Dev`, `dev-eu`, `QA`,
  `Stg`, `staging`, `preprod`, `prod-local`, `Production`, `Demo`); confirmation shows
  once per page life per collection/env and before a run; Esc cancels a send but closes
  a modal first; Cancel button label and timer formatting; `VarInput` classifies
  `{{a}}`, `{{a.b[0]}}`, `{{missing}}`, `{{setLater}}`; quick open finds
  "sign doc" → "Sign document" ahead of "assign document type", Enter opens the tab,
  Cmd+K from inside Monaco opens it; a11y test covers the chip, palette and modal.
