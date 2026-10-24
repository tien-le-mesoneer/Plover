import { describe, expect, it, vi } from "vitest";
import { ApiError, createApiClient, exchangeLaunchCode } from "./client";
import type { RunEventDto } from "./types";

function jsonResponse(status: number, body: unknown): Response {
  return new Response(body === undefined ? null : JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

class FakeSocket {
  static last: FakeSocket | null = null;
  readyState = 0;
  sent: string[] = [];
  closed = false;
  onopen: (() => void) | null = null;
  onmessage: ((e: { data: string }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  constructor(
    public url: string,
    public protocols: string[],
  ) {
    FakeSocket.last = this;
  }
  send(data: string) {
    this.sent.push(data);
  }
  close() {
    this.closed = true;
    this.readyState = 3;
    this.onclose?.();
  }
  open() {
    this.readyState = 1;
    this.onopen?.();
  }
  emit(event: unknown) {
    this.onmessage?.({ data: JSON.stringify(event) });
  }
}

describe("launch codes", () => {
  it("exchanges a code for the token without sending any token", async () => {
    const fetch = vi.fn(async () => jsonResponse(200, { token: "tok" }));
    const token = await exchangeLaunchCode("http://127.0.0.1:4000", "c0de", fetch as unknown as typeof window.fetch);
    expect(token).toBe("tok");
    const [url, init] = (fetch.mock.calls as unknown as [string, RequestInit][])[0]!;
    expect(url).toBe("http://127.0.0.1:4000/api/session/exchange");
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body as string)).toEqual({ code: "c0de" });
    expect(new Headers(init.headers).get("authorization")).toBeNull();
  });

  it("rejects an invalid code", async () => {
    const fetch = vi.fn(async () => jsonResponse(401, { error: "invalid_code", message: "used" }));
    await expect(exchangeLaunchCode("", "x", fetch as unknown as typeof window.fetch)).rejects.toMatchObject({
      code: "invalid_code",
    });
  });
});

describe("ApiClient", () => {
  it("posts client-ready and subscribes to server events", async () => {
    const fetch = vi.fn(async () => jsonResponse(204, undefined));
    const api = createApiClient("http://127.0.0.1:4000", "tok", {
      fetch: fetch as unknown as typeof window.fetch,
      WebSocket: FakeSocket as unknown as typeof WebSocket,
    });
    await api.clientReady();
    const [url, init] = (fetch.mock.calls as unknown as [string, RequestInit][])[0]!;
    expect(`${init.method} ${url}`).toBe("POST http://127.0.0.1:4000/api/client-ready");
    expect(init.body).toBe("{}");
    await api.clientReady("boom");
    expect((fetch.mock.calls as unknown as [string, RequestInit][])[1]![1].body).toBe('{"error":"boom"}');
    const events: unknown[] = [];
    const sub = api.subscribeEvents((e) => events.push(e));
    const socket = FakeSocket.last!;
    expect(socket.url).toBe("ws://127.0.0.1:4000/api/events");
    expect(socket.protocols).toEqual(["plover.token.tok"]);
    socket.open();
    socket.emit({ type: "collection_opened", id: "c9" });
    expect(events).toEqual([{ type: "collection_opened", id: "c9" }]);
    sub.close();
    expect(socket.closed).toBe(true);
  });

  it("calls the capabilities and native routes", async () => {
    const fetch = vi.fn(async (url: string) =>
      url.endsWith("/capabilities")
        ? jsonResponse(200, { desktop: true, nativePicker: true, openExternal: true, nativeSave: true, secretBackend: "keychain" })
        : url.endsWith("/pick-folder")
          ? jsonResponse(200, { path: "/c" })
          : jsonResponse(204, undefined),
    );
    const api = createApiClient("http://127.0.0.1:4000", "tok", { fetch: fetch as unknown as typeof window.fetch });
    expect((await api.capabilities()).secretBackend).toBe("keychain");
    expect(await api.pickFolder()).toBe("/c");
    await api.openInBrowser();
    const calls = fetch.mock.calls as unknown as [string, RequestInit][];
    expect(calls.map(([u, i]) => `${i.method} ${u}`)).toEqual([
      "GET http://127.0.0.1:4000/api/capabilities",
      "POST http://127.0.0.1:4000/api/native/pick-folder",
      "POST http://127.0.0.1:4000/api/native/open-browser",
    ]);
  });

  it("sends the bearer token on every call", async () => {
    const fetch = vi.fn(async () => jsonResponse(200, []));
    const api = createApiClient("http://127.0.0.1:4000", "tok", { fetch });
    await api.listCollections();
    await api.tree("c1");
    for (const call of fetch.mock.calls as unknown as [string, RequestInit][]) {
      expect(new Headers(call[1].headers).get("authorization")).toBe("Bearer tok");
    }
    expect(fetch.mock.calls.map((c) => (c as unknown as [string])[0])).toEqual([
      "http://127.0.0.1:4000/api/collections",
      "http://127.0.0.1:4000/api/collections/c1/tree",
    ]);
  });

  it("encodes item ids into the query", async () => {
    const fetch = vi.fn(async () => jsonResponse(200, { request: {}, hash: "h" }));
    const api = createApiClient("", "tok", { fetch });
    await api.readRequest("c1", "echo/echo json.bru");
    expect((fetch.mock.calls[0] as unknown as [string])[0]).toBe(
      "/api/collections/c1/requests?item=echo%2Fecho%20json.bru",
    );
  });

  it("turns 409 into an ApiError with the conflict body", async () => {
    const body = {
      error: "conflict",
      message: "changed",
      current: { text: "x", hash: "h2" },
    };
    const fetch = vi.fn(async () => jsonResponse(409, body));
    const api = createApiClient("", "tok", { fetch });
    const err = await api
      .saveRequest("c1", "a.bru", {} as never, "h1")
      .catch((e: unknown) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect((err as ApiError).status).toBe(409);
    expect((err as ApiError).code).toBe("conflict");
    expect((err as ApiError).body).toEqual(body);
  });

  it("tolerates non-JSON error bodies", async () => {
    const fetch = vi.fn(async () => new Response("Failed to parse the request body", { status: 400 }));
    const api = createApiClient("", "tok", { fetch });
    const err = await api.tree("c1").catch((e: unknown) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect((err as ApiError).status).toBe(400);
    expect((err as ApiError).code).toBe("http_400");
    expect((err as ApiError).message).toBe("Failed to parse the request body");
  });

  it("setSecrets accepts 204 without a body", async () => {
    const fetch = vi.fn(async () => new Response(null, { status: 204 }));
    const api = createApiClient("", "tok", { fetch });
    await expect(api.setSecrets("c1", "local", { a: "b" })).resolves.toBeUndefined();
  });

  it("startRun uses the token subprotocol and forwards events", async () => {
    const api = createApiClient("http://127.0.0.1:4000", "tok", {
      WebSocket: FakeSocket as unknown as typeof WebSocket,
    });
    const events: RunEventDto[] = [];
    const run = api.startRun("c1", "", "local", (e) => events.push(e));
    const socket = FakeSocket.last!;
    expect(socket.url).toBe("ws://127.0.0.1:4000/api/ws");
    expect(socket.protocols).toEqual(["plover.token.tok"]);
    socket.open();
    const start = JSON.parse(socket.sent[0]!);
    expect(start).toMatchObject({ type: "start", collectionId: "c1", item: "", env: "local" });
    expect(start.runId).toBe(run.runId);
    socket.emit({ type: "started", runId: run.runId, iteration: 0, item: "a.bru", name: "a", method: "GET", url: "u" });
    socket.emit({ type: "started", runId: "other", iteration: 0, item: "x", name: "x", method: "GET", url: "u" });
    socket.emit({
      type: "finished",
      runId: run.runId,
      report: { passed: 1, failed: 0, skipped: 0, elapsedMs: 3 },
    });
    await run.done;
    expect(events.map((e) => e.type)).toEqual(["started", "finished"]);
    expect(socket.closed).toBe(true);
  });

  it("forwards error events, including ones without a runId", async () => {
    const api = createApiClient("http://127.0.0.1:4000", "tok", {
      WebSocket: FakeSocket as unknown as typeof WebSocket,
    });
    const events: RunEventDto[] = [];
    const run = api.startRun("c1", "", "nope", (e) => events.push(e));
    const socket = FakeSocket.last!;
    socket.open();
    socket.emit({ type: "error", runId: run.runId, error: "run_failed", message: "environment `nope` not found" });
    await run.done;
    expect(events).toEqual([
      { type: "error", runId: run.runId, error: "run_failed", message: "environment `nope` not found" },
    ]);
    expect(socket.closed).toBe(true);

    const other: RunEventDto[] = [];
    const run2 = api.startRun("c1", "", null, (e) => other.push(e));
    const socket2 = FakeSocket.last!;
    socket2.open();
    socket2.emit({ type: "error", runId: null, error: "bad_message", message: "x" });
    await run2.done;
    expect(other.map((e) => e.type)).toEqual(["error"]);
  });

  it("cancel sends cancel then closes", async () => {
    const api = createApiClient("https://localhost:4000", "tok", {
      WebSocket: FakeSocket as unknown as typeof WebSocket,
    });
    const run = api.startRun("c1", "f", null, () => {});
    const socket = FakeSocket.last!;
    expect(socket.url).toBe("wss://localhost:4000/api/ws");
    socket.open();
    run.cancel();
    expect(JSON.parse(socket.sent[1]!)).toEqual({ type: "cancel", runId: run.runId });
    expect(socket.closed).toBe(true);
    await run.done;
  });
});

describe("import and scratch calls", () => {
  it("uses the exact paths and bodies", async () => {
    const fetch = vi.fn(async () => jsonResponse(200, {}));
    const api = createApiClient("", "tok", { fetch });
    const draft = { name: "x" } as unknown as import("./types").HttpRequestDto;
    await api.parseCurl("curl x");
    await api.copyCurl("c1", { item: null, draft, env: "dev", resolve: true });
    await api.createRequest("c1", "f", "GET a", draft);
    await api.previewHar("c1", "{}");
    const har = { har: "{}", entries: [1], folder: "", newFolder: "cap", env: null };
    await api.planHar("c1", har);
    await api.importHar("c1", har);
    await api.send("c1", null, draft, null);
    const calls = (fetch.mock.calls as unknown as [string, RequestInit][]).map(([u, i]) => [
      `${i.method} ${u}`,
      JSON.parse(i.body as string),
    ]);
    expect(calls).toEqual([
      ["POST /api/curl/parse", { text: "curl x" }],
      ["POST /api/collections/c1/curl", { item: null, draft, env: "dev", resolve: true }],
      ["POST /api/collections/c1/requests/new", { folder: "f", name: "GET a", request: draft }],
      ["POST /api/collections/c1/import/har/preview", { har: "{}" }],
      ["POST /api/collections/c1/import/har/plan", har],
      ["POST /api/collections/c1/import/har", har],
      ["POST /api/collections/c1/send", { item: null, draft, env: null }],
    ]);
  });
});
