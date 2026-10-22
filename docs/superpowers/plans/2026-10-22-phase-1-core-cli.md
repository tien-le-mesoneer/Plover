# Phase 1 — core + CLI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `brs-core` library that loads Bruno collections losslessly and executes HTTP requests, plus a `bru-rs run` CLI.

**Architecture:** `.bru` files are parsed in two layers. The syntax layer (`bru::syntax`) splits a file into blocks and keeps each block's exact source text; the typed layer (`bru::model`) maps blocks to `HttpRequest`/folder/collection/environment types and writes changes back by replacing only blocks whose meaning changed. A pure resolver merges scopes and interpolates variables; the executor sends a `ResolvedRequest` with reqwest; the runner walks the tree and emits `RunEvent`s that the CLI prints.

**Tech Stack:** Rust stable (1.99), `pest`/`pest_derive`, `serde`/`serde_json`, `thiserror`, `tracing`, `tokio`, `reqwest` (rustls, cookies, multipart), `reqwest_cookie_store`, `base64`, `clap` (derive), dev: `wiremock`, `similar` (diff assertions), `assert_cmd`.

**Spec:** `docs/PRD.md` (sections 3, 5, 6 · Phase 1). Reference implementation: Bruno `packages/bruno-lang/v2/src/bruToJson.js`, `jsonToBru.js`, `envToJson.js`, `bruno-cli/src/utils/collection.js`.

## Global Constraints

- Crates: `crates/core` (package `brs-core`, lib `brs_core`), `crates/cli` (package `brs-cli`, binary `bru-rs`). No `server`/`desktop` crates yet.
- All logic in `brs-core`; `brs-cli` only parses args, calls `brs_core::runner::run`, prints, sets exit code.
- Library code returns `Result` with `thiserror` enums; no `unwrap`/`expect`/`panic!` outside tests.
- Variable precedence (highest first): runtime > request > folder (innermost first) > environment > collection. One constant: `vars::PRECEDENCE`.
- Unknown variables stay as written (`{{missing}}` sent literally).
- HTTP 4xx/5xx is not a failure. Failure = transport error or resolve error. An unparsable `.bru` file is skipped with a warning (matches Bruno `brunoSkippedFiles`) and does not affect the exit code.
- `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` pass at the end of every task.
- Commits: Conventional Commits, one per task.

## Review Focus

1. **Files from newer Bruno versions** (unknown block names such as `grpc`, `ws`, `example`, `auth:oauth2:additional_params:*`, or something we've never seen) must load, round-trip byte-identical, and not abort the collection. → Task 1 `unknown_block_preserved`, Task 5 `loads_bruno_testbench`.
2. **CRLF line endings, no trailing newline, UTF-8 BOM** must survive round-trip byte-for-byte. → Task 1 `crlf_bom_and_no_trailing_newline`.
3. **Disabled entries (`~key`)** are preserved on save but never sent. → Task 4 `disabled_entries_kept_not_enabled`, Task 7 `disabled_headers_not_sent`.
4. **Self-referencing or cyclic variables** (`a={{b}}`, `b={{a}}`) terminate and leave the placeholder. → Task 6 `cycle_terminates`.
5. **One broken `.bru` file** in a collection is reported as skipped with its path and parse error; the rest of the run continues and the exit code depends only on the other requests (Bruno behavior). → Task 9 `broken_file_skipped_run_continues`.

---

### Task 1: Workspace + lossless block scanner

**Files:**
- Create: `Cargo.toml` (workspace, `resolver = "2"`, shared `[workspace.dependencies]`), `rust-toolchain.toml` (`channel = "stable"`), `.gitignore` (`/target`)
- Create: `crates/core/Cargo.toml`, `crates/core/src/lib.rs`, `crates/core/src/bru/mod.rs`, `crates/core/src/bru/grammar.pest`, `crates/core/src/bru/syntax.rs`, `crates/core/src/error.rs`
- Create: `fixtures/bruno-tests/` — copy of Bruno `packages/bruno-tests/` (`.bru`, `bruno.json`, `environments/`, binary/body files referenced by requests; omit `node_modules`, `package-lock.json`), plus `fixtures/bruno-tests/LICENSE` (Bruno MIT license) and `fixtures/bruno-lang/` (Bruno `packages/bruno-lang/v2/tests/fixtures/*.bru`)
- Test: `crates/core/src/bru/syntax.rs` (unit), `crates/core/tests/roundtrip.rs` (golden)

**Interfaces:**
- Produces:
  ```rust
  pub struct BruDocument { /* Vec<Segment> */ }
  enum Segment { Trivia(String), Block(Block) }      // private
  pub struct Block { pub name: String, raw: String, pub content: BlockContent }
  pub enum BlockContent { Dict(Vec<Pair>), Text(String), List(Vec<String>), Opaque }
  impl BruDocument {
      pub fn parse(src: &str) -> Result<BruDocument, SyntaxError>;
      pub fn write(&self) -> String;
      pub fn blocks(&self) -> impl Iterator<Item = &Block>;
      pub fn block(&self, name: &str) -> Option<&Block>; // first match
  }
  impl Block { pub fn raw(&self) -> &str; }
  pub struct SyntaxError { pub line: usize, pub column: usize, pub message: String } // thiserror
  ```
  In this task `content` is always `Opaque`; Task 2 fills it.

- [ ] **Step 1: Write failing tests** in `syntax.rs`:
  - `roundtrip_simple`: `"meta {\n  name: a\n  seq: 1\n}\n\nget {\n  url: http://x\n}\n"` → `write()` equals input; `blocks().map(|b| &b.name)` == `["meta", "get"]`.
  - `crlf_bom_and_no_trailing_newline`: `"\u{feff}meta {\r\n  name: a\r\n}"` round-trips exactly.
  - `unknown_block_preserved`: `"grpc {\n  url: x\n}\n\nfuture:thing {\n  anything goes\n}\n"` round-trips; names `["grpc", "future:thing"]`.
  - `text_block_with_braces`: `"body:json {\n  {\n    \"a\": {}\n  }\n}\n"` is one block; inner `}` lines indented, so not the terminator.
  - `list_block`: `"vars:secret [\n  token\n]\n"` is one block named `vars:secret`.
  - `syntax_error_has_position`: `"meta {\n  name: a\n"` (unterminated) → `Err` whose `line` and `column` are ≥ 1 and whose `message` names the unterminated block `meta`.
- [ ] **Step 2: Golden test** `crates/core/tests/roundtrip.rs::all_fixtures_roundtrip_byte_identical`: walk `fixtures/` for `*.bru`, assert `BruDocument::parse(&s)?.write() == s` for each; print failing paths; assert at least 280 files were checked.
- [ ] **Step 3: Run** `cargo test -p brs-core` → FAIL (types missing).
- [ ] **Step 4: Implement grammar + scanner.** Top-level grammar: `file = SOI ~ (trivia | block)* ~ EOI`; `block = name ~ (" " | "\t")* ~ ("{" ~ brace_body ~ nl ~ "}" | "[" ~ list_body ~ nl ~ "]")`; `nl = "\r"? ~ "\n"`; a block ends at the first line that is exactly `}` (or `]`) at column 0 — this is Bruno's `tagend`. `name` = chars except whitespace, `{`, `[`. Trivia = whitespace/BOM between blocks, kept verbatim. Store each block's full source slice in `raw`; `write()` concatenates segments.
- [ ] **Step 5: Run** `cargo test -p brs-core` → PASS (all fixtures).
- [ ] **Step 6: Commit** `feat(core): lossless .bru block scanner with golden fixtures`

### Task 2: Block content parsing (dictionary, text, list)

**Files:**
- Modify: `crates/core/src/bru/grammar.pest`, `crates/core/src/bru/syntax.rs`
- Create: `crates/core/src/bru/kind.rs`
- Test: `crates/core/src/bru/syntax.rs` (unit), `crates/core/tests/roundtrip.rs`

**Interfaces:**
- Consumes: Task 1 `Block`, `BlockContent`.
- Produces:
  ```rust
  pub struct Pair { pub key: String, pub value: PairValue, pub enabled: bool, pub annotations: Vec<Annotation> }
  pub enum PairValue { Single(String), Multiline { text: String, content_type: Option<String> }, List(Vec<String>) }
  pub struct Annotation { pub name: String, pub arg: Option<String> }
  pub enum BlockKind { Dict, AssertDict, Text, List, Opaque }
  pub fn block_kind(name: &str, raw_opener: char) -> BlockKind; // in kind.rs
  impl BlockContent { pub fn pairs(&self) -> &[Pair]; pub fn text(&self) -> Option<&str>; }
  ```
- Kind table (from `bruToJson.js`/`envToJson.js`): **Text** = `body`, `body:json`, `body:text`, `body:xml`, `body:sparql`, `body:graphql`, `body:graphql:vars`, `script:*`, `tests`, `docs`; **AssertDict** = `assert`; **Opaque** = `example` and any name not in the known list; **List** = any block opened with `[`; **Dict** = every other known name (`meta`, `get`…`trace`, `http`, `grpc`, `ws`, `query`, `params:query`, `params:path`, `headers`, `metadata`, `auth`, `auth:*`, `body:form-urlencoded`, `body:multipart-form`, `body:file`, `body:grpc`, `body:ws`, `vars`, `vars:pre-request`, `vars:post-response`, `settings`, `app`).
- Semantics to match Bruno exactly:
  - Key: leading `~` → `enabled = false`, stripped. Quoted key `"…"` with `\"` unescaped. Unquoted key is trimmed. Assert keys may contain spaces (`res.status: eq 200` → key `res.status`).
  - Value: text after `:` trimmed (`singlelinevalue.trim()`). `'''`-delimited multiline value: each inner line has its first 4 characters removed (`line.slice(4)`); optional trailing `@contentType(x)`. `[`…`]` value → list of trimmed items.
  - Annotation lines before a pair: `@name`, `@name(arg)`, `@name('…')`, `@name("…")` (`\` escapes), `@name('''…''')`.
  - Text block: remove leading newlines, then remove up to 2 leading spaces per line (`outdentString`).
  - A block whose content fails to parse becomes `Opaque` plus a `tracing::warn!`; it never fails the document.

- [ ] **Step 1: Failing tests**, using `fixtures/bruno-lang/request.bru`:
  - `query_params_quoted_and_disabled`: `params:query` pairs include `("key with spaces","is allowed",true)`, `("nested escaped \"quote\"","is allowed",true)`, `("disabled:colon:parameter","is allowed",false)`, `("message","hello",false)`.
  - `meta_tags_list`: `meta` pair `tags` = `List(["foo","bar"])`.
  - `annotations_on_headers` (from `fixtures/bruno-tests/collection/collection.bru`): first header `check` has annotation `description` with arg `"asd"`; `vars:pre-request` pair `coll_num` has annotation `number`, no arg.
  - `multiline_value`: `coll_obj` value = `Multiline { text: "{\"scope\":\"collection\"}", content_type: None }`.
  - `json_body_outdented`: `"body:json {\n  {\n    \"a\": 1\n  }\n}\n"` → `text() == Some("{\n  \"a\": 1\n}")`.
  - `assert_keys_with_spaces`: `"assert {\n  res.status: eq 200\n  res.body.length: gt 0\n}\n"` → keys `["res.status","res.body.length"]`, values `["eq 200","gt 0"]`.
- [ ] **Step 2: Golden addition** `all_fixtures_have_no_unexpected_opaque`: for every fixture, every block with a known name parses to a non-`Opaque` content (list offenders).
- [ ] **Step 3: Run** → FAIL. **Step 4: Implement.** **Step 5: Run** `cargo test -p brs-core` → PASS.
- [ ] **Step 6: Commit** `feat(core): parse .bru dictionary, text and list blocks`

### Task 3: Canonical rendering + minimal-diff block edits

**Files:**
- Create: `crates/core/src/bru/render.rs`
- Modify: `crates/core/src/bru/syntax.rs`
- Test: `crates/core/src/bru/render.rs`, `crates/core/tests/roundtrip.rs`

**Interfaces:**
- Produces:
  ```rust
  pub fn render_block(name: &str, content: &BlockContent) -> String; // canonical Bruno form, no trailing blank line
  impl BruDocument {
      pub fn set_block(&mut self, name: &str, content: BlockContent); // first block with name
      pub fn remove_block(&mut self, name: &str);
      pub fn new() -> BruDocument;
  }
  ```
- Rules (from `jsonToBru.js`): 2-space indent for pairs; `~` prefix for disabled; quote a key when it contains `:`, whitespace, `{`, `}`, `"` or starts with `~`, escaping `"` as `\"`; annotations on their own line above the pair (`  @name(arg)`; arg containing newline → `'''` form); multiline value → `'''` newline, lines indented 4 spaces, closing `  '''`; list value `key: [\n    a\n  ]`; text block content indented 2 spaces per line; list block items indented 2 spaces.
- `set_block`: if the existing block's parsed `content == new content`, keep `raw` untouched (no-op). Otherwise replace `raw` with `render_block`. A new block is appended after the last block, separated by `"\n\n"`; preserve the document's line ending (CRLF if the first newline was CRLF). `remove_block` also removes the trivia immediately before it.

- [ ] **Step 1: Failing tests:**
  - `render_parse_is_identity_on_content`: for every fixture block with non-`Opaque` content, `parse(render_block(name, c)).content == c`.
  - `unchanged_set_block_is_noop`: parse `fixtures/bruno-lang/request.bru`, `set_block("headers", same content)`, `write()` equals the original bytes.
  - `edit_one_header_minimal_diff`: change header `content-type` value to `text/plain` → line diff (`similar::TextDiff::from_lines`) has exactly 1 removed and 1 added line.
  - `append_new_block`: `set_block("docs", Text("hi"))` on a doc without docs → output ends with `"\n\ndocs {\n  hi\n}\n"`.
  - `remove_block_drops_separator`.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement.** **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** `feat(core): canonical .bru rendering and minimal-diff edits`

### Task 4: Request model + typed file mapping

**Files:**
- Create: `crates/core/src/request.rs`, `crates/core/src/bru/model.rs`
- Test: `crates/core/src/bru/model.rs`

**Interfaces:**
- Consumes: `BruDocument`, `BlockContent`, `Pair` (Tasks 1–3).
- Produces (`request.rs`):
  ```rust
  pub enum Method { Get, Post, Put, Delete, Patch, Options, Head, Connect, Trace, Custom(String) }
  pub struct KeyValue { pub name: String, pub value: String, pub enabled: bool }
  pub enum Body { None, Json(String), Text(String), Xml(String), FormUrlEncoded(Vec<KeyValue>), Multipart(Vec<MultipartField>), Unsupported(String) }
  pub struct MultipartField { pub name: String, pub value: MultipartValue, pub enabled: bool, pub content_type: Option<String> }
  pub enum MultipartValue { Text(String), Files(Vec<String>) } // `@file(a|b)` → Files(["a","b"])
  pub enum Auth { None, Inherit, Basic { username: String, password: String }, Bearer { token: String }, Unsupported(String) }
  pub struct RequestSettings { pub follow_redirects: Option<bool>, pub max_redirects: Option<u32>, pub timeout_ms: Option<u64> }
  pub struct HttpRequest {
      pub name: String, pub seq: f64, pub kind: String, // meta.type, "http" default
      pub method: Method, pub url: String,
      pub query: Vec<KeyValue>, pub path_params: Vec<KeyValue>, pub headers: Vec<KeyValue>,
      pub body: Body, pub auth: Auth,
      pub vars_pre: Vec<KeyValue>, pub vars_post: Vec<KeyValue>,
      pub script_pre: Option<String>, pub script_post: Option<String>, pub tests: Option<String>,
      pub settings: RequestSettings,
  }
  ```
- Produces (`bru/model.rs`):
  ```rust
  pub struct RequestFile { pub doc: BruDocument, pub request: HttpRequest }
  impl RequestFile { pub fn parse(src: &str) -> Result<RequestFile, ModelError>; pub fn to_bru(&self) -> String; }
  // to_bru: apply `request` onto `doc` via set_block/remove_block, then doc.write()
  pub struct ScopeFile { pub doc: BruDocument, pub name: Option<String>, pub seq: Option<f64>, pub headers: Vec<KeyValue>, pub auth: Auth, pub vars_pre: Vec<KeyValue>, pub vars_post: Vec<KeyValue>, pub script_pre: Option<String>, pub script_post: Option<String>, pub tests: Option<String> }
  impl ScopeFile { pub fn parse(src: &str) -> Result<ScopeFile, ModelError>; pub fn to_bru(&self) -> String; } // collection.bru and folder.bru
  pub struct EnvironmentFile { pub doc: BruDocument, pub vars: Vec<KeyValue>, pub secret_names: Vec<String> }
  impl EnvironmentFile { pub fn parse(src: &str) -> Result<EnvironmentFile, ModelError>; pub fn to_bru(&self) -> String; }
  ```
- Mapping rules: method from the HTTP block name (`get`…`trace`; `http` block uses its `method:` pair → `Custom`). That block's `url`, `body` (mode), `auth` (mode) pairs. Body mode selects block `body:json`/`body:text`/`body:xml`/`body:form-urlencoded`/`body:multipart-form`; `none` or missing → `Body::None`; other modes → `Unsupported(mode)`. Auth mode `none`/`inherit`/`basic`/`bearer`, else `Unsupported(mode)`; missing → `None`. In `collection.bru`/`folder.bru` auth mode comes from the `auth { mode: … }` block. `settings` keys `followRedirects`, `maxRedirects`, `timeout`. `meta.seq` missing or non-numeric → `1.0`. `meta.type` `graphql`/others kept in `kind`.
- `to_bru` touches only blocks the typed model owns; unowned blocks (`assert`, `docs`, `auth:oauth2`, …) stay as raw.

- [ ] **Step 1: Failing tests:**
  - `parses_request_fixture`: `fixtures/bruno-lang/request.bru` → `method == Get`, `url == "https://api.textlocal.in/send/:id"`, `auth == Bearer { token: "123" }`, `path_params == [KeyValue{name:"id",value:"123",enabled:true}]`.
  - `disabled_entries_kept_not_enabled`: header `transaction-id` exists with `enabled == false`; `to_bru()` of unchanged model equals input bytes.
  - `edit_url_changes_one_line`: set `url = "https://x/y"` → diff is 1 line removed, 1 added.
  - `switch_body_mode`: Json → FormUrlEncoded writes `body: form-urlencoded` in the HTTP block and a `body:form-urlencoded` block; the old `body:json` block is kept (Bruno keeps inactive bodies).
  - `scope_file_auth_inherit`: `fixtures/bruno-tests/collection/url-serialization/folder.bru` → `auth == Inherit`, `seq == Some(13.0)`.
  - `environment_secrets`: `"vars {\n  host: http://localhost\n}\nvars:secret [\n  client_secret\n]\n"` → `secret_names == ["client_secret"]`.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement.** **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** `feat(core): typed request, folder, collection and environment files`

### Task 5: Collection loader

**Files:**
- Create: `crates/core/src/collection.rs`
- Test: `crates/core/tests/collection.rs`

**Interfaces:**
- Consumes: `RequestFile`, `ScopeFile`, `EnvironmentFile` (Task 4).
- Produces:
  ```rust
  pub struct BrunoConfig { pub version: String, pub name: String, #[serde(rename = "type")] pub kind: String, #[serde(default)] pub ignore: Vec<String>, #[serde(flatten)] pub extra: serde_json::Map<String, serde_json::Value> }
  pub struct Collection { pub root: PathBuf, pub config: BrunoConfig, pub scope: Option<ScopeFile>, pub environments: Vec<Environment>, pub tree: Folder }
  pub struct Environment { pub name: String, pub path: PathBuf, pub file: EnvironmentFile }
  pub struct Folder { pub path: PathBuf, pub name: String, pub seq: f64, pub scope: Option<ScopeFile>, pub items: Vec<Item> }
  pub enum Item { Folder(Folder), Request { path: PathBuf, file: RequestFile }, Invalid { path: PathBuf, error: String } }
  impl Collection {
      pub fn find_root(start: &Path) -> Result<PathBuf, LoadError>; // walk up to dir containing bruno.json
      pub fn load(root: &Path) -> Result<Collection, LoadError>;
      pub fn environment(&self, name: &str) -> Option<&Environment>;
      pub fn requests_under(&self, target: &Path) -> Vec<RequestRef<'_>>; // run order
  }
  pub struct RequestRef<'a> { pub path: &'a Path, pub item: &'a Item, pub folders: Vec<&'a Folder> } // folders: outermost → innermost, excluding root
  ```
- Rules: ignore dirs `node_modules`, `.git`, dot-dirs, `environments` at root, plus `config.ignore`. Request files = `*.bru` except `collection.bru`/`folder.bru`. Folder name = `folder.bru` `meta.name`, else dir name. Order (Bruno `utils/collection.js:85`): subfolders first sorted by name then stable-sorted by seq, then requests sorted by seq. A request file that fails to read or parse → `Item::Invalid`; loading continues. Missing/invalid `bruno.json` → `LoadError`. `requests_under` on a single `.bru` path returns just that request; on a folder returns its subtree in run order.

- [ ] **Step 1: Failing tests:**
  - `loads_bruno_testbench`: `Collection::load("fixtures/bruno-tests/collection")` → `config.name == "bruno-testbench"`, `Item::Invalid` count == 0, at least 1 environment, request count > 100.
  - `find_root_from_nested_file`: from `fixtures/bruno-tests/collection/ping.bru` → `fixtures/bruno-tests/collection`.
  - `ordering_folders_then_requests_by_seq` (temp dir): folders `b`(seq 1), `a`(seq 2), requests `r2`(seq 2), `r1`(seq 1) → order `b/…, a/…, r1, r2`.
  - `invalid_file_is_item_not_error` (temp dir with `broken.bru` = `"get {\n"`).
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement.** **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** `feat(core): load collection tree, config and environments`

### Task 6: Variables + interpolation

**Files:**
- Create: `crates/core/src/vars.rs`
- Test: `crates/core/src/vars.rs`

**Interfaces:**
- Produces:
  ```rust
  pub type VarMap = std::collections::BTreeMap<String, String>;
  pub enum Scope { Runtime, Request, Folder, Environment, Collection }
  pub const PRECEDENCE: [Scope; 5] = [Scope::Runtime, Scope::Request, Scope::Folder, Scope::Environment, Scope::Collection];
  pub struct VarStack { /* layers in PRECEDENCE order; folders innermost first */ }
  impl VarStack {
      pub fn new(runtime: VarMap, request: VarMap, folders_outer_to_inner: Vec<VarMap>, environment: VarMap, collection: VarMap) -> VarStack;
      pub fn get(&self, name: &str) -> Option<&str>;
      pub fn interpolate(&self, input: &str) -> String;
  }
  pub fn enabled_map(items: &[KeyValue]) -> VarMap; // disabled entries skipped, later wins
  ```
- Rules: placeholder `{{name}}`; `name` is trimmed; `{{` without `}}` left as is. Substituted values are themselves interpolated, max depth 10; on exceeding depth or a cycle, leave the remaining placeholder literal. Unknown names stay literal.

- [ ] **Step 1: Failing tests:**
  - `precedence_matches_bruno`: `host` set in all five scopes to `"runtime"`, `"request"`, `"folder"`, `"env"`, `"collection"` → `"runtime"`; remove runtime → `"request"`; remove request → `"folder"`; remove folder → `"env"`; remove env → `"collection"`.
  - `inner_folder_beats_outer`.
  - `nested_reference`: `base={{host}}/api`, `host=http://h` → `interpolate("{{base}}/x") == "http://h/api/x"`.
  - `cycle_terminates`: `a={{b}}`, `b={{a}}` → returns without hang, output contains `{{`.
  - `unknown_left_literal`: `"{{nope}}"` → `"{{nope}}"`; `"{{ host }}"` resolves.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement.** **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** `feat(core): variable scopes and interpolation`

### Task 7: Resolver (pure, no network)

**Files:**
- Create: `crates/core/src/resolve.rs`
- Test: `crates/core/src/resolve.rs`

**Interfaces:**
- Consumes: `Collection`, `RequestRef`, `Environment` (Task 5); `VarStack`, `enabled_map` (Task 6); `HttpRequest`, `Auth`, `Body` (Task 4).
- Produces:
  ```rust
  pub struct ResolvedRequest { pub method: Method, pub url: String, pub headers: Vec<(String, String)>, pub body: ResolvedBody, pub settings: RequestSettings }
  pub enum ResolvedBody { None, Bytes { content_type: Option<String>, data: Vec<u8> }, Form(Vec<(String, String)>), Multipart(Vec<ResolvedPart>) }
  pub enum ResolvedPart { Text { name: String, value: String, content_type: Option<String> }, File { name: String, path: PathBuf, content_type: Option<String> } }
  pub struct ResolveContext<'a> { pub collection: &'a Collection, pub environment: Option<&'a Environment>, pub env_overrides: &'a VarMap, pub runtime: &'a VarMap }
  pub fn resolve(ctx: &ResolveContext<'_>, req: &RequestRef<'_>) -> Result<ResolvedRequest, ResolveError>;
  ```
- Rules: environment layer = env file vars + `env_overrides` (overrides win; covers `vars:secret`). Request layer = `vars_pre`; folder layers = each folder's `vars_pre`; collection layer = `collection.bru` `vars_pre`. Headers merged collection → folders → request, later wins on case-insensitive name; disabled headers dropped. URL: interpolate, then replace `:name` path segments with enabled `path_params`; query params are **not** appended (Bruno sends `request.url` as written). Auth: request `Inherit` walks folders innermost → collection; collection `Inherit` → none. Basic → `Authorization: Basic base64(user:pass)`; Bearer → `Authorization: Bearer <token>`; replaces any existing `Authorization`. `Unsupported` body/auth → `ResolveError::Unsupported(String)`. Json/Text/Xml bodies set `content-type` (`application/json`, `text/plain`, `application/xml`) only if the merged headers lack one. Multipart file paths relative to collection root.

- [ ] **Step 1: Failing tests** (build `Collection` in a temp dir):
  - `headers_merge_request_wins`, `disabled_headers_not_sent`.
  - `auth_inherits_from_folder_then_collection`.
  - `path_params_substituted`: `http://h/users/:id` + `id=7` → `http://h/users/7`.
  - `query_not_duplicated`: url `http://h/?a=1` with `params:query a=1` → url unchanged.
  - `env_override_wins`: env `token=a`, override `token=b` → bearer header `Bearer b`.
  - `json_content_type_default_and_explicit`.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement.** **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** `feat(core): resolve requests from scopes, auth and variables`

### Task 8: Executor

**Files:**
- Create: `crates/core/src/executor.rs`
- Test: `crates/core/tests/executor.rs`

**Interfaces:**
- Consumes: `ResolvedRequest` (Task 7).
- Produces:
  ```rust
  pub struct ExecOptions { pub timeout: Option<Duration>, pub follow_redirects: bool, pub max_redirects: u32, pub verify_tls: bool } // Default: None, true, 5, true
  pub struct Session { /* shared Arc<CookieStoreMutex>; reqwest::Client cache keyed by (follow_redirects, max_redirects) */ }
  pub struct ResponseSummary { pub status: u16, pub headers: Vec<(String, String)>, pub body: Vec<u8>, pub elapsed: Duration, pub size: usize }
  impl Session {
      pub fn new(opts: ExecOptions) -> Result<Session, ExecError>;
      pub async fn send(&self, req: &ResolvedRequest) -> Result<ResponseSummary, ExecError>;
  }
  ```
- Rules: rustls only (`default-features = false`, features `rustls-tls`, `cookies`, `multipart`, `json`). Per-request `settings` override session options (`timeout_ms`, `follow_redirects`, `max_redirects`). `verify_tls = false` → `danger_accept_invalid_certs(true)`. One cookie jar per `Session`, shared by all clients in its cache. `elapsed` measured from send to full body read.

- [ ] **Step 1: Failing tests** (wiremock):
  - `sends_method_headers_and_json_body`: POST with `x-a: 1` and json body; mock matches method, header, body; status 201 returned.
  - `form_and_multipart_bodies`: mock asserts `application/x-www-form-urlencoded` body `a=1&b=2`; multipart contains a text part and a file part (temp file).
  - `basic_and_bearer_auth_headers`.
  - `redirect_toggle`: `/r` → 302 to `/ok`; `follow_redirects=true` → 200; `false` → 302.
  - `cookies_persist_in_session`: `/login` sets `sid=abc`; second request in same `Session` sends `Cookie: sid=abc`; a new `Session` does not.
  - `timeout_is_transport_error`: mock delays 500 ms, timeout 100 ms → `Err(ExecError::Timeout)`.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement.** **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** `feat(core): reqwest executor with per-session cookie jar`

### Task 9: Runner + `bru-rs run` CLI

**Files:**
- Create: `crates/core/src/runner.rs`, `crates/cli/Cargo.toml`, `crates/cli/src/main.rs`, `crates/cli/src/print.rs`
- Test: `crates/core/tests/runner.rs`, `crates/cli/tests/cli.rs`

**Interfaces:**
- Consumes: `Collection` (5), `VarMap` (6), `resolve` (7), `Session`, `ExecOptions` (8).
- Produces:
  ```rust
  pub struct RunOptions { pub env: Option<String>, pub env_overrides: VarMap, pub exec: ExecOptions }
  pub enum RunEvent {
      Started { path: PathBuf, name: String, method: String, url: String },
      Passed { path: PathBuf, status: u16, elapsed: Duration, size: usize },
      Failed { path: PathBuf, error: String },
      Skipped { path: PathBuf, reason: String },
  }
  pub struct RunReport { pub passed: usize, pub failed: usize, pub skipped: usize, pub elapsed: Duration }
  pub async fn run(collection: &Collection, target: &Path, opts: &RunOptions, on_event: &mut (dyn FnMut(&RunEvent) + Send)) -> Result<RunReport, RunError>;
  ```
  `RunError` only for setup problems (unknown env name, target outside collection). Per-request problems become `Failed`/`Skipped` events.
- Rules: one `Session` and one runtime `VarMap` per run (runtime stays empty in Phase 1; scripts fill it in Phase 3). `Item::Invalid` → `Skipped { reason: "parse error: <error>" }` plus `tracing::warn!`. `kind != "http"` (graphql, grpc, ws) → `Skipped { reason: "unsupported request type <kind>" }`. `ResolveError::Unsupported` → `Skipped`. Non-empty `vars_post`, scripts, tests or `assert` → still `Passed`/`Failed` by transport outcome, plus a `tracing::info!` that they run in Phase 3.
- CLI (`clap` derive): `bru-rs run <PATH> [--env <NAME>] [--env-var <KEY=VALUE>]... [--insecure] [--timeout-ms <N>] [--no-follow-redirects]`. Output per event: `✓ GET  ping  200  12ms  1.2KB`, `✗ path: error`, `- path: reason`; final line `N passed, N failed, N skipped (Xms)`. Exit codes: 0 all passed (skips allowed), 1 any failed, 2 usage/load/`RunError`. `RUST_LOG` controls `tracing` output to stderr.

- [ ] **Step 1: Failing tests:**
  - `runner.rs::runs_in_order_and_reports` (temp collection + wiremock, 3 requests, env `local` with `host`): events order Started/Passed ×3 in seq order; `RunReport { passed: 3, failed: 0, .. }`.
  - `runner.rs::broken_file_skipped_run_continues`: temp collection with `broken.bru` + 1 good request → `skipped == 1`, `passed == 1`, `failed == 0`; the `Skipped` reason contains `broken.bru`'s parse error.
  - `cli.rs::broken_file_exit_zero`: same collection via CLI → exit 0, output contains `- ` line for `broken.bru`.
  - `runner.rs::graphql_skipped`.
  - `cli.rs::exit_codes` (`assert_cmd`): all pass → 0; one 500 response → 0; connection refused → 1; unknown `--env` → 2; missing path → 2.
  - `cli.rs::env_var_flag_overrides`: `--env-var token=xyz` → mock sees `Authorization: Bearer xyz`.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement.** **Step 4: Run** `cargo test --workspace` → PASS.
- [ ] **Step 5: Phase gate:** `cargo clippy --workspace --all-targets -- -D warnings` → no warnings. Manual: `cargo run -p brs-cli -- run fixtures/bruno-tests/collection/ping.bru` prints one result line (failure is expected without Bruno's test server; exit code 1).
- [ ] **Step 6: Commit** `feat(cli): bru-rs run with per-request results and exit codes`

---

## Phase 1 exit checklist

- [ ] All fixture `.bru` files round-trip byte-identical (Task 1).
- [ ] One-field edits produce one-line diffs (Tasks 3, 4).
- [ ] Interpolation precedence tests (Task 6).
- [ ] wiremock integration: methods, bodies, auth, redirects, cookies, timeout (Task 8).
- [ ] CLI exit codes (Task 9).
- [ ] Summary written to the user; **stop** for confirmation before Phase 2.
