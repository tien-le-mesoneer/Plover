// Generated from crates/server/src/api by ts-rs. Do not edit.
// Regenerate: BRS_UPDATE_BINDINGS=1 cargo test -p brs-server --test bindings

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
otherBodies: Array<BodyDto>, auth: AuthDto, varsPre: Array<KeyValueDto>, varsPost: Array<KeyValueDto>, scriptPre: string | null, scriptPost: string | null, tests: string | null, settings: RequestSettingsDto, };

export type CollectionSummaryDto = { id: string, name: string, root: string, };

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
 * Secret names that have a value in this session (values are never sent).
 */
secretsSet: Array<string>, hash: string, };

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

export type OpenCollectionBody = { path: string, };

export type StoredRequestDto = { request: HttpRequestDto, hash: string, };

export type SaveRequestBody = { request: HttpRequestDto, 
/**
 * Hash of the file as last read; a mismatch returns 409.
 */
hash: string, };

export type SavedRequestDto = { request: HttpRequestDto, hash: string, };

export type HashDto = { hash: string, };

export type SendBody = { item: string, draft: HttpRequestDto | null, env: string | null, };

export type SaveEnvironmentBody = { vars: Array<KeyValueDto>, hash: string, };

export type SecretsBody = { 
/**
 * Merged into the current values: a string sets a secret, `null` clears it.
 */
values: { [key in string]: string | null }, };

export type ConflictDto = { text: string, hash: string, };

export type ApiErrorDto = { error: string, message: string, 
/**
 * Present on 409 `conflict`.
 */
current: ConflictDto | null, };

export type RunReportDto = { passed: number, failed: number, skipped: number, elapsedMs: number, };

export type RunEventDto = { "type": "started", runId: string, item: string, name: string, method: string, url: string, } | { "type": "passed", runId: string, item: string, status: number, elapsedMs: number, size: number, } | { "type": "failed", runId: string, item: string, error: string, } | { "type": "skipped", runId: string, item: string, reason: string, } | { "type": "finished", runId: string, report: RunReportDto, } | { "type": "cancelled", runId: string, } | { "type": "error", runId: string | null, error: string, message: string, };

export type ClientMessage = { "type": "start", runId: string, collectionId: string, item: string, env: string | null, } | { "type": "cancel", runId: string, };
