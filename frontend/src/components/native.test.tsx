import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../api/client";
import { setApi } from "../api/instance";
import type { CapabilitiesDto, EnvironmentDto } from "../api/types";
import { useWorkspace } from "../store/workspace";
import { EnvEditor } from "./EnvEditor";
import { OpenCollectionDialog } from "./OpenCollectionDialog";

const web: CapabilitiesDto = { desktop: false, nativePicker: false, openExternal: false, secretBackend: "memory" };
const desktop: CapabilitiesDto = { desktop: true, nativePicker: true, openExternal: true, secretBackend: "keychain" };

const env: EnvironmentDto = {
  name: "local",
  vars: [],
  secretNames: ["token"],
  secretsSet: [],
  hash: "h",
};

let pickFolder: ReturnType<typeof vi.fn>;
let openCollection: ReturnType<typeof vi.fn>;

beforeEach(() => {
  pickFolder = vi.fn();
  openCollection = vi.fn(async () => ({ id: "c1", name: "c", root: "/picked" }));
  setApi({
    pickFolder,
    openCollection,
    tree: vi.fn(async () => ({ kind: "folder", id: "", name: "c", method: null, seq: null, error: null, children: [] })),
    environments: vi.fn(async () => []),
  } as unknown as ApiClient);
  useWorkspace.setState({ collections: [], trees: {}, environments: { c1: [env] }, activeEnv: {}, capabilities: web });
});

afterEach(cleanup);

describe("native folder picker", () => {
  it("is hidden in web mode", () => {
    render(<OpenCollectionDialog />);
    expect(screen.queryByRole("button", { name: "Browse…" })).toBeNull();
  });

  it("opens the picked folder", async () => {
    useWorkspace.setState({ capabilities: desktop });
    pickFolder.mockResolvedValue("/picked");
    render(<OpenCollectionDialog />);
    fireEvent.click(screen.getByRole("button", { name: "Browse…" }));
    await vi.waitFor(() => expect(openCollection).toHaveBeenCalledWith("/picked"));
  });

  it("cancel keeps the typed path and opens nothing", async () => {
    useWorkspace.setState({ capabilities: desktop });
    pickFolder.mockResolvedValue(null);
    render(<OpenCollectionDialog />);
    const input = screen.getByRole("textbox", { name: "Collection path" });
    fireEvent.change(input, { target: { value: "/typed" } });
    fireEvent.click(screen.getByRole("button", { name: "Browse…" }));
    await vi.waitFor(() => expect(pickFolder).toHaveBeenCalled());
    expect(openCollection).not.toHaveBeenCalled();
    expect((input as HTMLInputElement).value).toBe("/typed");
  });
});

describe("secret storage status", () => {
  it("says memory only without a keychain", () => {
    render(<EnvEditor collectionId="c1" name="local" onClose={() => {}} />);
    expect(screen.getByText(/Memory only/)).toBeTruthy();
  });

  it("says OS keychain when available", () => {
    useWorkspace.setState({ capabilities: desktop });
    render(<EnvEditor collectionId="c1" name="local" onClose={() => {}} />);
    expect(screen.getByText(/Stored in the OS keychain/)).toBeTruthy();
  });
});
