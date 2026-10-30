import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../api/client";
import { setApi } from "../api/instance";
import type { HttpRequestDto, RunEventDto, TreeNodeDto } from "../api/types";
import { buildEntries } from "../lib/runRows";
import { useTabs } from "../store/tabs";
import { useWorkspace } from "../store/workspace";
import { RequestTabs } from "./RequestTabs";
import { RunList } from "./run/RunList";
import { Sidebar } from "./Sidebar";

const URL = "{{host}}/users/{{id}}/orders?include=items&page=2";

const tree: TreeNodeDto = {
  kind: "folder",
  id: "",
  name: "demo",
  method: null,
  seq: null,
  error: null,
  children: [{ kind: "request", id: "orders.bru", name: "orders", method: "GET", url: URL, seq: null, error: null, children: null }],
};

beforeEach(() => {
  setApi({ reload: vi.fn(async () => tree), environments: vi.fn(async () => []) } as unknown as ApiClient);
  useWorkspace.setState({
    collections: [{ id: "c1", name: "demo", root: "/x", available: true, error: null }],
    expanded: {},
    collapsed: {},
    filter: "",
    trees: { c1: tree },
    environments: { c1: [] },
    activeEnv: {},
    capabilities: null,
  });
  useTabs.setState({ tabs: [], runTabs: [], historyTab: null, active: null });
});

afterEach(() => cleanup());

describe("full URL on hover", () => {
  it("sidebar request rows show the method and the URL as written", () => {
    render(<Sidebar onRun={() => undefined} />);
    expect(screen.getByRole("treeitem", { name: "GET orders" }).getAttribute("title")).toBe(`GET ${URL}`);
  });

  it("request tabs show the method and the draft's URL, then the file", () => {
    const draft = { name: "orders", method: "POST", url: URL } as HttpRequestDto;
    useTabs.setState({
      tabs: [
        { key: "c1:orders.bru", collectionId: "c1", item: "orders.bru", saved: draft, draft, hash: "h", sending: false },
        { key: "scratch", collectionId: "c1", item: null, saved: draft, draft: { ...draft, url: "" }, hash: "h", sending: false },
      ],
      active: "c1:orders.bru",
    } as never);
    render(<RequestTabs />);
    const [saved, scratch] = screen.getAllByRole("tab");
    expect(saved!.getAttribute("title")).toBe(`POST ${URL}\norders.bru`);
    expect(scratch!.getAttribute("title")).toBe("POST\nUnsaved request");
  });

  it("run result rows show the method and the URL that was sent", () => {
    const events: RunEventDto[] = [
      { type: "started", runId: "r", iteration: 0, item: "orders.bru", name: "orders", method: "GET", url: "https://api.test/users/7/orders" },
      { type: "passed", runId: "r", index: 0, iteration: 0, item: "orders.bru", status: 200, elapsedMs: 5, size: 2, tests: [], asserts: [], console: [] },
    ];
    render(<RunList entries={buildEntries(events)} events={events} filter="all" name="" onSelect={() => undefined} />);
    expect(document.querySelector(".run-row")!.getAttribute("title")).toBe("GET https://api.test/users/7/orders");
  });
});
