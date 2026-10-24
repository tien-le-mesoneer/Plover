import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../api/client";
import { setApi } from "../api/instance";
import type { HttpRequestDto, TreeNodeDto } from "../api/types";
import App from "../App";
import { useTabs } from "../store/tabs";
import { useWorkspace } from "../store/workspace";

vi.mock("../editor/CodeEditor", () => ({ CodeEditor: () => null }));
vi.mock("../store/startup", () => ({ startUp: async () => undefined }));

const leaf = (id: string, name: string, method = "GET"): TreeNodeDto => ({
  kind: "request",
  id,
  name,
  method,
  seq: null,
  error: null,
  children: null,
});

const many = Array.from({ length: 60 }, (_, i) => leaf(`bulk/r${i}.bru`, `req ${i}`));

const tree: TreeNodeDto = {
  kind: "folder",
  id: "",
  name: "Demo",
  method: null,
  seq: null,
  error: null,
  children: [
    {
      kind: "folder",
      id: "auth",
      name: "auth",
      method: null,
      seq: null,
      error: null,
      children: [leaf("auth/login.bru", "Đăng nhập", "POST")],
    },
    leaf("doc.bru", "Sign document"),
    { kind: "folder", id: "bulk", name: "bulk", method: null, seq: null, error: null, children: many },
  ],
};

const request = (name: string) =>
  ({
    name,
    seq: 1,
    kind: "http",
    method: "GET",
    url: "http://example.test",
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
  }) as HttpRequestDto;

let client: Record<string, ReturnType<typeof vi.fn>>;

beforeEach(() => {
  client = {
    tree: vi.fn(async () => tree),
    subscribeEvents: vi.fn(() => ({ close() {} })),
    readRequest: vi.fn(async (_id: string, item: string) => ({ request: request(item), hash: "h" })),
    vars: vi.fn(async () => ({ vars: [], setLater: [] })),
  };
  setApi(client as unknown as ApiClient);
  useWorkspace.setState({
    collections: [{ id: "c1", name: "Demo", root: "/x", available: true, error: null }],
    trees: { c1: tree },
    environments: {},
    activeEnv: {},
    activeCollection: "c1",
    capabilities: null,
  });
  useTabs.setState({ tabs: [], runTabs: [], active: null, saveDialog: null, recent: [] });
});

afterEach(cleanup);

const dialog = () => screen.queryByRole("dialog", { name: "Quick open" });

describe("quick open", () => {
  it("cmd_k_opens", () => {
    render(<App hasToken />);
    fireEvent.keyDown(window, { key: "k", metaKey: true });
    expect(dialog()).toBeTruthy();
  });

  it("ctrl_k_opens", () => {
    render(<App hasToken />);
    fireEvent.keyDown(window, { key: "K", ctrlKey: true });
    expect(dialog()).toBeTruthy();
  });

  it("plover_quick_open_event_opens", () => {
    render(<App hasToken />);
    act(() => {
      window.dispatchEvent(new Event("plover:quick-open"));
    });
    expect(dialog()).toBeTruthy();
  });

  it("enter_opens_tab_and_closes", async () => {
    render(<App hasToken />);
    fireEvent.keyDown(window, { key: "k", metaKey: true });
    const input = screen.getByRole("combobox", { name: "Find a request" });
    fireEvent.change(input, { target: { value: "dang nhap" } });
    const options = within(screen.getByRole("listbox")).getAllByRole("option");
    expect(options[0]!.textContent).toContain("Đăng nhập");
    expect(options[0]!.textContent).toContain("auth");
    expect(options[0]!.textContent).toContain("POST");
    await act(async () => {
      fireEvent.keyDown(input, { key: "Enter" });
    });
    expect(client.readRequest).toHaveBeenCalledWith("c1", "auth/login.bru");
    expect(useTabs.getState().active).toBe("c1:auth/login.bru");
    expect(dialog()).toBeNull();
  });

  it("esc_restores_focus", () => {
    render(<App hasToken />);
    const runs = screen.getByRole("button", { name: "Runs" });
    runs.focus();
    fireEvent.keyDown(runs, { key: "k", metaKey: true });
    expect(document.activeElement).toBe(screen.getByRole("combobox", { name: "Find a request" }));
    fireEvent.keyDown(screen.getByRole("combobox", { name: "Find a request" }), { key: "Escape" });
    expect(dialog()).toBeNull();
    expect(document.activeElement).toBe(runs);
  });

  it("empty_query_lists_recent_tabs", async () => {
    render(<App hasToken />);
    await act(async () => {
      await useTabs.getState().open("c1", "doc.bru");
      await useTabs.getState().open("c1", "auth/login.bru");
    });
    fireEvent.keyDown(window, { key: "k", metaKey: true });
    const options = within(screen.getByRole("listbox")).getAllByRole("option");
    expect(options.map((o) => o.textContent)).toEqual([
      expect.stringContaining("Đăng nhập"),
      expect.stringContaining("Sign document"),
    ]);
  });

  it("results_cap_50", () => {
    render(<App hasToken />);
    fireEvent.keyDown(window, { key: "k", metaKey: true });
    fireEvent.change(screen.getByRole("combobox", { name: "Find a request" }), { target: { value: "req" } });
    expect(within(screen.getByRole("listbox")).getAllByRole("option")).toHaveLength(50);
  });

  it("arrow keys move the active option", () => {
    render(<App hasToken />);
    fireEvent.keyDown(window, { key: "k", metaKey: true });
    const input = screen.getByRole("combobox", { name: "Find a request" });
    fireEvent.change(input, { target: { value: "req" } });
    const options = within(screen.getByRole("listbox")).getAllByRole("option");
    expect(input.getAttribute("aria-activedescendant")).toBe(options[0]!.id);
    fireEvent.keyDown(input, { key: "ArrowDown" });
    expect(input.getAttribute("aria-activedescendant")).toBe(options[1]!.id);
    expect(options[1]!.getAttribute("aria-selected")).toBe("true");
    fireEvent.keyDown(input, { key: "ArrowUp" });
    expect(input.getAttribute("aria-activedescendant")).toBe(options[0]!.id);
  });
});
