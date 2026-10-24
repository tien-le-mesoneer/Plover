# Data-Driven Runs and Report Export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Starts after run history is merged.** This plan consumes the types, functions and routes that `docs/superpowers/plans/2026-10-23-run-history.md` produces (`RunRecord`, `ResultRecord`, `RunRecorder`, `RunEnd`, `Masker`, `secret_strings`, `RunHistory`, `plover_core::report::{json,html,junit}`, `ReportFilter`, `LiveRuns`, the `/collections/{id}/runs…` routes, `RunResultDetail`, `RunHistoryMenu`). Task 1 Step 0 checks they exist under these names.

**Goal:** `plv run` and the app run a collection once per row of a CSV or JSON file (or N times), with Bruno CLI 4.3.0's semantics, and the app exports any finished or saved run as HTML, JSON or JUnit.

**Architecture:** A core `data_file` module parses rows; `RunOptions.iterations` drives an outer loop in `run_with_state`, and each row becomes a new top variable scope (`Scope::Iteration`) plus `bru.runner.iterationData` in scripts. Events, run records and the core report renderers gain an iteration index. Credential-named column values join the masker's start secrets. The server accepts a data file on `Start`, previews files, and serves reports for a run; the desktop host gains a native save dialog.

**Tech Stack:** Rust (tokio, serde, `csv` crate, axum, ts-rs, clap, wiremock, assert_cmd), QuickJS script API (`crates/core/src/script/api.js`), React + TypeScript + Zustand + Vitest.

**Spec:** `docs/superpowers/specs/2026-10-23-data-driven-and-export-design.md` (UI layout: `docs/superpowers/specs/2026-10-23-dev-ux-design.md` §6)

## Global Constraints

- Bruno reference: Bruno CLI 4.3.0, `@usebruno/cli/src/commands/run.js` (flags at 452-470, iteration loop 1200-1560), `runner/interpolate-vars.js:70-85`, `@usebruno/js/src/bru.js:145-158, 204-229, 602-641`. When the spec and Bruno disagree, the spec's marked deviations win; everything else matches Bruno.
- Flags, Bruno's names: `--csv-file-path <FILE>`, `--json-file-path <FILE>`, `--iteration-count <N>` (whole number ≥ 1), `--parallel` (accepted, warns, runs sequentially).
- Loud failures exit **2** (`EXIT_SETUP` in `crates/cli/src/lib.rs`) and send nothing: missing/unreadable file, zero rows, file over **16 MiB** (`16 << 20` bytes), both data flags together, JSON top level not an array or an element not an object, CSV row with the wrong field count (message names the file and line: `data.csv:7: expected 4 fields, found 3`).
- Exact stderr texts: `--iteration-count ignored: <file> has N rows`; `--parallel is not supported yet; iterations run one after another`.
- `plv` resolves data-file paths against the current directory, not the collection root.
- Row values sit above runtime variables in `{{…}}` and assert right-hand sides; `process.env` stays the top layer; `bru.getVar` and `bru.interpolate` do **not** see row values.
- Credential column names (case-insensitive, exact, JSON keys at any depth): `password`, `passwd`, `client_secret`, `secret`, `token`, `access_token`, `refresh_token`, `id_token`, `api_key`, `apikey`, `assertion`, `client_assertion`. One list in core, shared with the curl/HAR import.
- The data file is never written to disk by Plover and never remembered by the UI.
- Exports are byte-identical to `plv runs show <id> --reporter-*` for the same run (JUnit timestamp/hostname excepted). HTML stays script-free with no external resources.
- All run-history constraints still hold (masking, history location, `PLOVER_STATE_DIR` in tests, no real-collection content in fixtures, fake tokens like `tok-AAAABBBBCCCC`).
- Gate before every commit (`set -o pipefail` when piping): `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`; frontend tasks also `cd frontend && npm run typecheck && npm run lint && npm test`. After DTO changes: `PLOVER_UPDATE_BINDINGS=1 cargo test -p plover-server --test bindings`, commit `frontend/src/api/types.ts`.
- Conventional Commit subjects; end each commit message with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Do not push.

## Review Focus

1. **A CSV saved on Windows** (CRLF line endings, a BOM, a quoted field containing a newline and a comma): parsed to the same rows as the LF version. Test in Task 1.
2. **A column with the same name as an environment variable** (`baseUrl`): `{{baseUrl}}` uses the row, `bru.getVar("baseUrl")` and `bru.getEnvVar("baseUrl")` are unaffected, and the environment file on disk is never changed by the run. Test in Task 2 and Task 3.
3. **Non-string JSON row values** (`3`, `true`, `null`, `{"a":1}`) in a URL and in an assert RHS: interpolate exactly as the same values do from a runtime variable today (numbers and booleans as text, objects as compact JSON). Test in Task 2.
4. **Cancel during iteration 3 of 5** (app) or Ctrl-C equivalent drop: the saved record is `cancelled`, has `iterations` for 1–3 only, and its Export works. Test in Task 7.
5. **Many iterations** (200 rows × 2 requests): the run-history 50 MiB body budget spans all iterations (later bodies `Omitted`), and JSON report entries stay one per iteration in order. Test in Task 4 and Task 5.

---

### Task 1: Data files and the credential list in core

**Files:**
- Create: `crates/core/src/data_file.rs`, `crates/core/src/credentials.rs`
- Modify: `crates/core/src/lib.rs`, `crates/core/Cargo.toml` (`csv = "1"`)
- Test: `crates/core/tests/data_file.rs`

**Interfaces:**
- Consumes: `vars::ValueMap`.
- Produces:
  ```rust
  // data_file.rs
  pub const MAX_DATA_FILE: usize = 16 << 20;
  pub enum DataFormat { Csv, Json }            // serde "csv" | "json"
  impl DataFormat { pub fn from_path(p: &Path) -> Option<DataFormat> }  // .csv / .json, case-insensitive
  pub enum DataFileError { TooLarge { size: usize }, Empty, Csv { line: u64, message: String }, Json(String), NotArray, NotObject { index: usize } }
  // Display: "<name>:<line>: <message>" is formed by the caller via `with_name(name)`.
  impl DataFileError { pub fn with_name(&self, name: &str) -> String }
  pub fn parse(format: DataFormat, text: &str) -> Result<Vec<ValueMap>, DataFileError>;
  pub fn parse_csv(text: &str) -> Result<Vec<ValueMap>, DataFileError>;
  pub fn parse_json(text: &str) -> Result<Vec<ValueMap>, DataFileError>;
  /// Column names in first-seen order across rows.
  pub fn columns(rows: &[ValueMap]) -> Vec<String>;
  /// String values of credential-named keys at any depth in every row (for the masker).
  pub fn credential_values(rows: &[ValueMap]) -> Vec<String>;
  // credentials.rs
  pub const CREDENTIAL_FIELDS: [&str; 12] = ["password", "passwd", "client_secret", "secret", "token", "access_token", "refresh_token", "id_token", "api_key", "apikey", "assertion", "client_assertion"];
  pub fn is_credential_field(name: &str) -> bool;   // ASCII case-insensitive, exact
  ```

- [ ] **Step 0: Verify the run-history interfaces exist** — `rg -n "pub struct RunRecord|pub struct ResultRecord|pub struct RunRecorder|pub enum RunEnd|pub struct Masker|pub fn secret_strings|pub struct RunHistory|pub struct ReportFilter|pub mod report|LiveRun" crates/` and `ls frontend/src/components/RunResultDetail.tsx frontend/src/components/RunHistoryMenu.tsx`. If any name, signature or path differs from this plan, update this plan before coding. If the curl/HAR branch is merged and already has a credential list in `har.rs`, move it to `credentials.rs` (same list) and make `har.rs` use it instead of creating a second list.
- [ ] **Step 1: Write the failing tests** in `crates/core/tests/data_file.rs`:
  - `csv_basic`: `"user,pass\nalice,p1\nbob,p2\n"` → 2 rows, `{"user":"alice","pass":"p1"}` …, values are strings.
  - `csv_bom_crlf_quotes_blank_lines` (Review Focus 1): `"\u{feff}a,b\r\n\"x,1\",\"line1\nline2\"\r\n\r\n y , z \r\n"` → rows `[{a:"x,1",b:"line1\nline2"},{a:"y",b:"z"}]` (fields trimmed, blank line skipped); the LF-only version gives equal rows.
  - `csv_wrong_field_count_names_line`: header 4 fields, line 7 has 3 → `Csv { line: 7, .. }`, `with_name("data.csv")` starts with `data.csv:7:`.
  - `csv_header_only_is_empty`: `"a,b\n"` → `Empty`.
  - `json_rows_keep_types`: `[{"n":3,"b":true,"z":null,"o":{"a":1}}]` → values `Number(3)`, `Bool(true)`, `Null`, object.
  - `json_not_array` → `NotArray`; `[1]` → `NotObject { index: 0 }`; `[]` → `Empty`.
  - `too_large`: `parse` with text of `MAX_DATA_FILE + 1` bytes → `TooLarge`.
  - `columns_first_seen_order`: rows `{b,a}`, `{a,c}` → `["b","a","c"]`.
  - `credential_values_by_name`: rows with `password`, `Client_Secret`, nested `{"auth":{"token":"t-1234567"}}`, a numeric `token: 7` and `username` → returns the three string values only; `newPassword` not matched.
  - For duplicate CSV column names, first check what csv-parse `columns: true` does (last value wins) and add `csv_duplicate_columns_last_wins` to match it.
- [ ] **Step 2: Run** `cargo test -p plover-core --test data_file` → FAIL.
- [ ] **Step 3: Implement.** CSV via `csv::ReaderBuilder::new().has_headers(true).trim(csv::Trim::All).flexible(false)`; strip a leading BOM first; skip records whose every field is empty. JSON via `serde_json::from_str::<serde_json::Value>`.
- [ ] **Step 4: Run** `cargo test -p plover-core` → PASS.
- [ ] **Step 5: Commit** `feat(core): parse CSV and JSON data files`.

### Task 2: Iteration scope and `bru.runner.iterationData`

**Files:**
- Modify: `crates/core/src/vars.rs` (`Scope::Iteration`, `PRECEDENCE`, `VarStack::from_values`), `crates/core/src/script/api.rs` (`ScriptVars.iteration`, `var_stack`), `crates/core/src/script/api.js` (`bru.runner`), `crates/core/src/runner.rs` (`save` copies `iteration` back)
- Test: `crates/core/tests/vars.rs` (`precedence_matches_bruno`), `crates/core/tests/script_gaps.rs` (or a new `crates/core/tests/iteration_scripts.rs`)

**Interfaces:**
- Consumes: none from Task 1.
- Produces:
  ```rust
  pub enum Scope { Iteration, Runtime, Request, Folder, Environment, Collection }
  pub const PRECEDENCE: [Scope; 6] = [Scope::Iteration, Scope::Runtime, Scope::Request, Scope::Folder, Scope::Environment, Scope::Collection];
  pub struct ScriptVars { pub iteration: ValueMap, pub runtime, pub environment, pub collection, pub folder, pub request }   // iteration: #[serde(default)]
  /// For one script run: the iteration position, `None` for a single send.
  pub struct IterationInfo { pub index: usize, pub total: usize }
  ```
  `VarStack::from_values` gains a leading `iteration: ValueMap` parameter; update every caller. JS: `bru.runner.iterationData` = `{ get(key?), set(key, value), has(key), unset(key), stringify() }` over `vars.iteration`; `bru.runner.iterationIndex` / `totalIterations` from the script context (`undefined` when `IterationInfo` is `None`).

- [ ] **Step 1: Write the failing tests.**
  - `vars.rs` `precedence_matches_bruno`: an `iteration` layer value beats runtime, request, folder, environment, collection; `process.env` still beats it.
  - `row_beats_runtime_in_url_and_assert`: `iteration {x:"row"}`, runtime `{x:"rt"}` → URL `{{x}}` is `row`; assert `res.body.v: eq {{x}}` compares with `row`.
  - `getvar_and_interpolate_ignore_row` (spec): in a pre-request script `bru.getVar("x") === "rt"` and `bru.interpolate("{{x}}") === "rt"`.
  - `same_name_as_env_var` (Review Focus 2): env `baseUrl=http://env`, row `baseUrl=http://row` → request goes to `http://row`; `bru.getEnvVar("baseUrl") === "http://env"`.
  - `non_string_values_interpolate_like_runtime` (Review Focus 3): row values `3`, `true`, `null`, `{"a":1}` in `{{n}}`/`{{b}}`/`{{z}}`/`{{o}}` give the same URL text as the same values set with `bru.setVar`.
  - `iteration_data_api`: `get()` returns the whole row; `set("k","v")` then `has("k")`; `unset("k")`; `stringify()` is the row's JSON; `set` in a pre-request script changes that request's interpolated URL.
  - `single_send_has_no_iteration`: `bru.runner.iterationIndex === undefined`, `totalIterations === undefined`, `iterationData.get()` deep-equals `{}`.
- [ ] **Step 2: Run** `cargo test -p plover-core` → FAIL.
- [ ] **Step 3: Implement.** Pass `IterationInfo` into the script context the same way the request path is passed today. `bru.getVar`/`bru.interpolate` keep reading `vars.runtime` and the existing stack without `iteration` (build that stack from the five old scopes). The runner's `save` copies `vars.iteration` back to the iteration's row so later requests see `set`/`unset`.
- [ ] **Step 4: Run** `cargo test -p plover-core` → PASS.
- [ ] **Step 5: Commit** `feat(core): add the iteration variable scope and bru.runner.iterationData`.

### Task 3: The iteration loop in the runner

**Files:**
- Modify: `crates/core/src/runner.rs` (`RunOptions`, `RunEvent`, `RunReport`, `run_with_state`)
- Test: `crates/core/tests/runner.rs`

**Interfaces:**
- Consumes: Task 2 (`ScriptVars.iteration`, `IterationInfo`), `ValueMap`.
- Produces:
  ```rust
  pub struct Iterations { pub rows: Vec<ValueMap>, pub count: usize }   // Default: rows empty, count 1
  impl Iterations { pub fn from_rows(rows: Vec<ValueMap>) -> Iterations; pub fn repeat(count: usize) -> Iterations }
  pub struct RunOptions { /* existing */ pub iterations: Iterations }
  pub enum RunEvent {
      IterationStarted { index: usize, total: usize, data: ValueMap },
      Started { iteration: usize, /* existing */ },
      Passed { iteration: usize, /* existing */ },
      Failed { iteration: usize, /* existing */ },
      Skipped { iteration: usize, /* existing */ },
      Finished { report: RunReport },
  }
  pub struct RunReport { /* existing */ pub iterations: usize }   // iterations reached
  ```
  `IterationStarted` is sent before every iteration, including the single default one (consumers ignore it when `total == 1` and `data` is empty).

- [ ] **Step 1: Write the failing tests** in `crates/core/tests/runner.rs` (wiremock; two requests `a` then `b`):
  - `runs_once_per_row`: rows `[{id:"1"},{id:"2"}]`, `a` URL `/x/{{id}}` → mock sees `/x/1`, `/x/2`; events order `IterationStarted{0,2}`, a, b, `IterationStarted{1,2}`, a, b, `Finished` with `iterations == 2`; every per-request event's `iteration` matches.
  - `repeat_without_file`: `Iterations::repeat(3)` → each request sent 3 times; `IterationStarted.data` is empty.
  - `setvar_carries_over`: `a` post-response `bru.setVar("n", (bru.getVar("n")||0)+1)` → in iteration 2 `b` sees `n == 2`.
  - `iteration_data_set_is_per_iteration` (spec): `a` pre-request records `bru.runner.iterationData.has("k")` with `bru.setVar("seen" + bru.runner.iterationIndex, …)`, then calls `iterationData.set("k","v")`; `b`'s URL `/y/{{k}}` → mock sees `/y/v` in iteration 1; `seen1` is `false` (the set did not leak into iteration 2).
  - `delay_not_after_last_of_iteration`: `delay` 200 ms, 2 rows × 2 requests → total elapsed ≥ 400 ms and < 600 ms (two waits, not three).
  - `next_request_null_ends_iteration_only`: `a` calls `bru.runner.setNextRequest(null)` → `b` skipped in each iteration, both iterations run.
  - `stop_execution_stops_run`: `a` calls `stopExecution()` in iteration 1 → `b` skipped with `stopExecution`, no iteration 2 events, `report.iterations == 1`.
  - `jump_counter_per_iteration`: a 6000-jump loop per iteration over 2 iterations does not hit the 10 000 limit.
  - `env_file_untouched` (Review Focus 2): a run with a row column `baseUrl` leaves the environment `.bru` bytes unchanged.
- [ ] **Step 2: Run** `cargo test -p plover-core --test runner` → FAIL.
- [ ] **Step 3: Implement.** Wrap the existing `while index < refs.len()` loop in `for (i, row) in …`: reset `index` and `jumps` per iteration; keep `state` (`RunVars`) and the cookie jar; put a clone of the row into the per-request `ScriptVars.iteration` and write script changes back to the iteration's working row. `stop_execution` sets a flag that breaks both loops; `next_request == Some(None)` breaks the inner loop only. The existing delay check already skips the last request of an iteration. A dropped future stops everything, as now.
- [ ] **Step 4: Run** `cargo test --workspace` → PASS (fix event pattern matches in `crates/cli` and `crates/server` by adding `..`/`iteration` fields; behaviour unchanged with the default `Iterations`).
- [ ] **Step 5: Commit** `feat(core): run a selection once per data row or N times`.

### Task 4: Iterations in run records and masking

**Files:**
- Modify: `crates/core/src/run_record.rs` (`ResultRecord`, `RunRecord`, `RunRecorder`)
- Test: `crates/core/tests/run_record.rs`, `crates/core/tests/masking.rs`

**Interfaces:**
- Consumes: Task 1 `credential_values`, Task 3 events; run-history `RunRecorder::{new, event, masked_result, finish}`, `Masker`, `RunEnd`.
- Produces:
  ```rust
  pub struct IterationRecord { pub index: usize, pub data: ValueMap }   // serde camelCase
  pub struct ResultRecord { /* existing */ pub iteration_index: usize }      // #[serde(default)]
  pub struct RunRecord { /* existing */ #[serde(default)] pub iterations: Vec<IterationRecord> }
  impl RunRecorder {
      /// Row of an iteration, masked with the values known so far (for live events).
      pub fn masked_iteration(&self, index: usize) -> Option<IterationRecord>;
  }
  ```
  `RUN_RECORD_VERSION` stays 1 (both new fields default, so older files still load). Callers pass `credential_values(&rows)` inside `start_secrets` to `RunRecorder::new`; the masker needs no change.

- [ ] **Step 1: Write the failing tests.**
  - `run_record.rs` `results_carry_iteration`: events from Task 3's two-row run → `iteration_index` 0,0,1,1; `RunRecord.iterations` has two entries with the rows as they were at the end of each iteration (after a script `set`).
  - `old_record_without_iterations_loads`: a v1 JSON without `iterationIndex`/`iterations` deserializes with `0` / `[]`.
  - `body_budget_spans_iterations` (Review Focus 5): 200 iterations × 2 results with 200 KiB bodies → later bodies are `Omitted { reason: "run size limit", .. }`.
  - `masking.rs` `credential_column_masked_in_json_body` (user decision): row `{username:"alice-01", password:"pw-SECRET-1"}`, request body `{"u":"alice-01","p":"pw-SECRET-1"}` → after `finish` the body has `***` for the password and `alice-01` visible; `RunRecord.iterations[0].data.password == "***"`.
  - `secret_value_in_row_masked`: a row value equal to an env secret value → `***` in the record's iteration data and in URLs.
  - `masked_iteration_live`: `masked_iteration(0)` masks the password column before `finish`.
- [ ] **Step 2: Run** `cargo test -p plover-core` → FAIL.
- [ ] **Step 3: Implement.** `RunRecorder::event` handles `IterationStarted` (pushes an `IterationRecord`) and stamps `iteration_index` from each event. The final row of each iteration is taken from the run's state when the next `IterationStarted` or `Finished` arrives; add `data` to the masked fields in `Masker::mask_record` (string values at any depth).
- [ ] **Step 4: Run** `cargo test -p plover-core` → PASS.
- [ ] **Step 5: Commit** `feat(core): keep iterations in run records and mask data-file credentials`.

### Task 5: Iterations in the core report renderers

**Files:**
- Modify: `crates/core/src/report/{json.rs,html.rs,junit.rs}`
- Test: `crates/core/tests/report.rs`

**Interfaces:**
- Consumes: Task 4 records; run-history `json::render`, `html::render`, `junit::render`, `ReportFilter` (signatures unchanged).
- Produces: JSON output is an array of `{ iterationIndex, summary, results }`, one per iteration reached, each `summary` counting only its iteration; results also carry `iterationIndex`; no `iterationData` key. HTML has one `<section>` per iteration titled `Iteration N of M` with a `<table>` of the masked row and that iteration's counts, only when there is more than one iteration or a non-empty row. JUnit stays one flat list in run order with Bruno's suite names (no iteration in names).

- [ ] **Step 1: Write the failing tests** in `crates/core/tests/report.rs`:
  - `json_one_entry_per_iteration`: two-iteration record → array length 2, `[1].iterationIndex == 1`, `[1].summary.totalRequests == 2`; single-iteration record unchanged from the run-history golden.
  - `json_has_no_iteration_data`: no `iterationData` key anywhere.
  - `html_iteration_sections`: `Iteration 2 of 2` present, row table shows `***` for a password column, still no `<script`; a single-iteration report has no `Iteration` heading.
  - `junit_names_repeat`: suite names for iteration 1 and 2 are identical and match the single-iteration names.
  - `many_iterations_in_order` (Review Focus 5): 200 iterations → JSON indices 0..199 in order.
- [ ] **Step 2: Run** `cargo test -p plover-core --test report` → FAIL.
- [ ] **Step 3: Implement.** Group `record.results` by `iteration_index`; iterations with no results (stopped before any request) still get an entry only if they appear in `record.iterations`.
- [ ] **Step 4: Run** `cargo test --workspace` → PASS.
- [ ] **Step 5: Commit** `feat(core): report data-driven runs per iteration`.

### Task 6: `plv run` data flags

**Files:**
- Modify: `crates/cli/src/lib.rs` (`RunArgs`, setup), `crates/cli/src/print.rs` (`Iteration: N`, verbose row, summary line)
- Test: `crates/cli/tests/data_driven.rs` (new), fixture `crates/cli/tests/fixtures/data/` (synthetic CSV/JSON + a two-request collection)

**Interfaces:**
- Consumes: Task 1 `data_file::{parse, DataFormat, credential_values}`, Task 3 `Iterations`, Task 4 recorder.
- Produces (clap, in `RunArgs`):
  ```rust
  #[arg(long = "csv-file-path", value_name = "FILE", conflicts_with = "json_file_path")] csv_file_path: Option<PathBuf>,
  #[arg(long = "json-file-path", value_name = "FILE")] json_file_path: Option<PathBuf>,
  #[arg(long = "iteration-count", value_name = "N", value_parser = clap::value_parser!(u32).range(1..))] iteration_count: Option<u32>,
  #[arg(long)] parallel: bool,
  ```
  A clap conflict exits 2 (clap's usage error code is 2; check and keep it). `Printer` gains `pub fn iteration(&mut self, index: usize, total: usize, data: &ValueMap, verbose: bool)`; `summary` prints `Iterations: N` when N > 1.

- [ ] **Step 1: Write the failing tests** in `crates/cli/tests/data_driven.rs` (each sets `PLOVER_STATE_DIR` to a temp dir):
  - `csv_runs_each_row`: 3-row CSV → stdout has `Iteration: 1`…`Iteration: 3`, mock hit 3× per request, exit 0; JSON report has 3 entries.
  - `json_file_works`: same with JSON rows.
  - `iteration_count_without_file`: `--iteration-count 3` → 3 iterations; `--iteration-count 0` → exit 2.
  - `file_wins_over_count`: `--csv-file-path d.csv --iteration-count 9` with 2 rows → stderr `--iteration-count ignored: d.csv has 2 rows`, 2 iterations.
  - `loud_failures`: missing file, header-only CSV, `[]` JSON, `{}` JSON, short CSV row, 16 MiB + 1 file, and both flags together → each exit 2, no request reaches the mock, stderr names the file (and line for CSV).
  - `parallel_warns`: `--parallel` → stderr exact warning, iterations sequential (mock sees row 1's requests before row 2's).
  - `verbose_row_masked`: `--verbose` with a `password` column → stdout shows the row with `***` for the password.
  - `exit_one_if_any_iteration_fails`: row 2 makes an assert fail → exit 1.
  - `relative_path_from_cwd`: running from a temp cwd with `--csv-file-path rows.csv` reads `<cwd>/rows.csv`, not `<collection>/rows.csv`.
  - `history_keeps_iterations`: `plv runs show <id> --reporter-json` equals the live `--reporter-json` file.
- [ ] **Step 2: Run** `cargo test -p plover-cli --test data_driven` → FAIL.
- [ ] **Step 3: Implement.** Read the file (size check before reading the whole file via metadata), pick the format from the flag, parse, build `Iterations`, add `credential_values` to the recorder's start secrets, print iterations from `IterationStarted` events (skip the header when `total == 1` and no file).
- [ ] **Step 4: Run** the gate → PASS.
- [ ] **Step 5: Commit** `feat(cli): run with CSV or JSON data files and iteration counts`.

### Task 7: Server — data runs, preview, report export, native save

**Files:**
- Modify: `crates/server/src/api/runs.rs` (`ClientMessage::Start`, `RunEventDto`), `crates/server/src/api/history.rs` (report route), `crates/server/src/api/native.rs`, `crates/server/src/native.rs` (`NativeHost::save_file`), `crates/server/src/api/dto.rs` (`CapabilitiesDto.native_save`, preview DTOs), `crates/server/src/api/mod.rs` (routes), `crates/desktop/src/host.rs` (implement `save_file` with the Tauri dialog plugin), `frontend/src/api/types.ts` (regenerated)
- Create: `crates/server/src/api/data_file.rs`
- Test: `crates/server/tests/data_runs.rs` (new), `crates/server/tests/run_history.rs` (export cases)

**Interfaces:**
- Consumes: Tasks 1–5; run-history `LiveRuns`, `RunHistory::load`, report renderers.
- Produces:
  ```rust
  // ClientMessage::Start gains
  data: Option<DataFileDto>,               // #[serde(default)]
  iteration_count: Option<u32>,            // #[serde(default)]
  pub struct DataFileDto { pub format: DataFormat, pub text: String }
  // RunEventDto gains
  IterationStarted { index: u32, total: u32, data: serde_json::Value /* masked */ },
  // and `iteration: u32` on started/passed/failed/skipped
  pub struct DataPreviewDto { pub rows: u32, pub columns: Vec<String> }
  // NativeHost
  fn save_file<'a>(&'a self, default_name: &'a str, bytes: Vec<u8>) -> HostFuture<'a, Result<Option<PathBuf>, String>>;
  pub struct SavedFileDto { pub path: Option<String> }   // null when cancelled
  ```
  Routes: `POST /collections/{id}/data-file/preview` (body `DataFileDto`) → `DataPreviewDto`, 400 `bad_data` with the parser message; `GET /collections/{id}/runs/{runId}/report?format=html|json|junit` → file with `Content-Type` and `Content-Disposition: attachment; filename="<collection>-<runId>.<ext>"`, 409 `run_in_progress` while running, 400 `bad_format`, lookups and errors as the run-history detail route; `POST /native/save-report` (body `{ collectionId, runId, format }`) → `SavedFileDto`, 501 when there is no native host. A `Start` with bad data answers `RunEventDto::Error { code: "bad_data", .. }` and runs nothing. `text` over `MAX_DATA_FILE` → `bad_data`.

- [ ] **Step 1: Write the failing tests.**
  - `data_runs.rs` `start_with_csv_runs_rows`: socket `Start` with a 2-row CSV → events `iterationStarted{0,2}`, …, `iterationStarted{1,2}`, … with `iteration` fields; `finished.report.iterations == 2`.
  - `bad_data_is_an_error_event`: JSON `{}` → `error` code `bad_data`, no request to the mock.
  - `iteration_count_runs_n_times`.
  - `iteration_started_data_masked`: `password` column → `iterationStarted.data.password == "***"`.
  - `preview_returns_counts_not_values`: preview of a CSV with a password column → `{rows: 2, columns: [...]}` and the response text contains no row value.
  - `cancel_mid_iteration_saves_partial` (Review Focus 4): 5 rows with a delayed mock, cancel during iteration 3 → saved run `cancelled`, `iterations` length 3; its JSON export has 3 entries.
  - `run_history.rs` `export_matches_plv_show`: a finished app run's `…/report?format=json` equals `plv runs show <id> --reporter-json` output for the same record (load the saved file and render with `json::render`); HTML has `Content-Disposition` with `.html`; while running → 409.
  - `native_save_without_host_is_501`.
- [ ] **Step 2: Run** `cargo test -p plover-server` → FAIL.
- [ ] **Step 3: Implement.** `run_task` parses `data` with `data_file::parse` before creating the recorder, builds `Iterations` (data wins over `iteration_count`, default 1), adds `credential_values` to start secrets, and sends `IterationStarted` with `recorder.masked_iteration(index)`. The export handler renders with `ReportFilter::default()`. Desktop `save_file` uses the dialog plugin's save dialog and writes through `std::fs::write`; `CapabilitiesDto.native_save` is true only in the desktop app.
- [ ] **Step 4: Regenerate bindings and run** `PLOVER_UPDATE_BINDINGS=1 cargo test -p plover-server --test bindings`, then the gate → PASS.
- [ ] **Step 5: Commit** `feat(server): data-driven runs, data file preview and report export`.

### Task 8: UI — Run with data…, iteration groups, Export

**Files:**
- Create: `frontend/src/components/RunWithDataDialog.tsx` (on `Modal`), `frontend/src/components/ExportMenu.tsx`
- Modify: `frontend/src/api/client.ts`, `frontend/src/store/runs.ts`, `frontend/src/components/RunPanel.tsx` (or the Run tab if the dev-UX work has landed), `frontend/src/styles.css`
- Test: `frontend/src/store/runs.test.ts`, `frontend/src/components/ux.test.tsx`, `frontend/src/components/a11y.test.tsx`

**Interfaces:**
- Consumes: Task 7 DTOs and routes; run-history `RunResultDetail`, `RunHistoryMenu`, `useRuns`.
- Produces:
  ```ts
  // ApiClient
  previewDataFile(collectionId: string, file: DataFileDto): Promise<DataPreviewDto>;
  reportUrl(collectionId: string, recordId: string, format: "html" | "json" | "junit"): string;
  downloadReport(collectionId: string, recordId: string, format: "html" | "json" | "junit"): Promise<Blob>;
  saveReport(collectionId: string, recordId: string, format: "html" | "json" | "junit"): Promise<string | null>;
  // RunsState
  start(collectionId: string, item: string, env: string | null, label: string, options?: { data?: DataFileDto; iterationCount?: number }): void;
  // RunState additions
  iterations: { index: number; total: number; data: Record<string, unknown> }[];
  ```
  "Run with data…" sits in the runner header next to Re-run (user decision); the sidebar ▶ is unchanged. Export ⤓ opens HTML / JSON / JUnit, enabled for finished, cancelled and history runs, disabled while running.

- [ ] **Step 1: Write the failing tests.**
  - `runs.test.ts`: `iterationStarted` events build `iterations`; rows keep their `iteration`; `start(..., { data })` sends `data` in the `Start` message.
  - `ux.test.tsx`: the dialog reads a picked `.csv` file, calls `previewDataFile`, shows "2 rows · user, password" and no row values; the iteration count input is disabled with "set by file (2 rows)"; a parse error shows the server message and disables Run; Run sends `Start` with the file text; closing and reopening the dialog shows no file (not remembered).
  - Grouping: with two iterations the list shows `Iteration 1 of 2` and `Iteration 2 of 2` header rows with pass/fail counts and `key=value` chips (masked values from events; values over 40 chars cut with a `title` tooltip); with one iteration and no data there is no header row.
  - Export: the ⤓ menu has HTML, JSON, JUnit; disabled while running; in a browser it calls `downloadReport` and triggers an `<a download>` named `<collection>-<runId>.<ext>`; with `capabilities.nativeSave` it calls `saveReport` instead.
  - `a11y.test.tsx`: dialog focus trap and Esc like other `Modal`s; Export menu items reachable by keyboard; iteration headers are `role="rowheader"` (or a group label) so screen readers announce them.
- [ ] **Step 2: Run** `cd frontend && npm test` → FAIL.
- [ ] **Step 3: Implement.** Read the file with `File.text()`; format from the extension (reject others in the picker with `accept=".csv,.json"`). Keep existing tokens and the dev-UX doc's layout; no new colours.
- [ ] **Step 4: Run** `cd frontend && npm run typecheck && npm run lint && npm test`, then `cargo test --workspace` → PASS.
- [ ] **Step 5: Commit** `feat(ui): run with a data file, group results by iteration, export reports`.

### Task 9: Docs, Bruno comparison and final check

**Files:**
- Create: `fixtures/data-driven/` (a public two-request collection against a local mock plus `rows.csv` and `rows.json`, synthetic values only)
- Modify: `README.md` (flags table: the four data flags; "Run with data…" and Export in the app), `docs/development.md` (how to run the Bruno comparison)
- Test: `crates/cli/tests/bruno_compat.rs` (ignored by default, runs when `BRU_BIN` is set)

- [ ] **Step 1: Write** `data_driven_json_matches_bruno` (`#[ignore]` unless `BRU_BIN` is set): run `bru run fixtures/data-driven --csv-file-path rows.csv --reporter-json b.json` and `plv run … --reporter-json p.json` against the same local mock; compare after removing timings, `responseTime`, `duration` and header `date`.
- [ ] **Step 2: Run** it locally with the scratch Bruno CLI (`BRU_BIN=<path to bru>`) → PASS, and record the result in the commit message.
- [ ] **Step 3: Document** the flags, the loud-failure deviations from Bruno, `--parallel` being sequential, data-file credential masking, and Export.
- [ ] **Step 4: Run** the full gate plus `cd frontend && npm run check:console` → PASS.
- [ ] **Step 5: Commit** `docs: describe data-driven runs and report export`.

## File overlap with other work

- **Run history** (must be merged first): `run_record.rs`, `masking.rs`, `report/*`, `api/runs.rs`, `api/history.rs`, `store/runs.ts`, `RunPanel.tsx`. This plan only extends them.
- **Curl/HAR** (`feat/import-curl-har`): `runner.rs` (its Task 4 changes `SendInput` and the start of `send_one_with`; this plan changes `run_with_state` and `RunEvent` — separate regions), `api/mod.rs` and `api/dto.rs` (both append), `api/client.ts` (both append), `types.ts` (regenerate after rebase, never merge by hand), and the credential list (Task 1 Step 0: one shared `credentials.rs`).
- **Dev-UX** (later): the Run tab replaces `RunPanel`; build `RunWithDataDialog` and `ExportMenu` as standalone components that take `collectionId`/`recordId` props so they move without changes.

## Self-review notes

- Spec coverage: data parsing and loud failures (T1, T6), iteration count rules (T3, T6), precedence and `bru.getVar`/`bru.interpolate` exclusion (T2), `bru.runner` API and single-send values (T2), loop flow: carry-over, delay, `setNextRequest(null)`, `stopExecution`, jump counter (T3), events and `RunReport.iterations` (T3), parallel warning (T6), CLI printing (T6), credential-column masking (T1, T4, T6, T7), records (T4), JSON/HTML/JUnit (T5), UI dialog, preview, grouping, not remembering the file (T7, T8), export endpoint, desktop save, identical bytes (T7, T8), Bruno comparison (T9).
- Claude's calls made here (beyond the spec): `IterationStarted` is sent even for a single default iteration; `RUN_RECORD_VERSION` stays 1 because the new fields default; data-file credential values enter the masker through `start_secrets` rather than a new `Masker` parameter; `CapabilitiesDto.nativeSave` flag name; the Bruno comparison test is `#[ignore]` unless `BRU_BIN` is set.
