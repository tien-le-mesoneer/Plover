import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../api/client";
import { setApi } from "../api/instance";
import type { RunEventDto } from "../api/types";
import { useRuns } from "./runs";

let emit: (e: RunEventDto) => void = () => {};
let finish: () => void = () => {};
const cancel = vi.fn();

beforeEach(() => {
  cancel.mockReset();
  useRuns.setState({ runs: {}, order: [] });
  setApi({
    startRun: vi.fn((_id: string, _item: string, _env: string | null, onEvent: (e: RunEventDto) => void) => {
      emit = onEvent;
      const done = new Promise<void>((r) => (finish = r));
      return { runId: "r1", cancel, done };
    }),
  } as unknown as ApiClient);
});

describe("runs store", () => {
  it("accumulates events and finishes with the report", async () => {
    const id = useRuns.getState().start("c1", "folder", "local", "folder");
    expect(id).toBe("r1");
    expect(useRuns.getState().runs.r1!.status).toBe("running");
    emit({ type: "started", runId: "r1", item: "a.bru", name: "a", method: "GET", url: "u" });
    emit({ type: "passed", runId: "r1", index: 0, item: "a.bru", status: 200, elapsedMs: 5, size: 2, tests: [], asserts: [], console: [] });
    const report = { passed: 1, failed: 0, skipped: 0, testsPassed: 0, testsFailed: 0, assertsPassed: 0, assertsFailed: 0, elapsedMs: 6 };
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
    emit({ type: "started", runId: "r1", item: "a.bru", name: "a", method: "GET", url: "u" });
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

  it("runId-less errors forwarded by the client also end the run", () => {
    useRuns.getState().start("c1", "", null, "all");
    emit({ type: "error", runId: null, error: "bad_message", message: "bad" });
    expect(useRuns.getState().runs.r1!.status).toBe("error");
  });
});
