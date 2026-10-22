import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../api/client";
import { setApi } from "../api/instance";
import { useWorkspace } from "./workspace";

const tree = { kind: "folder" as const, id: "", name: "c", method: null, seq: null, error: null, children: [] };

beforeEach(() => {
  useWorkspace.setState({ collections: [], trees: {}, environments: {}, activeEnv: {} });
});

describe("workspace store", () => {
  it("select_env_per_collection", () => {
    const ws = useWorkspace.getState();
    ws.selectEnv("c1", "local");
    ws.selectEnv("c2", "prod");
    expect(useWorkspace.getState().activeEnv).toEqual({ c1: "local", c2: "prod" });
    useWorkspace.getState().selectEnv("c1", null);
    expect(useWorkspace.getState().activeEnv.c1).toBeNull();
  });

  it("open loads the tree and environments once per collection", async () => {
    const api = {
      openCollection: vi.fn(async () => ({ id: "c1", name: "c", root: "/x" })),
      tree: vi.fn(async () => tree),
      environments: vi.fn(async () => [{ name: "local", vars: [], secretNames: [], hash: "h" }]),
    } as unknown as ApiClient;
    setApi(api);
    await useWorkspace.getState().open("/x");
    await useWorkspace.getState().open("/x/sub");
    const state = useWorkspace.getState();
    expect(state.collections).toEqual([{ id: "c1", name: "c", root: "/x" }]);
    expect(state.trees.c1).toEqual(tree);
    expect(state.environments.c1![0]!.name).toBe("local");
  });
});

describe("secrets", () => {
  it("sends only changed names and refreshes which secrets are set", async () => {
    const api = {
      setSecrets: vi.fn(async () => undefined),
      environments: vi.fn(async () => [
        { name: "local", vars: [], secretNames: ["a", "b"], secretsSet: ["a", "b"], hash: "h" },
      ]),
    } as unknown as ApiClient;
    setApi(api);
    await useWorkspace.getState().setSecrets("c1", "local", { b: "2", a: null });
    expect(api.setSecrets).toHaveBeenCalledWith("c1", "local", { b: "2", a: null });
    expect(useWorkspace.getState().environments.c1![0]!.secretsSet).toEqual(["a", "b"]);
  });
});
