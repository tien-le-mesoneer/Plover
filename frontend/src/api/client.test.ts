import { describe, expect, it, vi } from "vitest";
import { ApiError, createApiClient } from "./client";
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

describe("ApiClient", () => {
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
    expect(socket.protocols).toEqual(["brs.token.tok"]);
    socket.open();
    const start = JSON.parse(socket.sent[0]!);
    expect(start).toMatchObject({ type: "start", collectionId: "c1", item: "", env: "local" });
    expect(start.runId).toBe(run.runId);
    socket.emit({ type: "started", runId: run.runId, item: "a.bru", name: "a", method: "GET", url: "u" });
    socket.emit({ type: "started", runId: "other", item: "x", name: "x", method: "GET", url: "u" });
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
