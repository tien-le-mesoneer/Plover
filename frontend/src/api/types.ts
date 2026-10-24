// Generated from crates/server/src/api by ts-rs. Do not edit.
// Regenerate: PLOVER_UPDATE_BINDINGS=1 cargo test -p plover-server --test bindings

export type KeyValueDto = { name: string, value: string, enabled: boolean, 
/**
 * `@name` request-local variable (vars blocks only).
 */
local: boolean, };

export type MultipartValueDto = { "type": "text", value: string, } | { "type": "files", paths: Array<string>, };

export type MultipartFieldDto = { name: string, value: MultipartValueDto, enabled: boolean, contentType: string | null, };

export type BodyDto = { "mode": "none" } | { "mode": "json", text: string, } | { "mode": "text", text: string, } | { "mode": "xml", text: string, } | { "mode": "formUrlEncoded", fields: Array<KeyValueDto>, } | { "mode": "multipartForm", fields: Array<MultipartFieldDto>, } | { "mode": "unsupported", name: string, };

export type AuthDto = { "mode": "none" } | { "mode": "inherit" } | { "mode": "basic", username: string, password: string, } | { "mode": "bearer", token: string, } | { "mode": "unsupported", name: string, };

export type RequestSettingsDto = { followRedirects: boolean | null, maxRedirects: number | null, timeoutMs: number | null, };

export type HttpRequestDto = { name: string, seq: number, 
/**
 * `meta.type`, usually `http`.
 */
kind: string, 
/**
 * Uppercase method name; any other string is a custom method.
 */
method: string, url: string, query: Array<KeyValueDto>, pathParams: Array<KeyValueDto>, headers: Array<KeyValueDto>, body: BodyDto, 
/**
 * Bodies of the other modes present in the file (Bruno keeps one per mode).
 */
otherBodies: Array<BodyDto>, auth: AuthDto, varsPre: Array<KeyValueDto>, varsPost: Array<KeyValueDto>, scriptPre: string | null, scriptPost: string | null, tests: string | null, 
/**
 * The `assert` block: `name` is the left-hand expression, `value` is `operator rhs`.
 */
asserts: Array<KeyValueDto>, settings: RequestSettingsDto, };

export type CollectionSummaryDto = { id: string, name: string, root: string, 
/**
 * `false` for a remembered collection that could not be reopened (moved, deleted).
 */
available: boolean, 
/**
 * Why an unavailable collection could not be opened.
 */
error: string | null, };

export type RecentDto = { 
/**
 * Identifies the entry (`DELETE /api/recent?root=`).
 */
root: string, 
/**
 * For display and for reopening (`POST /api/collections`).
 */
path: string, name: string, 
/**
 * Unix milliseconds.
 */
lastOpened: number, 
/**
 * Currently open.
 */
open: boolean, };

export type UiStateDto = { 
/**
 * Item IDs of the open request tabs, in order.
 */
tabs: Array<string>, active: string | null, 
/**
 * Item IDs of expanded folders.
 */
expanded: Array<string>, 
/**
 * Selected environment.
 */
env: string | null, };

export type TreeNodeKind = "folder" | "request" | "invalid";

export type TreeNodeDto = { kind: TreeNodeKind, 
/**
 * `/`-separated path relative to the collection root; `""` for the root.
 */
id: string, name: string, method: string | null, seq: number | null, 
/**
 * Parse error of an invalid file.
 */
error: string | null, 
/**
 * Present on folders only.
 */
children: Array<TreeNodeDto> | null, };

export type EnvironmentDto = { name: string, vars: Array<KeyValueDto>, secretNames: Array<string>, 
/**
 * Secrets that have a value and where each is kept (values are never sent).
 */
secretsSet: Array<SecretStateDto>, hash: string, };

export type ResponseDto = { status: number, headers: Array<[string, string]>, elapsedMs: number, 
/**
 * Decoded body length in bytes. When `sizeComplete` is false, reading
 * stopped at the server's cap and this is the number of bytes read.
 */
size: number, sizeComplete: boolean, 
/**
 * The body when it is valid UTF-8.
 */
bodyText: string | null, 
/**
 * The body otherwise.
 */
bodyBase64: string | null, 
/**
 * The body was cut to the first 10 MiB.
 */
truncated: boolean, };

export type TestResultDto = { name: string, passed: boolean, error: string | null, };

export type AssertResultDto = { lhs: string, rhs: string, operator: string, passed: boolean, error: string | null, };

export type ConsoleLineDto = { 
/**
 * `log`, `info`, `warn`, `error` or `debug`.
 */
level: string, text: string, };

export type ScriptedRequestDto = { 
/**
 * `sendRequest` or `axios`.
 */
source: string, method: string, url: string, status: number | null, error: string | null, durationMs: number, };

export type ScriptErrorDto = { 
/**
 * `pre-request`, `post-response` or `tests`.
 */
phase: string, 
/**
 * `thrown`, `forbiddenModule`, `timeout`, `outOfMemory`, `unsettled` or `internal`.
 */
kind: string, 
/**
 * Error name for thrown errors, e.g. `TypeError`.
 */
name: string | null, message: string, 
/**
 * `collection`, `folder <name>` or `request`.
 */
scope: string | null, 
/**
 * 1-based line within that scope's script.
 */
line: number | null, column: number | null, };

export type SendResultDto = { response: ResponseDto | null, tests: Array<TestResultDto>, asserts: Array<AssertResultDto>, console: Array<ConsoleLineDto>, scriptError: ScriptErrorDto | null, 
/**
 * Requests the scripts sent with `bru.sendRequest` or `axios`.
 */
scriptedRequests: Array<ScriptedRequestDto>, 
/**
 * Why the request was not sent (`bru.runner.skipRequest()`).
 */
skipped: string | null, };

export type OpenCollectionBody = { path: string, };

export type StoredRequestDto = { request: HttpRequestDto, hash: string, };

export type SaveRequestBody = { request: HttpRequestDto, 
/**
 * Hash of the file as last read; a mismatch returns 409.
 */
hash: string, };

export type SavedRequestDto = { request: HttpRequestDto, hash: string, };

export type HashDto = { hash: string, };

export type SendBody = { 
/**
 * The request file; `null` sends `draft` as a scratch request (collection scope only).
 */
item: string | null, draft: HttpRequestDto | null, env: string | null, };

export type SaveEnvironmentBody = { vars: Array<KeyValueDto>, hash: string, };

export type SecretsBody = { 
/**
 * Merged into the current values: a string sets a secret, `null` clears it.
 */
values: { [key in string]: string | null }, };

export type SecretStateDto = { name: string, 
/**
 * `"keychain"` or `"memory"` (kept only until Plover quits).
 */
backend: "keychain" | "memory", };

export type SecretsResultDto = { warnings: Array<string>, };

export type ConflictDto = { text: string, hash: string, };

export type ApiErrorDto = { error: string, message: string, 
/**
 * Present on 409 `conflict`.
 */
current: ConflictDto | null, };

export type RunReportDto = { passed: number, failed: number, skipped: number, testsPassed: number, testsFailed: number, assertsPassed: number, assertsFailed: number, elapsedMs: number, };

export type RunMetaDto = { recordId: string, 
/**
 * `app` or `plv`.
 */
source: string, 
/**
 * What ran, relative to the collection root; `.` for the whole collection.
 */
target: string, env: string | null, startedAtMs: number, 
/**
 * `completed` or `cancelled`.
 */
outcome: string, report: RunReportDto, };

export type RunDto = { meta: RunMetaDto, label: string, events: Array<RunEventDto>, };

export type PartDetailDto = { name: string, 
/**
 * Text parts only.
 */
value: string | null, 
/**
 * File parts only: the file name, never its contents.
 */
fileName: string | null, contentType: string | null, };

export type BodyDetailDto = { 
/**
 * `none`, `text`, `base64`, `form`, `multipart` or `omitted`.
 */
kind: string, text: string | null, base64: string | null, fields: Array<[string, string]>, parts: Array<PartDetailDto>, 
/**
 * The whole body's size in bytes.
 */
size: number, 
/**
 * Only the first 1 MiB was kept.
 */
truncated: boolean, 
/**
 * Why the body was not kept (`kind == "omitted"`).
 */
omitted: string | null, };

export type RequestDetailDto = { method: string, url: string, headers: Array<[string, string]>, body: BodyDetailDto, };

export type RunResultDetailDto = { index: number, item: string, name: string, 
/**
 * `pass`, `fail`, `error` or `skipped`.
 */
status: string, error: string | null, skipReason: string | null, 
/**
 * `None` when the request was never sent.
 */
request: RequestDetailDto | null, 
/**
 * How the response body was kept (cut at 1 MiB, or omitted), when there was one.
 */
responseBody: BodyDetailDto | null, result: SendResultDto, };

export type RunEventDto = { "type": "accepted", runId: string, recordId: string, } | { "type": "started", runId: string, item: string, name: string, method: string, url: string, } | { "type": "passed", runId: string, 
/**
 * The result's position in the run (a request run twice has two).
 */
index: number, item: string, status: number, elapsedMs: number, size: number, tests: Array<TestResultDto>, asserts: Array<AssertResultDto>, console: Array<ConsoleLineDto>, } | { "type": "failed", runId: string, index: number, item: string, error: string, status: number | null, elapsedMs: number | null, tests: Array<TestResultDto>, asserts: Array<AssertResultDto>, console: Array<ConsoleLineDto>, scriptError: ScriptErrorDto | null, } | { "type": "skipped", runId: string, index: number, item: string, reason: string, } | { "type": "finished", runId: string, report: RunReportDto, } | { "type": "cancelled", runId: string, } | { "type": "error", runId: string | null, error: string, message: string, };

export type ClientMessage = { "type": "start", runId: string, collectionId: string, item: string, env: string | null, } | { "type": "cancel", runId: string, };

export type CapabilitiesDto = { 
/**
 * Running inside the desktop app.
 */
desktop: boolean, nativePicker: boolean, openExternal: boolean, 
/**
 * Where secrets are kept: `"keychain"` or `"memory"`.
 */
secretBackend: "keychain" | "memory", };

export type PickedFolderDto = { 
/**
 * `null` when the user cancelled.
 */
path: string | null, };

export type ExchangeBody = { 
/**
 * The single-use launch code from the URL fragment (`#code=…`).
 */
code: string, };

export type ClientReadyBody = { error?: string, };

export type SessionTokenDto = { token: string, };

export type ServerEvent = { "type": "collection_opened", id: string, } | { "type": "reload_requested" };

export type CurlParseBody = { text: string, };

export type CurlNoticeDto = { option: string, reason: string, };

export type CurlParsedDto = { request: HttpRequestDto, notices: Array<CurlNoticeDto>, };

export type CopyCurlBody = { 
/**
 * The request file; `null` for a scratch request (then `draft` is required).
 */
item: string | null, draft: HttpRequestDto | null, env: string | null, 
/**
 * Fill in variables for `env` (no scripts run); otherwise copy as written.
 */
resolve: boolean, };

export type CurlTextDto = { text: string, 
/**
 * A secret value of the environment appears in `text`.
 */
containsSecret: boolean, 
/**
 * What was left out because Plover cannot send it.
 */
omitted: Array<string>, };

export type CreateRequestBody = { 
/**
 * The folder ID (`""` is the collection root).
 */
folder: string, 
/**
 * The display name; the file name is made from it.
 */
name: string, request: HttpRequestDto, };

export type CreatedRequestDto = { item: string, request: HttpRequestDto, hash: string, };

export type HarPreviewBody = { 
/**
 * The HAR file's text.
 */
har: string, };

export type HarRowDto = { index: number, method: string, url: string, host: string, status: number | null, mime: string, size: number | null, hasSecrets: boolean, 
/**
 * Why the row is hidden and unticked by default; `null` when it is shown.
 */
hidden: "image" | "script" | "style" | "font" | "media" | "sourceMap" | "preflight" | "dataUrl" | "extension" | "webSocket" | null, duplicates: number, };

export type HarImportBody = { har: string, entries: number[], folder: string, newFolder: string | null, env: string | null, };

export type HarPlanDto = { env: string, secretNames: Array<string>, requestNames: Array<string>, notices: Array<string>, };

export type HarImportResultDto = { 
/**
 * Item IDs of the request files written.
 */
items: Array<string>, 
/**
 * Secret variables created (names only).
 */
secretNames: Array<string>, env: string, notices: Array<string>, 
/**
 * The step that failed; what was written before it stays.
 */
failedAt: string | null, };
