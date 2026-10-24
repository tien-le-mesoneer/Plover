# Run Detail and Run History Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every request in a run keeps what was sent and received, the app and `plv` reports show it, and every run (app or `plv`) is saved to a masked local history that the app and `plv runs` can reopen.

**Architecture:** Core captures a `RequestDetail`/`ResponseDetail` on `RequestOutcome` at the send site. A new core `RunRecorder` turns `RunEvent`s into `ResultRecord`s (replacing the CLI's `Collector`), a `Masker` replaces collected secret values with `***`, and `RunHistory` stores one masked `RunRecord` per run under the state directory. Core renders reports from those records (`plover_core::report`, moved out of `crates/cli`); the CLI writes them and saves the run. The server keeps live recorders in memory, saves on finish or cancel, and serves list and detail endpoints. The UI fetches a row's detail on click.

**Tech Stack:** Rust (tokio, serde, axum, ts-rs, clap, wiremock, assert_cmd), React + TypeScript + Zustand + Vitest.

**Spec:** `docs/superpowers/specs/2026-10-23-run-history-design.md`

## Global Constraints

- History lives under `app_state::default_dir()` + `runs/`, which honours `PLOVER_STATE_DIR`. Every test that can save history sets `PLOVER_STATE_DIR` (or passes a temp dir); no test touches the real state dir or the real keychain.
- History is never written inside a collection folder.
- Per-body cap **1 MiB** (`1 << 20` bytes); per-run stored-body cap **50 MiB** (`50 << 20`); retention **20** runs per collection.
- Masked headers (case-insensitive): `Authorization`, `Proxy-Authorization`, `Cookie`, `Set-Cookie`. Mask text: `***`. Header-derived values under **8** characters are not collected; secret values under **4** are not collected. Longer values are replaced first.
- Run ID format: `<UTC yyyymmddThhmmssZ>-<6 chars of RFC 4648 lowercase base32 (a-z, 2-7)>`. Any ID that does not match `^\d{8}T\d{6}Z-[a-z2-7]{6}$` is rejected before it touches a path.
- A failed or slow history write never changes a run's exit code, events or reports; it logs a `tracing::warn!`.
- `plv` does not save when `CI` is set to anything except empty, `0` or `false` (case-insensitive); `--history` forces saving, `--no-history` disables it; they conflict with each other. After saving, `plv` prints `Saved run <id>` to **stderr**.
- Reports (JSON, HTML, JUnit) are built from the same masked records as history. HTML stays script-free with no external resources.
- Gate before every commit (`set -o pipefail` when piping): `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and for frontend tasks `cd frontend && npm run typecheck && npm run lint && npm test`. After DTO changes: `PLOVER_UPDATE_BINDINGS=1 cargo test -p plover-server --test bindings`, then commit `frontend/src/api/types.ts`.
- CI matrix stays macOS, Windows, Linux. Paths in records use `/` separators; file writes go through `store::write_atomic`.
- No content from real collections, real hosts or real tokens in fixtures. Test tokens are obviously fake (e.g. `tok-AAAABBBBCCCC`).
- Conventional Commit subjects; end each commit message with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Do not push.

## Review Focus

1. **Binary response bodies** (an image, gzip that is not UTF-8): stored as base64 with the true size, never searched by the masker, shown in the UI as "binary, N bytes", and the JSON report's `response.data` is the base64 string. Test in Task 1 and Task 3.
2. **The same request run twice in one run** (`bru.runner.setNextRequest` jumps back): two results with different indices; clicking either row in the UI shows its own detail, not the first match by item. Test in Task 2 (recorder indices) and Task 8 (UI selection by index).
3. **History directory not writable** (`PLOVER_STATE_DIR` points at a regular file): `plv run` prints a warning to stderr, still writes its reports, and exits with the same code as with `--no-history`. Test in Task 6.
4. **A corrupt or future-version run file** in the history folder: `plv runs` and `GET …/runs` skip it, `plv runs show <that id>` exits 2 with "cannot read run <id>: …", and pruning still deletes it once it is among the oldest. Test in Task 4.
5. **The collection moved or deleted before `plv runs show <ID>`**: the report is still produced from the self-contained run file, with the original collection name and root in it. Test in Task 6.

---

### Task 1: Capture request and response detail in core

**Files:**
- Create: `crates/core/src/run_record.rs` (detail types only in this task)
- Create: `crates/core/src/serde_ms.rs` (Duration as integer milliseconds)
- Modify: `crates/core/src/lib.rs`, `crates/core/src/runner.rs` (`RequestOutcome`, `RunReport`, `ScriptFailure`, `ScriptPhase`, `Exec::execute`), `crates/core/src/script/engine.rs` (`ScriptError` serde)
- Test: `crates/core/tests/runner.rs`

**Interfaces:**
- Produces (in `plover_core::run_record`):
  ```rust
  pub const MAX_DETAIL_BODY: usize = 1 << 20;
  pub struct RequestDetail { pub method: String, pub url: String, pub headers: Vec<(String, String)>, pub body: BodyDetail }
  pub struct ResponseDetail { pub status: u16, pub status_text: String, pub headers: Vec<(String, String)>, pub body: BodyDetail, pub url: String, pub size: usize, pub size_complete: bool }
  pub enum BodyDetail {
      None,
      Text { text: String, size: usize, truncated: bool },
      Base64 { data: String, size: usize, truncated: bool },
      Form { fields: Vec<(String, String)> },
      Multipart { parts: Vec<PartDetail> },
      Omitted { reason: String, size: usize },
  }
  pub enum PartDetail { Text { name: String, value: String, content_type: Option<String> }, File { name: String, file_name: String, content_type: Option<String> } }
  impl RequestDetail { pub fn of(resolved: &ResolvedRequest) -> RequestDetail }
  impl ResponseDetail { pub fn of(response: &ResponseSummary, url: &str) -> ResponseDetail }
  impl BodyDetail { pub fn of_bytes(bytes: &[u8], size: usize, truncated: bool) -> BodyDetail; pub fn stored_len(&self) -> usize }
  ```
  All derive `Debug, Clone, PartialEq, Serialize, Deserialize`, `#[serde(rename_all = "camelCase")]`; enums use `#[serde(tag = "kind", rename_all = "camelCase")]`.
- Produces: `RequestOutcome` gains `pub request: Option<RequestDetail>` and `pub response: Option<ResponseDetail>`. `RequestOutcome`, `RunReport`, `ScriptFailure`, `ScriptPhase`, `ScriptError` derive `Serialize, Deserialize`; their `Duration` fields serialize as integer milliseconds via `#[serde(with = "crate::serde_ms")]`.

- [ ] **Step 1: Write the failing tests** in `crates/core/tests/runner.rs` (wiremock, like the existing tests):
  - `run_keeps_request_and_response_detail`: a POST with JSON body `{"a":"{{x}}"}` and env var `x=1` to a mock returning 201 with header `x-r: y` and body `ok`. Assert `outcome.request` = method `POST`, URL as sent, headers include `content-type`, body `Text { text: "{\"a\":\"1\"}", truncated: false, .. }`; `outcome.response` = status 201, `status_text == "Created"`, header `("x-r","y")`, body `Text { text: "ok", size: 2, truncated: false }`.
  - `set_body_is_what_detail_shows`: post-response `res.setBody({b: 2})` → response detail body text `{"b":2}`.
  - `transport_error_keeps_request_detail`: URL `http://127.0.0.1:9/x` → `request.is_some()`, `response.is_none()`.
  - `pre_request_error_has_no_detail`: pre-request `throw new Error("x")` → both `None`.
  - `binary_response_is_base64`: mock returns bytes `[0xff, 0x00, 0x01]` → `Base64 { data: "/wAB", size: 3, truncated: false }`.
  - `large_body_is_capped`: mock returns `MAX_DETAIL_BODY + 10` bytes of `a` → `Text { size: MAX_DETAIL_BODY + 10, truncated: true }` and `text.len() == MAX_DETAIL_BODY`.
  - `multipart_file_part_keeps_only_name`: multipart with a file part from the fixture dir → `PartDetail::File { file_name: "<file name only>", .. }`, no file bytes in the detail.
- [ ] **Step 2: Run** `cargo test -p plover-core --test runner` → FAIL (fields missing).
- [ ] **Step 3: Implement.** In `Exec::execute`, set `executed.outcome.request = Some(RequestDetail::of(&resolved))` right after `finalize` succeeds (before `started`), and `executed.outcome.response = Some(ResponseDetail::of(&response, &resolved.url))` after the tests phase (so `res.setBody` is reflected). `status_text` is `reqwest::StatusCode::canonical_reason` (same as `script_response`). `of_bytes`: cut to `MAX_DETAIL_BODY` on a UTF-8 char boundary when the bytes are UTF-8, else base64 of the first `MAX_DETAIL_BODY` bytes; `truncated` is true when cut or when the executor already truncated. `size_complete` = `!response.truncated`. `ResolvedBody::Form` → `Form`, `Multipart` → `Multipart` with file parts reduced to `path.file_name()`.
- [ ] **Step 4: Run** `cargo test -p plover-core` → PASS (existing tests unchanged).
- [ ] **Step 5: Commit** `feat(core): keep request and response detail for each run result`.

### Task 2: Run records and the recorder in core

**Files:**
- Modify: `crates/core/src/run_record.rs`
- Create: `crates/core/src/time.rs` (move `iso_timestamp` civil-from-days math out of `crates/cli/src/report/junit.rs`)
- Modify: `crates/cli/src/report/junit.rs` (use `plover_core::time::iso_utc`)
- Test: `crates/core/tests/run_record.rs`

**Interfaces:**
- Consumes: Task 1 types, `runner::{RunEvent, RunReport, RequestOutcome, ScriptPhase}`.
- Produces:
  ```rust
  pub const RUN_RECORD_VERSION: u32 = 1;
  pub const MAX_RUN_BODIES: usize = 50 << 20;
  pub struct RunId(pub String);
  impl RunId { pub fn new(now: SystemTime) -> RunId; pub fn parse(s: &str) -> Option<RunId> }
  pub enum RunSource { App, Plv }                      // serde "app" | "plv"
  pub enum RunOutcome { Completed, Cancelled }         // serde "completed" | "cancelled"
  pub enum ResultStatus { Pass, Fail, Error, Skipped } // Bruno's per-result status; as_str()
  pub struct RunHeader { pub id: RunId, pub source: RunSource, pub collection_root: String, pub collection_name: String, pub target: String, pub env: Option<String>, pub started_at_ms: u64 }
  pub struct ResultRecord { pub index: usize, pub file: String, pub name: String, pub method: String, pub url: String, pub status: ResultStatus, pub outcome: RequestOutcome, pub error: Option<String>, pub skip_reason: Option<String> }
  impl ResultRecord { pub fn duration(&self) -> Duration; pub fn post_response_script_error(&self) -> Option<String> }
  pub const POST_RESPONSE_SCRIPT_ERROR: &str = "Post-Response Script Error";
  pub struct RunRecord { pub version: u32, #[serde(flatten)] pub header: RunHeader, pub outcome: RunOutcome, pub report: RunReport, pub results: Vec<ResultRecord> }
  pub struct RunRecorder { /* header, root, results, started, body budget, start secrets */ }
  impl RunRecorder {
      pub fn new(root: &Path, header: RunHeader, start_secrets: Vec<String>) -> RunRecorder;
      /// The index of the result this event completed, if any.
      pub fn event(&mut self, event: &RunEvent) -> Option<usize>;
      /// Unmasked, for live terminal output.
      pub fn results(&self) -> &[ResultRecord];
      pub fn header(&self) -> &RunHeader;
  }
  ```
  (`masked_result` and `finish` are added in Task 3.) In `plover_core::time`: `pub fn unix_ms(t: SystemTime) -> u64`, `pub fn iso_utc(t: SystemTime) -> String` (current JUnit format), `pub fn compact_utc(t: SystemTime) -> String` (`yyyymmddThhmmssZ`).

- [ ] **Step 1: Write the failing tests** in `crates/core/tests/run_record.rs`, driving `RunRecorder::event` with hand-built `RunEvent`s:
  - `records_pass_fail_error_skipped`: Started+Passed → `Pass`; Started+Failed with `status: Some(500)` → `Fail`; Failed with `status: None` and no Started → `Error` with the event's name, empty method/URL; Skipped → `Skipped` with `skip_reason`. Indices are `0, 1, 2, 3`; `file` is root-relative with `/`.
  - `pre_request_error_reports_thrown_message`: Failed whose `outcome.script_error` is `Thrown { message: "Request failed with status code 401", .. }` and `status: None` → `error == Some("Request failed with status code 401")` (moved `report_error` rule).
  - `same_item_twice_gets_two_indices` (Review Focus 2): two Started/Passed pairs for the same path → two records, indices 0 and 1, each with its own outcome.
  - `run_body_budget_drops_later_bodies`: feed results whose stored bodies add up past `MAX_RUN_BODIES` → later results have `BodyDetail::Omitted { reason: "run size limit", size }` on request and response bodies, headers kept.
  - `run_id_format`: `RunId::new(UNIX_EPOCH + 86_400s)` matches `^19700102T000000Z-[a-z2-7]{6}$`; `RunId::parse("../x")`, `parse("19700102T000000Z-ABCDEF")` → `None`.
  - `record_round_trips_json`: a `RunRecord` with every `BodyDetail` variant and a `ScriptError::Timeout` serializes and deserializes to an equal value.
- [ ] **Step 2: Run** `cargo test -p plover-core --test run_record` → FAIL.
- [ ] **Step 3: Implement** by moving the logic of `crates/cli/src/report/mod.rs` `Collector::event` and `report_error` into `RunRecorder::event` (the CLI keeps compiling against its old `Collector` until Task 5). Random suffix: `getrandom` → base32 lowercase. The body budget counts `BodyDetail::stored_len` of request and response bodies in arrival order.
- [ ] **Step 4: Run** `cargo test -p plover-core && cargo test -p plover-cli` → PASS (JUnit timestamp output unchanged).
- [ ] **Step 5: Commit** `feat(core): add run records and a recorder for run events`.

### Task 3: Value-based masking

**Files:**
- Create: `crates/core/src/masking.rs`
- Modify: `crates/core/src/run_record.rs` (`RunRecorder::masked_result`, `RunRecorder::finish`)
- Test: `crates/core/tests/masking.rs`

**Interfaces:**
- Consumes: Task 2 types.
- Produces:
  ```rust
  pub const MASK: &str = "***";
  pub const MASKED_HEADERS: [&str; 4] = ["authorization", "proxy-authorization", "cookie", "set-cookie"];
  pub struct Masker { /* values, longest first */ }
  impl Masker {
      pub fn new(secrets: &[String], results: &[ResultRecord]) -> Masker;
      pub fn mask_result(&self, result: &mut ResultRecord);
      pub fn mask_record(&self, record: &mut RunRecord);
      pub fn mask_text(&self, text: &str) -> String;
  }
  /// Strings as-is, other JSON scalars via `to_string()`, objects/arrays skipped.
  pub fn secret_strings(values: &ValueMap) -> Vec<String>;
  // in run_record.rs:
  pub enum RunEnd { Finished { report: RunReport, end_secrets: Vec<String> }, Cancelled { end_secrets: Vec<String> } }
  impl RunRecorder {
      /// Masked with the start secrets and the header values seen so far.
      pub fn masked_result(&self, index: usize) -> Option<ResultRecord>;
      /// Masked with start + end secrets and every header value of the run.
      pub fn finish(self, end: RunEnd) -> RunRecord;
  }
  ```
  `Cancelled` computes `report` from the results (counts of each status, tests and asserts, `elapsed` since the recorder was created).

- [ ] **Step 1: Write the failing tests** in `crates/core/tests/masking.rs`:
  - `masks_each_header_whole`: each of the four header names (mixed case) in request and response → value `***`.
  - `bearer_token_from_login_body_is_masked` (user decision): result 0 response body `{"access_token":"tok-AAAABBBBCCCC"}`, result 1 request header `Authorization: Bearer tok-AAAABBBBCCCC` → after `finish`, result 0 body is `{"access_token":"***"}`.
  - `cookie_values_masked_but_not_attributes`: `Set-Cookie: sid=cookieVALUE123; Path=/; HttpOnly` collects `cookieVALUE123` (not `Path`), and a later URL `?s=cookieVALUE123` becomes `?s=***`.
  - `short_header_values_left_alone`: `Authorization: Bearer abc1234` (7 chars) → the bare `abc1234` elsewhere stays visible.
  - `secret_masked_everywhere`: secret `s3cr3t-value` in URL, header `X-Key`, request body, response body, console line, test error message, assert error message and script error message → all `***`.
  - `secret_floor_is_four_chars`: secret `abc` not collected, secret `abcd` collected.
  - `longest_first`: secrets `abcdefgh` and `abcdefghijkl` → text `abcdefghijkl` becomes `***`, not `***ijkl`.
  - `secret_changed_by_script_is_masked`: start secret `old-secret-1`, end secret `new-secret-2` passed in `RunEnd::Finished` → both masked.
  - `base64_bodies_not_searched` (Review Focus 1): a `Base64` body is unchanged even if its decoded text contains a secret.
  - `live_detail_uses_values_so_far`: `masked_result(0)` before a later result sends the header shows the token; after `finish` it is `***`.
- [ ] **Step 2: Run** `cargo test -p plover-core --test masking` → FAIL.
- [ ] **Step 3: Implement.** Collection: for the four headers, the whole value, plus for `Authorization`/`Proxy-Authorization` the part after the first space when present, and for `Cookie` every `name=value` value and for `Set-Cookie` the first pair's value. Replacement walks URLs, all header values, `Text` bodies, `Form` values, multipart text values, console text, test/assert `error` strings, script error messages and `ResultRecord::error`. Headers named in `MASKED_HEADERS` are replaced whole.
- [ ] **Step 4: Run** `cargo test -p plover-core` → PASS.
- [ ] **Step 5: Commit** `feat(core): mask secrets and credential values in run records`.

### Task 4: Run history storage

**Files:**
- Create: `crates/core/src/run_history.rs`
- Modify: `crates/core/src/lib.rs`, `crates/core/src/store.rs` (`write_atomic` stays `pub(crate)`; reuse)
- Test: `crates/core/tests/run_history.rs`

**Interfaces:**
- Consumes: Task 2/3 types, `workspace::CollectionId::of_canonical`, `store::write_atomic`.
- Produces:
  ```rust
  pub const KEEP_RUNS: usize = 20;
  pub struct RunMeta { pub version: u32, #[serde(flatten)] pub header: RunHeader, pub outcome: RunOutcome, pub report: RunReport }
  impl RunMeta { pub fn of(record: &RunRecord) -> RunMeta }
  pub enum HistoryError { NotFound(String), Unreadable { id: String, reason: String }, Io(std::io::Error) }
  pub struct RunHistory { /* Dir(PathBuf) | Memory(Mutex<..>) */ }
  impl RunHistory {
      /// `<state_dir>/runs`.
      pub fn at(state_dir: &Path) -> RunHistory;
      pub fn in_memory() -> RunHistory;
      /// Writes `<key>/<id>.json`, then `<key>/<id>.meta.json`, then prunes to KEEP_RUNS.
      pub fn save(&self, record: &RunRecord) -> Result<(), HistoryError>;
      /// Newest first; unreadable or unknown-version entries skipped.
      pub fn list(&self, collection: &CollectionId) -> Vec<RunMeta>;
      pub fn load(&self, collection: &CollectionId, id: &RunId) -> Result<RunRecord, HistoryError>;
      /// Searches every collection folder (for `plv runs show`).
      pub fn find(&self, id: &RunId) -> Result<RunRecord, HistoryError>;
      /// Number of runs deleted.
      pub fn clear(&self, collection: &CollectionId) -> Result<usize, HistoryError>;
  }
  ```
  The folder key is `CollectionId::of_canonical(&record.header.collection_root)`, so it equals the server's collection ID.

- [ ] **Step 1: Write the failing tests** in `crates/core/tests/run_history.rs` (temp dirs only):
  - `save_list_load`: save 2 records → `list` newest first by ID; `load` returns equal records; both `.json` and `.meta.json` exist under `runs/<key>/`.
  - `keeps_twenty`: save 23 → `list().len() == 20`, the 3 oldest IDs' files are gone.
  - `corrupt_and_future_files_skipped` (Review Focus 4): write `garbage.meta.json`-style files with a valid ID name: one with invalid JSON, one with `"version": 99` → `list` skips both; `load` → `Unreadable`; after 20 newer saves both are pruned.
  - `concurrent_saves_and_prunes`: two `RunHistory::at(same dir)` instances, 8 threads each saving 5 records → no error, exactly 20 listed, every listed run loads.
  - `meta_written_last`: a `.json` without its `.meta.json` is not listed.
  - `find_across_collections`: two collection roots → `find(id)` returns the right record; unknown → `NotFound`.
  - `clear_deletes_only_that_collection`.
  - `in_memory_behaves_the_same`: `save_list_load` and `keeps_twenty` against `in_memory()`.
- [ ] **Step 2: Run** `cargo test -p plover-core --test run_history` → FAIL.
- [ ] **Step 3: Implement.** IDs come only from file names that pass `RunId::parse`; other files are ignored. Missing files during prune or clear are ignored (`NotFound` I/O errors).
- [ ] **Step 4: Run** `cargo test -p plover-core` → PASS.
- [ ] **Step 5: Commit** `feat(core): store run history per collection`.

### Task 5: Reports from core records, rendered in core (JSON, HTML, JUnit, skip flags)

User decision: the renderers live in core from this task on, so the app's Export (data-driven/export plan) and `plv` share them without a later move.

**Files:**
- Create: `crates/core/src/report/{mod.rs,json.rs,html.rs,junit.rs}` (moved from `crates/cli/src/report/` with `git mv`, then changed to take `RunRecord`)
- Modify: `crates/core/src/lib.rs` (`pub mod report`), `crates/cli/src/lib.rs` (flags → `ReportFilter`, writes the files), `crates/cli/src/print.rs`; delete `crates/cli/src/report/`
- Test: `crates/core/tests/report.rs` (renderer unit tests: shape, masking, skip flags, no `<script`), `crates/cli/tests/report.rs` (end-to-end through `plv run`)

**Interfaces:**
- Consumes: `RunRecord`, `ResultRecord`, `RunRecorder`, `Masker` from core.
- Produces:
  ```rust
  // crates/core/src/report/mod.rs (the CLI's Collector and RequestRecord are deleted)
  pub struct ReportFilter { pub skip_all_headers: bool, pub skip_headers: Vec<String>, pub skip_request_body: bool, pub skip_response_body: bool }
  pub fn json::render(record: &RunRecord, filter: &ReportFilter) -> serde_json::Value;
  pub fn html::render(record: &RunRecord, filter: &ReportFilter) -> String;
  pub fn junit::render(record: &RunRecord) -> String;
  // print.rs: unchanged. Printer keeps printing per event (Printer::event), because the
  // terminal shows the raw error text while records keep Bruno's report text.
  pub fn summary(report: &RunReport);
  ```
- New `plv run` flags, Bruno's names and meaning (`sanitize-results.js`): `--reporter-skip-all-headers`, `--reporter-skip-headers <NAME>...`, `--reporter-skip-request-body`, `--reporter-skip-response-body`, `--reporter-skip-body` (= both).

- [ ] **Step 1: Write the failing tests** in `crates/cli/tests/report.rs` (a new `detail_collection` fixture, so the existing fixture's counts stay, with a request sending `Authorization: Bearer tok-AAAABBBBCCCC` and a JSON response `{"id":1}`):
  - `json_report_has_bruno_request_and_response`: result entry has `request.{method,url,headers,data}` and `response.{status,statusText,headers,data,url,responseTime,duration,size}`; `headers` is an object (repeated names become arrays); `response.data` is `{"id":1}` as JSON (Bruno `parseDataFromResponse`: parsed whenever `JSON.parse` of the BOM-stripped text succeeds, else the string); the errored request has `response.status == "error"` and `size == 0`.
  - `reports_are_masked` (user decision): JSON, HTML and JUnit files contain `***` and not `tok-AAAABBBBCCCC`.
  - `skip_flags_match_bruno`: `--reporter-skip-all-headers` → `{}` headers; `--reporter-skip-headers x-r` removes only `x-r` (case-insensitive); `--reporter-skip-body` removes `request.data` and `response.data` keys.
  - `html_has_details_and_no_script`: the HTML has one `<details>` per sent result with header tables and `<pre>` bodies, and no `<script`.
  - Existing report tests keep passing unchanged except the removed "minus headers" note.
- [ ] **Step 2: Run** `cargo test -p plover-core --test report` and `cargo test -p plover-cli --test report` → FAIL.
- [ ] **Step 3: Implement.** `plv run` builds a `RunRecorder` (header source `Plv`, target = root-relative path or `.`), switches from `runner::run` to `RunVars::start` + `run_with_state` so end secrets are known, prints each event via `Printer::event` (unmasked, as today), then `finish(RunEnd::Finished { report, end_secrets })` and renders all reports from the masked record with `plover_core::report::{json,html,junit}::render`; the CLI only maps flags to `ReportFilter` and writes the files. Start/end secrets: `masking::secret_strings(&runner::secret_values(environment, &state.environment))`. Delete `Collector`, `RequestRecord`, `RecordStatus` and the moved helpers; update the `json.rs` module comment.
- [ ] **Step 4: Run** the gate → PASS.
- [ ] **Step 5: Commit** `feat(core): render masked reports in core with request and response detail`.

### Task 6: `plv` saves history; `plv runs`

**Files:**
- Modify: `crates/cli/src/lib.rs` (flags, `Runs` subcommand), `crates/cli/src/print.rs` (list table)
- Test: `crates/cli/tests/history.rs` (new)

**Interfaces:**
- Consumes: `RunHistory`, `RunRecord`, `RunMeta`, `RunId`, Task 5 renderers.
- Produces:
  ```rust
  /// `true` unless CI is set to a truthy value; flags win.
  fn history_enabled(force: bool, disable: bool, ci: Option<&OsStr>) -> bool;
  enum Command { Run(RunArgs), Serve(ServeArgs), Runs(RunsArgs) }
  // plv runs [DIR] | plv runs show <ID> [--reporter-json F] [--reporter-html F] [--reporter-junit F] [skip flags] | plv runs clear [DIR] [--yes]
  ```
  Use clap `args_conflicts_with_subcommands = true` so `plv runs DIR` and `plv runs show ID` both parse. `show` exit codes: 0 found, 2 not found or unreadable. `clear` without `--yes` asks `Delete N runs of <name>? [y/N]` on stderr and reads stdin; non-interactive stdin without `--yes` exits 2 with a hint.

- [ ] **Step 1: Write the failing tests** in `crates/cli/tests/history.rs` (each test sets `PLOVER_STATE_DIR` to its own temp dir and clears `CI` unless testing it):
  - `run_saves_history`: `plv run` → stderr has `Saved run <id>`, stdout has no `Saved run`; `plv runs <dir>` lists that ID with source `plv`.
  - `ci_disables_history`: `CI=true` → nothing saved; `CI=true --history` → saved; `--no-history` → not saved; `CI=0` and `CI=false` → saved; `--history --no-history` → exit 2.
  - `show_matches_live_report`: `plv run --reporter-json a.json` then `plv runs show <id> --reporter-json b.json` → the two JSON files are equal.
  - `show_after_collection_deleted` (Review Focus 5): delete the collection dir, `plv runs show <id> --reporter-html r.html` → exit 0, report has the collection name.
  - `unwritable_history_keeps_exit_code` (Review Focus 3): `PLOVER_STATE_DIR` = a regular file → stderr warning, reports written, exit code equals the `--no-history` run's.
  - `failed_run_is_saved_with_exit_1`: a failing run is saved and still exits 1.
  - `clear_requires_yes`: `plv runs clear <dir>` with empty stdin → exit 2; `--yes` → `plv runs <dir>` lists nothing.
  - `concurrent_plv_runs` (spec test list): two `plv run` processes on the same collection at once → both IDs listed, both load.
  - `unknown_or_malformed_id`: `plv runs show ../x` and an unknown valid ID → exit 2 with `run <id> not found` / `invalid run ID`.
- [ ] **Step 2: Run** `cargo test -p plover-cli --test history` → FAIL.
- [ ] **Step 3: Implement.** No state dir (`default_dir()` is `None`) → no history, one warning. A save error is a warning; it never changes the exit code. `plv runs` prints ID, start time (UTC, since core has no time-zone data), source, env, `passed/failed/skipped`, duration, newest first; empty → `No runs saved for <name>`.
- [ ] **Step 4: Run** the gate → PASS.
- [ ] **Step 5: Commit** `feat(cli): save runs to history and add plv runs`.

### Task 7: Server — live records, saving, history endpoints

**Files:**
- Create: `crates/server/src/api/history.rs`
- Modify: `crates/server/src/state.rs` (`history: Arc<RunHistory>`, `live: Arc<LiveRuns>`), `crates/server/src/lib.rs` (`RunHistory::at(dir)` when `state_dir` is set, else `in_memory()`), `crates/server/src/api/runs.rs`, `crates/server/src/api/mod.rs` (routes), `crates/server/src/api/dto.rs`, `frontend/src/api/types.ts` (regenerated)
- Test: `crates/server/tests/run_history.rs` (new), update `crates/server/tests/runs.rs` for the new event fields

**Interfaces:**
- Consumes: Tasks 2–4.
- Produces:
  ```rust
  // state.rs
  pub enum LiveRun { Running(RunRecorder), Done(Arc<RunRecord>) }
  pub type LiveRuns = Mutex<HashMap<String /* record id */, Arc<Mutex<LiveRun>>>>;
  // RunEventDto changes
  Accepted { run_id: String, record_id: String },          // first event of every run
  Passed { index: u32, .. }, Failed { index: u32, .. }, Skipped { index: u32, .. }
  // dto.rs
  pub struct RunMetaDto { pub record_id, pub source, pub target, pub env: Option<String>, pub started_at_ms: u64 (ts number), pub outcome: String, pub report: RunReportDto }
  pub struct RunDto { pub meta: RunMetaDto, pub label: String, pub events: Vec<RunEventDto> }   // replayed: started + outcome per result + finished/cancelled
  pub struct RequestDetailDto { pub method, pub url, pub headers: Vec<(String, String)>, pub body: BodyDetailDto }
  pub struct BodyDetailDto { pub kind: String, pub text: Option<String>, pub base64: Option<String>, pub fields: Vec<(String, String)>, pub parts: Vec<PartDetailDto>, pub size: u64 (ts number), pub truncated: bool, pub omitted: Option<String> }
  pub struct RunResultDetailDto { pub index: u32, pub item: String, pub name: String, pub status: String, pub error: Option<String>, pub skip_reason: Option<String>, pub request: Option<RequestDetailDto>, pub result: SendResultDto }
  ```
  Routes: `GET /collections/{id}/runs` → `RunMetaDto[]`; `GET /collections/{id}/runs/{runId}` → `RunDto`; `GET /collections/{id}/runs/{runId}/results/{index}` → `RunResultDetailDto`; `DELETE /collections/{id}/runs` → 204. Unknown collection 404 `not_found`; invalid run ID 400 `bad_run_id`; unknown run or index 404 `not_found`; unreadable file 500 `run_unreadable`. `SendResultDto.response` for a detail is built from `ResponseDetail` (`ResponseDto.body_text`/`body_base64` from the stored body, `truncated` from the detail).

- [ ] **Step 1: Write the failing tests** in `crates/server/tests/run_history.rs` (server started with `state_dir: Some(tempdir)`, wiremock target):
  - `accepted_then_indexed_events`: first event is `accepted` with a valid record ID; `passed`/`failed`/`skipped` carry `index` 0..n.
  - `detail_while_running_and_after`: with a delayed mock, fetch result 0 detail while result 1 is in flight (from memory); after `finished`, the same URL returns the same detail from the file (`runs/<id>/<recordId>.json` exists).
  - `finished_is_sent_after_masking`: a run whose second request sends `Authorization: Bearer tok-AAAABBBBCCCC` that the first response returned → detail of result 0 fetched right after the `finished` event shows `***`.
  - `cancelled_run_is_saved`: cancel message and socket disconnect each → `GET …/runs` lists the run with outcome `cancelled` and the results it had.
  - `list_get_clear`: two runs → list newest first; `GET …/runs/{id}` replays events that `RunPanel`'s row builder accepts (started + passed/failed/skipped with indices + finished); `DELETE` → list empty.
  - `bad_ids_rejected`: `…/runs/..%2Fx` → 400 `bad_run_id`; index out of range → 404.
  - `repeated_item_has_distinct_details` (Review Focus 2): a collection that jumps back once with `setNextRequest` → results 0 and 2 are the same item with different response bodies.
- [ ] **Step 2: Run** `cargo test -p plover-server --test run_history` → FAIL.
- [ ] **Step 3: Implement.** `run_task` creates the `RunRecorder` (source `App`, start secrets from `secret_values(environment, &state.environment)`), registers `LiveRun::Running`, sends `Accepted`, and for each event gets the index from `recorder.event`. It holds back the core `Finished` event: after `run_with_state` returns it calls `finish(RunEnd::Finished { .. })`, swaps the entry to `Done`, sends `Finished`, saves with `spawn_blocking`, then removes the entry. `ActiveRun` keeps the record ID; cancel and disconnect take the `Running` recorder, `finish(RunEnd::Cancelled { end_secrets: vec![] })` (end secrets are unknown after an abort — Claude's call), save, remove. Detail lookup: live entry first, else `history.load`.
- [ ] **Step 4: Regenerate bindings and run** `PLOVER_UPDATE_BINDINGS=1 cargo test -p plover-server --test bindings`, then the gate → PASS.
- [ ] **Step 5: Commit** `feat(server): save app runs and serve run history and result detail`.

### Task 8: UI — result detail pane and History

> Layout note (user decision, after this plan was written): the runner becomes a **tab** with a Bruno-style `All | Fail | Pass | Skip` filter and a "Previous runs ▾" menu — see `docs/superpowers/specs/2026-10-23-dev-ux-design.md` §6. This task still builds `RunResultDetail` and `RunHistoryMenu` as standalone components and mounts them in today's `RunPanel`; the dev-UX work moves them into the Run tab after the run-history and curl/HAR branches merge. Build them so they take a `runId` prop and do not depend on `RunPanel` internals.

**Files:**
- Create: `frontend/src/components/RunResultDetail.tsx`, `frontend/src/components/RunHistoryMenu.tsx`
- Modify: `frontend/src/api/client.ts`, `frontend/src/store/runs.ts`, `frontend/src/components/RunPanel.tsx`, `frontend/src/components/ResponseViewer.tsx` (optional `request?: RequestDetailDto` prop adds a Request tab), `frontend/src/styles.css`
- Test: `frontend/src/store/runs.test.ts`, `frontend/src/components/ux.test.tsx`, `frontend/src/components/a11y.test.tsx`

**Interfaces:**
- Consumes: Task 7 DTOs and routes.
- Produces:
  ```ts
  // ApiClient
  listRuns(id: string): Promise<RunMetaDto[]>;
  getRun(id: string, recordId: string): Promise<RunDto>;
  runResult(id: string, recordId: string, index: number): Promise<RunResultDetailDto>;
  clearRuns(id: string): Promise<void>;
  // RunState additions
  recordId?: string; source: "live" | "history"; selected?: number;
  detail?: { index: number; loading: boolean; data?: RunResultDetailDto; error?: string };
  // RunsState additions
  history: Record<string /* collectionId */, RunMetaDto[]>;
  select(runId: string, index: number): void;
  loadHistory(collectionId: string): Promise<void>;
  openHistory(collectionId: string, recordId: string): Promise<void>;
  clearHistory(collectionId: string): Promise<void>;
  ```
  Tab labels, in order: Response, Headers, Request, Tests, Console.

- [ ] **Step 1: Write the failing tests.**
  - `runs.test.ts`: `accepted` sets `recordId`; `select(r1, 1)` calls `runResult("c1", rec, 1)` and stores the data; a `finished` event while a detail is open refetches it once; `openHistory` adds a run with `source: "history"`, status from the meta outcome and events from `getRun`; `clearHistory` empties `history.c1`.
  - `ux.test.tsx`: clicking a finished row (or Enter on it) opens the detail with the five tabs; the Request tab shows method, URL, headers and body; a skipped row shows its reason and no tabs; a running row is not selectable; with two rows for the same item, selecting the second calls `runResult` with index 1 (Review Focus 2); a `base64` body shows "binary, 3 bytes" (Review Focus 1); a truncated body shows "Showing the first 1 MiB of N" and an omitted one "Body not kept (run size limit)".
  - History: the History button lists runs with time, source (`app`/`plv`), env and counts; choosing one renders it read-only (no Cancel button) with the same rows; "Clear history" asks for confirmation (existing `Modal`) and then calls `clearRuns`.
  - `a11y.test.tsx`: rows are keyboard-focusable with `aria-selected`; the History menu traps focus like `OpenCollectionPopover` and Esc returns focus to the button.
- [ ] **Step 2: Run** `cd frontend && npm test` → FAIL.
- [ ] **Step 3: Implement.** Rows map by event `index`, never by item. The detail pane sits beside or below the table per `docs/ui-ux-review.md` rules (existing tokens, no new colors). `RunPanel` shows the selected run (newest live run by default; an opened history run replaces it until closed).
- [ ] **Step 4: Run** `cd frontend && npm run typecheck && npm run lint && npm test`, then `cargo test --workspace` (embedded UI build) → PASS.
- [ ] **Step 5: Commit** `feat(ui): show run result detail and earlier runs`.

### Task 9: Docs and final check

**Files:**
- Modify: `README.md` (flags table: `--history`, `--no-history`, the five `--reporter-skip-*` flags; `plv runs`), `docs/development.md` (history location and `PLOVER_STATE_DIR` for tests)

- [ ] **Step 1:** Document where history lives per OS, the 20-run retention, the masking rule and its limits (copy the spec's "Limits" list), and that CI does not save by default.
- [ ] **Step 2: Run** the full gate plus `cd frontend && npm run check:console` → PASS.
- [ ] **Step 3: Commit** `docs: describe run history and report detail`.

## Self-review notes

- Spec coverage: detail capture (T1), partial results (T1/T2), status text (T1), base64/form/multipart (T1), 1 MiB / 50 MiB caps (T1/T2), masking passes, limits, live masking (T3, T7), layout/IDs/atomic/meta-last (T4), save timing, cancel, failure handling (T6, T7), retention (T4), `plv` CI switches and stderr line (T6), `plv runs` list/show/clear (T6), UI detail, fetch-on-click, History, endpoints (T7, T8), reports shape, HTML details, JUnit masked, skip flags, shared record (T5), test list (T1–T7).
- Claude's calls made here (beyond the spec): server sends `Finished` only after the record is masked; cancelled app runs are masked with start secrets and header values only; an opened history run is served as replayed `RunEventDto`s so `RunPanel` uses one row builder; without a state dir the server keeps history in memory (same retention); all five Bruno `--reporter-skip-*` flags are accepted.
