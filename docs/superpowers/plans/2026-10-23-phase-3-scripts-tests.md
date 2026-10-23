# Phase 3 — scripts & tests Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Run Bruno `script:pre-request`, `script:post-response`, `tests`, `assert` and `vars:post-response` in a sandboxed `rquickjs` runtime, with results in the CLI (text, JUnit XML, JSON) and the web UI.

**Architecture:** A new `plover_core::script` module owns a sandboxed QuickJS engine. Its library set mirrors **Bruno's own safe mode** (`packages/bruno-js/src/sandbox/bundle-libraries.js`): chai (`expect`, `assert`), `Buffer`, `moment`, `btoa`, `atob`, `crypto-js`, `tv4`, `ajv`, `ajv-formats`, prebuilt into one committed JS bundle. `require()` resolves only those names; anything else throws a clear error and the request fails (user decision, PRD §8 Q2). The runner calls the engine at Bruno's hook points; results flow out through `RunEvent`/`send_one` to the CLI reporters and the server DTOs.

**Tech Stack:** Phase 1–2 stack + `rquickjs` (features: `futures`, `macro`, `loader` only if needed), `quick-xml` (JUnit writer) or hand-written XML escaping; bundling: `esbuild` (dev-only, in `crates/core/js/`).

**Spec:** `docs/PRD.md` (Phase 3, D4). Bruno references (clone: `bruno-ref/packages/`): `bruno-js/src/bru.js`, `bruno-request.js`, `bruno-response.js`, `runtime/{script,test,assert,vars}-runtime.js`, `sandbox/quickjs/`, `sandbox/bundle-libraries.js`; `bruno-cli/src/runner/prepare-request.js:23-27` (`scripts.flow`, default `sandwich`), `bruno-cli/src/utils/collection.js:310-435` (`mergeScripts`), `bruno-cli/src/reporters/` (JUnit/JSON).

## Global Constraints

- Sandbox: no filesystem, network, process, or timers beyond what this plan exposes. `require(x)` for `x` outside the safe-mode set → `Error: Cannot find module 'x' — Plover runs scripts like Bruno safe mode; available: chai, buffer, moment, btoa, atob, crypto-js, tv4, ajv, ajv-formats`. That request is reported **failed** with this message.
- Limits (defaults, all configurable via `ScriptLimits`, CLI flags `--script-timeout-ms`, `--script-memory-mb`): timeout 30 000 ms per script phase (wall clock, includes awaited promises), memory 64 MiB per runtime, max stack 1 MiB. Exceeding a limit fails that request with `script timed out after N ms` / `script exceeded memory limit`; the process and the rest of the run continue.
- Bruno parity is the default answer for any behavior question; cite the reference file:line in a test comment.
- Script/test/assert blocks are written losslessly (Phase 1 guarantees); editing a script in the UI changes only that block.
- Performance: the library bundle is compiled to QuickJS bytecode **once per process** and loaded per context; measure per-request overhead (Task 9) and report it.
- Gate per task: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`; UI tasks add `npm run typecheck && npm run lint && npm test && npm run check:no-cdn`.
- Localhost only in tests and checks. No push.

## Review Focus

1. **Infinite loop** (`while(true){}`) in any script → that request fails with a timeout message within limit + 1 s; next requests run. → Task 2 `infinite_loop_times_out`.
2. **Memory bomb** (`const a=[];for(;;)a.push(new Array(1e6).fill(1))`) → fails with memory message; process RSS stays bounded. → Task 2 `memory_bomb_is_contained`.
3. **Forbidden require** (`require('fs')`, `require('axios')`) → failed request whose error names the module and the allowed list. → Task 2 `require_outside_safe_set_fails`.
4. **Promise that never settles** in a test or script (`await new Promise(()=>{})`) → timeout failure, not a hang. → Task 4 `never_settling_test_times_out`.
5. **Script error location**: a throw in folder-level post-response script reports scope (`folder users`), phase and line number; runtime vars set earlier in the same run remain. → Task 6 `error_reports_scope_and_line`.

---

### Task 1: Safe-mode library bundle

**Files:**
- Create: `crates/core/js/package.json` (exact versions from Bruno `bruno-js/package.json`: `chai@^4.3.7` → pin the resolved 4.x, `buffer`, `moment`, `btoa`, `atob`, `crypto-js@^4.2.0`, `tv4`, `ajv@^8`, `ajv-formats`; dev: `esbuild`), `crates/core/js/package-lock.json`, `crates/core/js/entry.js` (same globals + `requireObject` map as Bruno `bundle-libraries.js:9-37`), `crates/core/js/build.mjs` (esbuild → IIFE, minified, `platform: 'neutral'`, target `es2020`), `crates/core/js/dist/sandbox-libs.js` (committed), `crates/core/js/README.md` (how to rebuild; licenses)
- Create: `crates/core/js/dist/THIRD_PARTY_LICENSES.txt` (licenses of bundled packages)
- Test: `crates/core/js/check.mjs` (`npm run check` rebuilds to a temp file and fails if it differs from the committed bundle)

**Interfaces:**
- Produces: `pub const SANDBOX_LIBS_JS: &str = include_str!("../js/dist/sandbox-libs.js");` in `crates/core/src/script/libs.rs`. The bundle defines `globalThis.{expect, assert, moment, btoa, atob, Buffer, tv4, Ajv, addFormats}` and `globalThis.__plvRequireObject`.

- [ ] **Step 1:** `npm ci && npm run build` in `crates/core/js` → bundle written; record its size.
- [ ] **Step 2:** `npm run check` → PASS (bundle reproducible).
- [ ] **Step 3: Commit** `build(core): bundle Bruno safe-mode script libraries`

### Task 2: Sandboxed engine

**Files:**
- Create: `crates/core/src/script/mod.rs`, `crates/core/src/script/engine.rs`, `crates/core/src/script/libs.rs`, `crates/core/src/script/console.rs`
- Test: `crates/core/src/script/engine.rs` (unit), `crates/core/tests/script_sandbox.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct ScriptLimits { pub timeout: Duration, pub memory_bytes: usize, pub max_stack_bytes: usize } // Default: 30 s, 64 MiB, 1 MiB
  pub struct ScriptEngine { /* rquickjs AsyncRuntime + bytecode of SANDBOX_LIBS_JS compiled once */ }
  pub struct ConsoleLine { pub level: LogLevel, pub text: String }        // LogLevel: Log | Info | Warn | Error | Debug
  pub enum ScriptError {
      Thrown { message: String, line: Option<u32>, column: Option<u32>, stack: Option<String> },
      Timeout(Duration), OutOfMemory, ForbiddenModule(String), Internal(String),
  }
  impl ScriptEngine {
      pub fn new(limits: ScriptLimits) -> Result<ScriptEngine, ScriptError>;
      /// Runs `source` as the body of an async function in a fresh context with the safe-mode libs,
      /// `console`, `require`, and the given host bindings installed. Returns captured console output.
      pub async fn run(&self, source: &str, bindings: &dyn HostBindings) -> (Result<(), ScriptError>, Vec<ConsoleLine>);
  }
  pub trait HostBindings { fn install<'js>(&self, ctx: &rquickjs::Ctx<'js>) -> rquickjs::Result<()>; }
  ```
- Rules: fresh context per `run` (no state leaks between scripts except through host bindings). Interrupt handler enforces the deadline; memory limit via `set_memory_limit`; `console.*` formats objects like Node's `util.inspect` closely enough for JSON-ish values (strings unquoted at top level). `require` throws `ForbiddenModule` for names not in `__plvRequireObject`. Top-level `await` works.

- [ ] **Step 1: Failing tests:** `expect_and_assert_available` (`expect(1).to.equal(1)` passes; `expect(1).to.equal(2)` → `Thrown` with chai message `expected 1 to equal 2`); `require_safe_set_works` (`require('crypto-js').MD5('a').toString() == '0cc175b9c0f1b6a831c399e269772661'`, `require('moment')` defined, `Buffer.from('hi').toString('base64') == 'aGk='`); `require_outside_safe_set_fails` (`fs`, `axios`); `no_ambient_globals` (`typeof process`, `typeof fetch`, `typeof setTimeout` → `'undefined'` unless provided by bindings — `setTimeout` is provided only as part of `bru.sleep`); `infinite_loop_times_out` (limit 200 ms → `Timeout` in < 1.2 s); `memory_bomb_is_contained` (limit 32 MiB → `OutOfMemory`; second `run` on same engine still works); `throw_reports_line` (`\n\nthrow new Error('x')` → line 3); `console_captured`; `top_level_await`.
- [ ] **Step 2–4:** run → FAIL, implement, run → PASS. Record cold `ScriptEngine::new` time and per-`run` overhead for an empty script (print in test output with `--nocapture`, not asserted).
- [ ] **Step 5: Commit** `feat(core): sandboxed QuickJS script engine`

### Task 3: `bru`, `req`, `res` bindings

**Files:**
- Create: `crates/core/src/script/bru.rs`, `crates/core/src/script/req.rs`, `crates/core/src/script/res.rs`, `crates/core/src/script/context.rs`
- Test: `crates/core/tests/script_api.rs`

**Interfaces:**
- Consumes: `ScriptEngine`, `HostBindings` (Task 2); `ResolvedRequest`, `ResponseSummary`, `VarMap`, `VarStack`.
- Produces:
  ```rust
  pub struct ScriptState {                 // shared, mutable across the scripts of one request
      pub runtime: VarMap,                 // bru.setVar/getVar (persist for the run / server session)
      pub environment: VarMap,             // bru.setEnvVar (in memory only in Phase 3)
      pub env_name: Option<String>, pub collection_name: String,
      pub collection_vars: VarMap, pub folder_vars: VarMap, pub request_vars: VarMap,
      pub flow: FlowControl,               // next_request: Option<String>, skip: bool, stop: bool
  }
  pub struct RequestBinding<'a> { pub req: &'a mut ResolvedRequest, pub name: &'a str, pub auth_mode: &'a str }
  pub struct ResponseBinding<'a> { pub res: &'a ResponseSummary, pub url: &'a str, pub body_override: Option<serde_json::Value> }
  pub struct ScriptBindings<'a> { pub state: &'a mut ScriptState, pub req: RequestBinding<'a>, pub res: Option<ResponseBinding<'a>>, pub tests: Option<&'a mut Vec<TestResult>> }
  impl HostBindings for ScriptBindings<'_> { … }
  ```
- API surface (names and semantics from `bru.js`, `bruno-request.js`, `bruno-response.js`):
  - `bru`: `getVar/setVar/hasVar/deleteVar/deleteAllVars/getAllVars`, `getEnvVar/setEnvVar/hasEnvVar/deleteEnvVar/getAllEnvVars`, `getEnvName`, `getCollectionName`, `getCollectionVar/hasCollectionVar`, `getFolderVar`, `getRequestVar`, `interpolate(strOrObj)` (Phase 1 `VarStack` precedence), `sleep(ms)` (async, counts toward timeout), `runner.setNextRequest(name)`, `runner.skipRequest()`, `runner.stopExecution()`, `setNextRequest(name)` alias, `cwd()` (collection root as string). `setVar` name rule from Bruno (`/^[\w-.]*$/`, else throw same message).
  - `req`: `getUrl/setUrl`, `getMethod/setMethod`, `getHeader/getHeaders/setHeader/setHeaders/deleteHeader`, `getBody/setBody` (JSON bodies as objects, like Bruno), `getTimeout/setTimeout`, `setMaxRedirects`, `getName`, `getAuthMode`; properties `req.url`, `req.method`, `req.headers`, `req.body`. Mutations change the `ResolvedRequest` that is sent.
  - `res`: `getStatus/getStatusText/getHeader/getHeaders/getBody/setBody/getResponseTime/getSize/getUrl`; properties `res.status`, `res.statusText`, `res.headers`, `res.body` (JSON-parsed when the content type is JSON, like Bruno), `res.responseTime`. `res(...)` query function → **not in Phase 3** (gap; throws "res() query is not supported yet").
- Unsupported Bruno APIs present in Bruno (`bru.sendRequest`, `bru.runRequest`, `bru.cookies`, global env vars, `bru.getProcessEnv`, `req.onFail`, `bru.setEnvVar(..., {persist:true})`) → throw `Error("bru.X is not supported by Plover yet")`; listed in the final gap report.

- [ ] **Step 1: Failing tests** (one engine, fake request/response): set/get var round trip and invalid-name error; env var set visible to later `bru.interpolate('{{x}}')`; `req.setHeader('x-a','1')` reflected in the `ResolvedRequest`; `req.getBody()` returns object for JSON body; `res.body.items.length` for JSON response; `res.getHeader('Content-Type')` case-insensitive; flow control flags; unsupported API messages.
- [ ] **Step 2–4:** FAIL → implement → PASS.
- [ ] **Step 5: Commit** `feat(core): bru, req and res script APIs`

### Task 4: `test()`, `tests` block, `assert` block, `vars:post-response`

**Files:**
- Create: `crates/core/src/script/tests.rs`, `crates/core/src/script/assert.rs`, `crates/core/src/script/post_vars.rs`
- Test: `crates/core/tests/script_tests.rs`, `crates/core/tests/script_assert.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct TestResult { pub name: String, pub passed: bool, pub error: Option<String> }        // `test(name, fn)`; fn may be async
  pub struct AssertResult { pub lhs: String, pub rhs: String, pub passed: bool, pub error: Option<String> }
  pub async fn run_asserts(engine: &ScriptEngine, asserts: &[KeyValue], b: &mut ScriptBindings<'_>) -> Vec<AssertResult>;
  pub async fn eval_post_response_vars(engine: &ScriptEngine, vars: &[KeyValue], b: &mut ScriptBindings<'_>) -> Result<(), ScriptError>; // values are JS expressions (vars-runtime.js), results go to runtime vars
  ```
- Assert semantics: port `assert-runtime.js` exactly — all 28 operators (`eq, neq, gt, gte, lt, lte, in, notIn, contains, notContains, length, matches, notMatches, startsWith, endsWith, between, isEmpty, isNotEmpty, isNull, isUndefined, isDefined, isTruthy, isFalsy, isJson, isNumber, isString, isBoolean, isArray`), rhs parsing (numbers, quoted strings, booleans, null, `{{var}}` interpolation, arrays for `in`/`between`), lhs evaluated as a JS expression with `res` bound, chai-based messages identical to Bruno's. Disabled (`~`) asserts skipped.
- `tests` block and `test()` calls inside post-response scripts both produce `TestResult`s (Bruno allows both).

- [ ] **Step 1: Failing tests:** one test per operator (pass and fail case, expected error message copied from Bruno's runtime); `async_test_awaited`; `never_settling_test_times_out`; `test_inside_post_response_counted`; `post_response_var_from_body` (`token: res.body.token` → runtime `token`); `disabled_assert_skipped`.
- [ ] **Step 2–4:** FAIL → implement → PASS.
- [ ] **Step 5: Commit** `feat(core): tests, asserts and post-response vars`

### Task 5: Model + persistence for script blocks

**Files:**
- Modify: `crates/core/src/request.rs`, `crates/core/src/bru/model.rs`
- Test: `crates/core/src/bru/model.rs`, `crates/core/tests/roundtrip.rs`

**Interfaces:**
- Produces: `HttpRequest.asserts: Vec<KeyValue>` (owned `assert` block); `ScopeFile` already has scripts/tests — confirm it round-trips. `RequestFile::to_bru` writes `script:pre-request`, `script:post-response`, `tests`, `assert` with minimal diffs (text blocks re-indented 2 spaces, unchanged blocks keep raw bytes).

- [ ] **Step 1: Failing tests:** edit one line inside `script:post-response` of a fixture → 1-line diff; add an assert → only an `assert` block is added/changed; golden round-trip of all fixtures still byte-identical through the typed layer.
- [ ] **Step 2–4:** FAIL → implement → PASS.
- [ ] **Step 5: Commit** `feat(core): own script, tests and assert blocks in the request model`

### Task 6: Runner integration and script flow

**Files:**
- Modify: `crates/core/src/runner.rs`, `crates/core/src/resolve.rs` (expose a hook between resolve and send), `crates/core/src/workspace.rs`
- Test: `crates/core/tests/runner_scripts.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct RequestOutcome { pub response: Option<ResponseSummary>, pub tests: Vec<TestResult>, pub asserts: Vec<AssertResult>,
                              pub console: Vec<ConsoleLine>, pub script_error: Option<ScriptFailure>, pub skipped: bool }
  pub struct ScriptFailure { pub scope: String /* "collection" | "folder <name>" | "request" */, pub phase: ScriptPhase, pub error: ScriptError }
  pub enum ScriptPhase { PreRequest, PostResponse, Tests, PostResponseVars, Asserts }
  // RunEvent::Passed / Failed gain: tests: Vec<TestResult>, asserts: Vec<AssertResult>, console: Vec<ConsoleLine>
  // RunReport gains: tests_passed, tests_failed, asserts_passed, asserts_failed
  // send_one returns RequestOutcome instead of ResponseSummary
  ```
- Order per request (Bruno `run-single-request.js`): resolve vars → pre-request scripts (flow) → re-interpolate request with updated vars → `req` mutations applied → send → `vars:post-response` → post-response scripts (flow) → asserts → tests (flow). Flow from `bruno.json` `scripts.flow`: `sandwich` (default; pre: collection→folders→request, post/tests: request→folders→collection) or `sequential` (collection→folders→request for all). Each scope's script runs as a separate `run` with shared `ScriptState`. `skipRequest()` in pre-request → `Skipped`; `stopExecution()` → stop after current; `setNextRequest(name)` → jump to that request by name in the run list (Bruno semantics; unknown name → stop with warning).
- Pass/fail (D4): failed test, failed assert, or script error → `Failed`. Transport error → `Failed`, and post-response scripts/tests are not run (Bruno parity — verify in `run-single-request.js`).
- Runtime vars persist across requests in one run (CLI) and across sends in a server session (`OpenCollection.runtime`).

- [ ] **Step 1: Failing tests** (wiremock): `chained_login_sets_token` (post-response `bru.setVar('token', res.body.token)` → next request sends `Authorization: Bearer <token>`); `sandwich_order` and `sequential_order` (scripts append to a runtime var; assert final value `"c,f,r,r,f,c"` / `"c,f,r,c,f,r"`); `pre_request_mutation_sent`; `skip_request`; `stop_execution`; `set_next_request_jumps`; `error_reports_scope_and_line`; `failed_test_fails_request_and_run_exit`; `require_fs_fails_request`.
- [ ] **Step 2–4:** FAIL → implement → PASS.
- [ ] **Step 5: Commit** `feat(core): run scripts, tests and asserts in Bruno order`

### Task 7: CLI output and reporters

**Files:**
- Modify: `crates/cli/src/main.rs`, `crates/cli/src/print.rs`
- Create: `crates/cli/src/report/junit.rs`, `crates/cli/src/report/json.rs`
- Test: `crates/cli/tests/cli.rs`, `crates/cli/tests/report.rs`

**Interfaces:**
- CLI flags: `--reporter-junit <FILE>`, `--reporter-json <FILE>`, `--script-timeout-ms <N>`, `--script-memory-mb <N>`, `--verbose` (prints console output).
- Text output per request: existing line, then indented `✓ test name` / `✗ test name: message`, `✓ assert res.status eq 200`, script errors as `✗ script error (folder users, post-response, line 3): message`. Summary: `Requests: 3 passed, 1 failed · Tests: 5/6 · Asserts: 2/2 (123 ms)`.
- JUnit: match Bruno CLI's JUnit layout (`bruno-cli/src/reporters/junit.js`): one `<testsuite>` per request, one `<testcase>` per test/assert, `<failure>` with message, `time` in seconds; valid XML escaping. JSON: match Bruno's JSON reporter shape where reasonable (`bruno-cli/src/reporters/json.js` or equivalent); document differences.
- Exit codes unchanged: 0 all passed, 1 any failed, 2 usage/setup.

- [ ] **Step 1: Failing tests:** `junit_matches_bruno_layout` (parse with `quick-xml`, assert structure + counts for a 2-request run with 1 failing test); `json_report_counts`; `cli_prints_tests_and_summary`; `failed_assert_exit_1`; `junit_escapes_xml` (test name `a<b & "c"`).
- [ ] **Step 2–4:** FAIL → implement → PASS.
- [ ] **Step 5: Commit** `feat(cli): test output, JUnit and JSON reporters`

### Task 8: Server + UI

**Files:**
- Modify: `crates/server/src/api/dto.rs`, `requests.rs`, `runs.rs`; regenerate `frontend/src/api/types.ts`
- Create: `frontend/src/components/request/ScriptTab.tsx`, `TestsTab.tsx`, `AssertTab.tsx`, `frontend/src/components/response/TestResults.tsx`, `frontend/src/components/response/ConsoleOutput.tsx`
- Modify: `frontend/src/components/ResponseViewer.tsx`, `RunPanel.tsx`, `frontend/src/editor/monaco.ts` (add `javascript` language if not already)
- Test: `crates/server/tests/api.rs`, `crates/server/tests/runs.rs`, `frontend/src/components/response/TestResults.test.tsx`, store tests

**Interfaces:**
- `ResponseDto` → `SendResultDto { response?: ResponseDto, tests: TestResultDto[], asserts: AssertResultDto[], console: ConsoleLineDto[], scriptError?: ScriptFailureDto, skipped: boolean }`. `RunEventDto` passed/failed carry tests/asserts counts plus lists; finished report carries test/assert totals.
- UI: request tabs gain **Script** (Pre Request / Post Response, Monaco JavaScript), **Tests** (Monaco JavaScript), **Assert** (key/value table with operator hint). Response pane gains **Tests** (pass/fail list with counts in the tab label) and **Console** tabs; script errors shown as a red banner with scope, phase, line. Run panel rows show `tests 3/4`. Editing scripts marks the tab dirty and saves with minimal diff.

- [ ] **Step 1: Failing tests:** API `send_runs_scripts_and_returns_tests`; WS `run_events_include_tests`; ts bindings up to date; `TestResults` renders counts and failure messages; store keeps script edits in draft.
- [ ] **Step 2–4:** FAIL → implement → PASS (full frontend gate incl. `check:no-cdn`).
- [ ] **Step 5: Commit** `feat(server,frontend): scripts, tests and asserts in the web UI`

### Task 9: Acceptance, Bruno parity spot-check, performance

**Files:**
- Create: `fixtures/phase3-chain/` (bruno.json, `environments/local.bru` with `host`, `collection.bru` with a pre-request script, folder `auth/` with `login.bru` (post-response sets `token`, tests on status/body), folder `users/` with `me.bru` (uses `{{token}}`, asserts `res.status: eq 200`, `res.body.name: isString`) and `create.bru` (pre-request `req.setHeader`, tests with chai)). A tiny mock server for it: `crates/cli/tests/support/` (wiremock in tests) and `scripts/phase3-mock.py` (localhost, for manual/browser checks).
- Modify: `docs/development.md`, `README.md` (scripts support matrix: supported / unsupported APIs).

- [ ] **Step 1: CLI acceptance test** `phase3_chain_passes` (wiremock serving the chain) → exit 0, `Tests: N/N`, JUnit file valid.
- [ ] **Step 2: Bruno parity spot-check** (localhost, `@usebruno/cli` already installed at `/private/tmp/claude-501/-Users-tienle-projects-rea/1bb5dcfd-43a5-447b-a541-d24ed1b9cd89/scratchpad/bench/brunocli`): run the same fixture with `bru run --env local --sandbox safe` (check the exact flag name with `bru run --help`) and with `Plover`; compare per-request pass/fail and test counts. Record differences.
- [ ] **Step 3: Browser acceptance** (agent-browser, localhost only, release build): open fixture, send `login` then `me` → Tests tab shows all passed; edit a test to fail → red result; Save → `git diff --stat` 1 line; run folder → run panel shows test counts. Screenshots to the scratchpad. Console errors must be 0.
- [ ] **Step 4: Performance:** repeat the earlier benchmark (`.../scratchpad/bench/`, `gen.py` + `server.py`) with a variant collection where every request has a small post-response script (`bru.setVar('id', res.body.items[0])`) and one `test()` with `expect`. Report wall time, CPU, peak RSS for 1/200/1000 requests, Bruno CLI vs Plover, cold vs warm, plus per-request script overhead.
- [ ] **Step 5: Full gate** and commit `test: phase 3 acceptance fixture and docs`.

---

## Phase 3 exit checklist

- [ ] Chained collection with scripts/tests/asserts passes in CLI and UI (Task 9).
- [ ] JUnit + JSON reporters (Task 7).
- [ ] Sandbox limits + forbidden require proven (Tasks 2, 4, 6).
- [ ] Parity spot-check and benchmark reported (Task 9).
- [ ] Gap list written; **stop** before Phase 4.

## Known gaps accepted for Phase 3 (report, don't build)

`bru.sendRequest`/`runRequest`, `bru.cookies`, global environments, `process.env`/`.env`, persisting `setEnvVar` to disk, `res(...)` query helper, Bruno developer mode (Node VM, npm modules, `moduleWhitelist`), gRPC/WS script hooks, `req.onFail`, OAuth2 auth scripting.
