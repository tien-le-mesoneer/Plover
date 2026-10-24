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
