# Run detail and run history — design

Status: draft for review · 2026-10-23

## Why

After an automated run (`plv run`, or Run in the app) you cannot see what was actually
sent or received. A failed row shows its test names and an error message, but not the
URL after variables were filled in, the headers, the request body, or the response. You
have to guess why it failed, or send the request again by hand.

Bruno keeps the request and response for every result. Its app runner shows them when
you click a row, and its CLI JSON and HTML reports include them. Bruno does not keep
runs once the app closes.

Plover drops this data. `RequestOutcome` (`crates/core/src/runner.rs`) holds the
status, times, size, tests, asserts, console, script error and scripted requests. It
holds no headers and no bodies. `crates/cli/src/report/json.rs` says so in its module
comment.

## Goals

1. Every finished request in a run keeps:
   - the request as sent: method, final URL, headers and body;
   - the response: status, status text, headers and body.
2. App runner: clicking a row opens that request's detail, like Bruno's runner (decided
   by the user: "like Bruno").
3. The `plv` JSON and HTML reports include the same detail, shaped like Bruno's.
4. Every run is saved to a local run history, so it can be reopened after the app
   restarts. Runs started by `plv run` are saved too (decided by the user).
5. The app can list earlier runs and open one, and it looks exactly as it did when that
   run finished.

## Non-goals

- Comparing two runs ("3 more failures than last time"). Deferred.
- History for single sends from the request editor. Only runner runs are saved.
- Bodies of requests that scripts send with `bru.sendRequest` or `axios`. Those keep
  their current summary in `ScriptedRequest`.
- Syncing history between machines, or storing it in CI artifacts.
- Any testing against the Stg environment.

## Anti-goals (a failure even if it works)

- Secrets reaching disk in plain text that Plover could have masked, for example
  environment secrets or `Authorization` headers.
- History written inside the collection folder, where it could be committed to git.
- A failed or slow history write that changes a run's result, exit code or report.
- A run with large bodies (16 MiB uploads exist in real collections) that bloats memory,
  the WebSocket or the history directory without limit.

## Constraints

- **Result shape for reports.** Match Bruno CLI's result entry
  (`bruno-cli/src/runner/run-single-request.js`, the object returned near the end):
  - `request: { method, url, headers, data }`
  - `response: { status, statusText, headers, data, url, responseTime, duration, size }`

  A request that errored before getting a response carries nulls (`status: "error"`,
  `size: 0`). This is what Bruno does, and Plover already does it for `status`.
- **Bruno's report flags.** Bruno's HTML report (`bruno-common/src/runner/reports/html/template.ts`)
  shows request and response headers as tables and bodies as `<pre>`. Plover's HTML report
  must stay self-contained: no scripts, no external resources (`crates/cli/src/report/html.rs`).
  Bruno CLI has `--reporter-skip-all-headers`, `--reporter-skip-headers <names…>` and
  `--reporter-skip-body` (`bruno-cli/src/commands/run.js`). Plover accepts them with the
  same meaning, so existing `bru run` command lines keep working.
- **State directory.** History lives under `app_state::default_dir()`
  (`crates/core/src/app_state.rs`), which honours `PLOVER_STATE_DIR`. Tests and CI must
  set `PLOVER_STATE_DIR` to a temporary directory and must never touch the real one.
- **Concurrency.** The app server and `plv` can run at the same time on the same
  collection.
- **Platforms.** The same CI matrix as now: macOS, Windows and Linux. Writes are atomic
  and survive the Windows rename issues already handled by `store::write_atomic`.

## Decisions

Each decision says who made it. **User** means the user decided it in conversation.
**Claude's call** means the user has not reviewed it yet.

### What a result keeps

- Add `RequestDetail` and `ResponseDetail` to the core types. *Claude's call.*
  - They are captured in `crates/core/src/runner.rs` at the send site (`self.session.send(&resolved)`).
    The request side comes from `ResolvedRequest` (method, URL, headers, body). The
    response side comes from `ResponseSummary` (status, headers, body, `truncated`).
  - They are stored on `RequestOutcome` as `request: Option<RequestDetail>` and
    `response: Option<ResponseDetail>`.
  - If a post-response script replaces the body (`res.setBody`), the detail shows the
    replaced body, like Bruno.
- Partial results (*Claude's call*, from the builder check): a pre-request failure has
  no request or response detail (Bruno's nulls); a transport error keeps the request
  detail and has no response; a skipped request has neither.
- Status text is the standard reason phrase for the code. *Claude's call.*
- Bodies are stored as text when they are valid UTF-8, otherwise as base64 with
  `encoding: "base64"`. *Claude's call.*
  - Form bodies are stored as key/value pairs.
  - Multipart bodies are stored as a list of parts. Text parts keep their values; file
    parts keep only the file name.
- **Body cap: 1 MiB** for each request body and each response body. *Claude's call.*
  - Anything longer is cut, and the detail records `truncated: true` and the full size.
  - **Per-run cap: 50 MiB of stored bodies.** After that, later results keep their
    headers but drop their bodies, with a `bodyOmitted: "run size limit"` marker.

### Masking (Claude's call)

Masking runs once, when a run finishes, before the run is saved or any report is
written. It is a single function in core, used by both history and reports. It replaces
values with `***`:

1. The values of all headers named `Authorization`, `Proxy-Authorization`, `Cookie` and
   `Set-Cookie`, in both requests and responses. Names are matched case-insensitively.
2. Every occurrence of a secret's value in URLs, headers and bodies. A secret is a
   `vars:secret` variable of the selected environment, whatever its source: keychain,
   in-memory value, or `--env-var` with a secret's name. This matches what
   `runner::secret_values` selects. Values shorter than 4 characters are not masked, so
   `***` does not appear everywhere.
3. Values read from `process.env` are not masked.

Masking applies to the live detail in the app too. The app never shows a different
value than history would store.

### Run history

- **Location and layout.** *Claude's call.*
  - Each collection has a folder `<state_dir>/runs/<key>/`, where `<key>` is the first
    16 hex characters of the SHA-256 of the canonical collection root.
  - Each run writes its own files, so concurrent runs never write the same file:
    - `<run-id>.json` holds the whole run.
    - `<run-id>.meta.json` holds a summary: collection root and name, target, environment
      name, source (`app` or `plv`), start time, duration, `RunReport` counts and outcome.
      Listing runs reads only these summaries.
  - The run ID is `<UTC yyyymmddThhmmssZ>-<6 random base32 chars>`, so IDs sort by time.
  - Files are written with `store::write_atomic` (made `pub` if needed). The summary file
    is written last, so a run is listed only after its full file exists.
- **When a run is saved.** *Claude's call.*
  - The run is saved once, when it ends. A cancelled run is saved with outcome
    `cancelled` and the results it had so far.
  - If the process is killed during the run, nothing is saved.
  - If the save fails, Plover logs a warning and carries on. The run's exit code and
    reports do not change.
- **Retention: 20 runs per collection.** *Claude's call.*
  - After saving, the oldest runs beyond 20 are deleted, both files of each.
  - A file that is already gone is ignored, which keeps concurrent pruning safe.
  - Unreadable or unknown-version files are skipped when listing. They count toward the
    limit and are pruned like any other run.
- **`plv run` saves to history** (user decision), with these switches. *Claude's call.*
  - `plv` does not save when the environment variable `CI` is set to anything other than
    empty, `0` or `false`.
  - `--history` forces saving and `--no-history` turns it off.
  - After saving, `plv` prints `Saved run <id>` to stderr, so stdout stays unchanged for
    scripts that read it.
- **App runs always save.** The app runs with source `app`. *Claude's call.*

### `plv runs` (Claude's call)

- `plv runs [DIR]` lists the runs of the collection at `DIR` (default: the current
  directory), newest first. It shows ID, time, source, environment, counts and duration.
- `plv runs show <ID> [--reporter-json F] [--reporter-html F] [--reporter-junit F]`
  rebuilds the console summary and any reports from a saved run, without sending
  anything.
  - `<ID>` is looked up across all collections' history folders.
  - The skip-header and skip-body flags work here too.
- `plv runs clear [DIR]` deletes that collection's history. It asks for confirmation
  unless `--yes` is given.

### App UI

- **Request detail in the runner** ("like Bruno", user decision).
  - Clicking a finished row in `RunPanel.tsx` opens a detail pane for that result.
  - The pane has tabs: Response, Headers, Request, Tests, Console. Request holds what
    Bruno's Timeline tab shows: method, URL, headers and body as sent. *Tab names are
    Claude's call.*
  - The pane reuses the existing response components (`ResponseViewer.tsx`,
    `components/response/`) where they fit.
  - Rows still running, or skipped, have no detail. Skipped rows show their reason.
- **Detail fetched on click.** *Claude's call.*
  - Run events over the WebSocket (`RunEventDto` in `crates/server/src/api/runs.rs`) stay
    small and carry no bodies.
  - When a row is clicked, the UI fetches that result's detail from the server by run ID
    and result index.
  - While a run is going, the server answers from the run's records in memory. Afterwards
    it answers from the history file.
  - Live runs and old runs use the same code path.
- **Earlier runs.** *Claude's call.*
  - The run panel header gets a "History" control that lists the collection's earlier
    runs, from both app and `plv`, with time, source, environment and counts.
  - Opening one shows it in the same panel, read-only, exactly as a finished live run
    looks.
  - The list has "Clear history".
- **New server endpoints**, following the existing `/collections/{id}/...` style in
  `crates/server/src/api/mod.rs`. *Paths are Claude's call.*
  - `GET /collections/{id}/runs` lists runs.
  - `GET /collections/{id}/runs/{runId}` returns a run without bodies.
  - `GET /collections/{id}/runs/{runId}/results/{index}` returns one result's detail.
  - `DELETE /collections/{id}/runs` clears the history.
  - DTOs are generated by ts-rs, like the other API types.

### Reports

- The JSON report adds `request.headers`, `request.data`, `response.statusText`,
  `response.headers`, `response.data`, `response.url` and `response.duration`. This
  makes it match Bruno's entry, and removes the "minus headers and bodies" note in
  `json.rs`.
  - `data` is parsed JSON when the content type is JSON and the body parses. Otherwise it
    is a string. Check Bruno's output for the exact rule before relying on it.
- The HTML report gets a collapsible `<details>` section per result, with header tables
  and the escaped body in `<pre>`. It stays script-free.
- The JUnit report does not change.
- Reports are built from the same masked records as history (see "Masking").
- Reports are built from a single shared record shape. `report::Collector` and
  `RequestRecord` (`crates/cli/src/report/mod.rs`) become, or are built from, the core
  "run record" type that history also uses. This way a saved run and a live run render
  through the same code. *Claude's call.*

## Left to the builder

- Exact Rust and TypeScript type and field names, and the module layout. One natural
  layout is a `run_history` module in core next to `app_state`.
- How the on-disk JSON versions itself. It needs a `version` field. Unknown versions are
  skipped, not fatal.
- Visual layout of the detail pane and the History list. Use the existing tokens and
  components, and follow the UI review rules in `docs/ui-ux-review.md`.
- How to show truncated and omitted bodies in the UI and in reports.
- Whether `plv runs` output has a `--json` form.
- Test design. At minimum:
  - masking of each header and of a secret value in a URL, header and body;
  - the 1 MiB body cap and the 50 MiB run cap;
  - concurrent saves and pruning from two processes;
  - `CI=true` with no flag, with `--history` and with `--no-history`;
  - `plv runs show` producing the same JSON report as the live run;
  - a cancelled run being saved.

## Open questions for the user

Only the user can answer these, and each answer changes what is built.

1. **Tokens in response bodies.** Masking covers known secrets and auth headers. It does
   not cover a token that a login request *returns* in its body (for example
   `access_token`) and a script stores with `bru.setVar`. As designed, those bodies are
   saved in plain text in the local history (20 runs). Is that acceptable, or should
   Plover also mask runtime variables that scripts set and later send in auth headers?
   Or should it not save bodies of selected requests?
2. **Reports and Bruno parity.** Bruno writes headers and bodies to `--reporter-json` and
   `--reporter-html` unmasked. As designed, Plover's reports use the same masking as
   history, which differs from Bruno, so CI artifacts contain `***`. Keep the masking, or
   match Bruno exactly and rely on the `--reporter-skip-*` flags?
