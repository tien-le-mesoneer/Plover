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
    auth: { mode: "none" },
    varsPre: [],
    varsPost: [],
    scriptPre: null,
    scriptPost: null,
    tests: null,
    settings: { followRedirects: null, maxRedirects: null, timeoutMs: null },
  };
}

function mockApi(overrides: Partial<ApiClient> = {}): ApiClient {
  return {
    readRequest: vi.fn(async (_id: string, item: string) => ({
      request: request(`http://h/${item}`),
      hash: `hash-${item}`,
    })),
    saveRequest: vi.fn(async () => ({ hash: "new-hash" })),
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
    (api.saveRequest as ReturnType<typeof vi.fn>).mockResolvedValueOnce({ hash: "h3" });
    await useTabs.getState().resolveConflict(key, "overwrite");
    expect(api.saveRequest).toHaveBeenLastCalledWith("c1", "a.bru", expect.anything(), "disk-hash");
    expect(useTabs.getState().tabs[0]!.conflict).toBeUndefined();
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
    const response = { status: 200, headers: [], elapsedMs: 1, size: 0, bodyText: "", bodyBase64: null, truncated: false };
    const api = mockApi({ send: vi.fn(async () => response) });
    setApi(api);
    useWorkspace.getState().selectEnv("c1", "local");
    await useTabs.getState().open("c1", "a.bru");
    const key = useTabs.getState().active!;
    useTabs.getState().edit(key, (d) => ({ ...d, url: "draft" }));
    await useTabs.getState().send(key);
    expect(api.send).toHaveBeenCalledWith("c1", "a.bru", expect.objectContaining({ url: "draft" }), "local");
    expect(useTabs.getState().tabs[0]!.response).toEqual(response);
    expect(useTabs.getState().tabs[0]!.sending).toBe(false);
  });
});
