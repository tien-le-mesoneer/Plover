// The only module that talks to the backend (fetch + WebSocket).
import type {
  ApiErrorDto,
  ClientMessage,
  CollectionSummaryDto,
  EnvironmentDto,
  HashDto,
  HttpRequestDto,
  KeyValueDto,
  ResponseDto,
  RunEventDto,
  SavedRequestDto,
  StoredRequestDto,
  TreeNodeDto,
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
  openCollection(path: string): Promise<CollectionSummaryDto>;
  listCollections(): Promise<CollectionSummaryDto[]>;
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
  ): Promise<ResponseDto>;
  environments(id: string): Promise<EnvironmentDto[]>;
  saveEnvironment(id: string, name: string, vars: KeyValueDto[], hash: string): Promise<HashDto>;
  /** Merges secret values; `null` clears one. */
  setSecrets(id: string, name: string, values: Record<string, string | null>): Promise<void>;
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
    if (!res.ok) {
      // Error bodies are `{error, message}` JSON, but tolerate anything else.
      let err: Partial<ApiErrorDto> | undefined;
      try {
        err = text ? (JSON.parse(text) as Partial<ApiErrorDto>) : undefined;
      } catch {
        err = undefined;
      }
      throw new ApiError(
        res.status,
        err?.error ?? `http_${res.status}`,
        err ?? text,
        err?.message ?? (text || `HTTP ${res.status}`),
      );
    }
    return (text ? JSON.parse(text) : undefined) as T;
  }

  function wsUrl(): string {
    const url = new URL("/api/ws", baseUrl || window.location.href);
    url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
    return url.toString();
  }

  const collections = (id: string) => `/collections/${enc(id)}`;

  return {
    openCollection: (path) => call("POST", "/collections", { path }),
    listCollections: () => call("GET", "/collections"),
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
      const socket = new Socket(wsUrl(), [`brs.token.${token}`]);
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
