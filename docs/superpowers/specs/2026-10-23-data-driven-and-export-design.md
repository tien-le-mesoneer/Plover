# Data-driven runs and report export — design

Status: draft for review · 2026-10-23

Build this after [run detail and run history](2026-10-23-run-history-design.md). Both
features here depend on its run record, masking and history files.

## Why

Two features that Bruno puts behind a paid plan in its app
(https://www.usebruno.com/pricing) are useful for a local, single-user tool:

1. **Running a collection once per row of a CSV or JSON file.** One flow runs against
   many inputs, for example several tenants or document types. Bruno CLI 4.3.0 does
   this for free (`--csv-file-path`, `--json-file-path`, `--iteration-count`). Its app
   needs Pro or Ultimate. Plover has neither.
2. **Exporting a run's report from the app.** `plv` writes HTML, JSON and JUnit
   reports, but the app cannot. Bruno's app needs Ultimate for the HTML download.

## Goals (user decisions)

1. `plv run --csv-file-path F`, `--json-file-path F` and `--iteration-count N`, with
   Bruno CLI 4.3.0's behaviour (`@usebruno/cli/src/commands/run.js`).
2. In each request, `{{column}}` reads the current row. Scripts get
   `bru.runner.iterationData`, `bru.runner.iterationIndex` and
   `bru.runner.totalIterations`. A request without placeholders is sent unchanged in
   every iteration.
3. The app's Run panel can take a data file and an iteration count, and it groups the
   results by iteration.
4. Every result, history record and report says which iteration it belongs to.
5. "Export" in the app writes HTML, JSON or JUnit for a finished run or for a run from
   history.

## Non-goals

- Running iterations in parallel (`--parallel`). Deferred (*Claude's call*, see
  "Parallel" below).
- `--bail`. Plover has no bail today. It is a separate feature.
- Bruno's app runner configuration (choosing and reordering requests before a run).
- Choosing the data file per folder or saving it with the collection.
- Any run against the Stg environment.

## Anti-goals

- Values from a data file (they can be passwords) reaching history or reports
  unmasked when they are also secrets or credential header values. The run-history
  masking must cover them like any other text.
- An export that differs from what `plv runs show <id> --reporter-*` writes for the
  same run.
- A data run that silently does nothing: a missing file, a broken file or an empty
  file must fail loudly.

## Bruno's behaviour to match

All from Bruno CLI 4.3.0 in `@usebruno/cli/src` and `@usebruno/js/src`:

- **Iteration count** (`commands/run.js:1123-1145`). A CSV or JSON file sets the
  count to its number of rows and overrides `--iteration-count`. With no file,
  `--iteration-count` sets it (default 1). Each iteration runs the whole selection in
  the usual order.
- **CSV** (`utils/filesystem.js` `parseCSV`). The first row holds the column names. A
  leading BOM is removed, empty lines are skipped, and fields are trimmed. Values are
  strings. A row whose field count differs from the header is an error (csv-parse
  default).
- **JSON.** The top level is an array, and each element is one row. Values keep their
  JSON type.
- **Row values in `{{…}}`** (`runner/interpolate-vars.js:70-85`). Row data sits *above*
  runtime variables, which makes it the highest layer apart from `process.env`. The
  same holds for assert right-hand sides (`runtime/assert-runtime.js`
  `evaluateRhsOperand`, via `interpolate-string.js`). `bru.interpolate()`
  (`bru.js:204-229`) and `bru.getVar` do **not** see row data.
- **`bru.runner`** (`bru.js:145-158`, `IterationDataManager` at `bru.js:602-641`,
  QuickJS shim `sandbox/quickjs/shims/bru.js:190-230`):
  - `iterationData` is an object with `get(key?)` (no key returns the whole row),
    `set(key, value)`, `has(key)`, `unset(key)` and `stringify()`.
  - `set` and `unset` change the row for the rest of that iteration. Later requests,
    and the current request's own interpolation after its pre-request script, see the
    change (`prepare-request.js:31` shares the object).
  - `iterationIndex` is zero-based, and `totalIterations` is the count.
- **Variables across iterations.** One runtime-variable map is used for the whole run
  (`commands/run.js:721`), so `bru.setVar` in iteration 1 is visible in iteration 2.
  The cookie jar and environment changes also carry over.
- **Flow** (`commands/run.js:1282-1450`):
  - `--delay` waits after every request except the *last of each iteration*, so there
    is no wait between iterations.
  - `setNextRequest(null)` ends the current iteration only. The jump counter (10 000)
    is per iteration.
  - `bru.runner.stopExecution()` marks the rest of the current iteration as skipped
    (`stopExecution`) and runs no further iterations. Unreached iterations are not
    reported.
- **Output.** Every result carries `iterationIndex`. The JSON report is a list of
  `{ iterationIndex, summary, results }`, one per iteration reached
  (`utils/sanitize-results.js`). Plover's `json.rs` already writes this shape with one
  iteration. JUnit is one flat list of suites, and suite names do not mention the
  iteration (`reporters/junit.js`). The exit code is 1 when anything failed in any
  iteration.

## Decisions

**User** means the user decided it. **Claude's call** means the user has not reviewed
it yet.

### Core: data and iterations

- **One parser for both front ends**: a new module `crates/core/src/data_file.rs` with
  `parse_csv(text) -> Result<Vec<ValueMap>, DataFileError>` and
  `parse_json(text) -> Result<Vec<ValueMap>, …>`. The CLI and the server both use it.
  *Claude's call.*
  - CSV uses the `csv` crate. Errors name the line (`data.csv:7: expected 4 fields,
    found 3`).
  - `plv` resolves the file path against the current directory, as `bru` does, not
    against the collection root.
  - For JSON, a top level that is not an array, or an element that is not an object,
    is an error. Bruno would run zero iterations or crash. *Claude's call.*
- **Loud failures** (*Claude's call*, a deviation from Bruno, which runs nothing and
  exits 0):
  - a missing or unreadable file;
  - a file with zero rows;
  - a file larger than 16 MiB;
  - both `--csv-file-path` and `--json-file-path` on one command (Bruno merges rows by
    index and keeps the JSON count).

  All of these exit with 2 (`EXIT_SETUP`) and run nothing.
- **`--iteration-count` together with a data file**: the file wins, as in Bruno, and
  `plv` prints `--iteration-count ignored: <file> has N rows` to stderr.
  *Claude's call* on the message. `--iteration-count` must be a whole number ≥ 1.
- **`RunOptions`** gets `iterations: Iterations`, for example
  `Iterations { rows: Vec<ValueMap>, count: usize }`. Empty `rows` with `count` 1 is
  today's behaviour. *Claude's call.*
- **`run_with_state`** (`crates/core/src/runner.rs`) wraps its request loop in an
  iteration loop:
  - `jumps` and `index` reset each iteration, and `state` (`RunVars`) carries over.
  - The `--delay` check `index + 1 < refs.len()` already gives Bruno's "not after the
    last request of an iteration".
  - `setNextRequest(null)` breaks the inner loop only. `stop_execution` breaks both
    loops.
  - A cancel (the future is dropped) stops everything, as now.
- **Precedence**: add `Scope::Iteration` at the top of `vars::PRECEDENCE`, and
  `iteration: ValueMap` to `ScriptVars` (`script/api.rs`).
  - `ScriptVars::var_stack()` puts it above runtime. That covers request interpolation
    (`runner.rs`, where `var_stack().with_process_env` is built) and asserts, which use
    the same stack. `process.env` stays the top layer, as now.
  - `bru.interpolate` and `bru.getVar` in `script/api.js` must leave it out, to match
    Bruno.
  - `vars.rs` test `precedence_matches_bruno` gains `iteration`.
- **`bru.runner` in `script/api.js`** gains `iterationData` (the manager object above),
  `iterationIndex` and `totalIterations`, next to `skipRequest`, `stopExecution` and
  `setNextRequest`.
  - The row goes into the script as `vars.iteration`, and changes come back the same way
    the other scopes do (`runner.rs` `save`). Later requests in the iteration then see
    them.
  - For a single send from the request editor, `iterationIndex` and `totalIterations`
    are `undefined` and `iterationData.get()` is `{}`, as in Bruno's app. *Claude's call.*
- **Events**: `RunEvent::Started`, `Passed`, `Failed` and `Skipped` gain
  `iteration: usize`, and a new `RunEvent::IterationStarted { index, total, data }`
  comes before each iteration, `data` being the row as read from the file. `RunReport` totals cover all iterations and gain
  `iterations: usize` (the number reached). *Claude's call.*

### Parallel (Claude's call)

`--parallel` is accepted so that copied `bru run` command lines keep working. Plover
prints `--parallel is not supported yet; iterations run one after another` to stderr
and runs the iterations one at a time. Parallel iterations would multiply request
rates, and the real Dev server already answered with 429 at full speed. Revisit with
auto-retry on 429.

### CLI output

- When there is more than one iteration, or a data file is used, the printer shows
  `Iteration: N` before each iteration, as Bruno does (`commands/run.js:1229-1237`).
  `--verbose` also prints the row, as Bruno does with "CSV data:". It is masked with
  the values known at that point (secret values, and credential header values from
  earlier requests), the same rule as the live detail in the run-history design.
- The summary adds an "Iterations: N" line when N > 1. *Claude's call.*

### History and reports

- Each result in the run record (run-history design, "What a result keeps") gains
  `iteration_index`.
- The run record gains `iterations: [{ index, data }]`, holding each row reached after
  `set`/`unset` at the end of the iteration. It is masked with the same value-based
  pass as everything else. *Claude's call.* The UI shows the row, and Bruno's JSON
  shape is unaffected.
- **JSON report**: one `{ iterationIndex, summary, results }` entry per iteration
  reached, each with its own summary. `iterationData` is left out, as in Bruno's
  output.
- **JUnit report**: one flat list in run order. Suite names match Bruno and do not
  mention the iteration, so names repeat across iterations. *Claude's call*: match
  Bruno so the existing `bru`-vs-`plv` JUnit comparison keeps working.
- **HTML report**: one section per iteration, titled `Iteration N of M`, with the row's
  columns in a small table and that iteration's summary. With one iteration it looks
  as it does now. Still script-free.

### Run panel (UI)

- One-click "Run" in the sidebar stays as it is. The collection and folder actions
  gain **"Run with data…"**, which opens a `Modal` with these controls. *Claude's call.*
  - A file picker (`<input type="file" accept=".csv,.json">`). The browser reads the
    text, and the format comes from the extension. After a file is picked, the dialog
    shows the row count and column names, or the parse error, from a server call. It
    never shows row values.
  - An iteration count, disabled with "set by file (N rows)" once a file is chosen.
  - The environment comes from the current selection, as for Run.
- **`ClientMessage::Start`** (`crates/server/src/api/runs.rs`) gains optional
  `data: { format: "csv" | "json", text: String }` and `iterationCount: number`.
  - The server parses the text with `data_file`. A failure is answered with
    `RunEventDto::Error` code `bad_data` and the parser's message. The 16 MiB limit
    applies to `text`.
  - `POST /collections/{id}/data-file/preview` with the same `{ format, text }` returns
    `{ rows, columns }` for the dialog. *Path is Claude's call.*
  - `RunEventDto` events gain `iteration`, and a new
    `iterationStarted { index, total, data }` is added, with `data` masked like the
    live detail. ts-rs regenerates the TS types.
- **Results grouped by iteration** in `RunPanel.tsx`.
  - When there is more than one iteration, each one starts with a header row:
    `Iteration 2 of 5`, its pass/fail counts and its row as `key=value` chips. The values
    are masked by the server, and long values are cut at 40 characters with the full
    text in a tooltip.
  - The row detail pane from the run-history design works the same per row.
  - The data file is not remembered after the dialog closes or the app restarts,
    because it can hold credentials. *Claude's call.*

### Export (UI)

- **Report code moves to core** (*Claude's call*): `crates/cli/src/report/{mod,json,
  junit,html}.rs` become `crates/core/src/report/`, rendering from the core run record
  that the run-history design introduces. `plv run`, `plv runs show` and the app all
  call the same functions. The same masked record therefore gives the same bytes,
  apart from timestamps and hostname in JUnit.
- **Endpoint**: `GET /collections/{id}/runs/{runId}/report?format=html|json|junit`.
  - It returns the file with `Content-Type` and
    `Content-Disposition: attachment; filename="<collection>-<runId>.<ext>"`.
  - It answers from the finished run in memory, or from the history file, with the same
    lookup as the run-history detail endpoint. A run whose history save failed is
    still exportable until the server stops.
  - 409 `run_in_progress` while the run is still going.
- **Button**: the Run panel header gets **Export ▾ (HTML, JSON, JUnit)**. It is
  enabled for a finished, cancelled or history run, and disabled while running.
  `--reporter-skip-*` options are not offered, so export always writes the full masked
  record. *Claude's call.*
- **Saving the file** (*Claude's call*):
  - In a browser, the UI fetches with the token header, then saves through a `Blob`
    and `<a download>`.
  - In the desktop app, webviews do not handle that reliably, so `NativeHost`
    (`crates/server/src/native.rs`) gains
    `save_file(default_name, bytes) -> Option<PathBuf>`, a native save dialog, through
    a `POST /native/save-report` route.
  - `capabilities` tells the UI which path to use, the same way `pick_folder` does.

## Left to the builder

- Exact Rust and TS names, and whether `Iterations` is a struct or an enum.
- Visual design of the "Run with data…" dialog, the iteration header rows and the
  Export menu. Use `Modal`, the existing tokens and `docs/ui-ux-review.md`.
- How JSON rows that hold non-string values look in chips and in the HTML table
  (compact JSON is fine).
- Whether duplicate CSV column names are an error or last-wins. Check csv-parse's
  `columns: true` behaviour and match it.
- Tests. At minimum:
  - `{{col}}` wins over a runtime variable of the same name; `bru.getVar` and
    `bru.interpolate` do not see it; an assert RHS does;
  - `iterationData.set` in a pre-request script is seen by that request's URL and by
    the next request, and is gone in the next iteration;
  - `bru.setVar` carries over from iteration 1 to 2;
  - `--delay` is not applied after the last request of an iteration;
  - `setNextRequest(null)` ends one iteration only; `stopExecution` stops the run and
    `RunReport.iterations` shows how far it got;
  - CSV with a BOM, quoted commas and newlines, blank lines, a short row (error with
    the line); JSON that is not an array (error); zero rows (exit 2); CSV plus JSON
    (exit 2); `--iteration-count 3` with no file runs three times;
  - JSON report has one entry per iteration with its own summary; the HTML report shows
    iteration sections; JUnit suite names match Bruno's;
  - a data value equal to a secret's value is `***` in history, reports and the UI
    chips;
  - export through the endpoint gives the same JSON as `plv runs show --reporter-json`
    for the same run;
  - a Bruno comparison with a small public fixture (`fixtures/`): the same
    data file through `bru run` and `plv run` gives the same JSON report, apart from
    timings.

## Open questions for the user

1. **Credential columns in a data file.** A CSV often has a `password` or
   `client_secret` column. Value-based masking (run-history design) only hides values
   that are environment secrets or that were sent in `Authorization`, `Cookie` and
   similar headers. A password sent in a JSON login body would be stored in plain text
   in history, reports, the `--verbose` output and the UI chips.
   *Recommendation:* treat the values of columns whose names are on the credential
   list from the curl/HAR design (`password`, `client_secret`, `token`, `api_key`, …,
   matched case-insensitively) as values to mask in pass 1 of the masking. Other
   columns stay visible.
