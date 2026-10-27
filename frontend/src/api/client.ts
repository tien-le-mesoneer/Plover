// The only module that talks to the backend (fetch + WebSocket).
import type {
  ResolvedUrlBody,
  ResolvedUrlDto,
  LayoutDto,
  ApiErrorDto,
  CapabilitiesDto,
  ClientMessage,
  CollectionSummaryDto,
  CopyCurlBody,
  CreatedRequestDto,
  CurlParsedDto,
  CurlTextDto,
  DataFileDto,
  DataPreviewDto,
  EnvironmentDto,
  HarImportBody,
  HarImportResultDto,
  HarPlanDto,
  HarRowDto,
  HashDto,
  HttpRequestDto,
  KeyValueDto,
  PickedFolderDto,
  RecentDto,
  RunDto,
  RunEventDto,
  RunMetaDto,
  RuntimeVarsDto,
  RunResultDetailDto,
  SavedFileDto,
  SavedRequestDto,
  SecretsResultDto,
  SendResultDto,
  ServerEvent,
  SessionTokenDto,
  StoredRequestDto,
  TreeNodeDto,
  UiStateDto,
  VarsBody,
  VarsDto,
} from "./types";

export class ApiError extends Error {
  constructor(
    public status: number,
    public code: string,
    public body: ApiErrorDto | unknown,
    message: string,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

export interface RunHandle {
  runId: string;
  cancel(): void;
  /** Resolves when the run finished, failed to start, was cancelled, or the socket closed. */
  done: Promise<void>;
}

export interface ApiClient {
  /** What this server can do beyond the web API (desktop app features, secret storage). */
  capabilities(): Promise<CapabilitiesDto>;
  /** Native folder picker (desktop only); `null` when cancelled. */
  pickFolder(): Promise<string | null>;
  /** Opens this session in the default browser (desktop only). */
  openInBrowser(): Promise<void>;
  /** Tells the server the page has mounted (the desktop smoke test waits for it). */
  /** Startup finished; `error` says why it failed (the desktop smoke test prints it). */
  clientReady(error?: string): Promise<void>;
  /** Server → UI notifications (collection opened natively, reload requested). */
  subscribeEvents(onEvent: (e: ServerEvent) => void): { close(): void };
  openCollection(path: string): Promise<CollectionSummaryDto>;
  /** Open collections, then remembered ones that could not be reopened (`available: false`). */
  listCollections(): Promise<CollectionSummaryDto[]>;
  /** Closes a collection (or drops an unavailable one); it stays in the recent list. */
  closeCollection(id: string): Promise<void>;
  /** Recently opened collections, newest first. */
  recent(): Promise<RecentDto[]>;
  removeRecent(root: string): Promise<void>;
  /** Remembered layout (tabs, expanded folders, environment) per collection ID. */
  uiState(): Promise<Record<string, UiStateDto>>;
  saveUiState(id: string, ui: UiStateDto): Promise<void>;
  /** The window layout shared by all collections (sidebar width). */
  layout(): Promise<LayoutDto>;
  saveLayout(layout: LayoutDto): Promise<void>;
  tree(id: string): Promise<TreeNodeDto>;
  reload(id: string): Promise<TreeNodeDto>;
  readRequest(id: string, item: string): Promise<StoredRequestDto>;
  /** Resolves to the request as re-read from the written file, with its new hash. */
  saveRequest(
    id: string,
    item: string,
    request: HttpRequestDto,
    hash: string,
  ): Promise<SavedRequestDto>;
  /**
   * `item: null` sends `draft` as a scratch request (collection scope only). `sendId` lets
   * `cancelSend` stop it on the server; `signal` aborts the fetch.
   */
  send(
    id: string,
    item: string | null,
    draft: HttpRequestDto | null,
    env: string | null,
    opts?: SendOptions,
  ): Promise<SendResultDto>;
  /** Stops send `sendId` on the server; resolves whether or not it was still running. */
  cancelSend(id: string, sendId: string): Promise<void>;
  /** A pasted curl command as a request, plus the options it could not fill in. */
  parseCurl(text: string): Promise<CurlParsedDto>;
  copyCurl(id: string, body: CopyCurlBody): Promise<CurlTextDto>;
  /** The URL Send would use, variables filled in and secret values masked. */
  resolvedUrl(id: string, body: ResolvedUrlBody): Promise<ResolvedUrlDto>;
  /** Where each variable a send would use comes from; masked, runs nothing. */
  vars(id: string, body: VarsBody): Promise<VarsDto>;
  /** Sets a runtime variable like `bru.setVar`; the next send sees it. */
  setRuntimeVar(collectionId: string, name: string, value: unknown): Promise<void>;
  /** The runtime variables, masked like `/vars` and against `env`'s secrets. */
  runtimeVars(collectionId: string, env: string | null): Promise<RuntimeVarsDto>;
  /** Removes one runtime variable; resolves also when it was not set. */
  deleteRuntimeVar(collectionId: string, name: string): Promise<void>;
  /** Removes every runtime variable of the collection. */
  clearRuntimeVars(collectionId: string): Promise<void>;
  /** Shows a request file in the file manager (desktop only). */
  reveal(collection: string, item: string): Promise<void>;
  /** Writes a new request file; rejects with code `exists` when the name is taken. */
  createRequest(
    id: string,
    folder: string,
    name: string,
    request: HttpRequestDto,
  ): Promise<CreatedRequestDto>;
  previewHar(id: string, har: string): Promise<HarRowDto[]>;
  /** The secret and request names an import would create; writes nothing. */
  planHar(id: string, body: HarImportBody): Promise<HarPlanDto>;
  importHar(id: string, body: HarImportBody): Promise<HarImportResultDto>;
  environments(id: string): Promise<EnvironmentDto[]>;
  saveEnvironment(id: string, name: string, vars: KeyValueDto[], hash: string): Promise<HashDto>;
  /**
   * Merges secret values; `null` clears one. Warns for values kept in memory only. With
   * `declare`, names that get a value are also added to the environment's `vars:secret`.
   */
  setSecrets(
    id: string,
    name: string,
    values: Record<string, string | null>,
    declare?: boolean,
  ): Promise<SecretsResultDto>;
  startRun(
    id: string,
    item: string,
    env: string | null,
    onEvent: (e: RunEventDto) => void,
    options?: RunOptions,
  ): RunHandle;
  /** Row count and column names of a data file (never its values), or a `bad_data` error. */
  previewDataFile(id: string, file: DataFileDto): Promise<DataPreviewDto>;
  /** The report of a finished, cancelled or saved run, as a file to download. */
  downloadReport(id: string, recordId: string, format: ReportFormat): Promise<{ blob: Blob; fileName: string }>;
  /** Desktop app: saves the report through the native save dialog; `null` when cancelled. */
  saveReport(id: string, recordId: string, format: ReportFormat): Promise<string | null>;
  /** Saved runs of a collection (app and `plv`), newest first. */
  listRuns(id: string): Promise<RunMetaDto[]>;
  /** A saved run as replayed run events. */
  getRun(id: string, recordId: string): Promise<RunDto>;
  /** One result's request and response, masked; live runs answer from memory. */
  runResult(id: string, recordId: string, index: number): Promise<RunResultDetailDto>;
  clearRuns(id: string): Promise<void>;
}

export interface SendOptions {
  sendId?: string;
  signal?: AbortSignal;
}

export type ReportFormat = "html" | "json" | "junit";

/** A data file (one iteration per row) or a repeat count for a run. */
export interface RunOptions {
  data?: DataFileDto;
  iterationCount?: number;
}

interface Deps {
  fetch?: typeof fetch;
  WebSocket?: typeof WebSocket;
}

/** Turns a failed response into an `ApiError` (bodies are `{error, message}` JSON, or not). */
function errorFrom(res: Response, text: string): ApiError {
  let err: Partial<ApiErrorDto> | undefined;
  try {
    err = text ? (JSON.parse(text) as Partial<ApiErrorDto>) : undefined;
  } catch {
    err = undefined;
  }
  return new ApiError(
    res.status,
    err?.error ?? `http_${res.status}`,
    err ?? text,
    err?.message ?? (text || `HTTP ${res.status}`),
  );
}

/**
 * Trades the single-use launch code from `#code=…` for the session token. Sends no
 * token (the page has none yet); the server still checks Host and Origin.
 */
export async function exchangeLaunchCode(
  baseUrl: string,
  code: string,
  fetchImpl: typeof fetch = window.fetch.bind(window),
): Promise<string> {
  const res = await fetchImpl(`${baseUrl}/api/session/exchange`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ code }),
  });
  const text = await res.text();
  if (!res.ok) throw errorFrom(res, text);
  return (JSON.parse(text) as SessionTokenDto).token;
}

const TERMINAL: RunEventDto["type"][] = ["finished", "cancelled", "error"];

export function createApiClient(baseUrl: string, token: string, deps: Deps = {}): ApiClient {
  const doFetch = deps.fetch ?? window.fetch.bind(window);
  const Socket = deps.WebSocket ?? window.WebSocket;
  const enc = encodeURIComponent;

  async function call<T>(method: string, path: string, body?: unknown, signal?: AbortSignal): Promise<T> {
    const headers: Record<string, string> = { authorization: `Bearer ${token}` };
    if (body !== undefined) headers["content-type"] = "application/json";
    const res = await doFetch(`${baseUrl}/api${path}`, {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
      signal,
    });
    const text = await res.text();
    if (!res.ok) throw errorFrom(res, text);
    return (text ? JSON.parse(text) : undefined) as T;
  }

  function wsUrl(): string {
    const url = new URL("/api/ws", baseUrl || window.location.href);
    url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
    return url.toString();
  }

  const collections = (id: string) => `/collections/${enc(id)}`;

  return {
    capabilities: () => call("GET", "/capabilities"),
    pickFolder: async () => (await call<PickedFolderDto>("POST", "/native/pick-folder", {})).path,
    openInBrowser: () => call("POST", "/native/open-browser", {}),
    clientReady: (error) => call("POST", "/client-ready", error ? { error } : {}),
    subscribeEvents(onEvent) {
      const url = new URL("/api/events", baseUrl || window.location.href);
      url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
      const socket = new Socket(url.toString(), [`plover.token.${token}`]);
      socket.onmessage = (e: MessageEvent<string>) => {
        try {
          onEvent(JSON.parse(e.data) as ServerEvent);
        } catch {
          // Ignore malformed frames.
        }
      };
      return { close: () => socket.close() };
    },
    openCollection: (path) => call("POST", "/collections", { path }),
    listCollections: () => call("GET", "/collections"),
    closeCollection: (id) => call("DELETE", collections(id)),
    recent: () => call("GET", "/recent"),
    removeRecent: (root) => call("DELETE", `/recent?root=${enc(root)}`),
    uiState: () => call("GET", "/ui-state"),
    saveUiState: (id, ui) => call("PUT", `/ui-state/${enc(id)}`, ui),
    layout: () => call("GET", "/layout"),
    saveLayout: (layout) => call("PUT", "/layout", layout),
    tree: (id) => call("GET", `${collections(id)}/tree`),
    reload: (id) => call("POST", `${collections(id)}/reload`),
    readRequest: (id, item) => call("GET", `${collections(id)}/requests?item=${enc(item)}`),
    saveRequest: (id, item, request, hash) =>
      call("PUT", `${collections(id)}/requests?item=${enc(item)}`, { request, hash }),
    send: (id, item, draft, env, opts) =>
      call("POST", `${collections(id)}/send`, { item, draft, env, sendId: opts?.sendId ?? null }, opts?.signal),
    cancelSend: (id, sendId) => call("POST", `${collections(id)}/send/${enc(sendId)}/cancel`, {}),
    parseCurl: (text) => call("POST", "/curl/parse", { text }),
    copyCurl: (id, body) => call("POST", `${collections(id)}/curl`, body),
    resolvedUrl: (id, body) => call("POST", `${collections(id)}/resolved-url`, body),
    reveal: (collection, item) => call("POST", "/native/reveal", { collection, item }),
    vars: (id, body) => call("POST", `${collections(id)}/vars`, body),
    setRuntimeVar: (id, name, value) => call("PUT", `${collections(id)}/runtime-vars/${enc(name)}`, { value }),
    runtimeVars: (id, env) =>
      call("GET", `${collections(id)}/runtime-vars${env ? `?env=${enc(env)}` : ""}`),
    deleteRuntimeVar: (id, name) => call("DELETE", `${collections(id)}/runtime-vars/${enc(name)}`),
    clearRuntimeVars: (id) => call("DELETE", `${collections(id)}/runtime-vars`),
    createRequest: (id, folder, name, request) =>
      call("POST", `${collections(id)}/requests/new`, { folder, name, request }),
    previewHar: (id, har) => call("POST", `${collections(id)}/import/har/preview`, { har }),
    planHar: (id, body) => call("POST", `${collections(id)}/import/har/plan`, body),
    importHar: (id, body) => call("POST", `${collections(id)}/import/har`, body),
    environments: (id) => call("GET", `${collections(id)}/environments`),
    saveEnvironment: (id, name, vars, hash) =>
      call("PUT", `${collections(id)}/environments/${enc(name)}`, { vars, hash }),
    setSecrets: (id, name, values, declare) =>
      call(
        "PUT",
        `${collections(id)}/environments/${enc(name)}/secrets`,
        declare ? { values, declare } : { values },
      ),
    listRuns: (id) => call("GET", `${collections(id)}/runs`),
    getRun: (id, recordId) => call("GET", `${collections(id)}/runs/${enc(recordId)}`),
    runResult: (id, recordId, index) =>
      call("GET", `${collections(id)}/runs/${enc(recordId)}/results/${index}`),
    clearRuns: (id) => call("DELETE", `${collections(id)}/runs`),
    previewDataFile: (id, file) => call("POST", `${collections(id)}/data-file/preview`, file),
    async downloadReport(id, recordId, format) {
      const res = await doFetch(
        `${baseUrl}/api${collections(id)}/runs/${enc(recordId)}/report?format=${format}`,
        { method: "GET", headers: { authorization: `Bearer ${token}` } },
      );
      if (!res.ok) throw errorFrom(res, await res.text());
      const disposition = res.headers.get("content-disposition") ?? "";
      const fileName = /filename="([^"]+)"/.exec(disposition)?.[1] ?? `${recordId}.${format === "junit" ? "xml" : format}`;
      return { blob: await res.blob(), fileName };
    },
    saveReport: async (id, recordId, format) =>
      (await call<SavedFileDto>("POST", "/native/save-report", { collectionId: id, runId: recordId, format })).path,
    startRun(id, item, env, onEvent, options) {
      const runId = crypto.randomUUID();
      const socket = new Socket(wsUrl(), [`plover.token.${token}`]);
      let resolve!: () => void;
      const done = new Promise<void>((r) => (resolve = r));
      const send = (msg: ClientMessage) => socket.send(JSON.stringify(msg));
      socket.onopen = () =>
        send({
          type: "start",
          runId,
          collectionId: id,
          item,
          env,
          ...(options?.data ? { data: options.data } : {}),
          ...(options?.iterationCount ? { iterationCount: options.iterationCount } : {}),
        });
      socket.onmessage = (e: MessageEvent) => {
        const event = JSON.parse(String(e.data)) as RunEventDto;
        // One run per socket: errors without a runId (e.g. bad_message) belong to it too.
        const ours = event.runId === runId || (event.type === "error" && event.runId === null);
        if (!ours) return;
        onEvent(event);
        if (TERMINAL.includes(event.type)) socket.close();
      };
      socket.onclose = () => resolve();
      socket.onerror = () => resolve();
      return {
        runId,
        done,
        cancel() {
          if (socket.readyState === 1) send({ type: "cancel", runId });
          socket.close();
        },
      };
    },
  };
}
