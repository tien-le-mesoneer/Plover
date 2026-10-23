import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HttpRequestDto, SendResultDto, TreeNodeDto } from "../api/types";
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
    expect(screen.getByText(/1 of 1 checks failed/)).toBeTruthy();
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
