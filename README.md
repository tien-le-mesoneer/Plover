# Plover

A Rust API client that opens [Bruno](https://github.com/usebruno/bruno)
collections (`.bru` files) as they are, and saves them with minimal diffs.

- `plv run` — run a collection, folder or request from the command line (CI).
- `plv serve` — local web UI on `127.0.0.1`, embedded in the same binary.

All HTTP requests are sent by the Rust backend, never by the browser.

## Usage

```bash
plv run path/to/collection --env local
plv run path/to/collection/folder/request.bru --env local --env-var token=abc
plv serve path/to/collection          # opens the browser
plv serve --no-open --port 4000       # prints the URL instead
```

`run` exits 0 when every request passed (HTTP 4xx/5xx is not a failure, as in
Bruno), 1 when a request failed to send or a test, assert or script failed, and
2 on usage or load errors. Files that cannot be parsed are skipped with a
warning, like Bruno's CLI.

Reports and script limits:

```bash
plv run coll --env local --reporter-junit out/junit.xml --reporter-json out/results.json
plv run coll --env local --verbose                  # also print script console.* output
plv run coll --script-timeout-ms 10000 --script-memory-mb 64
```

## Scripts and tests

`script:pre-request`, `script:post-response`, `tests`, `assert` and
`vars:post-response` run like Bruno's **safe mode** (QuickJS), in Bruno's order
(collection → folders → request; post-response scripts and tests from the request
outwards unless `bruno.json` sets `"scripts": { "flow": "sequential" }`).

| Supported | Not supported yet (throws `… is not supported by Plover yet`) |
|---|---|
| `bru`: get/set/has/delete for runtime, env and collection vars (any JSON value), `getFolderVar`, `getRequestVar`, `interpolate`, `getEnvName`, `getCollectionName`, `cwd`, `sleep`, `setNextRequest`, `runner.skipRequest/stopExecution/setNextRequest` | `bru.sendRequest`, `bru.runRequest`, `bru.cookies`, global environments, `bru.getProcessEnv`, OAuth2 helpers |
| `req`: URL, method, headers, body, timeout, max redirects (get/set), `getName`, `getAuthMode` | `req.onFail`, `req.getPathParams`, `req.getTags`, `req.getExecutionMode`, `req.disableParsingResponseJson`; reading/replacing multipart bodies |
| `res`: `status`, `statusText`, `headers`, `body`, `responseTime`, `res('a.b[0]')`, `getSize`, `setBody` | `res.jq`, extra function arguments to `res(...)` |
| `test()`, `expect`/`assert` (chai 4 + Bruno's `json`, `jsonSchema`, `jsonBody`), all 28 assert operators | — |
| `require`: `chai`, `buffer`, `moment`, `btoa`, `atob`, `crypto-js`, `tv4`, `ajv`, `ajv-formats`; `setTimeout`/`setInterval` | Any other module (`fs`, `axios`, npm packages, local files): the request fails with `Cannot find module 'x'`. Bruno's developer mode is not supported. |

Limits: 30 s per script phase and 128 MiB per script runtime by default; a
script that exceeds them fails its request and the run continues.

## Status

Phase 3 of 4: core + CLI, local server + web UI, scripts and tests. The desktop
app (Phase 4) is not built yet. See `docs/PRD.md` and `docs/phase-3-report.md`.

## Building

See [docs/development.md](docs/development.md). Release build:
`scripts/build-release.sh`.

Test fixtures under `fixtures/bruno-tests` come from Bruno (MIT, see the
license in that folder).

## Building

`scripts/build-release.sh` builds `target/release/plv` with the web UI embedded (about 13 MB). For CI-only use, `cargo build --release -p plover-cli --no-default-features` builds a CLI-only `plv` (about 8 MB). Size and speed measurements: [docs/development.md](docs/development.md).

## License

MIT — see [LICENSE](LICENSE). Test fixtures under `fixtures/bruno-tests/` come from [Bruno](https://github.com/usebruno/bruno) (MIT, see `fixtures/bruno-tests/LICENSE`). Plover is not affiliated with Bruno.
