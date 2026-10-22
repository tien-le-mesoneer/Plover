# Phase 3 report — scripts & tests (2026-10-22)

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
| 1 | 1.33 s / 1.49 s / 232 MB | 0.05 s / 0.01 s / 16 MB (first, cold run 0.18 s) |
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
