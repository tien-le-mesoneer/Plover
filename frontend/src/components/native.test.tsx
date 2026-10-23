import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../api/client";
import { setApi } from "../api/instance";
import type { CapabilitiesDto, EnvironmentDto } from "../api/types";
import { useWorkspace } from "../store/workspace";
import { EnvEditor } from "./EnvEditor";
import { SidebarHeader } from "./SidebarHeader";

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
  openCollection = vi.fn(async () => ({ id: "c1", name: "c", root: "/picked", available: true, error: null }));
  setApi({
    pickFolder,
    openCollection,
    recent: vi.fn(async () => []),
    tree: vi.fn(async () => ({ kind: "folder", id: "", name: "c", method: null, seq: null, error: null, children: [] })),
    environments: vi.fn(async () => []),
  } as unknown as ApiClient);
  useWorkspace.setState({ collections: [], trees: {}, environments: { c1: [env] }, activeEnv: {}, capabilities: web, recent: [], pickerOpen: false });
});

afterEach(cleanup);

describe("native folder picker", () => {
  const openPopover = () => fireEvent.click(screen.getByRole("button", { name: "Open collection" }));

  it("is hidden in web mode", () => {
    render(<SidebarHeader />);
    openPopover();
    expect(screen.queryByRole("button", { name: "Browse for folder…" })).toBeNull();
  });

  it("opens the picked folder", async () => {
    useWorkspace.setState({ capabilities: desktop });
    pickFolder.mockResolvedValue("/picked");
    render(<SidebarHeader />);
    openPopover();
    fireEvent.click(screen.getByRole("button", { name: "Browse for folder…" }));
    await vi.waitFor(() => expect(openCollection).toHaveBeenCalledWith("/picked"));
  });

  it("cancel keeps the typed path and opens nothing", async () => {
    useWorkspace.setState({ capabilities: desktop });
    pickFolder.mockResolvedValue(null);
    render(<SidebarHeader />);
    openPopover();
    const input = screen.getByRole("textbox", { name: "Collection path" });
    fireEvent.change(input, { target: { value: "/typed" } });
    fireEvent.click(screen.getByRole("button", { name: "Browse for folder…" }));
    await vi.waitFor(() => expect(pickFolder).toHaveBeenCalled());
    expect(openCollection).not.toHaveBeenCalled();
    expect((input as HTMLInputElement).value).toBe("/typed");
  });
});

describe("per-secret storage", () => {
  it("badges each secret and warns when any is memory-only", () => {
    useWorkspace.setState({
      capabilities: desktop,
      environments: {
        c1: [{ ...env, secretNames: ["token", "pem"], secretsSet: [
          { name: "token", backend: "keychain" },
          { name: "pem", backend: "memory" },
        ] }],
      },
    });
    render(<EnvEditor collectionId="c1" name="local" onClose={() => {}} />);
    expect(screen.getByText("keychain")).toBeTruthy();
    expect(screen.getByText("memory only")).toBeTruthy();
    expect(screen.getByRole("alert").textContent).toMatch(/memory only.*lost when Plover quits/i);
  });

  it("keeps the dialog open and shows warnings after saving", async () => {
    useWorkspace.setState({
      capabilities: desktop,
      setSecrets: vi.fn(async () => ["token: keychain refused: denied; kept in memory until Plover quits"]),
      saveEnvironment: vi.fn(async () => "saved" as const),
    });
    const onClose = vi.fn();
    render(<EnvEditor collectionId="c1" name="local" onClose={onClose} />);
    fireEvent.change(screen.getByLabelText("Secret token"), { target: { value: "v" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await vi.waitFor(() => expect(screen.getByText(/keychain refused/)).toBeTruthy());
    expect(onClose).not.toHaveBeenCalled();
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
