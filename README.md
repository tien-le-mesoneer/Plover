# bru-rs

A Rust API client that opens [Bruno](https://github.com/usebruno/bruno)
collections (`.bru` files) as they are, and saves them with minimal diffs.

- `bru-rs run` — run a collection, folder or request from the command line (CI).
- `bru-rs serve` — local web UI on `127.0.0.1`, embedded in the same binary.

All HTTP requests are sent by the Rust backend, never by the browser.

## Usage

```bash
bru-rs run path/to/collection --env local
bru-rs run path/to/collection/folder/request.bru --env local --env-var token=abc
bru-rs serve path/to/collection          # opens the browser
bru-rs serve --no-open --port 4000       # prints the URL instead
```

`run` exits 0 when every request passed (HTTP 4xx/5xx is not a failure, as in
Bruno), 1 when a request failed to send, and 2 on usage or load errors. Files
that cannot be parsed are skipped with a warning, like Bruno's CLI.

## Status

Phase 2 of 4: core + CLI, local server + web UI. Scripts and tests (Phase 3)
and the desktop app (Phase 4) are not built yet. See `docs/PRD.md`.

## Building

See [docs/development.md](docs/development.md). Release build:
`scripts/build-release.sh`.

Test fixtures under `fixtures/bruno-tests` come from Bruno (MIT, see the
license in that folder).
