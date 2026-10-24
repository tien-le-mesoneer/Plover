import { beforeEach, describe, expect, it, vi } from "vitest";
import { ApiError, type ApiClient } from "../api/client";
import { setApi } from "../api/instance";
import type { HttpRequestDto } from "../api/types";
import { useTabs } from "./tabs";
import { useWorkspace } from "./workspace";

function request(url: string): HttpRequestDto {
  return {
    name: "r",
    seq: 1,
    kind: "http",
    method: "GET",
    url,
    query: [],
    pathParams: [],
    headers: [],
    body: { mode: "none" },
    otherBodies: [],
    auth: { mode: "none" },
    varsPre: [],
    varsPost: [],
    scriptPre: null,
    scriptPost: null,
    tests: null,
    asserts: [],
    settings: { followRedirects: null, maxRedirects: null, timeoutMs: null },
  };
}

function mockApi(overrides: Partial<ApiClient> = {}): ApiClient {
  return {
    readRequest: vi.fn(async (_id: string, item: string) => ({
      request: request(`http://h/${item}`),
      hash: `hash-${item}`,
    })),
    saveRequest: vi.fn(async (_id: string, _item: string, r: HttpRequestDto) => ({
      hash: "new-hash",
      request: r,
    })),
    tree: vi.fn(async () => ({
      kind: "folder" as const,
      id: "",
      name: "c",
      method: null,
      seq: null,
      error: null,
      children: [],
    })),
    send: vi.fn(),
    ...overrides,
  } as unknown as ApiClient;
}

beforeEach(() => {
  useTabs.setState({ tabs: [], active: null });
  useWorkspace.setState({ collections: [], trees: {}, environments: {}, activeEnv: {} });
});

describe("tabs store", () => {
  it("edit_marks_dirty_and_save_clears", async () => {
    const api = mockApi();
    setApi(api);
    await useTabs.getState().open("c1", "a.bru");
    const key = useTabs.getState().active!;
    expect(useTabs.getState().isDirty(key)).toBe(false);
    useTabs.getState().edit(key, (d) => ({ ...d, url: "http://changed" }));
    expect(useTabs.getState().isDirty(key)).toBe(true);
    expect(await useTabs.getState().save(key)).toBe("saved");
    expect(api.saveRequest).toHaveBeenCalledWith(
      "c1",
      "a.bru",
      expect.objectContaining({ url: "http://changed" }),
      "hash-a.bru",
    );
    const tab = useTabs.getState().tabs[0]!;
    expect(useTabs.getState().isDirty(key)).toBe(false);
    expect(tab.hash).toBe("new-hash");
  });

  it("save_conflict_returns_conflict_and_keeps_draft", async () => {
    const current = { text: "on disk", hash: "disk-hash" };
    const api = mockApi({
      saveRequest: vi.fn(async () => {
        throw new ApiError(409, "conflict", { error: "conflict", message: "m", current }, "m");
      }),
    });
    setApi(api);
    await useTabs.getState().open("c1", "a.bru");
    const key = useTabs.getState().active!;
    useTabs.getState().edit(key, (d) => ({ ...d, url: "mine" }));
    expect(await useTabs.getState().save(key)).toBe("conflict");
    const tab = useTabs.getState().tabs[0]!;
    expect(tab.draft.url).toBe("mine");
    expect(tab.conflict).toEqual(current);
    expect(useTabs.getState().isDirty(key)).toBe(true);

    // Overwrite re-saves against the hash found on disk.
    (api.saveRequest as ReturnType<typeof vi.fn>).mockResolvedValueOnce({ hash: "h3", request: useTabs.getState().tabs[0]!.draft });
    await useTabs.getState().resolveConflict(key, "overwrite");
    expect(api.saveRequest).toHaveBeenLastCalledWith("c1", "a.bru", expect.anything(), "disk-hash");
    expect(useTabs.getState().tabs[0]!.conflict).toBeUndefined();
    expect(useTabs.getState().isDirty(key)).toBe(false);
  });

  it("save adopts the request as re-read from disk", async () => {
    const reread = { ...request("http://h/a.bru"), headers: [{ name: "x", value: "v", enabled: false, local: false }] };
    const api = mockApi({ saveRequest: vi.fn(async () => ({ hash: "h2", request: reread })) });
    setApi(api);
    await useTabs.getState().open("c1", "a.bru");
    const key = useTabs.getState().active!;
    useTabs.getState().edit(key, (d) => ({ ...d, headers: [{ name: "~x", value: "  v  ", enabled: true, local: false }] }));
    await useTabs.getState().save(key);
    const tab = useTabs.getState().tabs[0]!;
    expect(tab.saved).toEqual(reread);
    expect(tab.draft).toEqual(reread);
    expect(useTabs.getState().isDirty(key)).toBe(false);
  });

  it("open_same_item_twice_reuses_tab", async () => {
    const api = mockApi();
    setApi(api);
    await useTabs.getState().open("c1", "a.bru");
    await useTabs.getState().open("c1", "b.bru");
    await useTabs.getState().open("c1", "a.bru");
    expect(useTabs.getState().tabs.map((t) => t.item)).toEqual(["a.bru", "b.bru"]);
    expect(useTabs.getState().active).toBe("c1:a.bru");
    expect(api.readRequest).toHaveBeenCalledTimes(2);
  });

  it("close_removes_and_activates_neighbor", async () => {
    setApi(mockApi());
    for (const item of ["a.bru", "b.bru", "c.bru"]) await useTabs.getState().open("c1", item);
    useTabs.getState().activate("c1:b.bru");
    useTabs.getState().close("c1:b.bru");
    expect(useTabs.getState().tabs.map((t) => t.item)).toEqual(["a.bru", "c.bru"]);
    expect(useTabs.getState().active).toBe("c1:c.bru");
    useTabs.getState().close("c1:c.bru");
    expect(useTabs.getState().active).toBe("c1:a.bru");
    useTabs.getState().close("c1:a.bru");
    expect(useTabs.getState().active).toBeNull();
  });

  it("send uses the collection's active environment and the draft", async () => {
    const response = { status: 200, headers: [], elapsedMs: 1, size: 0, sizeComplete: true, bodyText: "", bodyBase64: null, truncated: false };
    const result = {
      response,
      tests: [{ name: "ok", passed: true, error: null }],
      asserts: [],
      console: [{ level: "log", text: "hi" }],
      scriptError: null,
      scriptedRequests: [],
      skipped: null,
    };
    const api = mockApi({ send: vi.fn(async () => result) });
    setApi(api);
    useWorkspace.getState().selectEnv("c1", "local");
    await useTabs.getState().open("c1", "a.bru");
    const key = useTabs.getState().active!;
    useTabs.getState().edit(key, (d) => ({ ...d, url: "draft" }));
    await useTabs.getState().send(key);
    expect(api.send).toHaveBeenCalledWith("c1", "a.bru", expect.objectContaining({ url: "draft" }), "local", expect.objectContaining({ sendId: expect.any(String) }));
    expect(useTabs.getState().tabs[0]!.result).toEqual(result);
    expect(useTabs.getState().tabs[0]!.sending).toBe(false);
  });

  it("script, test and assert edits are draft changes", async () => {
    setApi(mockApi());
    await useTabs.getState().open("c1", "a.bru");
    const key = useTabs.getState().active!;
    useTabs.getState().edit(key, (d) => ({ ...d, scriptPost: "bru.setVar('a', 1);" }));
    expect(useTabs.getState().isDirty(key)).toBe(true);
    useTabs.getState().edit(key, (d) => ({ ...d, scriptPost: null }));
    expect(useTabs.getState().isDirty(key)).toBe(false);
    useTabs.getState().edit(key, (d) => ({
      ...d,
      asserts: [{ name: "res.status", value: "eq 200", enabled: true, local: false }],
    }));
    expect(useTabs.getState().isDirty(key)).toBe(true);
  });

  it("newScratch opens Untitled GET tab", () => {
    const key = useTabs.getState().newScratch("c1");
    const tab = useTabs.getState().tabs.find((t) => t.key === key)!;
    expect(key).toMatch(/^c1:scratch:\d+$/);
    expect(tab.item).toBeNull();
    expect(tab.draft.name).toBe("Untitled");
    expect(tab.draft.method).toBe("GET");
    expect(tab.draft.auth).toEqual({ mode: "inherit" });
    expect(useTabs.getState().active).toBe(key);
    expect(useTabs.getState().isDirty(key)).toBe(false);
    expect(useTabs.getState().hasScratchContent()).toBe(false);
    useTabs.getState().edit(key, (d) => ({ ...d, url: "http://x" }));
    expect(useTabs.getState().isDirty(key)).toBe(true);
    expect(useTabs.getState().hasScratchContent()).toBe(true);
    expect(useTabs.getState().newScratch("c1")).not.toBe(key);
  });

  it("send on scratch passes item null and draft", async () => {
    const api = mockApi({
      send: vi.fn(async () => ({
        response: null,
        tests: [],
        asserts: [],
        console: [],
        scriptError: null,
        scriptedRequests: [],
        skipped: null,
      })),
    });
    setApi(api);
    const key = useTabs.getState().newScratch("c1");
    useTabs.getState().edit(key, (d) => ({ ...d, url: "http://x/s" }));
    await useTabs.getState().send(key);
    expect(api.send).toHaveBeenCalledWith("c1", null, expect.objectContaining({ url: "http://x/s" }), null, expect.objectContaining({ sendId: expect.any(String) }));
  });

  it("saveScratch creates request and rekeys tab", async () => {
    const created = { ...request("http://x/orders"), name: "GET orders" };
    const api = mockApi({
      createRequest: vi.fn(async () => ({ item: "f/GET orders.bru", request: created, hash: "h1" })),
    });
    setApi(api);
    const key = useTabs.getState().newScratch("c1");
    useTabs.getState().edit(key, (d) => ({ ...d, url: "http://x/orders" }));
    expect(await useTabs.getState().saveScratch(key, "f", "GET orders")).toBe("saved");
    expect(api.createRequest).toHaveBeenCalledWith(
      "c1",
      "f",
      "GET orders",
      expect.objectContaining({ url: "http://x/orders" }),
    );
    const tab = useTabs.getState().tabs[0]!;
    expect(tab.key).toBe("c1:f/GET orders.bru");
    expect(tab.item).toBe("f/GET orders.bru");
    expect(tab.hash).toBe("h1");
    expect(useTabs.getState().active).toBe(tab.key);
    expect(useTabs.getState().isDirty(tab.key)).toBe(false);
  });

  it("saveScratch exists keeps tab and returns exists", async () => {
    const api = mockApi({
      createRequest: vi.fn(async () => {
        throw new ApiError(409, "exists", { error: "exists", message: "m" }, "m");
      }),
    });
    setApi(api);
    const key = useTabs.getState().newScratch("c1");
    useTabs.getState().edit(key, (d) => ({ ...d, url: "http://x" }));
    expect(await useTabs.getState().saveScratch(key, "", "a")).toBe("exists");
    expect(useTabs.getState().tabs[0]!.key).toBe(key);
    expect(useTabs.getState().tabs[0]!.draft.url).toBe("http://x");
  });

  it("applyCurl keeps name scripts tests and returns previous draft", async () => {
    setApi(mockApi());
    await useTabs.getState().open("c1", "a.bru");
    const key = useTabs.getState().active!;
    useTabs.getState().edit(key, (d) => ({ ...d, scriptPre: "x()", tests: "t()" }));
    const before = useTabs.getState().tabs[0]!.draft;
    const parsed = {
      ...request("https://api.test/p?q=1"),
      name: "",
      method: "POST",
      query: [{ name: "q", value: "1", enabled: true, local: false }],
      headers: [{ name: "a", value: "b", enabled: true, local: false }],
      body: { mode: "json" as const, text: "{}" },
      auth: { mode: "basic" as const, username: "u", password: "p" },
    };
    const previous = useTabs.getState().applyCurl(key, parsed);
    expect(previous).toEqual(before);
    const draft = useTabs.getState().tabs[0]!.draft;
    expect(draft).toMatchObject({
      name: "r",
      method: "POST",
      url: "https://api.test/p?q=1",
      query: parsed.query,
      headers: parsed.headers,
      body: parsed.body,
      auth: parsed.auth,
      scriptPre: "x()",
      tests: "t()",
    });
  });
});

describe("cancel", () => {
  const tab = (key = "c1:a.bru") => ({
    key,
    collectionId: "c1",
    item: "a.bru",
    saved: request("http://h/a"),
    draft: request("http://h/a"),
    hash: "h",
    sending: false,
  });
  const ok = { response: null, tests: [], asserts: [], console: [], scriptError: null, scriptedRequests: [], skipped: null };

  it("send_generates_send_id", async () => {
    const send = vi.fn(async () => ok);
    setApi(mockApi({ send }));
    useTabs.setState({ tabs: [tab()], active: "c1:a.bru" });
    await useTabs.getState().send("c1:a.bru");
    const opts = (send.mock.calls[0] as unknown[])[4] as { sendId: string; signal: AbortSignal };
    expect(opts.sendId).toMatch(/^[A-Za-z0-9-]{1,64}$/);
    expect(opts.signal).toBeInstanceOf(AbortSignal);
    expect(useTabs.getState().tabs[0]!.sending).toBe(false);
  });

  it("cancel_posts_route_and_aborts", async () => {
    let signal: AbortSignal | undefined;
    const send = vi.fn(
      (_i: string, _it: string, _d: unknown, _e: unknown, opts: { signal: AbortSignal }) =>
        new Promise((_, reject) => {
          signal = opts.signal;
          opts.signal.addEventListener("abort", () => reject(new DOMException("aborted", "AbortError")));
        }),
    );
    const cancelSend = vi.fn(async () => {});
    setApi(mockApi({ send, cancelSend } as unknown as Partial<ApiClient>));
    useTabs.setState({ tabs: [tab()], active: "c1:a.bru" });
    const sent = useTabs.getState().send("c1:a.bru");
    const sendId = useTabs.getState().tabs[0]!.sendId;
    await useTabs.getState().cancel("c1:a.bru");
    await sent;
    expect(cancelSend).toHaveBeenCalledWith("c1", sendId);
    expect(signal?.aborted).toBe(true);
    const t = useTabs.getState().tabs[0]!;
    expect(t.sending).toBe(false);
    expect(t.result).toBeUndefined();
    expect(t.error).toBeUndefined();
    expect(t.cancelledAfterMs).toEqual(expect.any(Number));
  });

  it("409_cancelled_is_not_an_error", async () => {
    const send = vi.fn(async () => {
      throw new ApiError(409, "cancelled", {}, "Request cancelled");
    });
    setApi(mockApi({ send }));
    useTabs.setState({ tabs: [tab()], active: "c1:a.bru" });
    await useTabs.getState().send("c1:a.bru");
    const t = useTabs.getState().tabs[0]!;
    expect(t.error).toBeUndefined();
    expect(t.cancelledAfterMs).toEqual(expect.any(Number));
  });

  it("late_cancel_keeps_result", async () => {
    let finish: (v: unknown) => void = () => {};
    const send = vi.fn(() => new Promise((resolve) => (finish = resolve)));
    let cancelDone: () => void = () => {};
    const cancelSend = vi.fn(() => new Promise<void>((resolve) => (cancelDone = resolve)));
    setApi(mockApi({ send, cancelSend } as unknown as Partial<ApiClient>));
    useTabs.setState({ tabs: [tab()], active: "c1:a.bru" });
    const sent = useTabs.getState().send("c1:a.bru");
    const cancelled = useTabs.getState().cancel("c1:a.bru");
    finish(ok);
    await sent;
    cancelDone();
    await cancelled;
    const t = useTabs.getState().tabs[0]!;
    expect(t.result).toEqual(ok);
    expect(t.cancelledAfterMs).toBeUndefined();
    expect(t.sending).toBe(false);
  });
});
