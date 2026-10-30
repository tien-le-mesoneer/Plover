import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../api/client";
import { setApi } from "../api/instance";
import type { TreeNodeDto } from "../api/types";
import { useTabs } from "../store/tabs";
import { useWorkspace } from "../store/workspace";
import { Sidebar, TREE_INDENT, TREE_ORIGIN } from "./Sidebar";

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
  node("folder", "users", "users", [node("folder", "users/admin", "admin"), node("request", "users/me.bru", "me")]),
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

const start = (name: string) => parseFloat(screen.getByRole("treeitem", { name }).style.paddingLeft);

describe("sidebar indentation", () => {
  it("children start one step in from the collection chevron", () => {
    render(<Sidebar onRun={() => undefined} />);
    // The header chevron sits at TREE_ORIGIN; level 1 starts where the collection's name does.
    expect(start("users")).toBe(TREE_ORIGIN + TREE_INDENT);
  });

  it("folders and requests at the same level start in the same column", () => {
    render(<Sidebar onRun={() => undefined} />);
    expect(start("GET ping")).toBe(start("users"));
    expect(start("GET me")).toBe(start("admin"));
  });

  it("the open request is marked like every selected row", () => {
    useTabs.setState({ tabs: [], active: "c1:ping.bru" });
    render(<Sidebar onRun={() => undefined} />);
    expect(screen.getByRole("treeitem", { name: "GET ping" }).classList.contains("is-selected")).toBe(true);
    expect(screen.getByRole("treeitem", { name: "GET me" }).classList.contains("is-selected")).toBe(false);
  });

  it("each level steps in by one caret and its gap, under the parent's name", () => {
    render(<Sidebar onRun={() => undefined} />);
    expect(start("admin") - start("users")).toBe(TREE_INDENT);
  });
});
