import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../api/client";
import { api, setApi } from "../api/instance";
import type {
  BodyDetailDto,
  HttpRequestDto,
  RunEventDto,
  RunResultDetailDto,
  SendResultDto,
  TreeNodeDto,
} from "../api/types";
import { useRuns } from "../store/runs";
import { useTabs } from "../store/tabs";
import { useWorkspace } from "../store/workspace";
import { watchColorScheme } from "../editor/theme";
import { EmptyState } from "./EmptyState";
import { EnvEditor } from "./EnvEditor";
import { Modal } from "./Modal";
import { KeyValueTable } from "./request/KeyValueTable";
import { RequestTabs } from "./RequestTabs";
import { ResponseViewer } from "./ResponseViewer";
import { RunPanel } from "./RunPanel";
import { Sidebar } from "./Sidebar";

// Monaco needs a real browser; the response body editor is not what these tests check.
vi.mock("../editor/CodeEditor", () => ({ CodeEditor: () => null }));

const leaf = (id: string, name: string, method = "GET"): TreeNodeDto => ({
  kind: "request",
  id,
  name,
  method,
  seq: null,
  error: null,
  children: null,
});

const tree: TreeNodeDto = {
  kind: "folder",
  id: "",
  name: "c",
  method: null,
  seq: null,
  error: null,
  children: [
    { kind: "folder", id: "users", name: "users", method: null, seq: null, error: null, children: [leaf("users/list.bru", "list users")] },
    leaf("ping.bru", "ping"),
  ],
};

const req = (name: string, method = "GET") => ({ name, method }) as HttpRequestDto;

beforeEach(() => {
  useWorkspace.setState({
    collections: [{ id: "c1", name: "ui-check", root: "/x", available: true, error: null }],
    expanded: {},
    filter: "",
    trees: { c1: tree },
    environments: {},
    activeEnv: {},
    capabilities: null,
  });
  useTabs.setState({ tabs: [], active: null });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("sidebar tree keyboard access (A2)", () => {
  it("is a tree with one tab stop that arrow keys move through", () => {
    render(<Sidebar onRun={() => {}} />);
    const treeEl = screen.getByRole("tree", { name: "ui-check" });
    const items = within(treeEl).getAllByRole("treeitem");
    expect(items.map((i) => i.textContent)).toEqual([expect.stringContaining("users"), expect.stringContaining("ping")]);
    expect(items.filter((i) => i.tabIndex === 0)).toHaveLength(1);
    items[0]!.focus();
    fireEvent.keyDown(items[0]!, { key: "ArrowDown" });
    expect(document.activeElement).toBe(items[1]);
  });

  it("ArrowRight expands a folder and Enter opens a request", () => {
    const open = vi.fn(async () => {});
    useTabs.setState({ open });
    render(<Sidebar onRun={() => {}} />);
    const folder = screen.getByRole("treeitem", { name: /users/ });
    expect(folder.getAttribute("aria-expanded")).toBe("false");
    folder.focus();
    fireEvent.keyDown(folder, { key: "ArrowRight" });
    expect(folder.getAttribute("aria-expanded")).toBe("true");
    fireEvent.keyDown(folder, { key: "ArrowDown" });
    const child = screen.getByRole("treeitem", { name: /list users/ });
    expect(document.activeElement).toBe(child);
    fireEvent.keyDown(child, { key: "Enter" });
    expect(open).toHaveBeenCalledWith("c1", "users/list.bru");
    // Cmd/Ctrl+Enter on a request is the app's "send" shortcut: the tree lets it bubble.
    const bubbled = vi.fn();
    window.addEventListener("keydown", bubbled);
    fireEvent.keyDown(child, { key: "Enter", metaKey: true });
    window.removeEventListener("keydown", bubbled);
    expect(bubbled).toHaveBeenCalled();
    expect(open).toHaveBeenCalledTimes(1);
  });

  it("runs a folder without right-click: a visible button and Ctrl/Cmd+Enter", () => {
    const onRun = vi.fn();
    render(<Sidebar onRun={onRun} />);
    fireEvent.click(screen.getByRole("button", { name: "Run folder users" }));
    expect(onRun).toHaveBeenLastCalledWith("c1", "users", "users");
    const folder = screen.getByRole("treeitem", { name: /users/ });
    fireEvent.keyDown(folder, { key: "Enter", metaKey: true });
    expect(onRun).toHaveBeenCalledTimes(2);
  });
});

describe("request tabs keyboard access (A2)", () => {
  beforeEach(() => {
    useTabs.setState({
      tabs: [
        { key: "c1:a", collectionId: "c1", item: "a", saved: req("a"), draft: req("a"), hash: "h", sending: false },
        { key: "c1:b", collectionId: "c1", item: "b", saved: req("b"), draft: req("b"), hash: "h", sending: false },
      ],
      active: "c1:a",
    });
  });

  it("uses a roving tab stop; arrows activate the neighbour; Delete closes", () => {
    render(<RequestTabs />);
    const [a, b] = screen.getAllByRole("tab");
    expect(a!.tabIndex).toBe(0);
    expect(b!.tabIndex).toBe(-1);
    a!.focus();
    fireEvent.keyDown(a!, { key: "ArrowRight" });
    expect(useTabs.getState().active).toBe("c1:b");
    fireEvent.keyDown(screen.getAllByRole("tab")[1]!, { key: "Delete" });
    expect(useTabs.getState().tabs.map((t) => t.key)).toEqual(["c1:a"]);
  });
});

describe("modal focus handling (A3)", () => {
  it("moves focus in, traps Tab, closes on Escape and restores focus", () => {
    const opener = document.createElement("button");
    document.body.appendChild(opener);
    opener.focus();
    const onDismiss = vi.fn();
    const { unmount } = render(
      <Modal title="Edit thing" onDismiss={onDismiss}>
        <input aria-label="first" />
        <button>last</button>
      </Modal>,
    );
    const dialog = screen.getByRole("dialog", { name: "Edit thing" });
    expect(dialog.getAttribute("aria-modal")).toBe("true");
    expect(document.activeElement).toBe(screen.getByLabelText("first"));
    screen.getByRole("button", { name: "last" }).focus();
    fireEvent.keyDown(dialog, { key: "Tab" });
    expect(document.activeElement).toBe(screen.getByLabelText("first"));
    fireEvent.keyDown(dialog, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "last" }));
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(onDismiss).toHaveBeenCalledTimes(1);
    unmount();
    expect(document.activeElement).toBe(opener);
    opener.remove();
  });
});

describe("Monaco follows the system colour scheme (A4)", () => {
  it("applies the theme now and on every change", () => {
    let listener: ((e: { matches: boolean }) => void) | undefined;
    const mql = {
      matches: false,
      addEventListener: (_: string, l: (e: { matches: boolean }) => void) => (listener = l),
      removeEventListener: vi.fn(),
    };
    const apply = vi.fn();
    const stop = watchColorScheme(mql, apply);
    expect(apply).toHaveBeenLastCalledWith("plover-light");
    listener!({ matches: true });
    expect(apply).toHaveBeenLastCalledWith("plover-dark");
    stop();
    expect(mql.removeEventListener).toHaveBeenCalled();
  });
});

describe("key/value labels (A6)", () => {
  it("has visible column headers and a label on every input", () => {
    render(<KeyValueTable rows={[{ name: "accept", value: "json", enabled: true, local: false }]} onChange={() => {}} />);
    expect(screen.getByRole("columnheader", { name: "Name" })).toBeTruthy();
    expect(screen.getByRole("columnheader", { name: "Value" })).toBeTruthy();
    expect(screen.getByRole("textbox", { name: "Name of accept" })).toBeTruthy();
    expect(screen.getByRole("textbox", { name: "Value of accept" })).toBeTruthy();
    expect(screen.getByRole("textbox", { name: "New name" })).toBeTruthy();
  });

  it("the assert table explains its example instead of showing a fake row", () => {
    render(
      <KeyValueTable
        rows={[]}
        onChange={() => {}}
        nameLabel="Assertion"
        valueLabel="Expected"
        hint="e.g. res.status — eq 200"
      />,
    );
    expect(screen.getByRole("columnheader", { name: "Assertion" })).toBeTruthy();
    expect(screen.getByText("e.g. res.status — eq 200")).toBeTruthy();
    const input = screen.getByRole("textbox", { name: "New assertion" }) as HTMLInputElement;
    expect(input.placeholder).not.toBe("res.status");
  });
});

const result = (over: Partial<SendResultDto> = {}): SendResultDto =>
  ({
    response: {
      status: 200,
      headers: [["content-type", "application/json"]],
      elapsedMs: 12,
      size: 2,
      sizeComplete: true,
      bodyText: "{}",
      bodyBase64: null,
      truncated: false,
    },
    tests: [],
    asserts: [],
    console: [],
    scriptError: null,
    skipped: null,
    scriptedRequests: [],
    ...over,
  }) as SendResultDto;

describe("response header (A9, quick wins)", () => {
  it("shows the reason phrase", () => {
    render(<ResponseViewer result={result()} sending={false} />);
    expect(screen.getByText("200 OK")).toBeTruthy();
  });

  it("signals a failed test before the green status and opens the Tests tab", () => {
    render(
      <ResponseViewer
        result={result({ tests: [{ name: "t", passed: false, error: "boom" }] } as Partial<SendResultDto>)}
        sending={false}
      />,
    );
    expect(screen.getByRole("img", { name: "Checks: 0 of 1 passed" })).toBeTruthy();
    expect(screen.getByRole("tab", { name: /Tests/, selected: true })).toBeTruthy();
  });

  it("signals a script error", () => {
    render(
      <ResponseViewer
        result={result({
          scriptError: { phase: "post-response", scope: "request", line: 1, name: "TypeError", message: "x" },
        } as Partial<SendResultDto>)}
        sending={false}
      />,
    );
    expect(screen.getByText(/Script error/)).toBeTruthy();
  });

  it("copies the body", async () => {
    const writeText = vi.fn(async () => {});
    Object.assign(navigator, { clipboard: { writeText } });
    render(<ResponseViewer result={result()} sending={false} />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Copy body" }));
    });
    expect(writeText).toHaveBeenCalledWith("{}");
    expect(screen.getByText("Copied")).toBeTruthy();
  });
});

describe("run panel (quick win)", () => {
  it("has column headers", () => {
    useRuns.setState({
      order: ["r1"],
      runs: {
        r1: { runId: "r1", label: "users", status: "done", events: [], report: undefined, error: undefined },
      } as never,
    });
    render(<RunPanel onClose={() => {}} />);
    for (const name of ["Result", "Method", "Name", "Status", "Time · Size", "Checks", "Message"]) {
      expect(screen.getByRole("columnheader", { name })).toBeTruthy();
    }
  });
});

describe("empty state (quick win)", () => {
  it("offers a primary open action when nothing is open", () => {
    useWorkspace.setState({ collections: [], pickerOpen: false });
    render(<EmptyState />);
    fireEvent.click(screen.getByRole("button", { name: /^Open collection…/ }));
    expect(useWorkspace.getState().pickerOpen).toBe(true);
  });
});

describe("environment editor dismissal (A3)", () => {
  beforeEach(() => {
    useWorkspace.setState({
      environments: {
        c1: [{ name: "local", vars: [{ name: "host", value: "h", enabled: true, local: false }], secretNames: [], secretsSet: [], hash: "x" }],
      } as never,
    });
  });

  it("closes on Escape when nothing changed", () => {
    const onClose = vi.fn();
    render(<EnvEditor collectionId="c1" name="local" onClose={onClose} />);
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
  });

  it("asks before discarding edits", () => {
    const onClose = vi.fn();
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    render(<EnvEditor collectionId="c1" name="local" onClose={onClose} />);
    fireEvent.change(screen.getByRole("textbox", { name: "Value of host" }), { target: { value: "changed" } });
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(confirm).toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });
});

describe("run result detail and history", () => {
  const ok = (index: number, item = "a.bru"): RunEventDto => ({
    type: "passed",
    runId: "r1",
    index,
    iteration: 0,
    item,
    status: 200,
    elapsedMs: 5,
    size: 2,
    tests: [],
    asserts: [],
    console: [],
  });
  const started = (item = "a.bru", name = "a"): RunEventDto => ({
    type: "started",
    runId: "r1",
    iteration: 0,
    item,
    name,
    method: "POST",
    url: "http://h/a",
  });
  const body = (b: Partial<BodyDetailDto>): BodyDetailDto => ({
    kind: "text",
    text: null,
    base64: null,
    fields: [],
    parts: [],
    size: 0,
    truncated: false,
    omitted: null,
    ...b,
  });
  const detail = (over: Partial<RunResultDetailDto> = {}): RunResultDetailDto => ({
    index: 0,
    item: "a.bru",
    name: "a",
    status: "pass",
    error: null,
    skipReason: null,
    request: {
      method: "POST",
      url: "http://h/a?x=1",
      headers: [["x-sent", "yes"]],
      body: body({ text: '{"q":1}', size: 7 }),
    },
    responseBody: body({ text: "ok", size: 2 }),
    result: {
      response: {
        status: 200,
        headers: [["content-type", "text/plain"]],
        elapsedMs: 5,
        size: 2,
        sizeComplete: true,
        bodyText: "ok",
        bodyBase64: null,
        truncated: false,
      },
      tests: [],
      asserts: [],
      console: [],
      scriptError: null,
      scriptedRequests: [],
      skipped: null,
    },
    ...over,
  });
  let runResult: ReturnType<typeof vi.fn>;

  const live = (events: RunEventDto[], status = "done") =>
    useRuns.setState({
      order: ["r1"],
      viewing: null,
      history: {},
      runs: {
        r1: { runId: "r1", collectionId: "c1", item: "", label: "all", status, events, recordId: "rec", source: "live", iterations: [], env: null },
      } as never,
    });

  beforeEach(() => {
    runResult = vi.fn(async (_c: string, _r: string, index: number) => detail({ index }));
    setApi({
      runResult,
      listRuns: vi.fn(async () => [
        {
          recordId: "rec-old",
          source: "plv",
          target: ".",
          env: "local",
          startedAtMs: 0,
          outcome: "completed",
          report: { passed: 2, failed: 1, skipped: 0, testsPassed: 0, testsFailed: 0, assertsPassed: 0, assertsFailed: 0, elapsedMs: 9, iterations: 1 },
        },
      ]),
      getRun: vi.fn(async () => ({
        meta: { recordId: "rec-old", source: "plv", target: ".", env: "local", startedAtMs: 0, outcome: "completed", report: { passed: 1, failed: 0, skipped: 0, testsPassed: 0, testsFailed: 0, assertsPassed: 0, assertsFailed: 0, elapsedMs: 9, iterations: 1 } },
        label: "saved-col",
        events: [
          { type: "started", runId: "rec-old", iteration: 0, item: "a.bru", name: "a", method: "GET", url: "u" },
          { type: "passed", runId: "rec-old", iteration: 0, index: 0, item: "a.bru", status: 200, elapsedMs: 5, size: 2, tests: [], asserts: [], console: [] },
          { type: "finished", runId: "rec-old", report: { passed: 1, failed: 0, skipped: 0, testsPassed: 0, testsFailed: 0, assertsPassed: 0, assertsFailed: 0, elapsedMs: 9, iterations: 1 } },
        ],
      })),
      clearRuns: vi.fn(async () => {}),
    } as unknown as ApiClient);
  });

  it("clicking a finished row opens the detail with five tabs, and the Request tab shows what was sent", async () => {
    live([started(), ok(0)]);
    render(<RunPanel onClose={() => {}} />);
    const row = within(screen.getByRole("table", { name: "Run results" })).getAllByRole("row")[1]!;
    await act(async () => {
      fireEvent.click(row);
    });
    const tabs = within(screen.getByRole("tablist", { name: "Response sections" })).getAllByRole("tab");
    expect(tabs.map((t) => t.textContent)).toEqual(["Body", expect.stringMatching(/^Headers/), "Request", "Tests", "Console"]);
    fireEvent.click(screen.getByRole("tab", { name: "Request as sent" }));
    expect(screen.getByText("http://h/a?x=1", { exact: false })).toBeTruthy();
    expect(screen.getByText("x-sent")).toBeTruthy();
    expect(screen.getByText('{"q":1}')).toBeTruthy();
    expect(row.getAttribute("aria-selected")).toBe("true");
  });

  it("Enter on a row opens it; a running row is not selectable", async () => {
    live([started(), ok(0), started("b.bru", "b")], "running");
    render(<RunPanel onClose={() => {}} />);
    const rows = within(screen.getByRole("table", { name: "Run results" })).getAllByRole("row").slice(1);
    expect(rows[1]!.tabIndex).toBe(-1);
    expect(rows[1]!.getAttribute("aria-selected")).toBeNull();
    fireEvent.click(rows[1]!);
    expect(runResult).not.toHaveBeenCalled();
    await act(async () => {
      fireEvent.keyDown(rows[0]!, { key: "Enter" });
    });
    expect(runResult).toHaveBeenCalledWith("c1", "rec", 0);
  });

  it("the second row of a request run twice asks for index 1", async () => {
    live([started(), ok(0), started(), ok(1)]);
    render(<RunPanel onClose={() => {}} />);
    const rows = within(screen.getByRole("table", { name: "Run results" })).getAllByRole("row").slice(1);
    await act(async () => {
      fireEvent.click(rows[1]!);
    });
    expect(runResult).toHaveBeenCalledWith("c1", "rec", 1);
  });

  it("a skipped row shows its reason and no tabs", async () => {
    runResult.mockResolvedValue(detail({ status: "skipped", skipReason: "skipRequest", request: null, responseBody: null }));
    live([{ type: "skipped", runId: "r1", iteration: 0, index: 0, item: "a.bru", reason: "skipRequest" }]);
    render(<RunPanel onClose={() => {}} />);
    const rows = within(screen.getByRole("table", { name: "Run results" })).getAllByRole("row").slice(1);
    await act(async () => {
      fireEvent.click(rows[0]!);
    });
    expect(screen.getByText("Skipped: skipRequest")).toBeTruthy();
    expect(screen.queryByRole("tablist", { name: "Response sections" })).toBeNull();
  });

  it("binary, truncated and omitted bodies say so", async () => {
    const binary = detail({
      request: { method: "POST", url: "u", headers: [], body: body({ kind: "base64", base64: "/wAB", size: 3 }) },
      responseBody: body({ text: "aaa", size: 5_000_000, truncated: true }),
    });
    runResult.mockResolvedValue(binary);
    live([started(), ok(0)]);
    render(<RunPanel onClose={() => {}} />);
    await act(async () => {
      fireEvent.click(within(screen.getByRole("table", { name: "Run results" })).getAllByRole("row")[1]!);
    });
    expect(screen.getByText(/Showing the first 1 MiB of/)).toBeTruthy();
    fireEvent.click(screen.getByRole("tab", { name: "Request as sent" }));
    expect(screen.getByText("Binary body, 3 bytes")).toBeTruthy();
    cleanup();
    runResult.mockResolvedValue(detail({ responseBody: body({ kind: "omitted", omitted: "run size limit", size: 9 }) }));
    live([started(), ok(0)]);
    render(<RunPanel onClose={() => {}} />);
    await act(async () => {
      fireEvent.click(within(screen.getByRole("table", { name: "Run results" })).getAllByRole("row")[1]!);
    });
    expect(screen.getByText("Body not kept (run size limit)")).toBeTruthy();
  });

  it("History lists saved runs and opens one read-only", async () => {
    live([started(), ok(0)], "running");
    render(<RunPanel onClose={() => {}} />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "History" }));
    });
    const menu = screen.getByRole("dialog", { name: "Run history" });
    const entry = within(menu).getByRole("button", { name: /plv/ });
    expect(entry.textContent).toMatch(/collection · local · 2 passed, 1 failed, 0 skipped/);
    await act(async () => {
      fireEvent.click(entry);
    });
    expect(screen.getByText("Run: saved-col")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Cancel" })).toBeNull();
    expect(within(screen.getByRole("table", { name: "Run results" })).getAllByRole("row")).toHaveLength(2);
    fireEvent.click(screen.getByRole("button", { name: "Back to latest run" }));
    expect(screen.getByText("Run: all")).toBeTruthy();
  });

  it("Clear history asks first", async () => {
    live([started(), ok(0)]);
    render(<RunPanel onClose={() => {}} />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "History" }));
    });
    fireEvent.click(within(screen.getByRole("dialog", { name: "Run history" })).getByRole("button", { name: "Clear history" }));
    const confirm = screen.getByRole("dialog", { name: "Clear run history" });
    await act(async () => {
      fireEvent.click(within(confirm).getByRole("button", { name: "Clear history" }));
    });
    expect(api().clearRuns).toHaveBeenCalledWith("c1");
  });

  it("with no run yet the panel still offers History", () => {
    useRuns.setState({ order: [], runs: {}, viewing: null, history: {} });
    render(<RunPanel collectionId="c1" onClose={() => {}} />);
    expect(screen.getByRole("button", { name: "History" })).toBeTruthy();
  });
});
