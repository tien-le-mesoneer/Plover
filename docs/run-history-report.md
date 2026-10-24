# Run history report — run detail and run history (2026-10-24)

## Tóm tắt

Đã làm xong cả 9 task của plan lịch sử chạy, mỗi task một commit trên `main` (chưa push).

- **Xem lại chi tiết từng request.** Mỗi kết quả chạy giữ request đã gửi (method, URL sau khi thay biến, headers, body) và response (status, headers, body). Trong app, bấm một dòng ở run panel là thấy 5 tab: Response, Headers, Request, Tests, Console.
- **Lịch sử chạy.** Mỗi lần chạy, cả `plv run` lẫn app, đều được lưu ở `runs/` cạnh `state.json`, giữ 20 lần gần nhất cho mỗi collection. Nút **History** trong run panel mở lại một lần chạy cũ. `plv runs`, `plv runs show <id>` và `plv runs clear` làm việc tương tự từ terminal. Trên CI (`CI=true`) mặc định không lưu.
- **Che secret.** Trước khi lưu hoặc ghi báo cáo, giá trị secret và token trong `Authorization`/`Cookie` được thay bằng `***` ở mọi chỗ, kể cả token login nằm trong body response. Báo cáo JSON/HTML/JUnit cũng bị che, khác Bruno.
- **Báo cáo.** JSON giờ có request/response giống Bruno, HTML có phần mở rộng cho từng request. Nhận đủ 5 cờ `--reporter-skip-*` của Bruno.
- **Kiểm tra.** Gate pass: 365 test Rust (đầu phiên là 314), 110 test frontend (trước là 96), clippy, fmt, build, no-cdn, check:console. Chạy thật trên `fixtures/phase3-chain`: `plv runs show` tạo lại file JSON giống hệt lần chạy, và `plv serve` thấy được lần chạy `plv` đó với `Authorization: ***`.

## What was built

| Task | Commit | What |
|---|---|---|
| 1 | 25c6e93 | `RequestDetail`/`ResponseDetail` on `RequestOutcome` (1 MiB body cap, base64 for binary, multipart file names only); serde on run types |
| 2 | 680ada4 | `RunRecorder`, `ResultRecord`, `RunRecord`, `RunId`; 50 MiB body budget per run; `plover_core::time` |
| 3 | 5d91045 | `Masker`: value-based masking, `RunRecorder::finish` / `masked_result` |
| 4 | 8cd35ca | `RunHistory`: per-collection folder, meta written last, prune to 20, find, clear, in-memory store |
| 5 | de4c31f | Report renderers moved to `plover_core::report`, Bruno request/response shape, HTML `<details>`, skip flags |
| 6 | 6b30a65 | `plv run` saves history (`--history`/`--no-history`, `CI`), `plv runs [show|clear]` |
| 7 | 2455332 | Server: `accepted` event, result indices, save on finish/cancel, 4 history routes |
| 8 | 9fe19b0 | UI: result detail pane, History menu, Runs button |
| 9 | 8994fbb | README and development docs |

## Deviations from the plan

1. The terminal printer still prints per event (`Printer::event`), not from records, so the CLI keeps printing the raw error text; records keep Bruno's report text.
2. New CLI report tests use their own `detail_collection` fixture, so the existing fixture's counts stay the same.
3. `plv runs` prints start times in UTC; core has no time-zone data.
4. Recorded durations are whole milliseconds, so a live report and `plv runs show` render byte-identical JSON.
5. Run IDs: the first two suffix characters encode the milliseconds, and the base32 alphabet is in ASCII order (`234567a…z`), because two app runs in the same second listed in the wrong order. The format still matches `^\d{8}T\d{6}Z-[a-z2-7]{6}$`.
6. The server saves a finished run before sending `finished` (the plan said after), so a History list fetched on `finished` already has it. `accepted` is sent with the first core event, so setup errors still arrive first.
7. `RunResultDetailDto` also carries `responseBody` (the stored body detail), so the pane can say "first 1 MiB of N" or "Body not kept".
8. `RunPanel` shows History before any run in the session, and the top bar has a "Runs" button to open it; otherwise saved runs were unreachable after a restart.
9. The repeated-request test checks results 0 and 1 (a request that jumps back to itself), not 0 and 2.
10. `reqwest` (already in the tree via core) is a dev-dependency of the server tests, to call the history routes over HTTP.

The plan file was updated inline for 1–3, 5, 6, 8 and 9.

## Known gaps

1. The new UI was checked by component tests and `check:console` only; nobody has looked at the detail pane and History menu in a browser or the desktop app yet.
2. The detail pane sits inside today's bottom run panel (at most 40% of the window), which is cramped; the dev-UX work moves it into the Run tab.
3. A cancelled app run is masked with the secrets it started with and the header values seen; secrets a script changed before the cancel are not known.
4. Windows and Linux CI have not run these commits (not pushed). The concurrent-save and prune paths are the ones to watch on Windows.
5. Bodies of requests that scripts send (`bru.sendRequest`, axios) are still only summarised, as the spec says.
