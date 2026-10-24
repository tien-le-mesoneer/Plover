# Data-driven runs and report export — report (2026-10-24)

## Tóm tắt

Đã làm xong cả 9 task của plan, mỗi task một commit trên `main` (chưa push).

- **Chạy theo file dữ liệu.** `plv run --csv-file-path rows.csv`, `--json-file-path rows.json` và `--iteration-count N` chạy giống Bruno CLI 4.3.0: mỗi dòng là một vòng, `{{cột}}` đọc dòng hiện tại (đứng trên biến runtime), script có `bru.runner.iterationData`, `iterationIndex`, `totalIterations`. Biến runtime và cookie giữ qua các vòng; `setNextRequest(null)` chỉ dừng một vòng, `stopExecution()` dừng cả lần chạy.
- **Báo lỗi rõ thay vì chạy rỗng.** File thiếu, không có dòng nào, lớn hơn 16 MiB, JSON không phải mảng object, CSV sai số cột (`rows.csv:7: expected 4 fields, found 3`), hoặc truyền cả CSV lẫn JSON: dừng với exit 2, không gửi request nào. `--parallel` được nhận nhưng vẫn chạy tuần tự (có cảnh báo).
- **Che mật khẩu trong file dữ liệu.** Giá trị các cột có tên trong danh sách credential (`password`, `client_secret`, `token`, …) bị che `***` ở lịch sử, báo cáo, `--verbose` và app.
- **App.** Nút **Run with data…** trong run panel: chọn CSV/JSON (chỉ hiện số dòng và tên cột, không hiện giá trị, không lưu file) hoặc nhập số vòng. Kết quả nhóm theo "Iteration N of M" kèm chip `key=value` đã che. Nút **Export** lưu HTML/JSON/JUnit — giống từng byte với `plv runs show <id> --reporter-*`; app desktop dùng hộp thoại lưu file.
- **So với Bruno.** Test so sánh (`BRU_BIN=… cargo test -p plover-cli --test bruno_compat`) chạy `fixtures/data-driven` bằng Bruno CLI 4.3.0 và `plv` trên cùng một mock local, với cả CSV và JSON: số vòng, `iterationIndex`, URL sau khi thay biến, status, test, assert và summary của từng vòng giống nhau (pass).
- **Kiểm tra.** Gate pass: 515 test Rust (trước phần này 465), 157 test frontend (trước 144), clippy, fmt, build, no-cdn, check:console. Chạy thật bằng agent-browser trên `plv serve` bản release với state tạm: Run with data với CSV 3 dòng, 3 nhóm iteration, Export JSON trả 200 và giống hệt `plv runs show`; không có giá trị mật khẩu nào trong state, báo cáo hay trang.

## What was built

| Task | Commit | What |
|---|---|---|
| 0 | dab5bc7 | Plan aligned with the merged run-history and curl/HAR code (Task 1 Step 0) |
| 1 | fcec6b5 | `plover_core::data_file`: CSV (csv-parse rules: BOM, trim, empty lines, last duplicate column wins) and JSON rows, `DataFile` with header-order columns, `credential_values` (reuses `credentials.rs`) |
| 2 | aaa712b | `Scope::Iteration` above runtime, `ScriptVars.iteration`, `ScriptInfo.iteration`; `bru.runner.iterationData/iterationIndex/totalIterations`; assert RHS sees the row, `bru.getVar`/`bru.interpolate` do not |
| 3 | 05a2c9e | `RunOptions.iterations` (`Iterations::from_rows`/`repeat`), outer loop in `run_with_state`, `iteration` on request events, `IterationStarted`/`IterationFinished`, `RunReport.iterations` |
| 4 | 807bbdc | `ResultRecord.iteration_index`, `RunRecord.iterations` (row at the end of each iteration), `RunRecorder::masked_iteration`, row masking |
| 5 | 8274144 | JSON one entry per iteration (+ `iterationIndex` per result), HTML "Iteration N of M" sections, JUnit flat |
| 6 | 510f27e | `plv run` data flags, `Iteration: N` headers, masked `--verbose` row, `Iterations: N` summary |
| 7 | 62f971e | Socket `start` with `data`/`iterationCount`, `bad_data`, `iterationStarted` events, `POST …/data-file/preview`, `GET …/runs/{runId}/report`, `POST /native/save-report`, `NativeHost::save_file` (Tauri dialog) |
| 8 | 9ee0e91 | UI: `RunWithDataDialog`, `ExportMenu`, iteration header rows in `RunPanel` |
| 9 | docs commit | `fixtures/data-driven`, Bruno comparison test, README and development docs, this report |

## Deviations from the plan

1. **`IterationFinished` event (core).** The plan left open how the record learns each row's final value after `iterationData.set`; core now sends `IterationFinished { index, data }` after every iteration that ran to its end. A cancelled iteration keeps the row as read.
2. **No iteration events for a plain run on the socket.** The server (and history replay) sends `iterationStarted` only when there is more than one iteration or a non-empty row, so a plain run's event stream and the existing UI are unchanged. Core still emits them for every run.
3. **Columns in file order.** `ValueMap` rows are sorted maps, so `data_file::parse_file` returns the CSV header order separately (`DataFile.columns`) for the preview; JSON columns are first-seen with keys sorted per row. Chips, `--verbose` and the HTML table list a row's keys sorted.
4. **Empty CSV rows.** csv-parse's `skip_empty_lines` keeps a row of empty fields (`,`); Plover does the same (the plan said to skip them).
5. **`iterationData.get` returns copies**, as Bruno's QuickJS shim does; `unset` only removes truthy values, as Bruno's `IterationDataManager` does.
6. **Socket message limit raised to 17 MiB** so a data file just over 16 MiB gets `bad_data` instead of a dropped connection; the dialog also refuses files over 16 MiB before reading them.
7. **Export tests live in `crates/server/tests/data_runs.rs`** (not `run_history.rs`), and the fixture is built in temp dirs instead of `crates/cli/tests/fixtures/data/`.
8. **`run_task` takes a `Settings { env, data }`** to stay under clippy's argument limit.

## Known gaps

1. The desktop save dialog (`DesktopHost::save_file`) compiles and the route is tested with a fake host, but it was not exercised in the built desktop app.
2. Windows and Linux CI have not run these commits.
3. `--parallel` runs iterations one after another.
4. Credential-column values that are not strings (a numeric `password` in JSON) and values shorter than 4 characters are not masked (the run-history minimum).
5. Bruno's report has fields Plover's does not (`uid`, `suitename`, transport headers, pre-request test counts in the summary); the comparison test ignores them because they are not about iterations.
