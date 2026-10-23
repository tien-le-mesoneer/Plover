# Phase 3 report — scripts & tests (2026-10-22)

## Tóm tắt (cập nhật 2026-10-23)

Phase 3 đã xong, kể cả 10 lỗi review: **đã sửa cả 10, không lỗi nào bị bác bỏ**, trong 8 commit trên `main` (chưa push, chưa sang Phase 4).

- 3 lỗi nặng đã hết. Biến lồng sâu không còn làm sập process: quá 1000 tầng thì giữ nguyên `{{x}}`. Kết quả script lỗi giờ làm request fail rõ ràng, không còn mất âm thầm. `bru.interpolate` bị giới hạn kích thước và thời gian; probe cũ từng lên 1776 MB RAM giờ chỉ còn 174 MB và báo lỗi rõ.
- `{{user.id}}` đọc được object như Bruno. JUnit có test case "Post-Response Script Error". Sửa 1 dòng script thì diff đúng 1 dòng; đã kiểm tra trên 427 block. Run và send chạy đồng thời không còn ghi đè biến của nhau. Huỷ run thì script dừng ngay.
- Check đầy đủ đều pass: 226 test Rust, 47 test frontend, clippy, no-cdn, console. So với Bruno CLI trên 2 collection thì không còn khác biệt nào.
- Benchmark với script, 1000 request: 4.07 s (trước là 4.64 s), so với 128.7 s của Bruno.

Plan: `docs/superpowers/plans/2026-10-23-phase-3-scripts-tests.md`. All 9 tasks done on `main`, not pushed; Phase 4 not started.

| Task | Commit | Tests added |
|---|---|---|
| plan doc | a89d348 | – |
| 1 safe-mode library bundles | 3178632 | `npm run check` (bundle reproducible) |
| 2 sandboxed engine | 41f621e | 13 sandbox + 2 unit |
| 3 `bru`/`req`/`res` | 5e163aa | 10 |
| 4 `test()`, asserts, post-response vars | 0388a55 | 6 (all 28 operators) |
| 5 model owns script/test/assert blocks | 8bbe347 | 4 |
| 6 runner integration + flow | 1c3a835 | 10 |
| 7 CLI output, JUnit, JSON | a9f8012 | 6 + 2 unit |
| 8 server + UI | 2324883 | 3 server + 5 frontend |
| 9 fixture, docs, parity, benchmark | bc22be4 | 1 CLI acceptance |

**Gate:** 207 Rust tests passed, 0 failed. Clippy `-D warnings` clean. Frontend: typecheck ok, lint ok, 47/47 tests, build ok. `check:no-cdn` clean. `check:console` reports no page errors.

**Browser acceptance** (release build, localhost):
- login shows Tests 2/2; me shows 3/3.
- Editing a test to fail turns it red ("expected 'Ana' to equal 'Bo'"). Saving gives `git diff --stat` 1 insertion, 1 deletion.
- The run panel shows 2 passed, 1 failed, tests 4/5, asserts 2/2.
- A script error shows the banner "post-response script error (request, line 2): Error: stop here".
- 0 page errors.
- Screenshots: `…/scratchpad/p3/ui/0{1..4}-*.png`.

**Sizes and overhead:**
- Library bundles total 394 KB; the release binary is 20 MB.
- Steady-state cost per script run: about 0.4 ms for an empty script and about 1.1 ms when the script uses chai.
- In the 1000-request benchmark, scripts add about 2 ms per phase.

**Benchmark** (macOS, local Python server). Every request has one post-response script and one chai test. Bruno 4.3.0 runs in safe mode. 5 runs each, 3 runs for 1000 requests:

| Requests | Bruno wall / CPU / RSS | bru-rs wall / CPU / RSS |
|---|---|---|
| 1 | 1.33 s / 1.49 s / 232 MB | 0.05 s / 0.01 s / 16 MB (first, cold run 0.18 s) | fixed b128523: iterative port of `replace`, depth cap 1000 (literal past it); 200k via `bru.interpolate`, 20k in URL: exit 0 |
| 200 | 27.5 s / 26.5 s / 284 MB | 0.80 s / 0.77 s / 24 MB |
| 1000 | 128.7 s / 125.5 s / 432 MB | 4.64 s / 4.08 s / 29 MB |

**Bruno parity spot-checks** (Bruno CLI 4.3.0, safe mode, same localhost mock):
- `phase3-chain`: both report 3 requests, Tests 5/5, Asserts 2/2.
- An edge-case collection with failing tests and asserts, a thrown error, a post-response script error and a skipped request gives the same summary counts and the same chai messages, with one difference: when an assert's left side throws, Bruno reports `expected { name: 'TypeError', …(2) } to equal 1`, while we report `cannot read property 'deep' of undefined`. I kept ours.

**Deviations from the plan:**
- The libraries are split into 6 bundles that are compiled once per worker and loaded on first use. A single bundle cost 14 ms per script.
- The engine uses a pool of synchronous QuickJS runtimes (up to 4 threads) instead of `AsyncRuntime`. Scripts exchange JSON with the engine, and the APIs live in `api.js`, not in Rust host bindings.
- The default memory limit is 128 MiB instead of 64 MiB, so scripts can handle large JSON bodies. The stack limit is 8 MiB because ajv overflowed at 1 MiB.
- A promise that can never settle (no timers pending) is reported immediately as `Unsettled` instead of waiting for the timeout.
- Each phase runs as one context with a closure per scope, like Bruno's `wrapAndJoinScripts`. Tests report in completion order, as Bruno does.
- `RunEvent::Passed` and `RunEvent::Failed` now carry a `RequestOutcome`. `resolve` is split into `prepare` and `finalize`, because pre-request scripts see the request before interpolation. Runtime variables and environment overrides hold JSON values.
- The server's send endpoint returns a `SendResultDto`. A pre-request script error comes back as a 200 response with no `response` field. A transport error still returns 502.
- Script changes to the environment are remembered in server memory for the session.
- The JSON report is a list of iterations. Its per-result `status` uses Bruno's meaning (whether the request completed), and a post-response script error appears as a failed test entry.
- The assert tests were merged into `script_tests.rs`. The CLI summary now reads `Requests: … · Tests: … · Asserts: …`.
- My gate script once hid a typecheck failure. I fixed the script before that commit was made, so every commit passed the gate.

## Gaps for tomorrow (most likely to affect users first)
1. These are not supported yet: `bru.sendRequest`, `bru.runRequest`, `bru.cookies`, `process.env` / `.env` (including `{{process.env.X}}`), and global environments.
2. Bruno's safe mode also provides `axios`, `uuid`, `nanoid`, `jsonwebtoken` and `path`, which we don't. Developer mode (npm modules, local `require`) is not supported.
3. `bru.setEnvVar(..., {persist: true})` does not write to disk. Script environment changes stay in memory (in the server session, or for one CLI run).
4. Scripts in the server see at most 10 MiB of a response body. A transport error on send loses the pre-request console output.
5. Each script phase costs about 2 ms. Reusing contexts or keeping chai loaded would cut this.
6. These parts of `req`, `res` and the reports are missing or partial:
   - `req.getAuthMode()` returns `none` for basic auth in pre-request scripts.
   - Multipart bodies can't be read or replaced from scripts.
   - `res.jq`, `req.onFail` and `req.getPathParams` are not implemented.
   - JUnit and JSON reports don't split pre-request and post-response test results.
7. Not verified:
   - Windows and Linux.
   - Bruno's own scripting fixtures in `fixtures/bruno-tests` (they need Bruno's test server).
   - Console output is formatted with JSON, not Node's `util.inspect`.

## Independent review findings (all fixed, 2026-10-23)

Read-only review of a89d348..967a1f6. Each finding was verified by a localhost probe, by a differential run against Bruno CLI 4.3.0 `--sandbox safe`, or by reading the code, as noted. Probes: `/private/tmp/claude-501/-Users-tienle-projects-rea/1bb5dcfd-43a5-447b-a541-d24ed1b9cd89/scratchpad/review3/`.

| # | Sev | Where | Defect → effect | Verified | Status |
|---|---|---|---|---|---|
| 1 | HIGH | `core/src/vars.rs:118-121` via `script/api.rs:272`, `runner.rs:588` | `VarStack::replace` recurses once per nested `{{var}}` level with no depth limit. A deep chain (20k levels in a URL, or 200k via `bru.interpolate`) overflows the Rust stack and the process aborts (exit 134). | probes `p_deep`, `p_deep2` | fixed b128523: iterative port of `replace`, depth cap 1000 (literal past it); 200k via `bru.interpolate`, 20k in URL: exit 0 |
| 2 | HIGH | `script/engine.rs:358-376`, `prelude.js:214` | If collecting a phase's result fails, the error is swallowed. Tests, console output, `setVar`s and request edits from that phase disappear, and a failing test is silently dropped so the request passes. Triggers: a lone surrogate (`console.log('\ud800')`), a circular object or an array nested deeper than 128 in `setVar`, or a 70 MB `setVar` that runs out of memory during stringify. Bruno fails all three requests. | probes `p_surrogate`, `p_nest`, `p_big` + Bruno diff | fixed ef4a09a: per-part JSON results, lone surrogates → U+FFFD, unserializable/deep/OOM → `ResultUnavailable` fails the request; all 3 probes fail like Bruno |
| 3 | HIGH | `api.rs:272`, `vars.rs:76` | `bru.interpolate` builds its result in Rust, outside the QuickJS memory limit and deadline. 29 doubling variables make a 512 MiB string, with peak RSS 2.06 GB before the request is reported as out of memory. | probe `p_interp` | fixed b128523: output cap (script memory limit; 64 MiB for requests), 2× work budget, deadline; probe RSS 1776 MB → 174 MB, clear error |
| 4 | MED | `vars.rs:64-69` | `{{obj.path}}` doesn't resolve JSON object variables. Bruno uses lodash `get`. `bru.setVar('user', {...})` then `/users/{{user.id}}` → bru-rs sends a literal `{{user.id}}`; Bruno sends `/users/42`. | Bruno diff `p_nested` | fixed b128523: variables keep JSON values; lodash-`get` paths incl. `a[0].b`; Bruno diff `p_nested` now identical |
| 5 | MED | `cli/src/report/junit.rs:21-26` | Post-response script errors are not emitted as JUnit testcases: the exit code is 1 but the suite shows `tests="0" failures="0"`, which CI reads as green. Bruno emits a failing "Post-Response Script Error" testcase. | Bruno diff `p_posterr` | fixed 11fa12e: failing `Post-Response Script Error` testcase with the thrown message |
| 6 | MED | `bru/syntax.rs:185-206`, `bru/render.rs:86-90` | Editing one line of a script/tests block re-indents the whole block: trimmed blank lines become `"  "` and tab indentation changes, so a one-line edit produces a multi-line diff. `fixtures/` has 399 such blank lines. | Rust probe test | fixed ef7c1c6: LCS line merge; golden test edits 1 line in all 427 fixture text blocks → 1/1 |
| 7 | MED | `runner.rs:843,855`, `server/src/api/runs.rs:222,240` | Server runtime vars are copied at the start of a send or run and fully replaced at the end, so concurrent sends and runs overwrite each other. Example: a slow folder run wipes the `token` set by a `login` send from the UI. Env changes are merged as diffs, but runtime vars are not. | code reading | fixed 7791c54: sends and runs apply set/delete diffs; concurrency test |
| 8 | LOW | `engine.rs:190-202` | Cancelling a run doesn't interrupt the running script. Four cancelled `while(true){}` runs keep every worker busy until the 30 s timeout, so UI sends stall. | code reading | fixed a77ee6f: cancel flag checked by interrupt handler, timer sleeps and job start; 8 cancelled loops free workers in < 1.5 s |
| 9 | LOW | `api.js:401-402` | `throw 'plain'` inside `test()` reports `plain`; Bruno reports `An unexpected error occurred.` | Bruno diff | fixed 7370068 |
| 10 | LOW | `cli/src/report/mod.rs:82-88` | Requests never started (e.g. after `stopExecution`) are named by file stem (`04-r4`) instead of meta name (`r4`). | Bruno diff | fixed 7370068 |

**What held up:**
- In a 5-request differential run against Bruno, everything matched except #9 and #10: counts, chai and assert messages, `{{n}}` on an assert's right side, `req.setBody` and `setMethod` being sent, unknown `setNextRequest`, and `stopExecution`.
- Workers recover after running out of memory.
- Microtask-loop timeouts work (about 500 ms).
- `require` limits hold.
- No escape to the filesystem, network or process was found.

**Fix notes (2026-10-23):**
- Tests were confirmed failing before each fix, except #1, #3 and #4. For those I ran the new tests together with the new interpolation code; the failure was confirmed by running the reviewer's probes on the old release binary (exit 134, 1776 MB, literal `{{user.id}}`).
- Re-running the 5-request Bruno comparison (`p_par`) and `p_nested` shows no differences in summaries, statuses, names, test messages or assert messages.
- Benchmark after the fixes (bru-rs, scripted): 1 request 0.014 s, 200 requests 0.83 s (was 0.80 s), 1000 requests 4.07 s (was 4.64 s); RSS unchanged at 16, 25 and 29 MB.
- Still open (unchanged from "Gaps"): unsupported APIs, Windows and Linux are untested, and Node `util.inspect` console formatting is not matched.
- #1 caps nesting at 1000 levels, which is lower than where Bruno's JS stack would overflow.

## Script gaps (A–D)

**Tóm tắt:** Đã đóng cả 4 gap người dùng chọn, mỗi gap đối chiếu với Bruno CLI 4.3.0 `--sandbox safe`:
- `process.env` và `.env`
- các module của safe mode: `uuid`, `nanoid`, `path`, `jsonwebtoken`, `axios`
- `bru.sendRequest` và `bru.cookies`
- lưu thay đổi biến xuống file (Bruno 4.3 luôn lưu, không cần `{persist: true}`)

Kết quả kiểm tra: 249 test Rust và 48 test frontend đều pass. Probe của reviewer cho kết quả như cũ. Benchmark có script nhanh hơn trước khoảng 43% (1000 request: 4.17 s xuống 2.39 s). Một số điểm khác Bruno là cố ý, liệt kê bên dưới.

Plan: `docs/superpowers/plans/2026-10-23-script-gaps.md`. Commits on `main` (not pushed):

| Commit | What |
|---|---|
| 74b8f58 | plan |
| b7b2d6c | (A) `.env` parsed like dotenv 16 (16 edge cases match node dotenv 16.6.1); `{{process.env.X}}` as the highest interpolation layer, also inside env-file values; `bru.getProcessEnv` |
| 6744ceb | (B, C) host calls from scripts; `uuid`, `nanoid`, `path`, `jsonwebtoken`, `axios`; `bru.sendRequest`; `bru.cookies` and `bru.cookies.jar()` |
| ff15af9 | scripted requests in `SendResultDto.scriptedRequests`, the UI Console tab and CLI `--verbose`; licenses for the new bundles |
| d71427d | (D) persisting script variable changes |
| 786a5d4 | prelude/API compiled to bytecode once per worker; JWT error shape |

**Bruno behavior found (source + probes):**
- *Process environment:* `.env` overrides the process environment and `process: { env }` is the last key in the combined variables (`commands/run.js:661-674`, `interpolate-vars.js:36-75`).
- *Modules:* safe mode's module shims are `bruno-js/src/sandbox/quickjs/shims/lib/*`. `sendRequest` resolves `{status, statusText, headers, data, config, request, responseTime}`. It rejects 4xx with `{code, config, message, name, stack, status}` and no `response`; `axios` rejects with `{code, config, isAxiosError, message, response}`.
- *Persistence:* Bruno 4.3 persists after every request with no flag (`utils/persist-variables.js`). The output of our merge for the probe collection is byte-identical to what Bruno CLI wrote.

**Design:**
- *Host calls:* scripts call `__brsHost`. Random bytes and JWT signatures are answered on the worker. HTTP and cookie operations go over a channel to the runner, which serves them with the run's `Session` inside the script deadline; cancellation interrupts the wait.
- *Persistence:* `RunOptions.persist` selects what is written.
  - The CLI writes both the environment file and `collection.bru`.
  - The server writes the environment file only, as the Bruno app does; secrets stay in memory and the collection is reloaded.
  - Library callers write nothing by default.
  - Only changed variables are touched, at the syntax layer, under the store's per-file lock with an atomic rename. A write failure only warns.

**Deliberate deviations:**
- *Cookies and `sendRequest`:* like Bruno, `bru.sendRequest` and `axios` don't use the run's cookie jar; a Bruno CLI probe showed no `cookie` header. This overrides the "same session cookies" design note.
- *Cookie data:*
  - `jar.setCookie(url, name, value)` works here; in Bruno 4.3.0 it silently does nothing.
  - Cookie objects have no `creation`/`lastAccessed`.
  - Cookies with equal path length are ordered by name, not by creation time.
- *`path.resolve`:* relative paths start at the collection root, not the host process's working directory.
- *Response `config`:* contains only `url, method, headers, data, timeout`, not axios internals.
- *Persisted rows:* new rows are appended in name order, not script order.
- *`jwt.verify`:* with a PEM key and no `algorithms` option, it accepts RS*/PS*/ES*/EdDSA; jsonwebtoken narrows by key type.

**Gate:** `cargo test --workspace` 249 passed, 0 failed. `cargo clippy -D warnings` is clean. Frontend: typecheck and lint are clean, 48/48 tests pass. `check:no-cdn`, `check:console` and the library `npm run check` (9 bundles) are clean, and the release build succeeds.

**Test-first:** the new B and C tests failed on the commit before them (6/7 in a worktree at `b7b2d6c`; the seventh is A, which that commit already had). The dotenv, persist and runner persistence tests were seen failing first against `todo!()` or a missing API.

**Regression:**
- *Phase 3 sandbox tests:* pass.
- *Reviewer probes* (`scratchpad/review3`): unchanged on all 7 probes with earlier results. For example, `p_interp` peaks at 178 MB and fails with a clear error, and `p_deep` exits 0.
- *Scripted benchmark* (macOS, localhost, one post-response script and one chai test per request, old vs new binary run alternately):

| Requests | before gaps (51badc5) wall / CPU / RSS | now (786a5d4) wall / CPU / RSS | Bruno CLI 4.3.0 safe (earlier run) |
|---|---|---|---|
| 1 | 0.020 s / 0.010 s / 16 MB | 0.020 s / 0.010 s / 18 MB | 1.33 s / 232 MB |
| 200 | 0.850 s / 0.794 s / 24 MB | 0.486 s / 0.438 s / 28 MB | 27.5 s / 284 MB |
| 1000 | 4.17 s / 3.95 s / 28 MB | 2.39 s / 2.18 s / 33 MB | 128.7 s / 432 MB |

Before the bytecode change, the gaps had made 1000 requests 5.52 s (api.js parsed per context); that was measured and fixed in 786a5d4. The benchmark collection doesn't use the new APIs, so their own per-call cost is not measured: each `sendRequest` is one round trip to the runner plus the HTTP request.

**Security notes:**
- *Process environment:* as in Bruno, collection scripts and `{{process.env.*}}` see the whole process environment. Values are never logged.
- *Host calls:* a closed allow-list (`random`, `jwt.sign`, `jwt.verify`, `http`, `cookies.*`). HTTP goes only through `Session::send_without_cookies`, with the session's TLS options and body cap; there are still no sockets, files or process access in JS.
- *Persistence:* never writes secret values; secret rows stay name-only. Values from `--env-var` reach disk only when a script changed them.
- *Test keys:* `crates/core/tests/fixtures/jwt/` holds throwaway RSA keys for tests only.

**Still open (supersedes gaps 1–3 above where they overlap):**
1. **Missing APIs:** `bru.runRequest`, global environments, `--env-file`/`extends`-aware persistence and local `require('./x')` modules (Bruno safe mode allows the last one).
2. **Typed environment values:** `@number/@boolean/@object` annotations are *written* on persist, but values read from environment files are still strings.
3. **Server collection variables:** `setCollectionVar` changes are not persisted by the server; they stay in session memory.
4. **`sendRequest` parity:** calls inside one script run one at a time; `Promise.all` waits for each. There is no proxy or client-certificate configuration, as before.
5. **Unverified:**
   - Windows/Linux builds.
   - Bruno's own scripting fixtures (they need Bruno's test server).
   - The per-call cost of the new APIs.
   - The UI display of scripted requests, which has a unit test only.
