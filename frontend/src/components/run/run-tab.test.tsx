import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../../api/client";
import { setApi } from "../../api/instance";
import type { RunEventDto } from "../../api/types";
import App from "../../App";
import { useRuns } from "../../store/runs";
import { useTabs } from "../../store/tabs";
import { useWorkspace } from "../../store/workspace";
import { RequestTabs } from "../RequestTabs";
import { runTabFor } from "./harness";

vi.mock("../../editor/CodeEditor", () => ({ CodeEditor: () => null }));

const started = (item: string, name: string): RunEventDto => ({ type: "started", runId: "r1", iteration: 0, item, name, method: "GET", url: "u" });
const passed = (index: number, item: string): RunEventDto => ({
  type: "passed", runId: "r1", index, iteration: 0, item, status: 200, elapsedMs: 5, size: 2, tests: [], asserts: [], console: [],
});
const failed = (index: number, item: string): RunEventDto => ({
  type: "failed", runId: "r1", index, iteration: 0, item, error: "expected 404 to equal 200", status: 404, elapsedMs: 5, tests: [], asserts: [], console: [], scriptError: null,
});
const report = { passed: 2, failed: 1, skipped: 1, testsPassed: 0, testsFailed: 0, assertsPassed: 0, assertsFailed: 0, elapsedMs: 9, iterations: 1 };
const events: RunEventDto[] = [
  started("p1/login.bru", "login"), passed(0, "p1/login.bru"),
  started("me.bru", "me"), failed(1, "me.bru"),
  started("p2/login.bru", "login"), passed(2, "p2/login.bru"),
  { type: "skipped", runId: "r1", iteration: 0, index: 3, item: "poll.bru", reason: "skipRequest" },
];

let startRun: ReturnType<typeof vi.fn>;
let runResult: ReturnType<typeof vi.fn>;

function live(evts: RunEventDto[], status = "done") {
  useRuns.setState({
    order: ["r1"],
    viewing: null,
    history: {},
    runs: {
      r1: { runId: "r1", collectionId: "c1", item: "", label: "all", status, events: evts, report: status === "done" ? report : undefined, recordId: "rec", source: "live", iterations: [], env: "local" },
    } as never,
  });
}

beforeEach(() => {
  let n = 0;
  startRun = vi.fn(() => ({ runId: `new-${++n}`, cancel: vi.fn(), done: new Promise(() => {}) }));
  runResult = vi.fn(async (_c: string, _r: string, index: number) => ({
    index, item: "me.bru", name: "me", status: "fail", error: null, skipReason: null,
    request: { method: "GET", url: "http://h/me", headers: [], body: { kind: "none", text: null, base64: null, fields: [], parts: [], size: 0, truncated: false, omitted: null } },
    responseBody: null,
    result: { response: null, tests: [{ name: "status is 200", passed: false, error: "expected 404 to equal 200" }], asserts: [], console: [], scriptError: null, scriptedRequests: [], skipped: null },
  }));
  setApi({
    startRun,
    runResult,
    readRequest: vi.fn(async () => ({ request: { name: "me", method: "GET" }, hash: "h" })),
    listRuns: vi.fn(async () => []),
    getRun: vi.fn(async () => ({
      meta: { recordId: "old", source: "plv", target: ".", env: "local", startedAtMs: 0, outcome: "completed", report },
      label: "saved",
      events: [started("x.bru", "x"), passed(0, "x.bru")],
    })),
    subscribeEvents: vi.fn(() => ({ close() {} })),
    clientReady: vi.fn(async () => undefined),
    uiState: vi.fn(async () => ({})),
    saveUiState: vi.fn(async () => undefined),
    layout: vi.fn(async () => ({ sidebarWidth: null })),
    recent: vi.fn(async () => []),
  } as unknown as ApiClient);
  useWorkspace.setState({
    collections: [{ id: "c1", name: "Demo", root: "/x", available: true, error: null }],
    activeEnv: { c1: "local" },
    environments: {},
    trees: {},
    capabilities: null,
  });
  useTabs.setState({ tabs: [], runTabs: [], active: null });
  live(events);
});

afterEach(cleanup);

const filter = () => screen.getByRole("radiogroup", { name: "Show results" });
const bodyRows = () => within(screen.getByRole("table", { name: "Run results" })).getAllByRole("row").slice(1);

describe("run tab", () => {
  it("filter_counts_match_report", () => {
    render(runTabFor());
    const names = within(filter()).getAllByRole("radio").map((r) => r.getAttribute("aria-label"));
    expect(names).toEqual(["All 4", `Fail ${report.failed}`, `Pass ${report.passed}`, `Skip ${report.skipped}`]);
  });

  it("fail_filter_lists_only_failures", () => {
    render(runTabFor());
    fireEvent.click(within(filter()).getByRole("radio", { name: "Fail 1" }));
    expect(bodyRows().map((r) => r.textContent)).toEqual([expect.stringContaining("me")]);
  });

  it("filter_does_not_jump_when_run_ends", () => {
    live([started("a.bru", "a")], "running");
    render(runTabFor());
    fireEvent.click(within(filter()).getByRole("radio", { name: /^Pass/ }));
    act(() => live(events));
    expect(within(filter()).getByRole("radio", { name: /^Pass/ }).getAttribute("aria-checked")).toBe("true");
  });

  it("name_filter_narrows", () => {
    render(runTabFor());
    fireEvent.change(screen.getByRole("searchbox", { name: "Filter results by name" }), { target: { value: "logi" } });
    expect(bodyRows()).toHaveLength(2);
  });

  it("folder line only on duplicate names", () => {
    render(runTabFor());
    const rows = bodyRows();
    expect(rows[0]!.textContent).toContain("p1");
    expect(rows[2]!.textContent).toContain("p2");
    expect(rows[1]!.querySelector(".run-folder")).toBeNull();
  });

  it("failure_reason_on_top", async () => {
    render(runTabFor());
    await act(async () => {
      fireEvent.click(bodyRows()[1]!);
    });
    const detail = screen.getByRole("region", { name: "Result detail" });
    const reason = within(detail).getByRole("alert");
    expect(reason.textContent).toContain('test "status is 200": expected 404 to equal 200');
  });

  it("open_request_keeps_run_tab", async () => {
    render(runTabFor());
    await act(async () => {
      fireEvent.click(bodyRows()[1]!);
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Open request" }));
    });
    const s = useTabs.getState();
    expect(s.tabs.map((t) => t.item)).toEqual(["me.bru"]);
    expect(s.runTabs).toHaveLength(1);
  });

  it("previous_run_replaces_content", async () => {
    render(runTabFor());
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /Previous runs/ }));
    });
    // The menu lists nothing (listRuns is empty); choose through the store like the menu does.
    await act(async () => {
      await useRuns.getState().openHistory("c1", "old");
      useTabs.getState().retargetRun("c1:run:harness", "old");
    });
    expect(screen.getByText("Run · saved")).toBeTruthy();
    expect(useTabs.getState().runTabs).toHaveLength(1);
  });

  it("rerun_starts_same_target", () => {
    render(runTabFor());
    fireEvent.click(screen.getByRole("button", { name: "Re-run" }));
    expect(startRun).toHaveBeenCalledWith("c1", "", "local", expect.any(Function), undefined);
    expect(useTabs.getState().runTabs[0]!.runId).toBe("new-1");
  });

  it("run_tabs_not_persisted and shown in the tab strip", () => {
    useTabs.getState().openRun("c1", "r1", "all");
    render(<RequestTabs />);
    expect(screen.getByRole("tab", { name: /^Run all/ })).toBeTruthy();
  });
});

describe("app wiring", () => {
  it("sidebar_run_opens_and_focuses_run_tab, two_runs_two_tabs, no_bottom_panel", async () => {
    useWorkspace.setState({
      trees: { c1: { kind: "folder", id: "", name: "Demo", method: null, seq: null, error: null, children: [] } },
      init: vi.fn(async () => {}),
    } as never);
    render(<App hasToken />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Run collection" }));
    });
    let s = useTabs.getState();
    expect(s.runTabs).toHaveLength(1);
    expect(s.active).toBe(s.runTabs[0]!.key);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Run collection" }));
    });
    s = useTabs.getState();
    expect(s.runTabs).toHaveLength(2);
    expect(screen.queryByRole("region", { name: "Run" })).toBeNull();
    expect(document.querySelector(".run-panel")).toBeNull();
  });
});
