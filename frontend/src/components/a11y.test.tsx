import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../api/client";
import { setApi } from "../api/instance";
import type { HttpRequestDto } from "../api/types";
import { useRuns } from "../store/runs";
import { useTabs } from "../store/tabs";
import { useWorkspace } from "../store/workspace";
import { KeyValueTable } from "./request/KeyValueTable";
import { RequestTabs } from "./RequestTabs";
import { runTabFor } from "./run/harness";

// Monaco needs a real browser; the response body editor is not what these tests check.
vi.mock("../editor/CodeEditor", () => ({ CodeEditor: () => null }));
import { Sidebar } from "./Sidebar";

const tree = {
  kind: "folder" as const,
  id: "",
  name: "c",
  method: null,
  seq: null,
  error: null,
  children: [],
};

const request = { name: "list users", method: "GET" } as HttpRequestDto;

beforeEach(() => {
  useWorkspace.setState({
    collections: [{ id: "c1", name: "ui-check", root: "/x", available: true, error: null }],
    expanded: {},
    filter: "",
    trees: { c1: tree },
    environments: {},
    activeEnv: {},
  });
  useTabs.setState({
    tabs: [{ key: "c1:a.bru", collectionId: "c1", item: "a.bru", saved: request, draft: request, hash: "h", sending: false }],
    active: "c1:a.bru",
  });
});

afterEach(cleanup);

describe("icon-only buttons have accessible names", () => {
  it("sidebar collection actions", () => {
    render(<Sidebar onRun={() => {}} />);
    expect(screen.getByRole("button", { name: "Run collection" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Reload from disk" })).toBeTruthy();
  });

  it("header_actions_reachable_by_tab", () => {
    render(<Sidebar onRun={() => {}} />);
    const header = document.querySelector(".collection-header")!;
    const buttons = [...header.querySelectorAll("button")];
    expect(buttons.map((b) => b.getAttribute("aria-label") ?? b.textContent)).toEqual([
      "ui-check",
      "Run collection",
      "Reload from disk",
      "Close collection ui-check",
    ]);
    for (const b of buttons) expect(b.getAttribute("tabindex")).not.toBe("-1");
    expect(buttons[0]!.className).toContain("collection-toggle");
  });

  it("tab close button", () => {
    render(<RequestTabs />);
    const tab = screen.getByRole("tab", { name: /list users/ });
    expect(within(tab).getByRole("button", { name: "Close" })).toBeTruthy();
  });

  it("key/value remove button", () => {
    render(<KeyValueTable rows={[{ name: "a", value: "1", enabled: true, local: false }]} onChange={() => {}} />);
    expect(screen.getByRole("button", { name: "Remove" })).toBeTruthy();
  });
});

describe("banners", () => {
  it("the ~ warning is one block of text, not a row of flex items", () => {
    render(<KeyValueTable rows={[{ name: "~x", value: "", enabled: true, local: false }]} onChange={() => {}} />);
    const note = screen.getByRole("note");
    expect(note.children).toHaveLength(1);
    expect(note.children[0]!.textContent).toMatch(/^A name starting with ~ means .disabled. in \.bru; .* without the ~\.$/);
  });
});

describe("run panel keyboard access", () => {
  beforeEach(() => {
    setApi({ listRuns: async () => [], runResult: async () => ({}) } as unknown as ApiClient);
    useRuns.setState({
      order: ["r1"],
      viewing: null,
      history: {},
      runs: {
        r1: {
          runId: "r1",
          collectionId: "c1",
          item: "",
          label: "all",
          status: "done",
          recordId: "rec",
          source: "live",
          iterations: [],
          env: null,
          events: [
            { type: "started", runId: "r1", iteration: 0, item: "a.bru", name: "a", method: "GET", url: "u" },
            { type: "passed", runId: "r1", iteration: 0, index: 0, item: "a.bru", status: 200, elapsedMs: 1, size: 1, tests: [], asserts: [], console: [] },
          ],
        },
      },
    });
  });

  it("finished rows are focusable and say whether they are selected", () => {
    render(runTabFor());
    const row = within(screen.getByRole("table", { name: "Run results" })).getAllByRole("row")[1]!;
    expect(row.tabIndex).toBe(0);
    expect(row.getAttribute("aria-selected")).toBe("false");
  });

  it("the History menu takes focus and Escape returns it to the button", async () => {
    render(runTabFor());
    const button = screen.getByRole("button", { name: /Previous runs/ });
    button.focus();
    await act(async () => {
      fireEvent.click(button);
    });
    const menu = screen.getByRole("dialog", { name: "Run history" });
    expect(menu.contains(document.activeElement)).toBe(true);
    fireEvent.keyDown(menu, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Run history" })).toBeNull();
    expect(document.activeElement).toBe(button);
  });
});

describe("run with data dialog", () => {
  it("focuses the file input first, keeps Tab inside, and Escape dismisses", async () => {
    const { RunWithDataDialog } = await import("./RunWithDataDialog");
    const onClose = vi.fn();
    render(<RunWithDataDialog collectionId="c1" item="" label="all" env={null} onClose={onClose} />);
    const dialog = screen.getByRole("dialog", { name: "Run with data" });
    expect(document.activeElement).toBe(dialog.querySelector('input[type="file"]'));
    expect(screen.getByLabelText("Data file (CSV or JSON)")).toBeTruthy();
    const run = within(dialog).getByRole("button", { name: "Run" });
    // Run is enabled (count 1); Tab from the last control wraps to the first.
    run.focus();
    fireEvent.keyDown(dialog, { key: "Tab" });
    expect(document.activeElement).toBe(dialog.querySelector('input[type="file"]'));
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
  });
});

describe("cancel button", () => {
  it("has an accessible name and takes focus while a send runs", async () => {
    const { UrlBar } = await import("./request/UrlBar");
    const draft = { ...request, url: "http://example.test", method: "GET" } as HttpRequestDto;
    render(<UrlBar draft={draft} sending sendStartedAt={Date.now()} onEdit={() => {}} onSend={() => {}} onCancel={() => {}} />);
    const button = screen.getByRole("button", { name: "Cancel request" });
    button.focus();
    expect(document.activeElement).toBe(button);
  });
});

describe("quick open", () => {
  it("is a labelled dialog with a combobox that points at the active option", async () => {
    const { QuickOpen } = await import("./QuickOpen");
    useWorkspace.setState({
      trees: {
        c1: { ...tree, children: [{ kind: "request", id: "a.bru", name: "list users", method: "GET", seq: null, error: null, children: null }] },
      },
    });
    render(<QuickOpen open onClose={() => {}} />);
    const box = screen.getByRole("combobox");
    fireEvent.change(box, { target: { value: "list" } });
    const listbox = screen.getByRole("listbox");
    expect(box.getAttribute("aria-controls")).toBe(listbox.id);
    expect(box.getAttribute("aria-expanded")).toBe("true");
    const option = within(listbox).getByRole("option");
    expect(box.getAttribute("aria-activedescendant")).toBe(option.id);
    expect(screen.getByRole("dialog", { name: "Quick open" })).toBeTruthy();
  });
});

describe("variable hover", () => {
  it("mirror is hidden from assistive tech; the card is described-by while shown", async () => {
    const { VarInput } = await import("./request/VarInput");
    const { useVars } = await import("../store/vars");
    useVars.setState({
      byTab: { "c1:a.bru": { vars: [{ name: "h", source: "runtime", value: "x", masked: false, truncated: false, shadows: [] }], setLater: [] } },
    });
    const { container } = render(<VarInput tabKey="c1:a.bru" aria-label="Field" value="{{h}}" onChange={() => {}} />);
    expect(container.querySelector(".var-mirror")!.getAttribute("aria-hidden")).toBe("true");
    const input = screen.getByLabelText("Field");
    expect(input.getAttribute("aria-describedby")).toBeNull();
    const span = container.querySelector<HTMLElement>(".var-mirror .var-defined")!;
    span.getBoundingClientRect = () => ({ left: 0, right: 50, top: 0, bottom: 20, width: 50, height: 20, x: 0, y: 0, toJSON: () => ({}) });
    fireEvent.mouseMove(input, { clientX: 5, clientY: 5 });
    expect(input.getAttribute("aria-describedby")).toBe(screen.getByRole("dialog").id);
  });
});
