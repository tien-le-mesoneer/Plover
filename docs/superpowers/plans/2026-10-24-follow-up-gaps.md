# Follow-up Gaps Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the gaps the last five plans left open:
- "Import HAR…" on a folder's context menu;
- credentials inside XML and text bodies become secrets on import and are masked in run history;
- numeric and short data-file credentials are masked;
- a runtime-variable list you can view and clear;
- Cancel no longer squeezes the URL field;
- Esc stops a running run.

**Architecture:**

- **One owner for credential spans.** `credentials.rs` gains the one function that finds credential spans in a body's text: JSON keys, XML elements and attributes, and JWT-shaped tokens. HAR import replaces those spans with `{{var}}`. The run masker replaces them with `***`.
- **The masker gets a by-name pass** next to its value pass. A credential found by its name is masked whatever its type or length. The value pass keeps its floors.
- **Runtime variables** get list and delete routes. They share masking with `/vars` and show in a small panel next to the environment label.

**Tech Stack:** Rust (regex, serde_json, axum, ts-rs, wiremock), React + TypeScript + Zustand + Vitest/Testing Library.

**Spec:** the gap lists in these reports:
- `docs/curl-har-report.md` §"Known gaps" 1 and 4
- `docs/data-driven-report.md` §"Known gaps" 4
- `docs/ui-style-report.md` §"Known gaps" 4
- `docs/dev-ux-report.md` §"Known gaps" 4 and 5

The rules they extend come from:
- `docs/superpowers/specs/2026-10-23-import-curl-har-design.md`: "Where" for import, and the body rules.
- `docs/superpowers/specs/2026-10-23-run-history-design.md`: "Masking".
- `docs/superpowers/specs/2026-10-23-data-driven-and-export-design.md`: "Masking data-file values".
- `docs/superpowers/specs/2026-10-23-dev-ux-design.md`: §7, variable hover.

Decisions made in this plan are marked *Claude's call*.

**Verified on `b31d443`** (all five plans merged and pushed).

## Global Constraints

### Credential names

- They come from `credentials.rs` and nowhere else:
  - `CREDENTIAL_FIELDS`, `TOKEN_HEADERS`, `TOKEN_QUERY_PARAMS`;
  - `is_credential_field` (exact, ASCII case-insensitive), `looks_like_jwt`.
- No new name lists.
- XML matching uses the element's local name, so `wsse:Password` matches `password`.

### Credential spans

- Body spans are found in exactly one place: `credentials::body_credential_spans`.
- HAR import and `Masker` both call it. Neither re-parses bodies for credentials on its own.

### HAR import (*Claude's call*)

- A credential span in an XML or text body becomes `{{<camelCase local name>}}`, numbered and reused like header secrets.
- A JWT span outside a named credential becomes `{{bodyToken}}`, `{{bodyToken2}}`, and so on.
- Bytes outside the spans are kept exactly.
- The secret holds the span's text **as written** (XML entities are not decoded), so the request that gets sent is byte-identical to the captured one.

### Masking (*Claude's call*; changes the run-history spec's "Limits")

**By-name pass.** It masks whole values, whatever their length or type:
- credential spans in request and response text bodies (`body_credential_spans`);
- form fields and multipart text parts named in `CREDENTIAL_FIELDS`;
- URL query values named in `CREDENTIAL_FIELDS` or `TOKEN_QUERY_PARAMS`;
- iteration-row keys named in `CREDENTIAL_FIELDS`, at any depth.

How each kind of span is masked:
- A JSON number or string span becomes the string `"***"` (with quotes), so the JSON stays valid.
- XML text, an XML attribute value, and a query value become `***`.

**Value pass, collected in pass 1.**

| Source | Minimum length |
|---|---|
| Values found by name above | 4 characters, the secret floor |
| Every JWT span found anywhere in the run (URLs, headers, bodies, console) | none |
| Values of `TOKEN_HEADERS` | 8 characters, the header floor |

`TOKEN_HEADERS` values are now masked whole, like `Authorization`. The run-history spec said `X-Api-Key` is not masked; this plan changes that.

**Data-file credentials (*Claude's call*).**
- Numbers and strings shorter than 4 characters are masked **by name only**, never by value.
- The trade-off: a 1–3 character or numeric credential that shows up away from its name stays visible. Examples are a URL path segment, a `console.log`, or a key with another name. In exchange, `***` never spreads over common short strings or numbers such as `200`, `2024` or ids.
- Strings of 4 or more characters are masked by name and by value, as today.

### Runtime-variable list

- The list masks a value when **any** of these is true:
  - its name is a credential field;
  - its text is a value this server session sent in a credential header;
  - its text equals a secret value of the selected environment, 4 characters or more.
- Values are cut at 1 KiB, like `/vars`.
- There is no reveal button.
- Clearing all variables asks for confirmation first.

### Engineering rules

- No new Rust crate and no new frontend dependency.
- New routes sit behind the session token.
- After DTO changes, run `PLOVER_UPDATE_BINDINGS=1 cargo test -p plover-server --test bindings` and commit `frontend/src/api/types.ts`. Never edit that file by hand.
- Use the same gate before every commit as earlier plans, with `set -o pipefail`:

  ```bash
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  (cd frontend && npm run typecheck && npm run lint && npm test && npm run build && npm run check:no-cdn && npm run check:console)
  ```

- Tests use a temporary `PLOVER_STATE_DIR` and `PLOVER_KEYCHAIN_SERVICE=dev.plover.test`.
- Test fixtures are synthetic: `example.test`, `tok-AAAA…`, and the JWT `eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.sig`.
- Commits use Conventional Commit subjects with the trailers:
  - `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`
  - `Claude-Session: https://claude.ai/code/session_01FnGwUwogsr835cxeJbvuWL`
- Do not push.

## Review Focus

1. **A credential element repeated in one XML body with different values**, such as two `<wsse:Password>` elements in one SOAP envelope.
   - Import: the values become `{{password}}` and `{{password2}}`, and the same value reuses its variable.
   - Masking: both spans are masked.
   - Test `xml_two_passwords_numbered` (Task 2).
2. **A `<password>` element whose text is a CDATA section, or holds entities** (`<password><![CDATA[a<b]]></password>`, `<password>a&amp;b</password>`). The span covers the CDATA content, or the raw entity text. The imported body sent with the secret filled in is byte-identical to the HAR text. Test `xml_cdata_and_entities_round_trip` (Task 1, span) and `imported_xml_sends_identical_bytes` (Task 2).
3. **A numeric data-file credential whose digits also appear elsewhere.** The column is `pin: 1234` and the response has `"count": 1234`. The request body's `"pin": 1234` becomes `"pin": "***"`, while `"count": 1234` and the duration stay. Test `numeric_credential_masked_by_name_only` (Task 4).
4. **Clear all runtime vars while a run is going.** The run's own later `bru.setVar` values come back when it ends (`record_runtime_changes` applies only its changes). Names the run never touched stay cleared. Neither the run nor the clear fails. Test `clear_during_run_keeps_only_run_changes` (Task 5).
5. **Right-click on a folder, then press Esc** (keyboard user: Shift+F10 or the ContextMenu key). The menu closes, focus returns to the folder row, and nothing runs. Before this change, right-click ran the folder straight away. Test `context_menu_escape_runs_nothing` (Task 7).

---

### Task 0: Verify the interfaces

No code.

- [ ] **Step 1: Check that these names exist on `b31d443`.** Fix this plan inline where one differs.
  - `crates/core/src/credentials.rs`: `CREDENTIAL_FIELDS`, `TOKEN_HEADERS`, `TOKEN_QUERY_PARAMS`, `is_credential_field`, `is_token_header`, `is_token_query_param`, `looks_like_jwt`.
  - `crates/core/src/har.rs`:
    - `has_secrets`, `entry_body` (XML and text fall through `other => other`), `credential_value`, `json_with_secrets`;
    - `SecretAlloc::{var, placeholder}`, `camel_case`;
    - `curl::body_from_text` returns `Body::Xml`/`Body::Text`.
  - `crates/core/src/masking.rs`:
    - `Masker::{new, mask_text, mask_result, mask_record, mask_iteration}`, `MASKED_HEADERS`;
    - `MIN_HEADER_VALUE = 8`, `MIN_SECRET_VALUE = 4`;
    - `collect_header`, `pub fn credential_values`.
  - `crates/core/src/run_record.rs`:
    - `BodyDetail::{Text{text,size,truncated}, Form{fields}, Multipart{parts}}`, `PartDetail::Text`;
    - `RunRecorder::{masked_result, masked_iteration, finish}`.
  - `crates/core/src/data_file.rs`: `credential_values(rows) -> Vec<String>`. Strings only.
  - `crates/core/src/workspace.rs`: `OpenCollection::{runtime, set_runtime_var, record_runtime_changes, sent_credentials, secrets}`.
  - `crates/server/src/api/vars.rs`: `explain`, `VarDto`, `MAX_VALUE`, `valid_name`, `set_runtime_var`. In `api/mod.rs`, route `PUT /collections/{id}/runtime-vars/{name}`.
  - Frontend:
    - `Sidebar.tsx` folder `onContextMenu` runs the folder;
    - `ImportHarDialog({collectionId, onClose})` has `folder` state;
    - `Menu`/`MenuItem` in `Menu.tsx`;
    - `SidebarHeader` mounts `ImportHarDialog`;
    - `UrlBar` `CancelButton` with `button.cancel-send { min-width: 9.5em }`;
    - App `onKey` Esc branch with `OVERLAY`;
    - `RunTab` Cancel button calling `useRuns().cancel(runId)`;
    - `useVars`/`watchVars` in `store/vars.ts`;
    - `EnvSelector` in the App top bar.
- [ ] **Step 2: Commit any plan fixes** as `docs: record verified interfaces in the follow-up plan`.

---

### Task 1: Credential spans in body text (core)

**Files:**
- Modify: `crates/core/src/credentials.rs`
- Test: `crates/core/tests/credentials.rs` (new)

**Interfaces:**
- Produces:

  ```rust
  pub enum SpanKind { JsonString, JsonNumber, XmlText, XmlAttr, Jwt }
  pub struct CredentialSpan { pub range: std::ops::Range<usize>, pub name: Option<String>, pub kind: SpanKind }
  /// Credential value spans in a body: JSON (text parses as JSON), XML (first non-space char is `<`),
  /// plus JWT spans anywhere not already inside a named span. Sorted, non-overlapping, never empty.
  pub fn body_credential_spans(text: &str) -> Vec<CredentialSpan>;
  /// Byte ranges of JWT-shaped tokens (same shape as `looks_like_jwt`, all three dot-separated parts).
  pub fn jwt_spans(text: &str) -> Vec<std::ops::Range<usize>>;
  ```

  `name` is the key or the local element/attribute name as written, and `None` for `Jwt`.

- [ ] **Step 1: Write the failing tests**
  - `json_string_and_number_spans`:
    - In `{"user":"a","password":"p@ss","pin":1234,"n":{"token":"t"}}`, there are three spans: `p@ss` (inside the quotes, `JsonString`), `1234` (`JsonNumber`) and `t`.
    - `"newPassword"` gives no span.
    - Key matching is case-insensitive (`"Password"`).
  - `json_string_with_escapes`: for `{"password":"a\"b"}`, the span covers the raw `a\"b`.
  - `invalid_json_gives_only_jwt_spans`: the text `{"password": oops eyJ…sig` returns the JWT span only.
  - `xml_element_and_attribute`:
    - `<wsse:Password Type="x">s3cret</wsse:Password>` gives a span over `s3cret` with name `Password`.
    - `<cred password='pw1'/>` gives `XmlAttr` over `pw1`.
    - `<passwords>x</passwords>` gives no span.
    - `<password></password>` gives no span (empty).
  - `xml_cdata_and_entities_round_trip`:
    - For `<password><![CDATA[a<b]]></password>`, the span covers `a<b`.
    - For `<password>a&amp;b</password>`, the span covers `a&amp;b`.
  - `jwt_inside_text_and_json_value`:
    - The plain text `token=eyJ….eyJ….sig rest` gives one `Jwt` span over the whole token.
    - In `{"note":"eyJ…sig"}`, the JWT inside a non-credential JSON value is found as `Jwt`.
  - `jwt_inside_named_span_not_duplicated`: `{"token":"eyJ…sig"}` gives one span, `JsonString` named `token`.
- [ ] **Step 2: Run** `cargo test -p plover-core --test credentials`. Expect FAIL: unresolved import.
- [ ] **Step 3: Implement.**
  - JSON: a small hand scanner over the text (strings with escapes, numbers, punctuation). After a string key followed by `:`, a string or number value is a span when `is_credential_field(key)` holds. Use it only when `serde_json::from_str::<Value>` succeeds.
  - XML: `regex` (no backreferences):
    - an opening tag `<(?:[\w.-]+:)?([\w.-]+)(\s[^<>]*)?>`;
    - then content up to the next `<`, or one CDATA section;
    - then a closing tag whose local name equals the opening one, checked in code;
    - attributes `([\w.-]+:)?([\w.-]+)\s*=\s*("([^"]*)"|'([^']*)')` inside any tag.
  - `jwt_spans`: share the `eyJ…` header scan with `looks_like_jwt`. Extend through `.payload.signature` over base64url characters.
- [ ] **Step 4: Run** `cargo test -p plover-core --test credentials`. Expect PASS.
- [ ] **Step 5: Gate and commit:** `feat(core): find credential spans in JSON, XML and text bodies`.

---

### Task 2: HAR import moves XML/text body credentials into secrets

**Files:**
- Modify: `crates/core/src/har.rs` (`entry_body`, `has_secrets`; `json_with_secrets` keeps its JSON path)
- Modify: `frontend/src/components/ImportHarDialog.tsx` (row note wording only)
- Test: `crates/core/tests/har.rs`; fixture `crates/core/tests/fixtures/har/xml-soap.har` (new, synthetic)

**Interfaces:**
- Consumes: `body_credential_spans` (Task 1), `SecretAlloc::placeholder`.
- Produces: `fn text_with_secrets(text: &str, alloc: &mut SecretAlloc<'_>) -> String` (private). It replaces spans back to front. The variable base is `camel_case(name)`, or `bodyToken` for `Jwt`. JSON spans are skipped here, because `json_with_secrets` already handles JSON keys. JWT spans inside JSON values are replaced in place (inside the quotes).

- [ ] **Step 1: Write the failing tests**
  - `xml_body_credentials_become_secrets`:
    - The SOAP fixture body `<wsse:Username>u</wsse:Username><wsse:Password>pw-AAAA</wsse:Password>` imports as `<wsse:Password>{{password}}</wsse:Password>`.
    - `new_secrets["password"] == "pw-AAAA"`.
    - `Username` is not on the list and stays.
  - `xml_two_passwords_numbered`: two different values give `{{password}}` and `{{password2}}`. A repeated value reuses its variable.
  - `text_body_jwt_becomes_body_token`: a `text/plain` body `id=eyJ…sig` becomes `id={{bodyToken}}`.
  - `json_body_jwt_in_plain_key`: `{"note":"eyJ…sig"}` becomes `{"note":"{{bodyToken}}"}` and stays valid JSON.
  - `imported_xml_sends_identical_bytes`: replace each `{{name}}` in the imported body with `new_secrets[name]` (plain string replace in the test). The result equals the HAR `postData.text` byte for byte, including `&amp;` and CDATA.
  - Update `xml_body_with_jwt_flags_row`. The row stays flagged (`has_secrets`). The body is now rewritten instead of being kept as-is. Rename the test to `xml_body_with_jwt_flagged_and_rewritten`.
  - `no_secret_value_in_any_written_bru` gains the SOAP fixture.
- [ ] **Step 2: Run** `cargo test -p plover-core --test har`. Expect FAIL: the XML body is unchanged.
- [ ] **Step 3: Implement.**
  - In `entry_body`, `Body::Xml(t)` becomes `Body::Xml(text_with_secrets(&t, alloc))`, and `Body::Text` likewise. `Body::Json` goes through `json_with_secrets` first, then through `text_with_secrets` for JWT spans only.
  - `has_secrets` uses `!body_credential_spans(t).is_empty()` in place of its own JWT/JSON check, keeping the form and params checks.
  - The dialog row's tooltip changes from "looks like it holds a JWT" to "credentials found; they become secret variables".
- [ ] **Step 4: Run** `cargo test -p plover-core --test har && (cd frontend && npx vitest run src/components/import-har.test.tsx)`. Expect PASS.
- [ ] **Step 5: Gate and commit:** `feat(core): import XML and text body credentials as secret variables`.

---

### Task 3: Masker by-name pass, token headers and JWTs

**Files:**
- Modify: `crates/core/src/masking.rs`
- Test: `crates/core/tests/masking.rs`
- Modify: `docs/superpowers/specs/2026-10-23-run-history-design.md` §"Masking", "Limits". Add an "Amended by the follow-up plan" note listing the new rules.

**Interfaces:**
- Consumes: `body_credential_spans`, `jwt_spans`, `is_credential_field`, `is_token_header`, `is_token_query_param`.
- Produces:
  - `Masker::new(secrets, results)` keeps its signature. Pass 1 also collects, as described in Global Constraints:
    - by-name body, form, multipart and query values of 4 characters or more;
    - every JWT span in any result's URL, headers, bodies or console;
    - `TOKEN_HEADERS` values of 8 characters or more.
  - `fn mask_named(&self, text: &mut String)`: by-name span replacement, back to front.
  - `fn mask_query(&self, url: &mut String)`.
  - `is_masked_header` also matches `TOKEN_HEADERS`.

- [ ] **Step 1: Write the failing tests**
  - `xml_password_in_request_masked_by_name`: a request body with `<password>ab</password>` (2 characters) is stored as `<password>***</password>`.
  - `json_number_credential_becomes_masked_string`: `{"pin":1234,"count":1234}` is stored as `{"pin":"***","count":1234}` and still parses as JSON.
  - `response_jwt_never_sent_is_masked`: a login response `{"refresh_token":"eyJ…sig","x":"eyJ…sig2"}` masks both, the first by name and the second as a JWT.
  - `x_api_key_masked_whole_and_by_value`: request header `X-Api-Key: tok-AAAAAAAA`, with the same value echoed in a later response body. Both are `***`.
  - `query_credential_masked_by_name`: the URL `https://example.test/a?password=ab&page=2` is stored as `?password=***&page=2`.
  - `short_named_value_not_masked_elsewhere`: with password `ab`, the text `about` in a response body stays `about`.
  - Existing tests still pass, unchanged.
- [ ] **Step 2: Run** `cargo test -p plover-core --test masking`. Expect FAIL.
- [ ] **Step 3: Implement.**
  - `mask_body` applies `mask_named` (Text), credential-named form fields and multipart text parts (whole `***`), then the value pass.
  - `mask_result` applies `mask_query` to `result.url` and to the request and response URLs.
  - Order: by-name first, then by value.
  - `has_masked_header` covers the new headers, so the early return in `mask_result` stays correct.
- [ ] **Step 4: Run** `cargo test -p plover-core --test masking --test report --test run_history`. Expect PASS. Reports are built from masked records.
- [ ] **Step 5: Gate and commit:** `feat(core): mask credentials by name in bodies and URLs, token headers and JWTs`.

---

### Task 4: Data-file credentials masked by name in rows

**Files:**
- Modify: `crates/core/src/masking.rs` (`mask_iteration`)
- Test: `crates/core/tests/masking.rs`, `crates/core/tests/data_runs.rs`, `crates/cli/tests/data_driven.rs`
- Modify: `docs/data-driven-report.md`: mark gap 4 as fixed by this plan.

**Interfaces:**
- Consumes: `is_credential_field`. Task 3's by-name body and query pass masks the sent request.
- Produces: `mask_iteration` sets every credential-named key at any depth to the string `"***"`, whatever its type and length. Other keys keep the value pass. `data_file::credential_values` is unchanged: strings only, and the masker drops those under 4 characters.

- [ ] **Step 1: Write the failing tests**
  - `numeric_credential_masked_by_name_only`:
    - The row is `{"pin":1234,"user":"a"}`. The request body `{"pin":{{pin}}}` is sent as `{"pin":1234}`, and the response is `{"count":1234}`.
    - The stored row is `{"pin":"***","user":"a"}`, the stored request body is `{"pin":"***"}`, and the response keeps `1234`.
  - `short_string_credential_masked_in_row_and_form`: the row is `{"password":"ab"}`. The form body `password=ab` is masked. The row is masked. The text `about` elsewhere stays.
  - `verbose_row_masks_numeric_password` (CLI): `plv run --csv-file-path` with the column `password` set to `12`. The `--verbose` row prints `password: ***`.
- [ ] **Step 2: Run** `cargo test -p plover-core --test masking --test data_runs && cargo test -p plover-cli`. Expect FAIL on the row.
- [ ] **Step 3: Implement** the by-name walk in `mask_iteration`. It runs before `mask_value`.
- [ ] **Step 4: Run** the same commands. Expect PASS.
- [ ] **Step 5: Gate and commit:** `feat(core): mask numeric and short data-file credentials by name`.

---

### Task 5: List, delete and clear runtime variables (core + server)

**Files:**
- Modify: `crates/core/src/workspace.rs`
- Modify: `crates/server/src/api/vars.rs`, `crates/server/src/api/mod.rs`
- Test: `crates/server/tests/runtime_vars.rs` (new); `crates/core/tests/workspace.rs`
- Modify: `frontend/src/api/types.ts` (generated)

**Interfaces:**
- Produces:
  - Core:
    - `OpenCollection::remove_runtime_var(&self, name: &str) -> bool`;
    - `OpenCollection::clear_runtime(&self)`.
  - Server: `fn shown_value(text: String, masked: bool) -> (Option<String>, bool)`, extracted from `explain` and used by both handlers.
  - Routes:

    | Route | Reply |
    |---|---|
    | `GET /collections/{id}/runtime-vars?env=<name>` | `RuntimeVarsDto { vars: Vec<VarDto> }`, sorted by name, `source: "runtime"`, `shadows: []` |
    | `DELETE /collections/{id}/runtime-vars/{name}` | `204`, also when the name was not set |
    | `DELETE /collections/{id}/runtime-vars` | `204` |

  - The mask rule is the one in Global Constraints.

- [ ] **Step 1: Write the failing tests**
  - `list_masks_credential_name_sent_value_and_secret_value`:
    - The runtime vars are `token=x1` (masked by name), `docId=abc12345` (masked because it was sent as `Bearer abc12345` earlier through `note_sent_credentials`), and `copy=s3cr3t-1` (masked because it equals the env secret).
    - `plain=hello` is shown.
  - `list_truncates_at_1kib`.
  - `delete_one_is_idempotent_and_next_send_misses_it`: set `a`, delete it, delete it again (`204` both times). The next send's `{{a}}` is undefined.
  - `clear_all_then_list_empty`.
  - `clear_during_run_keeps_only_run_changes` (Review Focus 4):
    - Start a run whose second request does `bru.setVar("late", 1)`.
    - Clear all while the first request waits on a wiremock delay.
    - After the run, the list has `late` and nothing else.
  - `routes_need_session_token`: all three routes return 401 without the token.
- [ ] **Step 2: Run** `cargo test -p plover-server --test runtime_vars`. Expect FAIL with 404 or 405.
- [ ] **Step 3: Implement the handlers.** Run `PLOVER_UPDATE_BINDINGS=1 cargo test -p plover-server --test bindings` and keep the new `RuntimeVarsDto`.
- [ ] **Step 4: Run** `cargo test -p plover-server && cargo test -p plover-core --test workspace`. Expect PASS.
- [ ] **Step 5: Gate and commit:** `feat(server): list, delete and clear runtime variables`.

---

### Task 6: Runtime-variable panel (frontend)

**Files:**
- Create: `frontend/src/components/RuntimeVarsPanel.tsx`
- Modify: `frontend/src/App.tsx` (top bar, next to `EnvSelector`), `frontend/src/api/client.ts`, `frontend/src/store/vars.ts`, `frontend/src/components/icons.tsx`, `frontend/src/styles.css`
- Test: `frontend/src/components/runtime-vars.test.tsx` (new)

**Interfaces:**
- Consumes: Task 5 routes.
- Produces:
  - API:
    - `api().runtimeVars(collectionId, env): Promise<RuntimeVarsDto>`;
    - `api().deleteRuntimeVar(collectionId, name): Promise<void>`;
    - `api().clearRuntimeVars(collectionId): Promise<void>`.
  - Store: `useVars.refreshCollection(collectionId: string): void`, exported from `store/vars.ts`; `watchVars` reuses it.
  - Panel:
    - A `{x}` SVG icon button, `aria-label="Runtime variables"`, `aria-haspopup="dialog"`, right of the environment label.
    - It opens a non-modal popover (`role="dialog"`, `aria-label="Runtime variables"`) with rows of name and value.
    - A masked value shows `••••` and `aria-label="masked"`.
    - Each row has a copy-name icon and a remove icon (`aria-label="Remove {name}"`).
    - Footer: "Clear all" opens the existing `Modal`: "Clear N runtime variables?" with Cancel / Clear.
    - Empty state: "No runtime variables. Scripts and Save as variable add them."
- *Claude's call:* the panel sits next to the environment label, as Bruno puts its variables viewer near the environment picker.

- [ ] **Step 1: Write the failing tests**
  - `opens_and_lists_with_masked_values`.
  - `remove_row_calls_delete_and_refreshes_hover`: after delete, `useVars.refreshCollection` was called for the collection.
  - `clear_all_asks_first`: Cancel sends nothing, Clear sends `clearRuntimeVars`.
  - `refetches_when_a_send_finishes_while_open`.
  - `escape_closes_and_returns_focus`.
  - `saved_note_then_panel_shows_var`: after `ValueMenu` saves `docId`, opening the panel lists `docId`.
- [ ] **Step 2: Run** `cd frontend && npx vitest run src/components/runtime-vars.test.tsx`. Expect FAIL.
- [ ] **Step 3: Implement.** The panel joins `OVERLAY` in App (it uses `role="dialog"`), so Esc does not also cancel a send.
- [ ] **Step 4: Run** the same command. Expect PASS. Also run `npx vitest run src/components/a11y.test.tsx`.
- [ ] **Step 5: Gate and commit:** `feat(ui): view and clear runtime variables`.

---

### Task 7: Folder context menu with "Import HAR…"

**Files:**
- Create: `frontend/src/components/ContextMenu.tsx`
- Modify: `frontend/src/components/Menu.tsx` (export `menuKeyDown` for reuse), `frontend/src/components/Sidebar.tsx`, `frontend/src/components/ImportHarDialog.tsx`, `frontend/src/components/SidebarHeader.tsx` (no behaviour change)
- Test: `frontend/src/components/folder-menu.test.tsx` (new); `frontend/src/components/ux.test.tsx` (right-click no longer runs straight away)

**Interfaces:**
- Produces:
  - `ContextMenu({ at: {x: number; y: number}, items: MenuItem[], label: string, onClose: () => void })`:
    - fixed position, clamped to the viewport;
    - the first item is focused;
    - arrow keys, Home and End work as in `Menu`;
    - Esc, Tab and a click outside close it and call `onClose`; the caller puts focus back.
  - `ImportHarDialog({ collectionId, onClose, initialFolder?: string })`: `folder` state starts at `initialFolder ?? ""`.
  - Folder rows and the collection header open the menu. A folder row opens it on `contextmenu`, `Shift+F10` or the `ContextMenu` key.

    | Where | Items |
    |---|---|
    | Folder | "Run folder" (only when `onRun`), "Import HAR…" |
    | Collection header | "Run collection", "Import HAR…" (into the root) |

  - The ▶ buttons are unchanged (user decision).

- [ ] **Step 1: Write the failing tests**
  - `right_click_folder_opens_menu`: the menu has Run folder and Import HAR…, and nothing ran.
  - `import_har_from_folder_preselects_it`: choosing Import HAR… on `auth/v2` opens the dialog with the folder select set to `auth/v2`, and the import body has `folder: "auth/v2"`.
  - `shift_f10_opens_menu_for_focused_folder`.
  - `context_menu_escape_runs_nothing` (Review Focus 5): focus returns to the row.
  - `collection_header_menu_imports_into_root`.
  - Update `ux.test.tsx` "runs a folder without right-click", which stays green, and any test that relied on right-click running.
- [ ] **Step 2: Run** `cd frontend && npx vitest run src/components/folder-menu.test.tsx src/components/ux.test.tsx`. Expect FAIL.
- [ ] **Step 3: Implement.** `Sidebar` owns `{collectionId, folder} | null` for the dialog and renders `ImportHarDialog` itself.
- [ ] **Step 4: Run** the same command plus `src/components/import-har.test.tsx` and `src/components/a11y.test.tsx`. Expect PASS.
- [ ] **Step 5: Gate and commit:** `feat(ui): folder context menu with Import HAR into that folder`.

---

### Task 8: Polish — steady URL field width, Esc stops a run

**Files:**
- Modify: `frontend/src/components/request/UrlBar.tsx`, `frontend/src/styles.css`, `frontend/src/App.tsx`
- Test: `frontend/src/components/request/cancel.test.tsx`, `frontend/src/components/run/run-tab.test.tsx`

**Interfaces:**
- Produces:
  - Send and Cancel render inside one `<span className="send-slot">`. The slot has a fixed `inline-size: 9.5em`, and both buttons fill it (`inline-size: 100%`), so the URL input keeps its width.
  - App Esc: when the active tab is a run tab whose run is `running` and not from history, call `useRuns.getState().cancel(runId)`. The overlay and `defaultPrevented` checks stay. The run tab's Cancel button gets `title="Cancel (Esc)"` and `aria-keyshortcuts="Escape"`.

- [ ] **Step 1: Write the failing tests**
  - `send_and_cancel_share_slot`: both render as the only child of `.send-slot`. The CSS rule is asserted through a `styles.css` text check: `.send-slot` has `inline-size: 9.5em`.
  - `escape_cancels_running_run_tab`: a run tab with `status: "running"`, Esc calls `cancel(runId)`.
  - `escape_in_run_tab_with_menu_open_does_nothing`.
  - `escape_on_finished_or_history_run_does_nothing`.
- [ ] **Step 2: Run** `cd frontend && npx vitest run src/components/request/cancel.test.tsx src/components/run/run-tab.test.tsx`. Expect FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same command. Expect PASS.
- [ ] **Step 5: Gate and commit:** `fix(ui): keep the URL field width while sending; Esc stops a run`.

---

### Task 9: Live check, docs and report

**Files:**
- Create: `docs/follow-up-gaps-report.md` (Vietnamese summary at the top)
- Modify: `README.md`: features list. Note runtime vars and folder Import HAR.

- [ ] **Step 1: Start the app.** Use release `plv serve` with a temporary `PLOVER_STATE_DIR`, `PLOVER_KEYCHAIN_SERVICE=dev.plover.test`, a copy of `fixtures/phase3-chain`, and a local mock (`example.test` mapped to the mock) with a slow endpoint.
- [ ] **Step 2: Drive it with agent-browser,** in light and dark. Save screenshots to the session scratchpad folder `follow-up-shots/`.
  1. Right-click a folder, choose Import HAR…, and import the SOAP fixture. The created `.bru` shows `{{password}}`. Its secret name is shown and its value is never shown.
  2. Run the folder with a JSON data file that has `pin: 1234`. In the run detail, `"pin": "***"` and the row chip shows `***`. Export JSON contains no `1234` next to `pin`.
  3. Open Runtime variables after login: `token` is masked. Remove one, then Clear all with the confirm dialog. The hover on `{{token}}` shows "Not set yet…" again.
  4. Start a slow send. The URL field width is the same before and during the send (compare `getBoundingClientRect().width`).
  5. Start a slow run and press Esc. The run shows cancelled.
- [ ] **Step 3: Check for leaks.** `grep -r` the state dir and the exported reports for the fixture secret values. Expect no match. Delete the temp state and keychain items.
- [ ] **Step 4: Write the report:** commits, gate counts, screenshots, deviations, gaps.
- [ ] **Step 5: Gate and commit:** `docs: report the follow-up gap fixes`.
