import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../api/client";
import { ApiError } from "../api/client";
import { setApi } from "../api/instance";
import type { RunEventDto } from "../api/types";
import { useRuns } from "../store/runs";
import { useWorkspace } from "../store/workspace";
import { runTabFor } from "./run/harness";
import { RunWithDataDialog } from "./RunWithDataDialog";

vi.mock("../editor/CodeEditor", () => ({ CodeEditor: () => null }));

const report = { passed: 2, failed: 0, skipped: 0, testsPassed: 0, testsFailed: 0, assertsPassed: 0, assertsFailed: 0, elapsedMs: 9, iterations: 2 };

const ok = (index: number, iteration: number): RunEventDto => ({
  type: "passed",
  runId: "r1",
  index,
  iteration,
  item: "a.bru",
  status: 200,
  elapsedMs: 5,
  size: 2,
  tests: [],
  asserts: [],
  console: [],
});
const started = (iteration: number): RunEventDto => ({
  type: "started",
  runId: "r1",
  iteration,
  item: "a.bru",
  name: "a",
  method: "GET",
  url: "u",
});

const live = (events: RunEventDto[], status = "done") =>
  useRuns.setState({
    order: ["r1"],
    viewing: null,
    history: {},
    runs: {
      r1: { runId: "r1", collectionId: "c1", item: "", label: "all", status, events, recordId: "rec", source: "live", iterations: [], env: "local" },
    } as never,
  });

let previewDataFile: ReturnType<typeof vi.fn>;
let startRun: ReturnType<typeof vi.fn>;
let downloadReport: ReturnType<typeof vi.fn>;
let saveReport: ReturnType<typeof vi.fn>;

function csvFile(text: string, name = "rows.csv"): File {
  return new File([text], name, { type: "text/csv" });
}

beforeEach(() => {
  useWorkspace.setState({
    collections: [{ id: "c1", name: "ui-check", root: "/x", available: true, error: null }],
    activeEnv: { c1: "local" },
    capabilities: { desktop: false, nativePicker: false, openExternal: false, nativeSave: false, reveal: false, secretBackend: "memory" },
  });
  previewDataFile = vi.fn(async () => ({ rows: 2, columns: ["user", "password"] }));
  startRun = vi.fn(() => ({ runId: "r9", cancel: vi.fn(), done: new Promise(() => {}) }));
  downloadReport = vi.fn(async () => ({ blob: new Blob(["{}"]), fileName: "ui-check-rec.json" }));
  saveReport = vi.fn(async () => "/saved/x.html");
  setApi({
    previewDataFile,
    startRun,
    downloadReport,
    saveReport,
    listRuns: vi.fn(async () => []),
    runResult: vi.fn(),
  } as unknown as ApiClient);
});

afterEach(cleanup);

async function pick(text: string, name = "rows.csv") {
  const input = document.querySelector<HTMLInputElement>('input[type="file"]')!;
  await act(async () => {
    fireEvent.change(input, { target: { files: [csvFile(text, name)] } });
  });
  // File.text() and the preview call settle on later ticks.
  await act(async () => {
    await new Promise((r) => setTimeout(r, 0));
  });
}

describe("Run with data", () => {
  it("previews counts and column names, never values, and sends the file text", async () => {
    const onClose = vi.fn();
    render(<RunWithDataDialog collectionId="c1" item="" label="all" env="local" onClose={onClose} />);
    await pick("user,password\nalice,pw-SECRET-1\nbob,pw-SECRET-2\n");
    expect(previewDataFile).toHaveBeenCalledWith("c1", {
      format: "csv",
      text: "user,password\nalice,pw-SECRET-1\nbob,pw-SECRET-2\n",
    });
    expect(screen.getByRole("status").textContent).toBe("rows.csv: 2 rows · user, password");
    expect(document.body.textContent).not.toContain("alice");
    expect(document.body.textContent).not.toContain("pw-SECRET");
    const count = screen.getByLabelText("Iterations") as HTMLInputElement;
    expect(count.disabled).toBe(true);
    expect(screen.getByText("set by file (2 rows)")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Run" }));
    expect(startRun).toHaveBeenCalledWith(
      "c1",
      "",
      "local",
      expect.any(Function),
      { data: { format: "csv", text: "user,password\nalice,pw-SECRET-1\nbob,pw-SECRET-2\n" } },
    );
    expect(onClose).toHaveBeenCalled();
  });

  it("a parse error shows the server message and disables Run", async () => {
    previewDataFile.mockRejectedValue(
      new ApiError(400, "bad_data", {}, "data file:3: expected 2 fields, found 1"),
    );
    render(<RunWithDataDialog collectionId="c1" item="" label="all" env={null} onClose={() => {}} />);
    await pick("a,b\n1,2\n3\n", "short.csv");
    expect(screen.getByRole("alert").textContent).toBe("short.csv:3: expected 2 fields, found 1");
    expect((screen.getByRole("button", { name: "Run" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("rejects other file types without asking the server", async () => {
    render(<RunWithDataDialog collectionId="c1" item="" label="all" env={null} onClose={() => {}} />);
    await pick("x", "rows.txt");
    expect(screen.getByRole("alert").textContent).toContain("choose a .csv or .json file");
    expect(previewDataFile).not.toHaveBeenCalled();
  });

  it("without a file, runs the count; the file is not remembered after closing", async () => {
    const { unmount } = render(
      <RunWithDataDialog collectionId="c1" item="users" label="users" env="local" onClose={() => {}} />,
    );
    fireEvent.change(screen.getByLabelText("Iterations"), { target: { value: "3" } });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));
    expect(startRun).toHaveBeenCalledWith("c1", "users", "local", expect.any(Function), { iterationCount: 3 });
    unmount();
    render(<RunWithDataDialog collectionId="c1" item="users" label="users" env="local" onClose={() => {}} />);
    expect(screen.queryByRole("status")).toBeNull();
    expect((screen.getByLabelText("Iterations") as HTMLInputElement).disabled).toBe(false);
  });

  it("opens from the run tab header", () => {
    live([started(0), ok(0, 0)]);
    render(runTabFor());
    fireEvent.click(screen.getByRole("button", { name: "Run with data…" }));
    expect(screen.getByRole("dialog", { name: "Run with data" })).toBeTruthy();
  });
});

describe("iteration groups", () => {
  it("shows a header row per iteration with counts and masked chips", () => {
    const long = "x".repeat(60);
    live([
      { type: "iterationStarted", runId: "r1", index: 0, total: 2, data: { user: "alice", password: "***" } },
      started(0),
      ok(0, 0),
      { type: "iterationStarted", runId: "r1", index: 1, total: 2, data: { user: long, n: 3 } },
      started(1),
      ok(1, 1),
      { type: "finished", runId: "r1", report },
    ]);
    render(runTabFor());
    const table = screen.getByRole("table", { name: "Run results" });
    const headers = within(table).getAllByRole("rowheader");
    expect(headers.map((h) => h.textContent?.split(" · ")[0])).toEqual(["Iteration 1 of 2", "Iteration 2 of 2"]);
    expect(headers[0]!.textContent).toContain("1 passed, 0 failed");
    expect(headers[0]!.textContent).toContain("password=***");
    const cut = within(headers[1]!).getByTitle(`user=${long}`);
    expect(cut.textContent).toBe(`user=${"x".repeat(40)}…`);
    expect(headers[1]!.textContent).toContain("n=3");
  });

  it("a plain run has no iteration header", () => {
    live([started(0), ok(0, 0)]);
    render(runTabFor());
    expect(within(screen.getByRole("table", { name: "Run results" })).queryAllByRole("rowheader")).toEqual([]);
  });
});

describe("Export", () => {
  it("offers HTML, JSON and JUnit, and downloads in a browser", async () => {
    live([started(0), ok(0, 0), { type: "finished", runId: "r1", report }]);
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});
    URL.createObjectURL = vi.fn(() => "blob:x");
    URL.revokeObjectURL = vi.fn();
    render(runTabFor());
    fireEvent.click(screen.getByRole("button", { name: "Export" }));
    const items = within(screen.getByRole("menu", { name: "Export report as" })).getAllByRole("menuitem");
    expect(items.map((i) => i.textContent)).toEqual(["HTML", "JSON", "JUnit"]);
    expect(document.activeElement).toBe(items[0]);
    await act(async () => {
      fireEvent.click(items[1]!);
    });
    expect(downloadReport).toHaveBeenCalledWith("c1", "rec", "json");
    expect(click).toHaveBeenCalled();
    expect(saveReport).not.toHaveBeenCalled();
    click.mockRestore();
  });

  it("uses the native save dialog in the desktop app", async () => {
    useWorkspace.setState({
      capabilities: { desktop: true, nativePicker: true, openExternal: true, nativeSave: true, reveal: true, secretBackend: "keychain" },
    });
    live([started(0), ok(0, 0), { type: "finished", runId: "r1", report }]);
    render(runTabFor());
    fireEvent.click(screen.getByRole("button", { name: "Export" }));
    await act(async () => {
      fireEvent.click(screen.getByRole("menuitem", { name: "HTML" }));
    });
    expect(saveReport).toHaveBeenCalledWith("c1", "rec", "html");
    expect(downloadReport).not.toHaveBeenCalled();
    expect(screen.getByRole("status").textContent).toBe("Saved /saved/x.html");
  });

  it("is disabled while the run is going; Escape closes the menu", () => {
    live([started(0)], "running");
    const { unmount } = render(runTabFor());
    expect((screen.getByRole("button", { name: "Export" }) as HTMLButtonElement).disabled).toBe(true);
    unmount();
    live([started(0), ok(0, 0), { type: "finished", runId: "r1", report }]);
    render(runTabFor());
    fireEvent.click(screen.getByRole("button", { name: "Export" }));
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
  });
});
