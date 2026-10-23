# Import from cURL and HAR, scratch tabs: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** paste a curl command into the URL bar, send unsaved scratch requests, copy any request as cURL, and import a HAR file into a folder with its credentials moved into secret variables. All of this is in the UI only.

**Architecture:** The parsing and writing live in Rust core. `curl.rs` parses and writes curl. `har.rs` parses a HAR file and turns it into an import plan without touching the disk. `store.rs` gains create operations for requests, folders and secret names. The axum server exposes all of these as thin handlers. The React frontend adds a scratch tab type to the tabs store, paste handling in `UrlBar`, a "Copy as cURL" action, and an "Import HAR" dialog.

**Tech Stack:** Rust (axum, serde, ts-rs), React + TypeScript + Zustand, vitest and Testing Library.

**Spec:** `docs/superpowers/specs/2026-10-23-import-curl-har-design.md`

**Branch / worktree:** `feat/import-curl-har` in `/Users/tienle/projects/bruno-rs-curl`. The run-history work happens on `main` at the same time; see "Merge risk with run history".

## Global Constraints

- UI only. There is no `plv curl` and no CLI part. Plover never acts as a recording proxy.
- Written files are valid Bruno `.bru` and round-trip through `RequestFile`, `ScopeFile` and `EnvironmentFile` (`crates/core/src/bru/model.rs`). Bruno must be able to open every file that is imported or saved.
- Every new write goes through `store::write_atomic`, the per-file lock (`lock_file`) and `resolve_item_path`. Nothing escapes the collection root.
- No token, cookie, `Authorization` value or listed body credential from a HAR file is ever written into a `.bru` file.
- Scratch tabs are never persisted. They are not part of `UiStateDto.tabs` or `state.json`.
- Unsupported curl options become a `CurlNotice`. They are never dropped silently. The notice for `-k` reads exactly: ``-k` not applied, TLS checking is a global setting``.
- API types are generated with ts-rs. Run `PLOVER_UPDATE_BINDINGS=1 cargo test -p plover-server --test bindings` and commit the result. Never edit `frontend/src/api/types.ts` by hand.
- The request body limit stays at `MAX_REQUEST_BODY` (64 MiB). A larger HAR file gets a 413 with a clear message.
- Secret values go only through `OpenCollection::update_secrets`.
  - Tests set a temporary `PLOVER_STATE_DIR` and `PLOVER_KEYCHAIN_SERVICE=dev.plover.test`, and never touch real keychain entries.
- Test fixtures are synthetic: `example.test` hosts and fake tokens like `tok-AAAA…`. No captured traffic from real systems.
- These must pass before each commit:
  - `cargo test --workspace`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cd frontend && npm run typecheck && npm run lint && npm test`
  - Use `set -o pipefail` whenever output is piped.

## Review Focus

These are the inputs most likely to hurt a user that the spec's tests do not exercise. Each one has a pinned test in the task named.

1. **A Chrome DevTools "Copy as cURL (bash)" command.** It has 15 or more `-H` lines, `-b`, a `$'…'` body, `--compressed`, `\` continuations and `-H 'sec-ch-ua: "Chromium";v="1"'` with nested quotes. It should paste with zero notices. Test `chrome_devtools_copy_parses_cleanly` (Task 1).
2. **A Windows `cmd` copy that uses `^` continuations and `^"` quoting.** Cmd syntax is out of scope, but it must not silently produce a wrong request. Either it parses the way Bruno's cleaner does, or it returns `invalid_curl` or a notice. It must never return a URL containing `^`. Test `cmd_style_curl_never_yields_caret_url` (Task 1).
3. **A minimal HAR from Firefox or Safari.** Only the required HAR 1.2 fields are present: no `postData.params`, no `_resourceType`, `content.mimeType` missing or empty, `postData.text` absent for a POST. Preview and import must still work. Test `minimal_har_imports` (Task 5).
4. **Hostile names on import.** These include a URL path of 300 characters, a path of only `/`, a path `/CON`, a path with `?:*|` characters, and two entries that map to the same name. Each must produce a valid, unique Bruno file name. Test `import_names_are_valid_and_unique` (Task 5).
5. **A 40 MiB HAR with large captured response bodies.** The preview returns only row summaries. No response content is echoed back, and the preview JSON stays under 1 MiB for 500 entries. Test `preview_does_not_echo_response_content` (Task 8).

## Merge risk with run history

These files are also changed by the run-history work on `main` (spec `docs/superpowers/specs/2026-10-23-run-history-design.md`):

| File | This branch changes | Run history changes | Handling |
|---|---|---|---|
| `crates/core/src/runner.rs` | `SendInput.item_id` → `Option<&str>`; scratch branch in `send_one_with` (Task 4) | captures request/response detail at the send site in `Exec::execute`; `RequestOutcome` grows | Keep Task 4 to `SendInput` and the top of `send_one_with`; do not touch `execute`. Rebase on `main` right before Task 4. |
| `crates/server/src/api/mod.rs` | new routes (Tasks 6–8) | new `/collections/{id}/runs…` routes | Append-only route lines; trivial conflict. |
| `crates/server/src/api/dto.rs` | `SendBody.item: Option`, new DTOs | run detail / history DTOs | Add new DTOs at the end of the file in one block. |
| `frontend/src/api/types.ts` | regenerated | regenerated | Never hand-merge: after rebase, regenerate with the bindings command. |
| `frontend/src/api/client.ts` | new methods (Task 9) | runs/history methods | Append-only. |
| `crates/server/src/api/requests.rs` | `send` passes `Option` item; new `create` (Tasks 7) | maybe `SendResultDto` detail | Small; rebase before Task 7. |
| `frontend/src/styles.css`, `README.md` | new classes / link | new classes / section | Append-only. |

Not shared: `RunPanel.tsx` and `store/runs.ts`, which only run history changes, and `tabs.ts` and `UrlBar.tsx`, which only this branch changes.

---

### Task 1: curl parser in core

**Files:**
- Create: `crates/core/src/curl.rs`
- Modify: `crates/core/src/lib.rs` (`pub mod curl;`)
- Test: `crates/core/tests/curl.rs`

**Interfaces:**
- Produces:
  - `pub fn parse_curl(text: &str) -> Result<ParsedCurl, CurlError>`
  - `pub struct ParsedCurl { pub request: HttpRequest, pub notices: Vec<CurlNotice> }`
  - `pub struct CurlNotice { pub option: String, pub reason: String }`
  - `#[derive(thiserror::Error)] pub enum CurlError { NotCurl, MissingUrl, Syntax(String) }`

  `request` is `HttpRequest` with `kind: "http"`, `name: ""`, `seq: 0.0`, `auth` `None` (or `Basic`), and everything outside method/url/query/headers/body/auth defaulted.

- [ ] **Step 1: Write the failing tests.** Port the cases from Bruno's `bruno-app/src/utils/curl/parse-curl.spec.js` (53 cases) and `curl-to-json.spec.js` (8 cases) that the mapping table covers. Each named test below asserts the full `(method, url, query, headers, body, auth)` tuple and `notices`:
  - `get_with_query_fills_query` covers `curl 'https://example.test/a?x=1&y=2'`, giving `GET` and query `[x=1, y=2]`.
  - `data_defaults_to_post_form_urlencoded` covers `-d 'a=1&b=2'`, giving `POST` and `Body::FormUrlEncoded([a=1, b=2])`.
  - `json_content_type_picks_json` covers `-H 'Content-Type: application/json' --data-raw '{"a":1}'`, giving `Body::Json("{\"a\":1}")`.
  - `xml_and_text_modes` covers `application/xml` giving `Body::Xml` and `text/plain` giving `Body::Text`.
  - `get_flag_moves_data_to_query` covers `-G -d a=1`, giving `GET` with query `[a=1]` and `Body::None`.
  - `form_fields_and_files` covers `-F name=x -F file=@/tmp/a.png`, giving `Body::Multipart` with `Text("x")` and `Files(["/tmp/a.png"])`.
  - `user_flag_is_basic_auth` covers `-u alice:s3cr3t`, giving `Auth::Basic{alice, s3cr3t}`.
  - `bearer_header_stays_header` covers `-H 'Authorization: Bearer t'`, which stays a header with `auth` `None`.
  - `head_user_agent_cookie` covers `-I`, `-A ua` and `-b 'a=1'`, giving `HEAD`, `User-Agent: ua` and `Cookie: a=1`.
  - `compressed_is_silent` covers `--compressed`, giving no notice.
  - `insecure_gives_exact_notice` covers `-k`, giving notice `{option: "-k", reason: "`-k` not applied, TLS checking is a global setting"}`.
  - `unknown_and_unsupported_flags_are_notices` covers `--resolve a:443:1.2.3.4`, `-x http://p:3128`, `--cert c.pem`, `--digest`, `--data-binary @f.bin`, `-T f` and `--frobnicate`. Each gives one notice whose `option` includes its value, and the request is still filled.
  - `ansi_c_quotes_and_continuations` covers `$'a\nb'`, `\`+newline continuations, and `'it'\''s'`.
  - `missing_url_is_error` covers `curl -H 'a: b'`, giving `Err(CurlError::MissingUrl)`.
  - `not_curl_is_error` covers `wget x`, giving `Err(CurlError::NotCurl)`.
  - `chrome_devtools_copy_parses_cleanly` (Review Focus 1) uses a synthetic Chrome-style command with 16 `-H` lines, `-b`, `--data-raw $'{"q":"a\'b"}'` and `--compressed`. Assert `notices.is_empty()` and that the headers are in source order.
  - `cmd_style_curl_never_yields_caret_url` (Review Focus 2) uses `curl ^"https://example.test/a^" ^\n -H ^"x: y^"`. Assert `Err(_)`, or `Ok` with `!request.url.contains('^')`.

- [ ] **Step 2: Run the tests and confirm they fail.** Run `cargo test -p plover-core --test curl`. Expected: FAIL, because `parse_curl` does not exist yet.

- [ ] **Step 3: Implement `parse_curl` in `crates/core/src/curl.rs`.**
  - Port the tokenizer and flag handling from Bruno's `parse-curl.js` and the body-mode choice from `utils/curl/index.js` and `content-type.js`.
  - Unknown flags still consume their value when the flag takes one, so that values do not become URLs. Keep a fixed table of known value-taking curl flags for this.
  - Query parsing reuses the URL query handling already used for `.bru` URLs, if core has one. Otherwise split on `&` and `=` without decoding, to match Bruno.

- [ ] **Step 4: Run the tests and confirm they pass.** Run `cargo test -p plover-core --test curl`. Expected: PASS.

- [ ] **Step 5: Commit** with message `feat(core): parse curl commands into requests`.

### Task 2: curl writer in core

**Files:**
- Modify: `crates/core/src/curl.rs`
- Test: `crates/core/tests/curl.rs`

**Interfaces:**
- Consumes: `ParsedCurl` and `parse_curl` (Task 1). `PreparedRequest` and `ResolvedRequest` from `crates/core/src/resolve.rs`.
- Produces:
  - `pub struct CurlCommand { pub method: Method, pub url: String, pub headers: Vec<(String, String)>, pub basic_auth: Option<(String, String)>, pub body: CurlBody }`
  - `pub enum CurlBody { None, Raw(String), Form(Vec<(String, String)>), Multipart(Vec<CurlPart>) }`, with `CurlPart::{Text{name,value}, File{name,path}}`
  - `impl From<&PreparedRequest> for CurlCommand`. This is the "as written" mode: scopes are merged and nothing is interpolated.
  - `impl From<&ResolvedRequest> for CurlCommand`. This is the "filled in" mode.
  - `pub fn to_curl(cmd: &CurlCommand) -> String`

- [ ] **Step 1: Write the failing tests.**
  - `writes_posix_shell_format` checks the exact output for GET with 2 headers: `curl -X GET 'https://example.test/a' \`, then `  -H 'a: 1' \`, then `  -H 'b: 2'`.
  - `single_quote_escaping` checks that a header value `it's` is written as `'it'\''s'`.
  - `round_trip_each_body_mode`: for `None`, `Json`, `Text`, `Xml`, `FormUrlEncoded`, `Multipart` (text and file) and `Basic` auth, `parse_curl(&to_curl(&cmd)).request` equals the source in method, URL, headers, body and auth.
  - `form_uses_data_urlencode_per_field` checks that form bodies are written with one `--data-urlencode 'a=1'` per field.

- [ ] **Step 2: Run the tests and confirm they fail.** Run `cargo test -p plover-core --test curl`.

- [ ] **Step 3: Implement the `From` impls and `to_curl`.** Use `--data-raw` for raw bodies, `-F` for multipart and `-u` for basic auth.

- [ ] **Step 4: Run the tests and confirm they pass.**

- [ ] **Step 5: Commit** with message `feat(core): write requests as curl commands`.

### Task 3: create request, folder and secret names in the store

**Files:**
- Modify: `crates/core/src/store.rs`
- Test: `crates/core/tests/workspace.rs` (store tests live here)

**Interfaces:**
- Produces:
  - `pub fn sanitize_name(name: &str) -> String` ports Bruno `@usebruno/common/utils/naming.ts` `sanitizeName`. It replaces `<>:"/\|?*` and control characters with `-`, trims leading spaces and hyphens, and trims trailing dots and spaces. It also caps the result at 200 bytes on a char boundary (*plan's call*) and turns reserved device names (`CON`, `PRN`, `AUX`, `NUL`, `COM0-9`, `LPT0-9`) into `<name>-`.
  - `pub fn unique_file_name(dir: &Path, name: &str) -> String` returns `name`, `name 2`, `name 3`, … (*plan's call*), whichever is the first `<n>.bru` that does not exist in `dir` (case-insensitive on Windows).
  - `pub fn create_request(root: &Path, folder_id: &str, name: &str, request: &HttpRequest) -> Result<(String, Stored<HttpRequest>), StoreError>`
    - Returns the new item ID and the stored request.
    - `folder_id` `""` is the root.
    - It writes `<sanitize_name(name)>.bru` with `create_new` semantics under the lock.
    - It fails with a new `StoreError::Exists(String)` when the file is already there.
    - `request.name` is set to `name`.
    - If `request.seq` is `0.0`, the next seq in the folder is used instead (max existing + 1).
    - The file is built from `RequestFile { doc: BruDocument::new(), request }.to_bru()`.
  - `pub fn create_folder(root: &Path, parent_id: &str, name: &str) -> Result<String, StoreError>` makes the directory and no `folder.bru`. It returns the new folder ID, or `Exists` if the folder is already there.
  - `pub fn add_secret_names(root: &Path, env: &str, names: &[String]) -> Result<ContentHash, StoreError>`
    - It adds the names to `vars:secret`, enabled, and leaves names that are already there unchanged.
    - It creates `environments/<env>.bru` (and `environments/`) when missing.
    - It keeps the diff minimal, like `save_environment`.

- [ ] **Step 1: Write the failing tests.**
  - `create_request_writes_bruno_readable_file`: create `orders/GET orders`, then `read_request` returns the same method and URL. `Collection::load` lists it, and the bytes parse with `RequestFile::parse`.
  - `create_request_existing_name_is_exists_error`.
  - `create_request_next_seq_in_folder`: with existing seqs 1 and 5, the new request gets 6.
  - `create_request_rejects_escape`: `folder_id` `"../x"` gives `OutsideRoot`.
  - `create_folder_then_request_inside`.
  - `add_secret_names_creates_env_file` and `add_secret_names_keeps_other_blocks_and_order`. The second asserts that only the `vars:secret` block lines change, using `changed_lines`-style counting.
  - `sanitize_name_matches_bruno` covers `"a/b:c"` → `"a-b-c"`, `" -x. "` → `"x"`, `"CON"` → `"CON-"`, and 300 × `"é"` → at most 200 bytes, still valid UTF-8.
  - `unique_file_name_numbers_collisions`.

- [ ] **Step 2: Run the tests and confirm they fail.** Run `cargo test -p plover-core --test workspace`.

- [ ] **Step 3: Implement the functions in `store.rs`.** Add `StoreError::Exists(String)`, with the message "`{0}` already exists".

- [ ] **Step 4: Run the tests and confirm they pass.**

- [ ] **Step 5: Commit** with message `feat(core): create requests, folders and secret names`.

### Task 4: scratch send in core

Rebase `feat/import-curl-har` on `main` first. See "Merge risk".

**Files:**
- Modify: `crates/core/src/runner.rs`, only `SendInput` and the start of `send_one_with`
- Modify: `crates/server/src/api/requests.rs`, only the `item_id:` line, to keep it compiling
- Test: `crates/core/tests/runner.rs`

**Interfaces:**
- Produces: `SendInput.item_id: Option<&'a str>`. `None` requires `draft: Some(_)` and sends with collection scope only:
  - It builds a synthetic `Item::Request { path: root.join(".plover-scratch.bru"), file }` with `folders: vec![]`. The file is never written.
  - It returns `SendError::NotFound("scratch request needs a draft")` when `draft` is `None`.

- [ ] **Step 1: Write the failing tests.**
  - `scratch_send_applies_collection_headers_not_folder_headers`: `collection.bru` sets `x-col: 1` and `folder.bru` sets `x-folder: 1`. A scratch send hits a local test server, and the received headers include `x-col` and exclude `x-folder`.
  - `scratch_send_runs_collection_pre_script`.
  - `scratch_send_without_draft_is_error`.
  - Existing `send_one` tests pass with `item_id: Some(..)`.

- [ ] **Step 2: Run the tests and confirm they fail.** Run `cargo test -p plover-core --test runner`.

- [ ] **Step 3: Implement the change.** Do not touch `Exec::execute`.

- [ ] **Step 4: Run the tests and confirm they pass.** Then run `cargo test --workspace`.

- [ ] **Step 5: Commit** with message `feat(core): send unsaved scratch requests with collection scope`.

### Task 5: HAR parsing and import planning in core

**Files:**
- Create: `crates/core/src/har.rs`
- Modify: `crates/core/src/lib.rs`
- Test: `crates/core/tests/har.rs` and `crates/core/tests/fixtures/har/*.har`. The fixtures are synthetic. Write them by hand: a Chrome-style one, a Firefox-style minimal one, and one with every secret kind.

**Interfaces:**
- Consumes: `sanitize_name` and `unique_file_name` (Task 3). The body-mode choice in `curl.rs` (Task 1); expose it as `pub(crate) fn body_from_text(mime: &str, text: &str) -> Body`.
- Produces:
  - `pub fn parse_har(text: &str) -> Result<Har, HarError>`. Serde structs take only the HAR 1.2 fields the import needs, and every optional field is `#[serde(default)]`.
  - `pub struct HarRow { pub index: usize, pub method: String, pub url: String, pub host: String, pub status: Option<u16>, pub mime: String, pub size: Option<u64>, pub has_secrets: bool, pub hidden: Option<HiddenReason>, pub duplicates: usize }`
  - `pub enum HiddenReason { Image, Script, Style, Font, Media, SourceMap, Preflight, DataUrl, Extension }`
  - `pub fn preview(har: &Har) -> Vec<HarRow>`. Rows are in capture order. Each row of identical requests (same method, URL and body) carries `duplicates`, and only the first index of the group is a row.
  - `pub struct ExistingSecrets { pub values: BTreeMap<String, String>, pub plain_names: BTreeSet<String> }`. These are the target environment's current secret values and its non-secret variable names.
  - `pub fn plan_import(har: &Har, entries: &[usize], existing_names: &BTreeSet<String>, start_seq: f64, secrets: &ExistingSecrets) -> ImportPlan`
  - `pub struct ImportPlan { pub requests: Vec<(String /*name*/, HttpRequest)>, pub new_secrets: BTreeMap<String, String>, pub notices: Vec<String> }`
  - This step is pure: no I/O. `new_secrets` holds only names that need to be created.

- [ ] **Step 1: Write the failing tests.**
  - `preview_hides_assets_by_default` covers images, JS, CSS, fonts, media, `.map`, `OPTIONS`, `data:` and `chrome-extension://`. Each row gets the matching `hidden` reason; the reason comes from the MIME type, then the file extension.
  - `preview_collapses_duplicates` checks that 3 identical GETs become 1 row with `duplicates == 3`.
  - `bearer_becomes_auth_bearer_with_secret`: `Authorization: Bearer tok-AAAA` becomes `Auth::Bearer{token:"{{authToken}}"}`, with `new_secrets {authToken: "tok-AAAA"}`.
  - `basic_becomes_auth_basic_with_two_secrets` gives `{{authUser}}` and `{{authPassword}}`.
  - `other_scheme_becomes_header_var` gives `{{authorization}}`. `Cookie` becomes `{{cookie}}`.
  - `listed_headers_and_query_params_become_secrets` covers `X-Api-Key`, `X-CSRF-Token` and `?access_token=`.
  - `same_value_shares_variable_different_value_numbers`: 2 entries with the same token share `authToken`, and a 3rd entry with a different token gets `authToken2`.
  - `existing_secret_reused_plain_name_skipped`: an existing secret `authToken` with the same value is reused. A plain variable `authToken` makes the import use `authToken2`.
  - `body_credentials_become_secrets`:
    - Form `password=p&user=u` becomes `password={{password}}`.
    - JSON `{"auth":{"client_secret":"s","n":5},"token":7}` becomes `{"auth":{"client_secret":"{{clientSecret}}","n":5},"token":7}`. The number stays, and the result is valid JSON.
    - `newPassword` is untouched.
  - `xml_body_with_jwt_flags_row`: `has_secrets` is true and the body is unchanged.
  - `pseudo_and_executor_headers_dropped` covers `:authority`, `Host`, `Content-Length`, `Connection` and `Accept-Encoding`.
  - `no_secret_value_in_any_written_bru`: for every planned request, `RequestFile{doc: BruDocument::new(), request}.to_bru()` contains none of the fixture's secret values.
  - `no_credentials_gives_auth_inherit`.
  - `seq_starts_after_existing`: with `start_seq` 7, the seqs are 8, 9, ….
  - `minimal_har_imports` (Review Focus 3): a Firefox-style minimal HAR with a POST without `postData`, an empty `mimeType` and no `_` fields gives `Ok`, with a plan of 2 requests.
  - `import_names_are_valid_and_unique` (Review Focus 4): the paths are 300 characters, `/`, `/CON`, `/a?:*|b`, and two `/x` entries. Every name satisfies Bruno's `validateName` rules (port them as a test helper), and all names are distinct. The naming rule, *the plan's call*, is the method, a space, then the URL path without the leading `/`, with `/` replaced by `-`. If that is empty, use the host. Then apply `sanitize_name` and make the name unique among `existing_names` and earlier plan names.

- [ ] **Step 2: Run the tests and confirm they fail.** Run `cargo test -p plover-core --test har`.

- [ ] **Step 3: Implement `har.rs`.**

- [ ] **Step 4: Run the tests and confirm they pass.**

- [ ] **Step 5: Commit** with message `feat(core): preview and plan HAR imports`.

### Task 6: server curl endpoints

**Files:**
- Create: `crates/server/src/api/curl.rs`
- Modify: `crates/server/src/api/mod.rs` (routes) and `crates/server/src/api/dto.rs` (new block at the end)
- Regenerate: `frontend/src/api/types.ts`
- Test: `crates/server/tests/api.rs`

**Interfaces:**
- Consumes: `parse_curl`, `CurlCommand` and `to_curl` (Tasks 1–2). `resolve::prepare` and `resolve::finalize` with `open.env_overrides(env)`.
- Produces:
  - `POST /api/curl/parse` takes `CurlParseBody { text }` and returns `CurlParsedDto { request: HttpRequestDto, notices: CurlNoticeDto[] }`. On failure it returns 400 `invalid_curl` with a message.
  - `POST /api/collections/{id}/curl` takes `CopyCurlBody { item: Option<String>, draft: Option<HttpRequestDto>, env: Option<String>, resolve: bool }` and returns `CurlTextDto { text, containsSecret: bool, omitted: string[] }`.
    - With `resolve: false`, it uses `prepare` (scopes merged, no interpolation).
    - With `resolve: true`, it uses `finalize` with the environment values and secrets. No scripts run.
    - `containsSecret` is true when any secret value of `env` appears in `text`.
    - `omitted` lists `Auth::Unsupported` and `Body::Unsupported` parts that were left out.

- [ ] **Step 1: Write the failing tests.**
  - `curl_parse_returns_request_and_notices` checks the `-k` notice text.
  - `curl_parse_invalid_is_400`.
  - `copy_as_written_keeps_vars`: the URL `{{host}}/a` stays literal.
  - `copy_filled_in_includes_folder_auth_and_flags_secret`: folder bearer `{{token}}` plus secret `token` gives `Authorization: Bearer <value>` and `containsSecret: true`.
  - `copy_unsupported_auth_is_omitted`.
  - `curl_routes_need_token`.

- [ ] **Step 2: Run the tests and confirm they fail.** Run `cargo test -p plover-server --test api`.

- [ ] **Step 3: Implement the handlers.** Add `CurlError → ApiError(400, "invalid_curl")`. Regenerate the bindings.

- [ ] **Step 4: Run the tests and confirm they pass.** Run `cargo test -p plover-server`, which includes `bindings`.

- [ ] **Step 5: Commit** with message `feat(server): parse curl and copy requests as curl`.

### Task 7: server create-request and scratch send

Rebase on `main` first.

**Files:**
- Modify: `crates/server/src/api/requests.rs`, `api/mod.rs`, `api/dto.rs`; regenerate `types.ts`
- Test: `crates/server/tests/api.rs`

**Interfaces:**
- Consumes: `store::create_request` (Task 3) and `SendInput.item_id: Option` (Task 4).
- Produces:
  - `POST /api/collections/{id}/requests/new` takes `CreateRequestBody { folder: String, name: String, request: HttpRequestDto }` and returns `CreatedRequestDto { item, request, hash }`.
    - It returns 409 `exists` when the file is already there, and 400 `bad_item` when the path escapes.
    - It reloads the collection afterwards.
  - `SendBody.item: Option<String>`. `null` means a scratch send, and `null` without a draft gives 400 `bad_item`.

- [ ] **Step 1: Write the failing tests.**
  - `create_request_then_tree_lists_it`.
  - `create_request_existing_is_409_exists`.
  - `scratch_send_null_item_uses_collection_scope`.
  - `scratch_send_null_item_without_draft_is_400`.

- [ ] **Step 2: Run the tests and confirm they fail.**

- [ ] **Step 3: Implement.** Map `StoreError::Exists` to 409 `exists` in `impl From<StoreError> for ApiError`.

- [ ] **Step 4: Run the tests and confirm they pass,** bindings included.

- [ ] **Step 5: Commit** with message `feat(server): create requests and send scratch drafts`.

### Task 8: server HAR endpoints

**Files:**
- Create: `crates/server/src/api/import.rs`
- Modify: `api/mod.rs` and `api/dto.rs`; regenerate `types.ts`
- Test: `crates/server/tests/api.rs`, using `PLOVER_KEYCHAIN_SERVICE=dev.plover.test` and a temporary state dir, as in the existing secrets tests

**Interfaces:**
- Consumes: `parse_har`, `preview` and `plan_import` (Task 5). `create_folder`, `create_request` and `add_secret_names` (Task 3). `OpenCollection::update_secrets`.
- Produces:
  - `POST /api/collections/{id}/import/har/preview` takes `HarPreviewBody { har: String }` and returns `HarRowDto[]` (fields as in `HarRow`, camelCase). It returns 400 `invalid_har`, and 413 (existing body limit) above 64 MiB.
  - `POST /api/collections/{id}/import/har` takes `HarImportBody { har, entries: number[], folder: String, newFolder: Option<String>, env: Option<String> }` and returns `HarImportResultDto { items: string[], secretNames: string[], env: string, notices: string[], failedAt: Option<String> }`.
    - `env: None` means `Imported`, created on demand.
    - The write order is fixed: `update_secrets`, then `add_secret_names`, then `create_folder` (if `newFolder`), then `create_request` for each entry.
    - There is no rollback. On the first failure, the handler stops and returns 200 with `failedAt` set and `items` listing what was written. *Plan's call*: 200 rather than 5xx, so the UI can show which files were written.
    - The handler reloads the collection at the end.

- [ ] **Step 1: Write the failing tests.**
  - `har_preview_rows_and_hidden_flags`.
  - `har_import_writes_files_and_secrets`. The files exist. The environment's `vars:secret` has the names. `GET environments` reports the secret status as set. `grep` over all written `.bru` files finds no fixture secret value.
  - `har_import_without_env_creates_imported_env`.
  - `har_import_into_new_folder`.
  - `har_over_limit_is_413`.
  - `preview_does_not_echo_response_content` (Review Focus 5). A synthetic HAR has 500 entries, each with a 64 KiB `response.content.text`. The preview JSON is under 1 MiB and does not contain the marker string placed in the content.

- [ ] **Step 2: Run the tests and confirm they fail.**

- [ ] **Step 3: Implement.**

- [ ] **Step 4: Run the tests and confirm they pass,** bindings included.

- [ ] **Step 5: Commit** with message `feat(server): preview and import HAR files`.

### Task 9: frontend API client and scratch tabs in the store

**Files:**
- Modify: `frontend/src/api/client.ts`, `frontend/src/store/tabs.ts`, `frontend/src/store/uiState.ts`
- Test: `frontend/src/store/tabs.test.ts`, `frontend/src/api/client.test.ts`

**Interfaces:**
- Consumes: the endpoints from Tasks 6–8 (types from `types.ts`).
- Produces:
  - `ApiClient` gains these methods:
    - `parseCurl(text): Promise<CurlParsedDto>`
    - `copyCurl(id, body: CopyCurlBody): Promise<CurlTextDto>`
    - `createRequest(id, folder, name, request): Promise<CreatedRequestDto>`
    - `previewHar(id, har): Promise<HarRowDto[]>`
    - `importHar(id, body: HarImportBody): Promise<HarImportResultDto>`
    - `send` now accepts `item: string | null`.
  - `Tab.item: string | null` (null = scratch); scratch `key` = `` `${collectionId}:scratch:${n}` ``.
  - `TabsState.newScratch(collectionId: string): string` returns the key of a tab named "Untitled" with method `GET` and an empty `saved`.
  - `TabsState.saveScratch(key, folder: string, name: string): Promise<"saved" | "exists">` turns the scratch tab into a file tab, with the new key, `item` and `hash`.
  - `TabsState.isDirty(key)` is true for a scratch tab whose draft differs from the empty request.
  - `TabsState.applyCurl(key, parsed: HttpRequestDto): HttpRequestDto` replaces method, URL, query, headers, body and auth, keeps the rest, and returns the previous draft for Undo.
  - `hasScratchContent(): boolean`
  - `uiState` save filters out `item === null`.

- [ ] **Step 1: Write the failing tests.**
  - `newScratch opens Untitled GET tab`.
  - `send on scratch passes item null and draft`.
  - `saveScratch creates request and rekeys tab`.
  - `saveScratch exists keeps tab and returns "exists"`.
  - `applyCurl keeps name scripts tests and returns previous draft`.
  - `ui state never contains scratch tabs`.
  - Client tests check the exact paths and bodies for the 5 new methods.

- [ ] **Step 2: Run the tests and confirm they fail.** Run `cd frontend && npm test -- tabs client`.

- [ ] **Step 3: Implement.**

- [ ] **Step 4: Run the tests and confirm they pass.** Also run `npm run typecheck && npm run lint`.

- [ ] **Step 5: Commit** with message `feat(ui): scratch tabs and API client for curl and HAR`.

### Task 10: UI for New request, Save dialog and close prompts

**Files:**
- Modify: `frontend/src/components/SidebarHeader.tsx`, `frontend/src/components/RequestTabs.tsx` (close prompt), `frontend/src/components/request/RequestEditor.tsx` (Save on scratch opens the dialog), `frontend/src/App.tsx` (`beforeunload`)
- Create: `frontend/src/components/SaveRequestDialog.tsx`, built on the existing `Modal`
- Test: `frontend/src/components/scratch.test.tsx`

**Interfaces:**
- Consumes: `newScratch`, `saveScratch`, `isDirty` and `hasScratchContent` (Task 9). Folder nodes come from `useWorkspace` (tree).
- Produces:
  - A "New request" icon button in `SidebarHeader`, next to "+". When no collection is active it is disabled, with the title "Open a collection first".
  - `SaveRequestDialog` has a folder picker (root allowed) and a name field.
    - The name is prefilled as the method plus the last URL path segment, for example `GET orders`.
    - On `"exists"` it shows "A request with this name already exists" under the name field.

- [ ] **Step 1: Write the failing tests.**
  - `new request button disabled without collection`.
  - `new request opens Untitled tab`.
  - `save on scratch opens dialog with prefilled name`.
  - `exists shows inline error`.
  - `closing scratch with content asks Discard this request?`. Cancel keeps the tab; confirm closes it.
  - `beforeunload set only when scratch has content`.
  - An a11y check: the dialog has a labelled folder select and name input, and focus returns to the trigger.

- [ ] **Step 2: Run the tests and confirm they fail.**

- [ ] **Step 3: Implement.** Follow `docs/ui-ux-review.md`, and reuse `Modal` and the icons from `icons.tsx`. Add a `NewRequestIcon` if none fits.

- [ ] **Step 4: Run the tests and confirm they pass,** plus typecheck and lint.

- [ ] **Step 5: Commit** with message `feat(ui): new request scratch tab with save dialog`.

### Task 11: UI for pasting curl in the URL bar

**Files:**
- Modify: `frontend/src/components/request/UrlBar.tsx` and `RequestEditor.tsx` (banner placement)
- Create: `frontend/src/components/request/CurlBanner.tsx`, which uses the existing `Banner.tsx`
- Test: `frontend/src/components/request/curl-paste.test.tsx`

**Interfaces:**
- Consumes: `api().parseCurl`, `applyCurl` (Task 9), and `api().setSecrets` plus `saveEnvironment` (existing) for "Move to secret".
- Produces:
  - `UrlBar` takes a new prop `onCurl(text: string): void`. On paste, if the clipboard text matches `/^\s*curl\s/i`, it calls `preventDefault()` and then `onCurl(text)`. Any other paste is normal.
  - `CurlBanner` shows "Filled from cURL", with a "Not imported" list of `option` and `reason` for each notice, an Undo button, and a "Move to secret" button.
    - "Move to secret" appears when an `Authorization` or `Cookie` header is present.
    - It replaces the header value with `{{authorization}}` or `{{cookie}}`, numbered when the name is taken.
    - It adds the name to the selected environment's `vars:secret` through `saveEnvironment`, then stores the value with `setSecrets`.
    - With no environment selected, the button is disabled, with the title "Select an environment first" (*plan's call*).

- [ ] **Step 1: Write the failing tests.**
  - `pasting curl fills the draft and shows banner`.
  - `non-curl paste is normal`.
  - `notices listed with exact -k text`.
  - `undo restores previous draft`.
  - `invalid curl shows error and leaves draft`.
  - `move to secret replaces header and calls setSecrets with value`. Assert that the store draft holds `{{authorization}}` and no longer the value.

- [ ] **Step 2: Run the tests and confirm they fail.**

- [ ] **Step 3: Implement.**

- [ ] **Step 4: Run the tests and confirm they pass,** plus typecheck and lint.

- [ ] **Step 5: Commit** with message `feat(ui): paste curl into the URL bar`.

### Task 12: UI for Copy as cURL

**Files:**
- Modify: `frontend/src/components/request/UrlBar.tsx` (a toolbar button next to Save) or `RequestEditor.tsx`
- Create: `frontend/src/components/request/CopyCurlMenu.tsx`
- Test: `frontend/src/components/request/copy-curl.test.tsx`

**Interfaces:**
- Consumes: `api().copyCurl` (Task 6).
- Produces: a "Copy as cURL" split button with two choices, "As written" (the default) and "With variables filled in".
  - If `containsSecret` is true, it asks "This command contains secret values. Copy anyway?" before calling `navigator.clipboard.writeText`.
  - If `omitted` is not empty, it shows a toast listing what was left out.
  - It works on scratch tabs too, by sending `item: null` with the draft.

- [ ] **Step 1: Write the failing tests.**
  - `copy as written writes text to clipboard`.
  - `filled in with secret asks before copying`. Cancelling means nothing is written.
  - `omitted parts toast`.
  - `works on scratch tab`.

- [ ] **Step 2: Run the tests and confirm they fail.**

- [ ] **Step 3: Implement.**

- [ ] **Step 4: Run the tests and confirm they pass,** plus typecheck and lint.

- [ ] **Step 5: Commit** with message `feat(ui): copy requests as curl`.

### Task 13: UI for the Import HAR dialog

**Files:**
- Create: `frontend/src/components/ImportHarDialog.tsx`, built on `Modal`
- Modify: `frontend/src/components/SidebarHeader.tsx` (an "Import HAR…" entry), `frontend/src/store/workspace.ts` (refresh the tree after import)
- Test: `frontend/src/components/import-har.test.tsx`

**Interfaces:**
- Consumes: `previewHar` and `importHar` (Task 9). The environments come from `api().environments`.
- Produces: a dialog with a file input (`accept=".har,application/json"`) and these parts:
  - Host chips ordered by request count.
  - A "Show hidden" toggle.
  - A table with checkboxes. Hidden rows are unticked and not shown until the toggle is on. Rows are marked `×N` for duplicates and with a warning icon when `hasSecrets` is true.
  - An "Import into" folder picker, defaulting to the focused folder in the sidebar or the root, with a "new subfolder" name prefilled as `captured-<first host>`.
  - An environment select (default: the selected environment, or "Imported (new)").
  - A summary of the secret variables to create, by name only. *Plan's call*: the names come from a dry run, which the dialog asks for by calling preview and import planning. To avoid a third endpoint, the client predicts the names from `hasSecrets` rows: it shows "N secret values will move into environment X", and lists the actual names after import from `secretNames`. The spec's "the dialog lists the secret variables it will create" is satisfied after the import, not before. See Ambiguity 1.
  - After import, a result line with the count of files and secrets, plus `failedAt` if set. The tree refreshes.

- [ ] **Step 1: Write the failing tests.**
  - `assets hidden and unticked by default`.
  - `domain chip filters rows`.
  - `show hidden reveals rows`.
  - `import sends ticked entries folder newFolder env`.
  - `result lists secret names never values`. Assert that the rendered text does not contain the fixture token.
  - `failedAt shown`.
  - `file over 64 MiB rejected client side with message`.

- [ ] **Step 2: Run the tests and confirm they fail.**

- [ ] **Step 3: Implement.**

- [ ] **Step 4: Run the tests and confirm they pass,** plus typecheck and lint.

- [ ] **Step 5: Commit** with message `feat(ui): import HAR files into a folder`.

### Task 14: docs page and live check

**Files:**
- Create: `docs/capture-with-claude.md`
- Modify: `README.md` (a link in the features or docs section)

- [ ] **Step 1: Write the docs page,** covering the three points in spec section 5:
  - The Playwright `recordHar` route. Include a 10-line example script, using `browser.newContext({ recordHar: { path: "flow.har", content: "embed" } })` and `context.close()` to flush the file.
  - The DevTools route: Network → "Save all as HAR with content".
  - The test-account warning, then import, then deleting the `.har` file.
  - Name only tools you have checked actually write HAR; Playwright's `recordHar` is documented to.

- [ ] **Step 2: Do a live check with `agent-browser`** against `plv serve` on a copy of `fixtures/phase3-chain`, with a temporary `PLOVER_STATE_DIR`:
  - Paste a synthetic curl that includes `-k`. The banner shows the exact notice.
  - Send.
  - Save into a folder. The `.bru` file exists, and Bruno CLI 4.3.0 `bru run` on that file parses it.
  - Use Copy as cURL in both modes.
  - Import the synthetic Chrome-style HAR fixture. Then run `grep -r tok- <copy>` over the `.bru` files; it finds nothing.
  - Take screenshots at 1440×900, in light and dark mode.

- [ ] **Step 3: Run the full gate:** `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cd frontend && npm run typecheck && npm run lint && npm test && npm run build && npm run check:no-cdn`.

- [ ] **Step 4: Commit** with message `docs: capture a browser flow with Claude and import it`.

---

## Self-review

- **Spec coverage:**
  - curl paste: Tasks 1, 6 and 11.
  - `-k` notice: Tasks 1, 6, 11 and 14.
  - Unsupported options reported: Tasks 1 and 11.
  - Move to secret on paste: Task 11.
  - Scratch tab (open, send, save, not remembered, close prompt, beforeunload): Tasks 4, 7, 9 and 10.
  - Copy as cURL (two modes, secret prompt, omitted toast): Tasks 2, 6 and 12.
  - HAR (preview, filters, duplicates, folder, env, `Imported.bru`, header, query and body secrets, reuse and numbering, no rollback with `failedAt`, 64 MiB): Tasks 5, 8 and 13.
  - New store pieces: Task 3.
  - Docs page: Task 14.
  - Non-goals (no CLI, no proxy): nothing is planned for them.
- **Type consistency:**
  - `ParsedCurl`, `CurlNotice`, `CurlCommand`, `HarRow`, `ImportPlan` and `ExistingSecrets` are defined once (Tasks 1, 2 and 5) and used by name in later tasks.
  - The DTO names are `CurlParsedDto`, `CurlNoticeDto`, `CopyCurlBody`, `CurlTextDto`, `CreateRequestBody`, `CreatedRequestDto`, `HarPreviewBody`, `HarRowDto`, `HarImportBody` and `HarImportResultDto`. They are the same in Tasks 6–9 and 13.
  - `StoreError::Exists` maps to 409 `exists` (Tasks 3 and 7). The UI's `"exists"` result comes from that code (Task 9).
- **Proportion:** the plan gives signatures, test names and assertions. It has no function bodies except the exact copy strings the spec fixes.

## Ambiguities left for the user / reviewer

1. **Secret names in the HAR dialog before import.** The spec says the dialog lists the secret variables it will create. Doing that before the import needs either a dry-run endpoint or planning in the client. This plan shows a count before the import and the names after it (Task 13). If the names must appear before the import, add `POST …/import/har/plan`, which returns `new_secrets` names only, and put it in Task 8.
2. **Import HAR on folders.** The spec puts it on the folder context menu, but right-click on a folder already runs it (`Sidebar.tsx` `onContextMenu`). This plan uses the sidebar header entry, defaulting the target folder to the focused folder, rather than adding a context menu. A real folder context menu would change existing behaviour.
3. **Choices this plan made that the spec leaves open.**
   - Duplicate names become `name 2`, `name 3`.
   - File names are capped at 200 bytes.
   - "Move to secret" needs a selected environment; it does not create `Imported.bru`.
   - A partial HAR import returns 200 with `failedAt`.
