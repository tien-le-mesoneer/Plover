# Import from cURL and HAR — design

Status: draft for review · 2026-10-23

## Why

Today the only way to make a request in Plover is to open a collection that already has
`.bru` files. There is no "new request", no paste-a-curl, and no way to turn what a
browser sent into requests. The user wants the Postman habit: copy a curl command, paste
it, press Send. They also want Claude to click through a web app in a browser, capture
what the browser sent, and turn that into Plover requests.

Plover has no code for any of this yet. `grep -ri curl crates frontend/src` finds
nothing. The server can read, save and send existing requests
(`crates/server/src/api/requests.rs`), but it cannot create a request file, a folder or a
secret variable name.

## Goals (user decisions)

1. Paste a `curl …` command into the URL bar and it fills method, URL, query, headers,
   body, basic auth (`-u`) and multipart form (`-F/--form`). Options Plover cannot
   represent, `-k` among them, are reported clearly, never dropped silently.
2. A scratch tab: "New request" opens an unsaved tab that is not tied to a file. Send
   works at once, with the selected environment. Save asks for a folder in the
   collection, and only then writes a `.bru` file.
3. "Copy as cURL" from any request, like Bruno's Generate Code.
4. "Import HAR" in the UI: list the entries, filter by domain, drop images, JS, CSS and
   fonts by default, tick the ones to keep, and import them into a chosen folder as
   requests. Tokens, cookies, `Authorization` and credential fields in request bodies
   move into environment secret variables instead of being written into the `.bru`
   files.
5. A docs page (not a skill) on how to have Claude drive a browser through a flow, save
   a HAR file, and import it into Plover.

All of this is UI only.

## Non-goals (user decisions)

- A `plv curl` command, or any other CLI part of this feature.
- Plover acting as a recording proxy (Postman Interceptor style). That needs a TLS MITM
  certificate, and it is heavy and risky.

Also out of scope (*Claude's call*): GraphQL detection on paste, `--data-binary @file`
file bodies, Windows `cmd`/PowerShell curl syntax, importing WebSocket entries from a
HAR, and Postman or Insomnia collection import.

## Anti-goals (a failure even if it works)

- A token, cookie or `Authorization` value from a HAR written into a `.bru` file, where
  it could be committed to git.
- A pasted token kept on disk because Plover remembered a scratch tab.
- A curl option that changes the request (proxy, client cert, `--resolve`) disappearing
  without a word, so the request Plover sends is not the one the user copied.
- Breaking Bruno compatibility of the files Plover writes. Bruno must open every
  imported or saved request.

## Constraints

- Files written must be valid Bruno `.bru` and round-trip through the existing typed
  layer (`RequestFile`, `ScopeFile`, `EnvironmentFile` in `crates/core/src/bru/model.rs`).
- New writes go through `store::write_atomic` and the per-file lock in
  `crates/core/src/store.rs`, and through `resolve_item_path` so nothing escapes the
  collection root.
- API types are generated with ts-rs, like the other DTOs in
  `crates/server/src/api/dto.rs`.
- The request body limit stays `MAX_REQUEST_BODY` (64 MiB) in
  `crates/server/src/api/mod.rs`. A HAR larger than that is refused with a clear message.
- Secret values go through `OpenCollection::update_secrets`
  (`crates/core/src/workspace.rs`), so they land in the keychain, or in memory when it
  is unavailable. Tests use a temporary `PLOVER_STATE_DIR` and a test
  `PLOVER_KEYCHAIN_SERVICE`, and never touch real keychain entries.
- Test fixtures are synthetic. No captured traffic from real systems goes in the repo.

## Decisions

**User** means the user decided it. **Claude's call** means the user has not reviewed it.

### Where the parsing lives (Claude's call)

The curl parser, the curl generator and the HAR parser live in Rust core, in new modules
`crates/core/src/curl.rs` and `crates/core/src/har.rs`. The frontend calls the server
for them. This gives one tested implementation, and makes `parse(to_curl(r)) == r` easy
to test. Every call is to the local server, so the extra round trip does not matter.

### 1. Paste a curl command

- **Trigger.** Match Bruno (`bruno-app/src/components/RequestPane/QueryUrl/index.js`).
  A paste into the URL bar (`frontend/src/components/request/UrlBar.tsx`) whose text
  matches `^\s*curl\s` is intercepted. Any other paste is a normal paste.
- **Server call.** `POST /curl/parse` with `{ text }` returns
  `{ request: HttpRequestDto, notices: CurlNotice[] }`, or a 400 `invalid_curl`.
- **What fills the tab.** The parsed result replaces the draft's method, URL, query,
  headers, body and auth. Name, seq, scripts, vars, tests, asserts and settings are kept.
  The tab becomes dirty. Nothing is saved until the user saves. *Claude's call:* a
  banner shows "Filled from cURL" with an **Undo** button that restores the draft from
  before the paste.
- **Parsing follows Bruno's parser** (`bruno-app/src/utils/curl/parse-curl.js` and
  `index.js`). Port its tokenizer rules: quotes, `$'…'` escapes, `\` line
  continuations, and `^` continuations as Bruno cleans them. Mapping:

  | curl | Plover |
  |---|---|
  | URL argument, `--url` | `url`; its query string also fills `query` |
  | `-X/--request` | `method` (default `GET`, or `POST` when there is data) |
  | `-H/--header`, `-A/--user-agent`, `-b/--cookie` | headers (`User-Agent`, `Cookie`) |
  | `-d/--data`, `--data-raw`, `--data-ascii`, `--data-urlencode`, `--data-binary` (inline), `--json` | body, mode picked by `Content-Type` as in Bruno's `index.js`: json, xml, text, form-urlencoded; no content type gives form-urlencoded |
  | `-G/--get` | data moves into the query |
  | `-F/--form` | multipart; `name=@path` becomes a file field |
  | `-u/--user` | `Auth::Basic` |
  | `-I/--head` | method `HEAD` |
  | `--compressed` | accepted, no effect (Plover already decompresses) |
  | `-k/--insecure` | not applied; notice "`-k` not applied, TLS checking is a global setting" (user decision, Bruno-compatible: a `.bru` file has no per-request TLS setting, and TLS checking is the global `ExecOptions.verify_tls`) |

  A `Bearer` token in an `Authorization` header stays a header. It does not become
  `Auth::Bearer`, as in Bruno.
- **Unsupported options are reported, never dropped** (user decision). Unlike Bruno,
  which skips unknown flags without a word, the parser returns one `CurlNotice` per
  ignored option, with the option and the reason. Examples: `--resolve`, `-x/--proxy`,
  `--cert`/`--key`/`--cacert`, `--digest`/`--ntlm` (auth modes Plover cannot send),
  `--data-binary @file`, `-T`, and any flag it does not know. The banner lists them:
  "Not imported: `--proxy http://…` (proxies are not supported per request)". The
  request is still filled. *Claude's call:* only a missing URL makes the paste fail.
- **Secrets on paste.** *Claude's call:* when the pasted request has an `Authorization`
  or `Cookie` header, the banner adds a button **Move to secret**. It turns the value
  into `{{name}}`, adds a secret variable to the selected environment, and stores the
  value there (the same path as HAR import). It is offered, not automatic, because the
  user only asked for automatic moving on HAR import.

### 2. Scratch tab

- **Opening.** *Claude's call:* a "New request" button in the sidebar header
  (`frontend/src/components/SidebarHeader.tsx`). No keyboard shortcut, because browsers
  keep Ctrl/Cmd+N for a new window. It opens a
  tab named "Untitled", method `GET`, everything else empty. Pasting a curl into its URL
  bar works as above.
- **Belongs to the active collection** (*Claude's call*). Plover sends through a
  collection's session (cookies, environment, collection-level headers, auth and
  scripts), so a scratch tab belongs to the collection that is active when it opens.
  With no collection open, the button is disabled with the hint "Open a collection
  first".
- **Store.** `Tab` in `frontend/src/store/tabs.ts` gets `item: string | null` (null for a
  scratch tab), an empty `saved`, and a generated key. `isDirty` is always true for a
  scratch tab that has content.
- **Sending.** `SendBody.item` (`dto.rs`) becomes optional. With `item: null`, the server
  sends the draft with collection scope only: collection headers, auth, vars and
  scripts apply, and no folder applies. In core, `SendInput.item_id` becomes an
  `Option<&str>`, and `send_one_with` skips the lookup in `requests_under` when it is
  `None`. *Claude's call.*
- **Saving.** Save on a scratch tab opens a dialog: a folder picker over the collection
  tree (the root is allowed) and a name, prefilled from the method and the last path
  segment (for example `GET orders`). It calls the new create endpoint (below). The tab
  then becomes a normal file tab for the new item. If the file name exists, the dialog
  says so and asks for another name.
- **Not remembered.** Scratch tabs are not saved in the UI state that restores tabs on
  restart. That state only stores item IDs anyway (`frontend/src/store/uiState.ts`).
  This keeps a pasted token off disk. *Claude's call:* closing a scratch tab with
  content asks "Discard this request?"; quitting the app shows the browser's
  leave-page prompt when any scratch tab has content.

### 3. Copy as cURL

- **Where.** *Claude's call:* a "Copy as cURL" button in the request tab's toolbar, next
  to Save, and the same action in a scratch tab.
- **Server call.** `POST /collections/{id}/curl` with
  `{ item: string | null, draft, env, resolve: boolean }` returns `{ text }`. Core
  `curl::to_curl(&ResolvedRequest) -> String` builds it.
- **Two modes** (*Claude's call*):
  - **Default: as written.** `{{variables}}` are left as they are. No environment is
    needed, and nothing secret is copied.
  - **"With variables filled in".** The request is resolved for the selected
    environment the way Send resolves it (collection and folder headers and auth
    included), but no scripts run. If the result contains a secret's value, the UI
    asks first: "This command contains secret values. Copy anyway?"
- **Format.** POSIX shell: `curl -X METHOD 'url'`, one `-H` per line with ` \`
  continuations, values in single quotes with `'` escaped as `'\''`. JSON and text bodies
  use `--data-raw`, form-urlencoded uses one `--data-urlencode` per field, multipart uses
  `-F`, and basic auth uses `-u`. A request whose auth or body mode Plover cannot send
  (`Auth::Unsupported`, `Body::Unsupported`) is copied without it, with a toast that
  says what was left out.

### 4. Import HAR

- **Where.** *Claude's call:* "Import HAR…" in the sidebar header menu and as a context
  action on folders. The user picks a `.har` file (browser file input; no native dialog
  is needed). Import goes into the folder it was started from, or the root.
- **Step 1, preview.** `POST /collections/{id}/import/har/preview` with the HAR text
  returns one row per entry: index, method, URL, status, MIME type, size, and a
  `hasSecrets` flag. The server keeps nothing.
- **The import dialog** shows the rows as a table with checkboxes.
  - A domain filter (a chip per host, all on). *Claude's call:* hosts are ordered by
    request count.
  - Dropped by default (user decision): images, JavaScript, CSS and fonts, judged by
    response MIME type and then by file extension. *Claude's call:* also dropped by
    default are media, source maps, `OPTIONS` preflights, `data:` URLs and browser
    extension URLs. A "Show hidden" toggle brings them back.
  - *Claude's call:* identical requests (same method, URL and body) collapse into one
    row marked "×3". Importing the row imports one request.
  - Target folder: "Import into" with the folder picker. A new subfolder name is
    prefilled from the HAR page title or the first host, for example `captured-<host>`.
  - Environment for secrets: a select of the collection's environments, defaulting to
    the selected one. With no environment, *Claude's call:* Plover creates
    `environments/Imported.bru`.
- **Step 2, import.** `POST /collections/{id}/import/har` with
  `{ har, entries: number[], folder, newFolder?, env }` writes the files and returns the
  new item IDs and a list of notices. The tree reloads.
- **Mapping an entry to a request.**
  - Name: method plus a short path, for example `POST api-orders`, made unique in the
    folder. File names come from the name, as Bruno would.
  - `seq`: capture order, starting after the folder's highest existing seq.
  - URL from `request.url`, with its query string in `query` as well. Headers come from
    `request.headers`. HTTP/2 pseudo-headers (`:authority` and the like) and headers the
    executor sets itself (`Host`, `Content-Length`, `Connection`, `Accept-Encoding`) are
    left out. *Claude's call.*
  - Body from `postData`: `params` becomes form-urlencoded or multipart; `text` with
    its `mimeType` picks json, xml or text the same way as curl paste. HAR files do not
    carry uploaded file content, so file parts become file fields with an empty path,
    and a notice says so.
  - Auth: an entry with no credential header gets `auth: inherit`, which is Bruno's
    default for new requests. *Claude's call.*
  - The captured response is not imported: no response body, and no asserts generated
    from the captured status. *Claude's call.*
  - Captured `Cookie` headers are sent as written, on top of the collection's cookie
    jar, the same as any request with a hand-written `Cookie` header.
- **Secrets** (user decision: tokens, cookies and `Authorization` move into secrets).
  - `Authorization: Bearer X` becomes `auth:bearer` with token `{{authToken}}`.
    `Authorization: Basic …` becomes `auth:basic` with `{{authUser}}` and
    `{{authPassword}}`. Any other scheme becomes the header value `{{authorization}}`.
  - `Cookie` becomes the header value `{{cookie}}`.
  - Tokens elsewhere (*Claude's call*, a fixed list that the dialog shows): the
    headers `X-Api-Key`, `Api-Key`, `X-Auth-Token`, `X-CSRF-Token`, `X-XSRF-Token`, and the
    query parameters `access_token`, `token`, `api_key`, `apikey`, `key`.
  - One secret variable is made per distinct value, numbered when values differ:
    `authToken`, `authToken2`, and so on. If two entries share a value, they share a
    variable.
  - The variable names are added to the environment's `vars:secret` block, and the
    values are stored with `update_secrets`. A variable that already exists with the
    same value is reused. A different value gets the next free number, never an
    overwrite. A name already used by a plain (non-secret) variable is skipped the
    same way.
  - The dialog lists the secret variables it will create **before the import**
    (*user decision*), with names only, never values. The names come from a dry-run
    plan endpoint that writes nothing and gives the same names the import will use.
- **Credential fields in request bodies move into secrets** (user decision).
  - A fixed list that the dialog shows (*Claude's call* on the list): `password`,
    `passwd`, `client_secret`, `secret`, `token`, `access_token`, `refresh_token`,
    `id_token`, `api_key`, `apikey`, `assertion`, `client_assertion`. Names match
    case-insensitively and exactly; `newPassword` or `password2` do not match.
  - Bodies covered: form-urlencoded (`postData.params`, or `text` with
    `application/x-www-form-urlencoded`), multipart text fields, and JSON. In JSON, a
    listed key at any depth whose value is a string is replaced; numbers, objects and
    arrays are left as they are. *Claude's call.*
  - The value becomes `{{<camelCase field name>}}`, for example `{{password}}`,
    `{{clientSecret}}`, numbered like the header secrets (`password2`) and reusing a
    variable with the same value.
  - In JSON the replacement keeps the quotes: `"password": "{{password}}"`, so the
    body stays valid JSON.
  - Other bodies (XML, text) are imported as they are. The dialog still marks rows
    whose body contains a value that looks like a JWT (`eyJ…`), so the user can untick
    or edit them. *Claude's call.*

### New server and core pieces

| Piece | Where | Purpose |
|---|---|---|
| `POST /curl/parse` | `api/curl.rs` (new) | curl text → `HttpRequestDto` + notices |
| `POST /collections/{id}/curl` | `api/curl.rs` | request → curl text |
| `POST /collections/{id}/requests/new` | `api/requests.rs` | create a request file: `{ folder, name, request }` → `{ item, hash }`; 409 `exists` if the file is there |
| `POST /collections/{id}/import/har/preview`, `…/import/har` | `api/import.rs` (new) | HAR preview and import |
| `store::create_request` | `crates/core/src/store.rs` | write a new `.bru` file with `create_new` semantics under the file lock |
| `store::create_folder` | `crates/core/src/store.rs` | create a folder; *Claude's call:* no `folder.bru` unless a seq is needed |
| `store::add_secret_names` | `crates/core/src/store.rs` | add names to `vars:secret` of an environment, creating `environments/<name>.bru` when missing; minimal diff, same as `save_environment` |
| `SendInput.item_id: Option<&str>` | `crates/core/src/runner.rs` | scratch send |

Every write in an import (folder, request files, environment, secret values) is done in
this order: secret values, then the environment file, then the request files. If a step
fails, the response lists what was written. *Claude's call:* there is no rollback; the
files written so far stay, and the error says which.

### 5. Docs page: capture a flow with Claude

`docs/capture-with-claude.md` (*Claude's call* on the name), linked from the README.
It covers:

1. Ask Claude to open the app in a browser and do a flow step by step, for example
   "log in, create a document, sign it", while recording a HAR. The way to record that
   works today is a short Playwright script with `recordHar` that Claude writes and
   runs. The page also shows the manual path: Chrome DevTools → Network → "Save all as
   HAR with content".
2. Use a test account, never production credentials, because a HAR holds cookies and
   tokens in plain text.
3. Import the HAR into Plover (section 4), check the secret variables it created, and
   delete the `.har` file.

## Left to the builder

- Exact Rust and TypeScript names, module layout, and DTO field names.
- Visual design of the banner, the import dialog, the save dialog and the toolbar
  buttons. Use the existing `Modal`, `SubTabs` and `EmptyState` components and the rules
  in `docs/ui-ux-review.md`.
- Which Bruno parser test cases (`parse-curl.spec.js`, `curl-to-json.spec.js`) to port.
  Port enough to cover every row of the mapping table and the tokenizer rules.
- File-name rules for imported names (invalid characters, length), following what Bruno
  does.
- Which browser-automation tool the docs page uses beyond Playwright's `recordHar`.
  Check that a tool really writes HAR before naming it.
- Tests. At minimum:
  - curl: each mapping row; `$'…'` and line continuations; unknown flags turning into
    notices, `-k` among them with its exact text; `parse(to_curl(r)) == r` for each
    body mode; single-quote escaping;
  - copy: "as written" keeps `{{vars}}`; "filled in" includes folder auth;
  - scratch: send with `item: null` applies collection headers and no folder headers;
    save creates the file, and an existing name gives 409;
  - HAR: filters, de-duplication, the secret mapping and variable reuse, no secret
    value in any written `.bru` file (headers, query and form/multipart/JSON body
    fields, including a nested JSON key and a JSON number left alone), Bruno-readable
    output, a HAR over 64 MiB refused;
  - UI: paste interception and Undo, the scratch tab close prompt, the import dialog
    defaults.

## Open questions for the user

None. `-k` and credential fields in bodies were settled by the user (sections 1 and 4).
