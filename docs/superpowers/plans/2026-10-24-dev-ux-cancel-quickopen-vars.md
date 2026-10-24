# Cancel, Quick Open and Variable Hover Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A send can be stopped (`Cancel · 3.2s` or Esc) and the server really stops it; Cmd/Ctrl+K finds any request in the open collections; hovering `{{var}}` shows its value and source, with undefined variables underlined red and script-set ones amber.

**Architecture:** Core already aborts the HTTP request and the QuickJS script when the `send_one` future is dropped, so cancel is a server registry of cancel signals plus `tokio::select!` in `requests::send`. Quick open is a frontend-only palette over `useWorkspace().trees` with an in-house scorer. Variable hover adds labelled layers to `VarStack`, a static `setLater` scan, a `POST /collections/{id}/vars` endpoint with masking, and frontend decoration for plain inputs (mirror layer) and Monaco (decorations + hover provider).

**Tech Stack:** Rust (tokio, axum, ts-rs, wiremock), React + TypeScript + Zustand + Monaco + Vitest/Testing Library.

**Spec:** `docs/superpowers/specs/2026-10-23-dev-ux-design.md` §4 (Cancel), §7 (variable hover), §9 (quick open).

**Starts after:** the run-history work (`main`, `docs/superpowers/plans/2026-10-23-run-history.md`) and the curl/HAR work (`feat/import-curl-har`, `docs/superpowers/plans/2026-10-23-import-curl-har.md`) are merged. **Exception: quick open (Task 3) can be built any time**, even before those merges; it only adds `QuickOpen.tsx`, `lib/fuzzy.ts` and two shortcut lines. The style/layout plan (`docs/superpowers/plans/2026-10-24-ui-style-and-layout.md`) touches the same `UrlBar.tsx` and `KeyValueTable.tsx`; build whichever plan comes second on top of the first, never in parallel branches.

**Pending user decision (not blocking this plan):** the Send button colour, owned by the style/layout plan's `--color-send` token. The Cancel button here is a neutral outline in both cases.

## Global Constraints

- Core's send pipeline (`runner::send_one`, `Exec::execute`, `ScriptEngine::run`) does not change for cancel. Cancel works by dropping the future.
- A cancelled send keeps no runtime or environment variable changes. Cookies stored by responses that arrived before the cancel stay (C1; pinned by a test, not rolled back).
- Cancel route is idempotent: unknown or finished `send_id` → `204`.
- Errors: cancelled send → HTTP 409 `code: "cancelled"`; duplicate `send_id` in flight → 409 `code: "duplicate_send"` (C11).
- `/vars` never runs scripts, sends requests or writes files.
- Masked in `/vars` (`value: null, masked: true`): environment secrets (`vars:secret` names), every `process.env` value, and runtime values that were also sent in an `Authorization`, `Proxy-Authorization` or `Cookie` header during this server session (exact match, values ≥ 8 characters, the run-history collection rule). The set of sent values lives in memory per collection, never on disk. Other values are shown, cut to 1 KiB (`truncated: true`). No reveal button.
- Underlines: `defined` subtle tint (`--var-defined-bg`), `setLater` amber dotted, `undefined` red wavy. Scripts and tests editors are not decorated.
- No new frontend dependency for fuzzy matching or highlighting.
- Quick open budget: under 16 ms per keystroke for 2,000 requests.
- New endpoints sit behind the session token. After DTO changes: `PLOVER_UPDATE_BINDINGS=1 cargo test -p plover-server --test bindings`, commit `frontend/src/api/types.ts`; never edit it by hand.
- Gate before every commit (`set -o pipefail` when piping): `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cd frontend && npm run typecheck && npm run lint && npm test`.
- Tests use a temporary `PLOVER_STATE_DIR` and `PLOVER_KEYCHAIN_SERVICE=dev.plover.test`; fixtures are synthetic (`example.test`, `tok-AAAA…`).
- Conventional Commit subjects ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Do not push.

## Review Focus

1. **Cancel racing completion.** The response arrives in the same instant the user hits Esc. Either the result shows and the cancel route returns 204 for a finished id, or the send reports `cancelled`; never both, never a stuck "Sending". Test `cancel_after_finish_is_noop_and_result_kept` (Task 1) and `late_cancel_keeps_result` (Task 2).
2. **Two tabs sending at once in the same collection, one cancelled.** Only the cancelled tab's send stops; the other completes and its runtime changes are kept. Test `cancel_one_of_two_concurrent_sends` (Task 1).
3. **A secret value that is also a plain substring elsewhere** (secret `abc12345` and a folder var `prefix-abc12345`): the hover masks the secret variable itself, and the folder var is shown as-is (the hover masks per variable, not by substring; substring masking is for run records). Test `vars_masks_by_variable_not_substring` (Task 5).
4. **Typing `{{` in the URL bar before the cached `/vars` result exists** (first open, slow server): no flicker of red over every token; tokens stay undecorated until the first result arrives, then classify. Test `no_underline_before_first_vars` (Task 6).
5. **Quick open with names that differ only by case or diacritics** (`Đăng nhập` vs `dang nhap`, `Login` vs `login`): matching is case-insensitive and diacritic-insensitive (NFD, strip combining marks), and ties keep tree order. Test `diacritics_and_case_insensitive` (Task 3).

---

### Task 0: Verify the merged interfaces

No code. Skip for Task 3 when it is built early.

- [ ] **Step 1: Check that these names exist** (use `codegraph explore` or grep). Update this plan where a name differs.
  - curl/HAR: `SendBody.item: Option<String>`, `SendInput.item_id: Option<&str>` (scratch sends), `Tab.item: string | null`, `UrlBar`'s `onCurl` prop and curl paste handling, `CurlBanner`.
  - run history: `plover_core::run_record::RequestDetail` (with `headers: Vec<(String, String)>`), `plover_core::masking::{MASKED_HEADERS, Masker}`, `RequestOutcome.request: Option<RequestDetail>`, `RunRecorder::results()`.
  - style/layout (only if it was built first): `--var-defined-bg` and `--warn` / `--err` tokens, `UrlBar` row with Send and `RequestMenu`.
- [ ] **Step 2:** Note "verified on <commit>" at the top of this plan and commit it with the first task.

### Task 1: Server cancel registry and route

**Files:**
- Modify: `crates/server/src/state.rs` (`sends` registry), `crates/server/src/api/dto.rs` (`SendBody.send_id`), `crates/server/src/api/requests.rs` (`send` with `tokio::select!`, `cancel`), `crates/server/src/api/mod.rs` (route), `crates/server/src/error.rs` or wherever `ApiError` codes live
- Regenerate: `frontend/src/api/types.ts`
- Test: `crates/server/tests/cancel.rs` (new)

**Interfaces:**
- Produces:
  ```rust
  // state.rs
  pub type SendRegistry = Mutex<HashMap<(String /* collection id */, String /* send id */), CancellationToken>>;
  // AppState: pub sends: Arc<SendRegistry>
  // dto.rs
  pub struct SendBody { /* existing */ pub send_id: Option<String> }
  ```
  `POST /collections/{id}/send/{send_id}/cancel` → `204`. `send_id` must match `^[A-Za-z0-9-]{1,64}$` (else 400 `bad_send_id`). The registry entry is removed by a drop guard when the send ends any way (ok, error, cancel, client disconnect). The choice of `CancellationToken` (tokio-util) vs `oneshot` is the builder's; the type above names the token version.

- [ ] **Step 1: Write the failing tests** in `crates/server/tests/cancel.rs` (wiremock with `set_delay`, server on a temp state dir):
  - `cancel_stops_slow_send_quickly`: send with `send_id: "s1"` to a mock delayed 10 s; cancel after 100 ms → the send returns 409 `cancelled` within 200 ms of the cancel.
  - `cancel_interrupts_looping_pre_script`: pre-request script `while (true) {}` (script timeout 30 s) → cancel returns the send as 409 `cancelled` within 200 ms.
  - `cancelled_send_keeps_no_runtime_vars`: pre-request `bru.setVar("x", 1)` then a slow request; cancel; a following send with test `expect(bru.getVar("x")).to.equal(undefined)` passes.
  - `cookies_before_cancel_are_kept` (C1): request A (fast, `Set-Cookie: sid=abc12345`) then in the same send a pre-request `bru.sendRequest` to a slow URL is cancelled → a later send carries `Cookie: sid=abc12345`.
  - `cancel_unknown_is_204`, `cancel_after_finish_is_noop_and_result_kept` (Review Focus 1).
  - `duplicate_send_id_is_409`: two concurrent sends with the same id → the second gets 409 `duplicate_send`.
  - `cancel_one_of_two_concurrent_sends` (Review Focus 2): `s1` and `s2` to slow mocks, cancel `s1` → `s2` completes 200 and its `bru.setVar` is visible afterwards.
  - `send_without_send_id_still_works` (older clients).
  - `cancel_route_needs_token`, `bad_send_id_is_400`.
- [ ] **Step 2: Run** `cargo test -p plover-server --test cancel` → FAIL.
- [ ] **Step 3: Implement.** `send` registers the token when `send_id` is present, then `tokio::select! { biased; _ = token.cancelled() => Err(ApiError::conflict("cancelled", "Request cancelled")), sent = send_one(..) => … }`. Dropping the `send_one` future is the whole cancellation; do not add flags to core.
- [ ] **Step 4: Regenerate bindings, run** the gate → PASS.
- [ ] **Step 5: Commit** `feat(server): cancel an in-flight send by id`.

### Task 2: Cancel in the UI — button, timer, Esc

**Files:**
- Modify: `frontend/src/store/tabs.ts` (`sendId`, `sendStartedAt`, `cancelledAfterMs`, `cancel`), `frontend/src/api/client.ts` (`send` takes `{ sendId, signal }`, `cancelSend`), `frontend/src/components/request/UrlBar.tsx`, `frontend/src/components/ResponseViewer.tsx` (cancelled banner), `frontend/src/App.tsx` (Esc in the shortcut effect)
- Create: `frontend/src/lib/elapsed.ts`
- Test: `frontend/src/lib/elapsed.test.ts`, `frontend/src/store/tabs.test.ts`, `frontend/src/components/request/cancel.test.tsx`, `frontend/src/components/a11y.test.tsx`

**Interfaces:**
- Consumes: Task 1 route and `SendBody.sendId`.
- Produces:
  - `formatElapsed(ms: number): string` — `3.2s` under 10 s (one decimal), `12s` under a minute, `1m 05s` above.
  - `ApiClient.send(id, item, draft, env, opts?: { sendId?: string; signal?: AbortSignal })`; `ApiClient.cancelSend(id: string, sendId: string): Promise<void>`.
  - Request tab state: `sendId?: string; sendStartedAt?: number; cancelledAfterMs?: number`.
  - `TabsState.cancel(key: string): Promise<void>` — posts the cancel route, aborts the fetch, sets `{ sending: false, result: undefined, cancelledAfterMs }`. A 409 `cancelled` from the send is treated the same way, not as an error.
  - `UrlBar`: while `sending`, the Send slot holds an **enabled** neutral-outline button `Cancel · 3.2s` (`aria-label="Cancel request"`; timer text in an `aria-live="off"` span), updated every 100 ms.
  - Response area after a cancel: neutral `Banner` "Cancelled after 3.2 s".
  - Esc (C2): in `App.tsx`'s keydown handler, when the active tab is a sending request tab, no `Modal`/quick open/value menu/var hover is open, and `!e.defaultPrevented` → `cancel(activeKey)`.

- [ ] **Step 1: Write the failing tests.**
  - `elapsed.test.ts`: 0 → `0.0s`, 3240 → `3.2s`, 9999 → `9.9s` (floor), 10000 → `10s`, 65000 → `1m 05s`.
  - `tabs.test.ts`: `send_generates_send_id`, `cancel_posts_route_and_aborts`, `409_cancelled_is_not_an_error`, `late_cancel_keeps_result` (Review Focus 1: the send resolves 200 before `cancelSend` resolves → the result is kept and no banner).
  - `cancel.test.tsx`: `button_shows_cancel_with_timer` (fake timers: after 3.2 s the button reads `Cancel · 3.2s`), `click_cancel_shows_banner`, `esc_cancels_active_send`, `esc_does_nothing_with_modal_open`, `esc_ignored_when_default_prevented`, `cancel_is_not_send_colour` (the Cancel button does not carry the `send` class).
  - `a11y.test.tsx`: the Cancel button has its accessible name and is focusable.
- [ ] **Step 2: Run** `cd frontend && npm test -- elapsed tabs cancel a11y` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the frontend gate → PASS.
- [ ] **Step 5: Commit** `feat(ui): cancel a send with a live timer or Esc`.

### Task 3: Quick open (Cmd/Ctrl+K) — can be built early

**Files:**
- Create: `frontend/src/lib/fuzzy.ts`, `frontend/src/components/QuickOpen.tsx`
- Modify: `frontend/src/App.tsx` (shortcut + mount), `frontend/src/editor/CodeEditor.tsx` (`KeyMod.CtrlCmd | KeyCode.KeyK` → dispatch `plover:quick-open`), `frontend/src/store/tabs.ts` (record recently opened keys for this page life)
- Test: `frontend/src/lib/fuzzy.test.ts`, `frontend/src/components/quick-open.test.tsx`, `frontend/src/components/a11y.test.tsx`

**Interfaces:**
- Consumes: `useWorkspace().trees` (request nodes of every open collection), `useWorkspace().collections` (names), `useTabs.open`.
- Produces:
  - `type QuickItem = { collectionId: string; item: string; method: string; name: string; path: string /* "Folder/Sub" */; collection: string }`.
  - `fold(s: string): string` — NFD, strip combining marks, map `đ/Đ` to `d`, lower-case.
  - `score(query: string, item: QuickItem): number | null` — subsequence match over `name` first, then over `collection / path / name`; `null` when not a subsequence of either; higher for contiguous runs, word starts and matches in `name` over `path` (weights are the builder's, C10).
  - `rank(query: string, items: QuickItem[], limit = 50): QuickItem[]` — stable: equal scores keep tree order.
  - `QuickOpen({ open, onClose })`: `role="dialog"` with `aria-label="Quick open"`, a `combobox` input (`aria-controls`, `aria-activedescendant`) and a `listbox`. Each option shows method, name, folder path and collection name. Empty query lists the tabs opened during this page life, newest first. Up/Down move, Enter opens the request (`useTabs.open`) and closes, Esc closes and returns focus to where it was. Opened by Cmd/Ctrl+K from `App.tsx` and from Monaco through `plover:quick-open` (C9 — Monaco's own Cmd+K chords are given up).
  - The sidebar's existing filter box stays.

- [ ] **Step 1: Write the failing tests.**
  - `fuzzy.test.ts`: `sign doc` ranks `Sign document` above `assign document type`; `diacritics_and_case_insensitive` (Review Focus 5: `dang nhap` matches `Đăng nhập`, `LOGIN` matches `login`); non-subsequence → `null`; `rank_is_stable_on_ties`; `rank_2000_under_16ms` (generated 2,000 items, query of 6 letters, median of 20 runs < 16 ms).
  - `quick-open.test.tsx`: `cmd_k_opens`, `ctrl_k_opens`, `plover_quick_open_event_opens` (what Monaco dispatches), `enter_opens_tab_and_closes`, `esc_restores_focus`, `empty_query_lists_recent_tabs`, `results_cap_50`.
  - `a11y.test.tsx`: the palette's combobox/listbox roles and `aria-activedescendant` follow the selection.
- [ ] **Step 2: Run** `cd frontend && npm test -- fuzzy quick-open a11y` → FAIL.
- [ ] **Step 3: Implement.** Precompute `fold`ed strings once per tree change (memoize on `trees`), not per keystroke.
- [ ] **Step 4: Run** the frontend gate → PASS.
- [ ] **Step 5: Commit** `feat(ui): quick open requests with Cmd/Ctrl+K`.

### Task 4: Labelled variable layers and the `setLater` scan in core

**Files:**
- Modify: `crates/core/src/vars.rs` (labels, `entries`), `crates/core/src/resolve.rs` (a public way to build the stack `send_one` uses, without running scripts), `crates/core/src/lib.rs`
- Create: `crates/core/src/var_scan.rs`
- Test: `crates/core/tests/vars.rs` (extend), `crates/core/tests/var_scan.rs`

**Interfaces:**
- Produces:
  ```rust
  // vars.rs
  pub enum VarSource { ProcessEnv, Runtime, Request, Folder, Environment, Collection } // serde camelCase
  pub struct VarEntry { pub name: String, pub source: VarSource, pub value: serde_json::Value, pub shadows: Vec<VarSource> }
  impl VarStack {
      /// One entry per name, winning source first by precedence (process env on top when set).
      pub fn entries(&self) -> Vec<VarEntry>;
  }
  // resolve.rs
  /// The stack a send of `item` (or a scratch draft when `item` is None) would start with,
  /// before scripts run: runtime and environment exactly as `send_one` builds them.
  pub fn preview_stack(open: &OpenCollection, item: Option<&str>, draft: Option<&HttpRequest>, env: Option<&str>) -> Result<VarStack, ResolveError>;
  // var_scan.rs
  pub struct SetLater { pub name: String, pub by: String /* item id of the first setter */ }
  pub fn scan_set_later(collection: &Collection) -> Vec<SetLater>;
  ```
  `scan_set_later` reads collection, folder and request scripts (pre and post) with the regex from the spec (`bru\.set(?:Env|Global(?:Env)?)?Var\(\s*["'`]([^"'`]+)["'`]`) plus names in `vars:post-response` blocks, in collection order (folders before requests, as the runner orders them); first setter wins.
  `lookup`/`interpolate` behaviour and performance are unchanged; how labels are carried is the builder's.

- [ ] **Step 1: Write the failing tests.**
  - `entries_report_winner_and_shadows`: `host` in collection and environment and runtime → `source: Runtime`, `shadows: [Environment, Collection]`.
  - `process_env_on_top`: `with_process_env` value wins with `source: ProcessEnv`.
  - `preview_uses_draft_vars_pre`: a draft whose `varsPre` defines `x` → `Request` source; the file's own value is ignored.
  - `preview_scratch_has_no_folder_layer` (`item: None`).
  - `lookup_unchanged`: existing `vars.rs` tests pass unchanged.
  - `var_scan.rs`: `finds_set_var_set_env_var_and_post_response_vars`, `ignores_computed_names` (`bru.setVar(name, 1)` not listed), `first_setter_wins_in_run_order`, `template_literal_without_interpolation_counts` (`` bru.setVar(`token`, t) ``).
- [ ] **Step 2: Run** `cargo test -p plover-core --test vars --test var_scan` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p plover-core` and the gate → PASS.
- [ ] **Step 5: Commit** `feat(core): report variable sources and names scripts set later`.

### Task 5: `POST /collections/{id}/vars` with masking

**Files:**
- Modify: `crates/core/src/workspace.rs` (`OpenCollection` keeps `sent_credentials` and the cached `set_later`), `crates/core/src/masking.rs` (expose the header collection rule), `crates/server/src/api/requests.rs` (record sent credential values after each send), `crates/server/src/api/runs.rs` (same for each run result), `crates/server/src/api/dto.rs`, `crates/server/src/api/mod.rs`
- Create: `crates/server/src/api/vars.rs`
- Regenerate: `frontend/src/api/types.ts`
- Test: `crates/server/tests/vars.rs` (new)

**Interfaces:**
- Consumes: Task 4; run history's `RequestOutcome.request: Option<RequestDetail>` and `MASKED_HEADERS`.
- Produces:
  ```rust
  // masking.rs — extracted from Masker::new, which then uses it
  pub fn credential_values(headers: &[(String, String)]) -> Vec<String>; // whole value, part after first space, cookie values; ≥ 8 chars
  // workspace.rs
  impl OpenCollection {
      pub fn note_sent_credentials(&self, values: Vec<String>);
      pub fn sent_credentials(&self) -> HashSet<String>;
      pub fn set_later(&self) -> Arc<Vec<SetLater>>; // computed on load and on reload
  }
  // dto.rs
  pub struct VarsBody { pub item: Option<String>, pub draft: Option<HttpRequestDto>, pub env: Option<String> }
  pub struct VarDto { pub name: String, pub source: String, pub value: Option<String>, pub masked: bool, pub truncated: bool, pub shadows: Vec<String> }
  pub struct SetLaterDto { pub name: String, pub by: String }
  pub struct VarsDto { pub vars: Vec<VarDto>, pub set_later: Vec<SetLaterDto> }
  ```
  Route `POST /collections/{id}/vars` → `VarsDto`. Values: strings as-is, other JSON as compact JSON, cut to 1024 bytes on a char boundary with `truncated: true`. `setLater` excludes names that `vars` already defines.

- [ ] **Step 1: Write the failing tests** in `crates/server/tests/vars.rs`:
  - `vars_lists_sources_and_shadows`.
  - `vars_masks_keychain_secret` (env `vars:secret [apiKey]` stored with the test keychain service).
  - `vars_masks_process_env` (`.env` value `DB_PASS`).
  - `vars_masks_runtime_value_sent_as_bearer`: a send whose post-response sets runtime `token = tok-AAAABBBBCCCC`, then a send with `Authorization: Bearer {{token}}` → `/vars` shows `token` masked; another runtime value of 8+ chars never sent in a header stays visible.
  - `vars_masks_by_variable_not_substring` (Review Focus 3).
  - `vars_cuts_to_1kib` (2 KiB value → 1024 bytes, `truncated: true`).
  - `vars_lists_set_later` (`token` set by `bru.setVar("token", …)` in `auth/login.bru` → `{ name: "token", by: "auth/login.bru" }`).
  - `vars_accepts_null_item_with_draft`, `vars_runs_no_script_and_sends_nothing` (wiremock expects zero calls; a pre-request script that would `bru.setVar` has no effect), `vars_needs_token`.
  - `run_results_feed_sent_credentials`: after a collection run that sends `Authorization: Bearer tok-…`, the runtime value equal to it is masked.
- [ ] **Step 2: Run** `cargo test -p plover-server --test vars` → FAIL.
- [ ] **Step 3: Implement.** After each send and each run result, call `open.note_sent_credentials(masking::credential_values(&detail.headers))` when a request detail exists.
- [ ] **Step 4: Regenerate bindings, run** the gate → PASS.
- [ ] **Step 5: Commit** `feat(server): explain variables with source and masking`.

### Task 6: Frontend variable cache and token classification

**Files:**
- Create: `frontend/src/store/vars.ts`, `frontend/src/lib/varTokens.ts`
- Modify: `frontend/src/api/client.ts` (`vars`), `frontend/src/store/tabs.ts` (trigger refresh after send/run and on env change), `frontend/src/store/workspace.ts` (refresh after environment save)
- Test: `frontend/src/lib/varTokens.test.ts`, `frontend/src/store/vars.test.ts`

**Interfaces:**
- Consumes: Task 5 `VarsDto`.
- Produces:
  - `ApiClient.vars(id: string, body: VarsBody): Promise<VarsDto>`.
  - `useVars` store: `byTab: Record<string /* tab key */, VarsDto | undefined>`; `refresh(tabKey: string): Promise<void>`. Refresh triggers: tab opened, its environment changed, after each send or run of its collection, after an environment save, and 300 ms after the draft's `varsPre` changes (debounced per tab).
  - `type VarClass = "defined" | "setLater" | "undefined"`.
  - `tokenize(text: string): Array<{ start: number; end: number; name: string }>` — every `{{name}}` with the same name rule as `VarStack::lookup` (dotted and indexed paths like `a.b[0]`, root name `a` decides the class).
  - `classify(name: string, vars: VarsDto | undefined): VarClass | null` — `null` when `vars` is `undefined` (no decoration before the first result, Review Focus 4).
  - `explain(name, vars): { value: string | null; masked: boolean; truncated: boolean; source: string; shadows: string[]; setBy?: string } | null` for the hover.

- [ ] **Step 1: Write the failing tests.**
  - `varTokens.test.ts`: tokenizes `{{a}}`, `{{a.b[0]}}`, `{{ a }}` (trimmed like core), ignores `{{` without close; classifies `{{a}}` defined, `{{missing}}` undefined, `{{token}}` setLater, `{{a.b[0]}}` by root `a`; `no_underline_before_first_vars` (Review Focus 4: `classify` with `undefined` → `null`).
  - `vars.test.ts`: `refresh_after_send`, `refresh_on_env_change`, `debounced_on_varsPre_edit` (three edits in 200 ms → one call 300 ms after the last), `scratch_tab_sends_item_null`.
- [ ] **Step 2: Run** `cd frontend && npm test -- varTokens vars` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the frontend gate → PASS.
- [ ] **Step 5: Commit** `feat(ui): cache and classify variables per tab`.

### Task 7: `VarInput` for plain fields, and the hover card

**Files:**
- Create: `frontend/src/components/request/VarInput.tsx`, `frontend/src/components/request/VarHover.tsx`
- Modify: `frontend/src/components/request/UrlBar.tsx` (URL input), `frontend/src/components/request/KeyValueTable.tsx` (value cells), `frontend/src/components/request/AuthTab.tsx` (value fields), `frontend/src/styles.css`
- Test: `frontend/src/components/request/var-input.test.tsx`, `frontend/src/components/a11y.test.tsx`, existing `curl-paste.test.tsx` must keep passing

**Interfaces:**
- Consumes: `tokenize`, `classify`, `explain` (Task 6), `useVars`.
- Produces:
  - `VarInput(props: InputHTMLAttributes<HTMLInputElement> & { tabKey: string })` — a normal `<input>` (all props and events pass through, including `onPaste` for curl paste) with an `aria-hidden` mirror `<div>` behind it, same font and padding, scrolled in sync, drawing `<span class="var var-defined|var-setlater|var-undefined">`. Hover hit-tests the mirror spans (`document.elementsFromPoint` or measured ranges, builder's choice).
  - `VarHover({ name, info, anchor })`: name; value or `•••• secret` when masked; `(cut to 1 KiB)` when truncated; source text (`Environment: Dev`, `Folder: <path>`, `Runtime`, `Request`, `Collection`, `Process env`); `overrides collection` / `overrides environment, collection` when `shadows` is not empty; for setLater: `Not set yet. Set at runtime by <request>.`; for undefined: `Not defined in any scope.` `role="tooltip"`, linked by `aria-describedby` while shown.
  - Keyboard: when the caret has been inside a `{{…}}` in a focused `VarInput` for 500 ms, the card shows for that token; Esc hides it (and marks the keydown handled so Esc does not also cancel a send).

- [ ] **Step 1: Write the failing tests.**
  - `var-input.test.tsx`: `mirror_draws_three_classes`, `hover_shows_value_and_source`, `hover_masks_secret`, `set_later_message`, `undefined_message`, `caret_inside_token_shows_after_500ms` (fake timers), `esc_hides_and_prevents_default`, `paste_curl_still_works` (the curl/HAR paste handler is still called through `VarInput`), `input_value_and_selection_unchanged` (typing and selection behave like a plain input).
  - `a11y.test.tsx`: the mirror is `aria-hidden`; the tooltip is referenced by `aria-describedby` while visible.
- [ ] **Step 2: Run** `cd frontend && npm test -- var-input curl-paste a11y` → FAIL.
- [ ] **Step 3: Implement.** Keep the mirror purely visual; never move text out of the input.
- [ ] **Step 4: Run** the frontend gate → PASS.
- [ ] **Step 5: Commit** `feat(ui): underline and explain variables in request fields`.

### Task 8: Variables in the Monaco body editor

**Files:**
- Modify: `frontend/src/editor/CodeEditor.tsx` (optional `varTabKey` prop), `frontend/src/editor/monaco.ts` (register one hover provider per language used by bodies), `frontend/src/components/request/BodyTab.tsx`
- Test: `frontend/src/editor/var-decorations.test.ts`

**Interfaces:**
- Consumes: `tokenize`, `classify`, `explain`, `useVars`.
- Produces: `varDecorations(model: { getValue(): string; getPositionAt(o: number): Position }, vars: VarsDto | undefined): IModelDeltaDecoration[]` (inline class names `var-defined`, `var-setlater`, `var-undefined`; empty when `vars` is `undefined`); a `HoverProvider` that returns the same text as `VarHover` in Markdown. Decorations update on model change (debounced 100 ms) and when the tab's `VarsDto` changes. Only `BodyTab` passes `varTabKey`; script and test editors do not.

- [ ] **Step 1: Write the failing tests** (pure functions over a fake model, no real Monaco): `decorations_for_three_classes`, `none_before_first_vars`, `hover_markdown_masks_secret`, `hover_markdown_set_later`.
- [ ] **Step 2: Run** `cd frontend && npm test -- var-decorations` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the frontend gate and `cargo test --workspace` (embedded UI build) → PASS.
- [ ] **Step 5: Commit** `feat(ui): variable underlines and hover in the body editor`.

### Task 9: Docs and final check

**Files:**
- Modify: `README.md` (keyboard: Cmd/Ctrl+K, Esc cancels; variable hover note), `docs/development.md` if it lists endpoints

- [ ] **Step 1:** Run the full gate.
- [ ] **Step 2:** Start `plv serve` against `fixtures/phase3-chain` and check by hand: Cancel on a slow local endpoint (a `python3 -m http.server` that sleeps, or wiremock in a test binary), Esc, Cmd+K in the URL bar and in the body editor, hover on a defined, a secret, a set-later and an undefined variable.
- [ ] **Step 3: Commit** `docs: cancel, quick open and variable hover`.

## Self-review

- **Spec coverage.** §4 server → Task 1, frontend + Esc → Task 2, C1 cookie rule → Task 1 test. §7 endpoint, labels, `setLater` → Tasks 4–5; masking rules incl. the user's Authorization rule → Task 5; classification, refresh triggers → Task 6; plain inputs (C8) → Task 7; Monaco → Task 8. §9 quick open (C9, C10, budget) → Task 3.
- **Spec tests.** Cancel within 200 ms, looping script, runtime vars not kept, unknown id 204 → Task 1. `/vars` masks keychain secret, `.env` value, bearer-sent runtime value; 1 KiB cut; `setLater`; `item: null` → Task 5. Frontend classes for `{{a}}`, `{{a.b[0]}}`, `{{missing}}`, `{{setLater}}` → Task 6. "sign doc" ranking, Enter opens, Cmd+K in Monaco, a11y → Task 3.
- **Type consistency.** `VarSource`/`VarEntry` (Task 4) feed `VarDto.source` strings (Task 5) which `explain` reads (Task 6); `SendRegistry` and `send_id` appear only in Task 1–2; `QuickItem`/`rank` only in Task 3.
- **Proportion.** Signatures, regexes and the values the spec fixes; no handler bodies.
