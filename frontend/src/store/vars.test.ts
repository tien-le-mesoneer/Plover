import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../api/client";
import { setApi } from "../api/instance";
import type { HttpRequestDto } from "../api/types";
import { useRuns } from "./runs";
import { useTabs, type Tab } from "./tabs";
import { useVars, watchVars } from "./vars";
import { useWorkspace } from "./workspace";

const draft = { name: "r", url: "http://example.test/{{a}}", varsPre: [] } as unknown as HttpRequestDto;
const tab = (over: Partial<Tab> = {}): Tab => ({
  key: "c1:a.bru",
  collectionId: "c1",
  item: "a.bru",
  saved: draft,
  draft,
  hash: "h",
  sending: false,
  ...over,
});

let vars: ReturnType<typeof vi.fn>;
let stop: () => void;

beforeEach(() => {
  vi.useFakeTimers();
  vars = vi.fn(async () => ({ vars: [], setLater: [] }));
  setApi({ vars } as unknown as ApiClient);
  useWorkspace.setState({ activeEnv: { c1: "dev" }, environments: {} });
  useRuns.setState({ runs: {}, order: [] });
  useTabs.setState({ tabs: [], runTabs: [], active: null });
  useVars.setState({ byTab: {} });
  stop = watchVars();
});

afterEach(() => {
  stop();
  vi.useRealTimers();
});

const flush = async () => {
  await vi.advanceTimersByTimeAsync(0);
};

describe("variable cache", () => {
  it("loads when a tab opens and stores the answer per tab", async () => {
    useTabs.setState({ tabs: [tab()], active: "c1:a.bru" });
    await flush();
    expect(vars).toHaveBeenCalledWith("c1", { item: "a.bru", draft, env: "dev" });
    expect(useVars.getState().byTab["c1:a.bru"]).toEqual({ vars: [], setLater: [] });
  });

  it("refresh_after_send", async () => {
    // Runtime variables are per collection: a send refreshes every tab of it.
    const other = tab({ key: "c1:b.bru", item: "b.bru" });
    useTabs.setState({ tabs: [tab({ sending: true }), other] });
    await flush();
    vars.mockClear();
    useTabs.setState({ tabs: [tab({ sending: false }), other] });
    await flush();
    expect(vars).toHaveBeenCalledTimes(2);
    expect(vars).toHaveBeenCalledWith("c1", expect.objectContaining({ item: "b.bru" }));
  });

  it("refresh_on_env_change", async () => {
    useTabs.setState({ tabs: [tab()] });
    await flush();
    vars.mockClear();
    useWorkspace.setState({ activeEnv: { c1: "stg" } });
    await flush();
    expect(vars).toHaveBeenCalledWith("c1", expect.objectContaining({ env: "stg" }));
  });

  it("refreshes after a run of the collection ends", async () => {
    useTabs.setState({ tabs: [tab()] });
    useRuns.setState({ runs: { r1: { collectionId: "c1", status: "running" } as never }, order: ["r1"] });
    await flush();
    vars.mockClear();
    useRuns.setState({ runs: { r1: { collectionId: "c1", status: "done" } as never } });
    await flush();
    expect(vars).toHaveBeenCalledTimes(1);
  });

  it("debounced_on_varsPre_edit", async () => {
    useTabs.setState({ tabs: [tab()] });
    await flush();
    vars.mockClear();
    for (const v of ["1", "12", "123"]) {
      const edited = { ...draft, varsPre: [{ name: "x", value: v, enabled: true, local: false }] };
      useTabs.setState({ tabs: [tab({ draft: edited })] });
      await vi.advanceTimersByTimeAsync(70);
    }
    expect(vars).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(300);
    expect(vars).toHaveBeenCalledTimes(1);
  });

  it("scratch_tab_sends_item_null", async () => {
    useTabs.setState({ tabs: [tab({ key: "c1:scratch:1", item: null })] });
    await flush();
    expect(vars).toHaveBeenCalledWith("c1", { item: null, draft, env: "dev" });
  });

  it("forgets closed tabs", async () => {
    useTabs.setState({ tabs: [tab()] });
    await flush();
    useTabs.setState({ tabs: [] });
    await flush();
    expect(useVars.getState().byTab["c1:a.bru"]).toBeUndefined();
  });
});
