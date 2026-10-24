# curl, scratch tab and HAR import report (2026-10-24)

## Tóm tắt

Đã làm xong cả 14 task của plan curl/HAR trên nhánh `feat/import-curl-har`, mỗi task một commit (chưa push).

- **Dán curl vào thanh URL.** Dán `curl …` thì tự điền method, URL, query, headers, body, `-u` và `-F`. Những tuỳ chọn Plover không làm được (`-k`, proxy, cert, `--resolve`, cờ lạ) hiện trong banner "Not imported", không bị bỏ qua im lặng. `-k` hiện đúng câu "`-k` not applied, TLS checking is a global setting". Có **Undo**, và **Move to secret** để chuyển giá trị `Authorization`/`Cookie` thành secret của environment đang chọn.
- **Tab nháp.** Nút **New request** (cạnh nút + trên sidebar) mở tab "Untitled" chưa lưu, Send chạy được ngay với header, auth, script của collection. Save mở hộp thoại chọn folder và tên (gợi ý sẵn, ví dụ `POST orders`). Tab nháp không được nhớ khi khởi động lại, nên token dán vào không xuống đĩa.
- **Copy as cURL.** Nút cạnh Save: mặc định giữ nguyên `{{biến}}`, menu "With variables filled in" thì thay biến theo environment (không chạy script) và hỏi trước nếu lệnh có giá trị secret.
- **Import HAR.** Nút Import HAR trên sidebar: lọc theo host, ẩn sẵn ảnh/JS/CSS/font/media/source map/OPTIONS, gộp request trùng (×N). Trước khi import, hộp thoại liệt kê tên các biến secret sẽ tạo (chỉ tên). Token, cookie, `Authorization`, token header/query, giá trị giống JWT và field mật khẩu trong body đều thành `{{biến}}`, giá trị vào keychain, không vào file `.bru`.
- **Kiểm tra.** Gate pass: 452 test Rust (trước nhánh là 365), 144 test frontend (trước là 110), clippy, fmt, build, no-cdn, check:console. Chạy thật bằng trình duyệt trên bản sao `fixtures/phase3-chain`: dán curl có `-k` → banner đúng câu; Send → 200; lưu vào `auth/` → Bruno CLI 4.3.0 `bru run` chạy được file đó; import HAR tổng hợp → 9 file, 14 biến secret, `grep` không thấy giá trị token nào trong collection.

## What was built

| Task | Commit | What |
|---|---|---|
| 1 | f7a5120 | `curl::parse_curl`: POSIX tokenizer (quotes, `$'…'`, continuations), Bruno's flag mapping, notices for everything else, cmd syntax refused |
| 2 | 7d721eb | `CurlCommand` from a request, a `PreparedRequest` or a `ResolvedRequest`; `to_curl`; round trip for every body mode |
| 3 | ecfbb0f | `store::create_request`, `create_folder`, `add_secret_names`, `sanitize_name` (Bruno), `unique_name`, `next_seq`; `StoreError::Exists` → 409 |
| 4 | 8b20342 | `SendInput.item_id: Option`; scratch send with collection scope only |
| 5 | 7c2e34d | `credentials.rs` (shared fixed lists), `har::preview`, `har::plan_import` (pure) |
| 6 | f22e651 | `POST /curl/parse`, `POST /collections/{id}/curl`; `runner::locate_request`, `runner::copy_as_curl` |
| 7 | 4e34dad | `POST /collections/{id}/requests/new`; `SendBody.item` optional |
| 8 | 67f0730 | HAR `preview`, `plan` and `import` endpoints |
| 9 | eb70a6d | Client methods; scratch tabs in the tabs store; scratch tabs never in UI state |
| 10 | 8329972 | New request button, Save request dialog, scratch close prompt |
| 11 | 870bd75 | curl paste, banner with notices, Undo, Move to secret (`declare` on PUT secrets) |
| 12 | ffd364a | Copy as cURL split button |
| 13 | 02595fb | Import HAR dialog |
| 14 | (this commit) | `docs/capture-with-claude.md`, README section, this report |

## Deviations from the plan

1. **JSON without a content type.** `curl -d '{"a":1}'` with no `Content-Type` becomes a JSON body; Bruno turns the whole JSON text into one form field.
2. **Silent curl options.** Options that change only curl's output or progress (`-s`, `-S`, `-v`, `-i`, `-o`, `-w`, `-f`, `-L`, `--compressed`, …) give no notice. `-e/--referer` becomes a `Referer` header and `--oauth2-bearer` an `Authorization: Bearer` header.
3. **Copy logic in core.** The plan put it in the server handler; `runner::copy_as_curl` and the shared `runner::locate_request` (also used by Send) live in core. An auth mode inherited from a folder or the collection that Plover cannot send is left out and listed in `omitted`.
4. **Plan endpoint body.** `…/import/har/plan` takes the same body as the import (`HarImportBody`), because request names depend on the target folder.
5. **Move to secret.** `saveEnvironment` cannot add secret names, so `PUT …/environments/{name}/secrets` takes `declare: true` to add the names to `vars:secret` as well.
6. **HAR secrets, more than the list.** A JWT-looking value in any header or query parameter also becomes a secret (named after the header or parameter). WebSocket entries are hidden (`webSocket`). A "new subfolder" that already exists is imported into instead of failing. Display names are cut to 100 characters.
7. **Import target.** The folder picker defaults to the root: the sidebar's focused folder lives in the tree component, not in a store.
8. **Tests.** Server tests for this feature are in `crates/server/tests/import.rs`, not `api.rs`.

## Known gaps

1. XML and plain-text bodies are imported as they are; a row whose body looks like it holds a JWT is flagged but not changed.
2. Move to secret cannot tell whether an existing secret already holds the same value (the UI never sees values), so it takes the next free name (`authorization2`).
3. Copy as cURL in the browser was checked by its "Copied" note only (headless Chrome refuses clipboard reads); the copied text was checked through the API.
4. No folder context-menu entry for Import HAR (right-click already runs a folder).
5. The synthetic HAR fixtures contain fake tokens and a JWT-shaped string; run gitleaks before pushing and allowlist `crates/core/tests/fixtures/har/` if it flags them. Windows and Linux CI have not run these commits.
