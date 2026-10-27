import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../api/client";
import { setApi } from "../api/instance";
import type { TreeNodeDto } from "../api/types";
import { useTabs } from "../store/tabs";
import { useWorkspace } from "../store/workspace";
import { Sidebar } from "./Sidebar";

const node = (kind: TreeNodeDto["kind"], id: string, name: string, children?: TreeNodeDto[]): TreeNodeDto => ({
  kind,
  id,
  name,
  method: kind === "request" ? "GET" : null,
  seq: null,
  error: null,
  children: children ?? null,
});

const tree = node("folder", "", "demo", [
  node("folder", "users", "users", [node("request", "users/me.bru", "me")]),
  node("request", "ping.bru", "ping"),
]);

beforeEach(() => {
  setApi({ reload: vi.fn(async () => tree), environments: vi.fn(async () => []) } as unknown as ApiClient);
  useWorkspace.setState({
    collections: [{ id: "c1", name: "demo", root: "/x", available: true, error: null }],
    expanded: { c1: ["users"] },
    collapsed: {},
    filter: "",
    trees: { c1: tree },
    environments: { c1: [] },
    activeEnv: {},
    capabilities: null,
  });
  useTabs.setState({ tabs: [], active: null });
});

afterEach(() => cleanup());

const toggle = () => screen.getByRole("button", { name: "demo" });

describe("collapse a collection", () => {
  it("collapse_hides_tree_and_sets_aria_expanded", () => {
    render(<Sidebar onRun={() => undefined} />);
    expect(toggle().getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByRole("tree", { name: "demo" })).toBeTruthy();
    fireEvent.click(toggle());
    expect(toggle().getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByRole("tree")).toBeNull();
    expect(useWorkspace.getState().collapsed.c1).toBe(true);
    // A native button: Enter and Space activate it (as a click).
    fireEvent.click(toggle());
    expect(screen.getByRole("tree")).toBeTruthy();
    expect(toggle().getAttribute("aria-controls")).toBe(screen.getByRole("tree").id);
  });

  it("filter_shows_matches_in_collapsed_collection", () => {
    useWorkspace.setState({ collapsed: { c1: true } });
    const { rerender } = render(<Sidebar />);
    expect(screen.queryByRole("tree")).toBeNull();
    act(() => useWorkspace.getState().setFilter("me"));
    rerender(<Sidebar />);
    expect(screen.getByRole("treeitem", { name: "GET me" })).toBeTruthy();
    expect(toggle().getAttribute("aria-expanded")).toBe("true");
    act(() => useWorkspace.getState().setFilter(""));
    rerender(<Sidebar />);
    expect(screen.queryByRole("tree")).toBeNull();
    expect(useWorkspace.getState().collapsed.c1).toBe(true);
  });

  it("collection_menu_opens_from_keyboard", () => {
    render(<Sidebar onRun={() => undefined} />);
    const t = toggle();
    expect(t.getAttribute("aria-haspopup")).toBe("menu");
    expect(t.getAttribute("aria-keyshortcuts")).toBe("Shift+F10");
    t.focus();
    fireEvent.keyDown(t, { key: "F10", shiftKey: true });
    const menu = screen.getByRole("menu", { name: "demo" });
    const items = screen.getAllByRole("menuitem");
    expect(items.map((i) => i.textContent)).toEqual(["Run collection", "Import HAR…"]);
    expect(document.activeElement).toBe(items[0]);
    fireEvent.keyDown(menu, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(document.activeElement).toBe(toggle());
    fireEvent.keyDown(toggle(), { key: "ContextMenu" });
    expect(screen.getByRole("menu", { name: "demo" })).toBeTruthy();
  });

  it("collection_right_click_still_opens_menu", () => {
    render(<Sidebar onRun={() => undefined} />);
    fireEvent.contextMenu(toggle());
    expect(screen.getByRole("menu", { name: "demo" })).toBeTruthy();
  });
});
