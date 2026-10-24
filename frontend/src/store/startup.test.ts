import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../api/client";
import { setApi } from "../api/instance";
import { startUp } from "./startup";
import { useWorkspace } from "./workspace";

let client: Record<string, ReturnType<typeof vi.fn>>;

beforeEach(() => {
  useWorkspace.setState({ collections: [], error: undefined });
  client = {
    capabilities: vi.fn(async () => ({ desktop: true, nativePicker: true, openExternal: true, nativeSave: true, secretBackend: "keychain" })),
    listCollections: vi.fn(async () => []),
    recent: vi.fn(async () => []),
    uiState: vi.fn(async () => ({})),
    saveUiState: vi.fn(async () => undefined),
    clientReady: vi.fn(async () => undefined),
  };
  setApi(client as unknown as ApiClient);
});

describe("startUp", () => {
  it("reports a clean start", async () => {
    const stop = await startUp();
    expect(client.clientReady).toHaveBeenCalledWith(undefined);
    stop?.();
  });

  it("reports why loading collections failed instead of staying silent", async () => {
    client.listCollections!.mockRejectedValue(new Error("listing failed: 500"));
    await startUp();
    expect(client.clientReady).toHaveBeenCalledWith("listing failed: 500");
    expect(useWorkspace.getState().error).toBe("listing failed: 500");
  });

  it("reports even when a step throws", async () => {
    const init = useWorkspace.getState().init;
    useWorkspace.setState({
      init: async () => {
        throw new TypeError("x is undefined");
      },
    });
    try {
      await startUp();
    } finally {
      useWorkspace.setState({ init });
    }
    expect(client.clientReady).toHaveBeenCalledWith("x is undefined");
  });
});
