import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../api/client";
import { setApi } from "../api/instance";
import type { RunEventDto } from "../api/types";
import { useRuns } from "./runs";

let emit: (e: RunEventDto) => void = () => {};
let finish: () => void = () => {};
const cancel = vi.fn();
const runResult = vi.fn();
const listRuns = vi.fn();
const getRun = vi.fn();
const clearRuns = vi.fn();

const report = { passed: 1, failed: 0, skipped: 0, testsPassed: 0, testsFailed: 0, assertsPassed: 0, assertsFailed: 0, elapsedMs: 6, iterations: 1 };

beforeEach(() => {
  cancel.mockReset();
  runResult.mockReset().mockResolvedValue({ index: 1, name: "a" });
  listRuns.mockReset().mockResolvedValue([]);
  getRun.mockReset();
  clearRuns.mockReset().mockResolvedValue(undefined);
  useRuns.setState({ runs: {}, order: [], viewing: null, history: {} });
  setApi({
    startRun: vi.fn((_id: string, _item: string, _env: string | null, onEvent: (e: RunEventDto) => void) => {
      emit = onEvent;
      const done = new Promise<void>((r) => (finish = r));
      return { runId: "r1", cancel, done };
    }),
    runResult,
    listRuns,
    getRun,
    clearRuns,
  } as unknown as ApiClient);
});

const flush = () => new Promise((r) => setTimeout(r, 0));

describe("runs store", () => {
  it("accumulates events and finishes with the report", async () => {
    const id = useRuns.getState().start("c1", "folder", "local", "folder");
    expect(id).toBe("r1");
    expect(useRuns.getState().runs.r1!.status).toBe("running");
    emit({ type: "started", runId: "r1", iteration: 0, item: "a.bru", name: "a", method: "GET", url: "u" });
    emit({ type: "passed", runId: "r1", iteration: 0, index: 0, item: "a.bru", status: 200, elapsedMs: 5, size: 2, tests: [], asserts: [], console: [] });
    emit({ type: "finished", runId: "r1", report });
    finish();
    await Promise.resolve();
    const run = useRuns.getState().runs.r1!;
    expect(run.events.map((e) => e.type)).toEqual(["started", "passed", "finished"]);
    expect(run.status).toBe("done");
    expect(run.report).toEqual(report);
  });

  it("cancel marks the run cancelled", async () => {
    useRuns.getState().start("c1", "", null, "all");
    useRuns.getState().cancel("r1");
    expect(cancel).toHaveBeenCalled();
    expect(useRuns.getState().runs.r1!.status).toBe("cancelled");
    finish();
    await Promise.resolve();
    expect(useRuns.getState().runs.r1!.status).toBe("cancelled");
  });

  it("a socket that closes mid-run is an error", async () => {
    useRuns.getState().start("c1", "", null, "all");
    emit({ type: "started", runId: "r1", iteration: 0, item: "a.bru", name: "a", method: "GET", url: "u" });
    finish();
    await new Promise((r) => setTimeout(r, 0));
    const run = useRuns.getState().runs.r1!;
    expect(run.status).toBe("error");
    expect(run.error).toMatch(/closed/);
  });

  it("server error events end the run", () => {
    // What the client forwards for a failed start: the server echoes the runId.
    useRuns.getState().start("c1", "", "nope", "all");
    emit({ type: "error", runId: "r1", error: "run_failed", message: "environment `nope` not found" });
    const run = useRuns.getState().runs.r1!;
    expect(run.status).toBe("error");
    expect(run.error).toBe("environment `nope` not found");
  });

  it("accepted sets the record ID, and select fetches that result's detail", async () => {
    useRuns.getState().start("c1", "", null, "all");
    emit({ type: "accepted", runId: "r1", recordId: "rec" });
    expect(useRuns.getState().runs.r1!.recordId).toBe("rec");
    useRuns.getState().select("r1", 1);
    expect(useRuns.getState().runs.r1!.detail).toEqual({ index: 1, loading: true });
    await flush();
    expect(runResult).toHaveBeenCalledWith("c1", "rec", 1);
    expect(useRuns.getState().runs.r1!.detail).toEqual({ index: 1, loading: false, data: { index: 1, name: "a" } });
  });

  it("a finished event refetches an open detail once (masked with the whole run)", async () => {
    useRuns.getState().start("c1", "", null, "all");
    emit({ type: "accepted", runId: "r1", recordId: "rec" });
    useRuns.getState().select("r1", 0);
    await flush();
    expect(runResult).toHaveBeenCalledTimes(1);
    emit({ type: "finished", runId: "r1", report });
    await flush();
    expect(runResult).toHaveBeenCalledTimes(2);
    expect(listRuns).toHaveBeenCalledWith("c1");
  });

  it("openHistory shows a saved run read-only until closed", async () => {
    getRun.mockResolvedValue({
      meta: { recordId: "rec", source: "plv", target: ".", env: null, startedAtMs: 1, outcome: "cancelled", report },
      label: "c",
      events: [{ type: "skipped", runId: "rec", iteration: 0, index: 0, item: "a.bru", reason: "x" }],
    });
    await useRuns.getState().openHistory("c1", "rec");
    const s = useRuns.getState();
    expect(s.viewing).toBe("rec");
    expect(s.runs.rec).toMatchObject({ source: "history", status: "cancelled", recordId: "rec", label: "c", report });
    expect(s.runs.rec!.events).toHaveLength(1);
    useRuns.getState().closeHistory();
    expect(useRuns.getState().viewing).toBeNull();
  });

  it("clearHistory empties the list and drops opened saved runs", async () => {
    useRuns.setState({
      history: { c1: [{ recordId: "rec" } as never] },
      runs: { rec: { runId: "rec", collectionId: "c1", source: "history" } as never },
      viewing: "rec",
    });
    await useRuns.getState().clearHistory("c1");
    const s = useRuns.getState();
    expect(clearRuns).toHaveBeenCalledWith("c1");
    expect(s.history.c1).toEqual([]);
    expect(s.runs.rec).toBeUndefined();
    expect(s.viewing).toBeNull();
  });

  it("runId-less errors forwarded by the client also end the run", () => {
    useRuns.getState().start("c1", "", null, "all");
    emit({ type: "error", runId: null, error: "bad_message", message: "bad" });
    expect(useRuns.getState().runs.r1!.status).toBe("error");
  });
});
