# data-driven fixture

A two-request collection for data-driven runs (`--csv-file-path rows.csv`,
`--json-file-path rows.json`). All values are synthetic. `host` points at a local mock;
override it with `--env-var host=http://127.0.0.1:<port>`.
