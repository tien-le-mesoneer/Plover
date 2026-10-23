# Script gaps A–D — Implementation Plan

> **For agentic workers:** executed inline (superpowers:executing-plans). Steps use checkbox syntax.

**Goal:** Close four Phase 3 gaps the user picked: (A) `process.env` + `.env`, (B) the rest of Bruno safe mode's modules, (C) `bru.sendRequest` + `axios` + `bru.cookies`, (D) persisting script changes to environment/collection variables.

**Spec:** `docs/PRD.md`, `docs/phase-3-report.md` (gaps 1–3). Bruno refs below are in the reference clone (`packages/…`); the installed Bruno CLI 4.3.0 is used for differential checks (`--sandbox safe`).

## Bruno behavior (source of truth)

| Gap | Bruno behavior | Reference |
|---|---|---|
| A | `processEnvVars = {...process.env}` then `.env` at the collection root (dotenv 16 `parse`, no expansion) overrides. Interpolation gets `process: { env }` as the **last** (highest) key; env-file values are first interpolated against it. `bru.getProcessEnv(key)` returns the raw value. | `bruno-cli/src/commands/run.js:661-674`, `runner/interpolate-vars.js:36-75`, `bruno-lang/v2/src/dotenvToJson.js`, `bruno-js/src/bru.js:209-214` |
| B | Safe mode adds host-backed shims: `axios` (+`get/post/put/patch/delete`; resolves `{status, headers, data}`, rejects `{message, code, isAxiosError, response, config}`), `uuid` (`version, parse, stringify, v1, v1ToV6, v3, v4, v5, v6, v6ToV1, v7, validate`), `nanoid` (`{ nanoid }`), `path` (`{ resolve }` only), `jsonwebtoken` (`sign, verify, decode`, sync or callback). Local `./module` requires are also allowed in safe mode (not in scope here). | `bruno-js/src/sandbox/quickjs/shims/lib/{index,axios,uuid,nanoid,path,jwt}.js`, `shims/require.js` |
| C | `bru.sendRequest(config \| url, cb?)` → axios-style response (`status, statusText, headers, data, config`); 4xx/5xx reject (with callback: `err.status` set). Each call is recorded as a scripted request. `bru.cookies`: a property list over cookies for the current request URL (`get/has/all/one/count/toObject/…`, async `add/upsert/remove/delete/clear`) and `bru.cookies.jar()` with async `getCookie, getCookies, setCookie, setCookies, clear, deleteCookies, deleteCookie, hasCookie` (callback or promise). | `bruno-js/src/bru.js:77-92`, `sandbox/quickjs/shims/bru.js:315-333,357-560`, `bruno-requests/src/scripting/send-request.ts:138-220`, `bruno-js/src/cookie-list.js` |
| D | **Bruno 4.3 always persists** script changes; there is no `persist` flag (`setEnvVar(key, value)` only marks the env dirty). After each request: env changes are merged into the selected env file (changed rows rewritten, deleted rows removed, new rows appended with inferred `@number/@boolean/@object` type, **secret rows stay name-only**, `--env-var` values written only if a script changed them); `setCollectionVar` changes are merged into `collection.bru` `vars:pre-request`. Verified with Bruno CLI 4.3.0 on a probe collection. The app also persists the active environment after script updates. | `bruno-cli/src/utils/persist-variables.js`, `runner/run-single-request.js:119-141`, `bruno-js/src/bru.js:224-245`, `bruno-app/src/providers/App/useIpcEvents.js:233-238` |

## Design

- **Host calls (B, C):** a script calls `__brsHost(op, argsJson)`. Pure ops (random bytes, JWT signatures) run on the worker. Ops needing the session (`http`, `cookies.*`) go over a channel to the runner, which serves them **in its async context with the borrowed `Session`** (`tokio::select!` between the engine reply and host requests), each bounded by the script deadline. The worker blocks on the reply, polling cancellation/deadline every 20 ms. No sockets or files from JS.
- **A:** `crates/core/src/dotenv.rs` ports dotenv 16 `parse` (LINE regex, quote stripping, `\n`/`\r` in double quotes). `process_env(root)` = process env + `.env`. `VarStack` gains a top `process` layer `{ "env": {...} }` (object path lookup already exists). Scripts get `info.processEnv`; `bru.getProcessEnv`. Values never logged.
- **B:** `uuid` and `nanoid` bundled from npm with `crypto.getRandomValues` mapped to `__brsCrypto` (Rust `getrandom`); `path.resolve` (posix, cwd = collection root — Bruno uses the host process cwd); `jsonwebtoken` in JS (claims, `expiresIn`/`notBefore` timespans, verify checks, errors with jsonwebtoken's messages) with signatures in Rust (`jsonwebtoken` crate `crypto::sign/verify`, HS/RS/PS/ES). `axios` is a JS wrapper over the `http` host op.
- **C:** `http` host op builds a `ResolvedRequest` from an axios config (`url, baseURL, method, headers, params, data, auth, timeout, maxRedirects`) and sends it through `Session::send` (same jar/TLS/body cap). Responses: `data` JSON-parsed when it parses (axios `forcedJSONParsing`). Recorded in `RequestOutcome.scripted_requests` (CLI `--verbose`, JSON report, server DTO). Cookies via `cookie_store` on the session jar.
- **D:** `store::persist_env_changes` / `persist_collection_var_changes`: under the per-file lock, re-read, apply **only changed keys** (diff of the env map before/after the request) with Bruno's merge rules, write with minimal diff. CLI: env file when `--env` is given, collection vars always. Server: env file (app parity), collection vars stay in session memory. Failures warn and never fail the run (Bruno parity).

## Tests (each written first, failing run confirmed)

- A: `dotenv` unit cases (quotes, `export`, comments, `#` in quotes, multiline double-quoted, `\n` escape, CRLF); `{{process.env.X}}` in URL and inside an env-file value; `.env` overrides process env; `bru.getProcessEnv`; missing var stays literal.
- B: `uuid.v4/validate/version`, `v5` known vector, `nanoid().length == 21`, `path.resolve`; JWT HS256 sign equals a token produced by Bruno CLI safe mode for the same payload/`iat`; verify ok / `invalid signature` / `jwt expired`; RS256 sign+verify round trip; `decode({complete:true})`; `axios.get` against wiremock with JSON data, 404 rejects with `response.status`.
- C: `sendRequest` promise + callback forms, 4xx reject + `err.status`, shares cookies with the main request, respects script timeout and cancellation, server body cap applies, recorded in outcome; cookies jar set/get/delete round trip and `bru.cookies.get(name)` after a `Set-Cookie` response.
- D: env file merge cases (changed row, deleted row, appended typed rows, secret unchanged on disk, override not written unless changed, disabled row untouched, minimal diff, CRLF); collection var persisted; server send persists env; failure to write warns only.
- Regression: Phase 3 sandbox tests, reviewer probes (`scratchpad/review3`), scripted benchmark.

## Security notes

`process.env` exposes the whole process environment to collection scripts and `{{process.env.*}}`, as Bruno does; values are never logged. Host ops are a closed allow-list; HTTP goes only through `Session::send`. Secrets are never written by persistence.
