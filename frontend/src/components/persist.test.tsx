import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../api/client";
import { setApi } from "../api/instance";
import type { CapabilitiesDto, CollectionSummaryDto, HttpRequestDto, RecentDto, TreeNodeDto } from "../api/types";
import { restoreUiState, startUiPersistence } from "../store/uiState";
import { useTabs } from "../store/tabs";
import { useWorkspace } from "../store/workspace";
import { Sidebar } from "./Sidebar";
import { SidebarHeader } from "./SidebarHeader";

const web: CapabilitiesDto = { desktop: false, nativePicker: false, openExternal: false, secretBackend: "memory" };
const desktop: CapabilitiesDto = { desktop: true, nativePicker: true, openExternal: true, secretBackend: "keychain" };

const node = (kind: TreeNodeDto["kind"], id: string, name: string, children?: TreeNodeDto[]): TreeNodeDto => ({
  kind,
  id,
  name,
  method: kind === "request" ? "GET" : null,
  seq: null,
  error: null,
  children: children ?? null,
});

const tree = node("folder", "", "Demo", [
  node("folder", "auth", "auth", [node("request", "auth/login.bru", "login")]),
  node("folder", "users", "users", [node("request", "users/me.bru", "me"), node("request", "users/create.bru", "create user")]),
]);

const demo: CollectionSummaryDto = { id: "c1", name: "Demo", root: "/work/demo", available: true, error: null };
const gone: CollectionSummaryDto = { id: "c2", name: "Old", root: "/work/old", available: false, error: "not found" };
const recent: RecentDto[] = [
  { root: "/work/demo", path: "/work/demo", name: "Demo", lastOpened: 2, open: true },
  { root: "/work/other", path: "/work/other", name: "Other", lastOpened: 1, open: false },
];

const request = { method: "GET", url: "http://x" } as unknown as HttpRequestDto;

let client: Record<string, ReturnType<typeof vi.fn>>;

beforeEach(() => {
  client = {
    capabilities: vi.fn(async () => web),
    listCollections: vi.fn(async () => [demo, gone]),
    openCollection: vi.fn(async () => demo),
    closeCollection: vi.fn(async () => undefined),
    recent: vi.fn(async () => recent),
    removeRecent: vi.fn(async () => undefined),
    pickFolder: vi.fn(async () => null),
    tree: vi.fn(async () => tree),
    environments: vi.fn(async () => [
      { name: "local", vars: [], secretNames: [], secretsSet: [], hash: "h" },
      { name: "prod", vars: [], secretNames: [], secretsSet: [], hash: "h" },
    ]),
    uiState: vi.fn(async () => ({})),
    saveUiState: vi.fn(async () => undefined),
    readRequest: vi.fn(async (_id: string, item: string) => {
      if (item === "users/gone.bru") throw new Error("not found");
      return { request, hash: "h" };
    }),
  };
  setApi(client as unknown as ApiClient);
  useWorkspace.setState({
    collections: [demo],
    trees: { c1: tree },
    environments: {},
    activeEnv: {},
    expanded: {},
    filter: "",
    recent,
    pickerOpen: false,
    capabilities: web,
    activeCollection: null,
    error: undefined,
  });
  useTabs.setState({ tabs: [], active: null });
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("open collection popover", () => {
  it("moves focus in, closes on Escape and returns focus to +", async () => {
    render(<SidebarHeader />);
    const plus = screen.getByRole("button", { name: "Open collection" });
    plus.focus();
    fireEvent.click(plus);
    const dialog = await screen.findByRole("dialog", { name: "Open collection" });
    expect(dialog.contains(document.activeElement)).toBe(true);
    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(plus);
  });

  it("opens a pasted path and a recent collection, and removes a recent entry", async () => {
    render(<SidebarHeader />);
    fireEvent.click(screen.getByRole("button", { name: "Open collection" }));
    const input = screen.getByRole("textbox", { name: "Collection path" });
    fireEvent.change(input, { target: { value: "/typed/path" } });
    fireEvent.submit(input);
    await vi.waitFor(() => expect(client.openCollection).toHaveBeenCalledWith("/typed/path"));

    fireEvent.click(screen.getByRole("button", { name: "Open collection" }));
    fireEvent.click(screen.getByRole("button", { name: /^Other/ }));
    await vi.waitFor(() => expect(client.openCollection).toHaveBeenCalledWith("/work/other"));

    fireEvent.click(screen.getByRole("button", { name: "Open collection" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove Other from recent" }));
    await vi.waitFor(() => expect(client.removeRecent).toHaveBeenCalledWith("/work/other"));
  });

  it("offers the native picker only in the desktop app", () => {
    render(<SidebarHeader />);
    fireEvent.click(screen.getByRole("button", { name: "Open collection" }));
    expect(screen.queryByRole("button", { name: "Browse for folder…" })).toBeNull();
    cleanup();
    useWorkspace.setState({ capabilities: desktop, pickerOpen: false });
    render(<SidebarHeader />);
    fireEvent.click(screen.getByRole("button", { name: "Open collection" }));
    expect(screen.getByRole("button", { name: "Browse for folder…" })).toBeTruthy();
  });

  it("Cmd/Ctrl+O opens it", () => {
    render(<SidebarHeader />);
    fireEvent.keyDown(window, { key: "o", metaKey: true });
    expect(screen.getByRole("dialog", { name: "Open collection" })).toBeTruthy();
  });
});

describe("request filter", () => {
  it("narrows the tree to matches with their folders expanded; Escape clears", () => {
    render(<Sidebar />);
    const filter = screen.getByRole("searchbox", { name: "Filter requests" });
    fireEvent.change(filter, { target: { value: "ME" } });
    const names = screen.getAllByRole("treeitem").map((el) => el.getAttribute("aria-label"));
    expect(names).toEqual(["users", "GET me"]);
    fireEvent.keyDown(filter, { key: "Escape" });
    expect((filter as HTMLInputElement).value).toBe("");
    expect(screen.getAllByRole("treeitem").map((el) => el.getAttribute("aria-label"))).toEqual(["auth", "users"]);
  });

  it("Cmd/Ctrl+P focuses it", () => {
    render(<Sidebar />);
    fireEvent.keyDown(window, { key: "p", ctrlKey: true });
    expect(document.activeElement).toBe(screen.getByRole("searchbox", { name: "Filter requests" }));
  });
});

describe("collections in the sidebar", () => {
  it("closes a collection", async () => {
    render(<Sidebar />);
    fireEvent.click(screen.getByRole("button", { name: "Close collection Demo" }));
    await vi.waitFor(() => expect(client.closeCollection).toHaveBeenCalledWith("c1"));
    expect(useWorkspace.getState().collections).toEqual([]);
  });

  it("shows an unavailable collection with Retry and Remove", async () => {
    useWorkspace.setState({ collections: [demo, gone] });
    render(<Sidebar />);
    const row = screen.getByText("Old").closest(".collection")!;
    expect(row.className).toContain("unavailable");
    fireEvent.click(screen.getByRole("button", { name: "Retry opening Old" }));
    await vi.waitFor(() => expect(client.openCollection).toHaveBeenCalledWith("/work/old"));
    fireEvent.click(screen.getByRole("button", { name: "Remove Old" }));
    await vi.waitFor(() => expect(client.closeCollection).toHaveBeenCalledWith("c2"));
  });
});

describe("remembered UI state", () => {
  it("restores tabs, expansion and environment, dropping missing items", async () => {
    client.uiState!.mockResolvedValue({
      c1: { tabs: ["users/me.bru", "users/gone.bru"], active: "users/me.bru", expanded: ["users"], env: "prod" },
    });
    useWorkspace.setState({ collections: [], trees: {}, recent: [] });
    await useWorkspace.getState().init();
    await restoreUiState();
    const tabs = useTabs.getState();
    expect(tabs.tabs.map((t) => t.item)).toEqual(["users/me.bru"]);
    expect(tabs.active).toBe("c1:users/me.bru");
    const ws = useWorkspace.getState();
    expect(ws.expanded.c1).toEqual(["users"]);
    expect(ws.activeEnv.c1).toBe("prod");
    expect(ws.collections.map((c) => c.id)).toEqual(["c1", "c2"]);
  });

  it("saves changes debounced, per collection", async () => {
    vi.useFakeTimers();
    const stop = startUiPersistence();
    act(() => useWorkspace.getState().toggleExpanded("c1", "users"));
    act(() => useWorkspace.getState().toggleExpanded("c1", "auth"));
    expect(client.saveUiState).not.toHaveBeenCalled();
    await act(async () => {
      vi.advanceTimersByTime(600);
    });
    expect(client.saveUiState).toHaveBeenCalledTimes(1);
    expect(client.saveUiState).toHaveBeenCalledWith("c1", {
      tabs: [],
      active: null,
      expanded: ["users", "auth"],
      env: null,
    });
    stop();
  });
});
