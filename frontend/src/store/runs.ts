import { create } from "zustand";
import { api } from "../api/instance";
import type { RunEventDto, RunReportDto } from "../api/types";

export interface RunState {
  runId: string;
  collectionId: string;
  item: string;
  label: string;
  events: RunEventDto[];
  status: "running" | "done" | "cancelled" | "error";
  report?: RunReportDto;
  error?: string;
}

export interface RunsState {
  runs: Record<string, RunState>;
  /** Run IDs, newest last. */
  order: string[];
  start(collectionId: string, item: string, env: string | null, label: string): string;
  cancel(runId: string): void;
}

const cancels = new Map<string, () => void>();

export const useRuns = create<RunsState>()((set, get) => {
  const update = (runId: string, f: (r: RunState) => Partial<RunState>) =>
    set((s) => {
      const run = s.runs[runId];
      return run ? { runs: { ...s.runs, [runId]: { ...run, ...f(run) } } } : s;
    });

  return {
    runs: {},
    order: [],

    start(collectionId, item, env, label) {
      let runId = "";
      const handle = api().startRun(collectionId, item, env, (event) => {
        // Events arrive asynchronously, after `runId` is assigned below.
        update(runId, (r) => {
          const events = [...r.events, event];
          switch (event.type) {
            case "finished":
              return { events, status: "done", report: event.report };
            case "cancelled":
              return { events, status: "cancelled" };
            case "error":
              return { events, status: "error", error: event.message };
            default:
              return { events };
          }
        });
      });
      runId = handle.runId;
      cancels.set(runId, handle.cancel);
      set((s) => ({
        runs: {
          ...s.runs,
          [runId]: { runId, collectionId, item, label, events: [], status: "running" },
        },
        order: [...s.order, runId],
      }));
      void handle.done.then(() => {
        cancels.delete(runId);
        if (get().runs[runId]?.status === "running") {
          update(runId, () => ({ status: "error", error: "connection closed before the run finished" }));
        }
      });
      return runId;
    },

    cancel(runId) {
      cancels.get(runId)?.();
      update(runId, (r) => (r.status === "running" ? { status: "cancelled" } : {}));
    },
  };
});
