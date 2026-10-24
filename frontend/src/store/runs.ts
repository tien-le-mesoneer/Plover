import { create } from "zustand";
import type { RunOptions } from "../api/client";
import { api } from "../api/instance";
import type { RunEventDto, RunMetaDto, RunReportDto, RunResultDetailDto } from "../api/types";

export interface RunDetail {
  index: number;
  loading: boolean;
  data?: RunResultDetailDto;
  error?: string;
}

export interface RunState {
  runId: string;
  collectionId: string;
  item: string;
  label: string;
  events: RunEventDto[];
  status: "running" | "done" | "cancelled" | "error";
  report?: RunReportDto;
  error?: string;
  /** The run's ID in the run history (from the `accepted` event, or the saved run). */
  recordId?: string;
  /** `history`: a saved run opened from the History menu (read-only). */
  source: "live" | "history";
  /** The result index whose detail is shown. */
  selected?: number;
  detail?: RunDetail;
  /** Iterations of a data-driven or repeated run, with their (masked) rows. */
  iterations: RunIteration[];
  /** The environment the run used (for "Run with data…" and re-runs). */
  env: string | null;
}

export interface RunIteration {
  index: number;
  total: number;
  data: Record<string, unknown>;
}

export interface RunsState {
  runs: Record<string, RunState>;
  /** Run IDs, newest last. */
  order: string[];
  /** A history run shown instead of the newest live run, until closed. */
  viewing: string | null;
  /** Saved runs per collection ID, newest first. */
  history: Record<string, RunMetaDto[]>;
  start(collectionId: string, item: string, env: string | null, label: string, options?: RunOptions): string;
  cancel(runId: string): void;
  /** Shows result `index` of a run, fetching its request and response. */
  select(runId: string, index: number): void;
  loadHistory(collectionId: string): Promise<void>;
  openHistory(collectionId: string, recordId: string): Promise<void>;
  closeHistory(): void;
  clearHistory(collectionId: string): Promise<void>;
}

const cancels = new Map<string, () => void>();

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export const useRuns = create<RunsState>()((set, get) => {
  const update = (runId: string, f: (r: RunState) => Partial<RunState>) =>
    set((s) => {
      const run = s.runs[runId];
      return run ? { runs: { ...s.runs, [runId]: { ...run, ...f(run) } } } : s;
    });

  const fetchDetail = async (runId: string, index: number) => {
    const run = get().runs[runId];
    if (!run?.recordId) {
      update(runId, () => ({ detail: { index, loading: false, error: "This run has no saved results." } }));
      return;
    }
    try {
      const data = await api().runResult(run.collectionId, run.recordId, index);
      update(runId, (r) => (r.selected === index ? { detail: { index, loading: false, data } } : {}));
    } catch (e) {
      update(runId, (r) => (r.selected === index ? { detail: { index, loading: false, error: message(e) } } : {}));
    }
  };

  return {
    runs: {},
    order: [],
    viewing: null,
    history: {},

    start(collectionId, item, env, label, options) {
      let runId = "";
      const handle = api().startRun(collectionId, item, env, (event) => {
        // Events arrive asynchronously, after `runId` is assigned below.
        update(runId, (r) => {
          const events = [...r.events, event];
          switch (event.type) {
            case "accepted":
              return { events, recordId: event.recordId };
            case "iterationStarted":
              return {
                events,
                iterations: [...r.iterations, { index: event.index, total: event.total, data: event.data }],
              };
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
        // A detail fetched during the run was masked with the values known then; the
        // finished run may mask more.
        if (event.type === "finished" || event.type === "cancelled") {
          const selected = get().runs[runId]?.selected;
          if (selected !== undefined) void fetchDetail(runId, selected);
          void get().loadHistory(collectionId);
        }
      }, options);
      runId = handle.runId;
      cancels.set(runId, handle.cancel);
      set((s) => ({
        runs: {
          ...s.runs,
          [runId]: {
            runId,
            collectionId,
            item,
            label,
            events: [],
            status: "running",
            source: "live",
            iterations: [],
            env,
          },
        },
        order: [...s.order, runId],
        viewing: null,
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

    select(runId, index) {
      update(runId, () => ({ selected: index, detail: { index, loading: true } }));
      void fetchDetail(runId, index);
    },

    async loadHistory(collectionId) {
      try {
        const runs = await api().listRuns(collectionId);
        set((s) => ({ history: { ...s.history, [collectionId]: runs } }));
      } catch {
        // The list stays as it was; the menu shows what it has.
      }
    },

    async openHistory(collectionId, recordId) {
      const run = await api().getRun(collectionId, recordId);
      const status: RunState["status"] = run.meta.outcome === "cancelled" ? "cancelled" : "done";
      set((s) => ({
        runs: {
          ...s.runs,
          [recordId]: {
            runId: recordId,
            collectionId,
            item: run.meta.target,
            label: run.label,
            events: run.events,
            status,
            report: run.meta.report,
            recordId,
            source: "history",
            iterations: run.events.flatMap((e) =>
              e.type === "iterationStarted" ? [{ index: e.index, total: e.total, data: e.data }] : [],
            ),
            env: run.meta.env,
          },
        },
        viewing: recordId,
      }));
    },

    closeHistory() {
      set({ viewing: null });
    },

    async clearHistory(collectionId) {
      await api().clearRuns(collectionId);
      set((s) => {
        const runs = Object.fromEntries(
          Object.entries(s.runs).filter(([, r]) => !(r.source === "history" && r.collectionId === collectionId)),
        );
        const viewing = s.viewing && runs[s.viewing] ? s.viewing : null;
        return { runs, viewing, history: { ...s.history, [collectionId]: [] } };
      });
    },
  };
});

/** The run the run panel shows: an opened history run, else the newest run. */
export function shownRun(s: Pick<RunsState, "runs" | "order" | "viewing">): RunState | undefined {
  return (s.viewing ? s.runs[s.viewing] : undefined) ?? s.runs[s.order[s.order.length - 1] ?? ""];
}
