// The only module that talks to the backend (fetch + WebSocket).
import type {
  ApiErrorDto,
  CapabilitiesDto,
  ClientMessage,
  CollectionSummaryDto,
  EnvironmentDto,
  HashDto,
  HttpRequestDto,
  KeyValueDto,
  PickedFolderDto,
  RecentDto,
  RunEventDto,
  SavedRequestDto,
  SecretsResultDto,
  SendResultDto,
  ServerEvent,
  SessionTokenDto,
  StoredRequestDto,
  TreeNodeDto,
  UiStateDto,
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
  clientReady(): Promise<void>;
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
  send(
    id: string,
    item: string,
    draft: HttpRequestDto | null,
    env: string | null,
  ): Promise<SendResultDto>;
  environments(id: string): Promise<EnvironmentDto[]>;
  saveEnvironment(id: string, name: string, vars: KeyValueDto[], hash: string): Promise<HashDto>;
  /** Merges secret values; `null` clears one. Warns for values kept in memory only. */
  setSecrets(id: string, name: string, values: Record<string, string | null>): Promise<SecretsResultDto>;
  startRun(
    id: string,
    item: string,
    env: string | null,
    onEvent: (e: RunEventDto) => void,
  ): RunHandle;
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

  async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
    const headers: Record<string, string> = { authorization: `Bearer ${token}` };
    if (body !== undefined) headers["content-type"] = "application/json";
    const res = await doFetch(`${baseUrl}/api${path}`, {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
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
    clientReady: () => call("POST", "/client-ready", {}),
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
    tree: (id) => call("GET", `${collections(id)}/tree`),
    reload: (id) => call("POST", `${collections(id)}/reload`),
    readRequest: (id, item) => call("GET", `${collections(id)}/requests?item=${enc(item)}`),
    saveRequest: (id, item, request, hash) =>
      call("PUT", `${collections(id)}/requests?item=${enc(item)}`, { request, hash }),
    send: (id, item, draft, env) => call("POST", `${collections(id)}/send`, { item, draft, env }),
    environments: (id) => call("GET", `${collections(id)}/environments`),
    saveEnvironment: (id, name, vars, hash) =>
      call("PUT", `${collections(id)}/environments/${enc(name)}`, { vars, hash }),
    setSecrets: (id, name, values) =>
      call("PUT", `${collections(id)}/environments/${enc(name)}/secrets`, { values }),
    startRun(id, item, env, onEvent) {
      const runId = crypto.randomUUID();
      const socket = new Socket(wsUrl(), [`plover.token.${token}`]);
      let resolve!: () => void;
      const done = new Promise<void>((r) => (resolve = r));
      const send = (msg: ClientMessage) => socket.send(JSON.stringify(msg));
      socket.onopen = () =>
        send({ type: "start", runId, collectionId: id, item, env });
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
