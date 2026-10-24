import type { RunEventDto } from "../api/types";

export type RunRowState = "running" | "pass" | "fail" | "skip";

export interface RunRow {
  /** The result's index in the run (`-1` while the request is still running). */
  index: number;
  item: string;
  name: string;
  method: string;
  /** The folder path of the request (`""` at the collection root). */
  folder: string;
  state: RunRowState;
  iteration: number;
  status?: number | null;
  ms?: number | null;
  size?: number;
  /** Failure or skip reason. */
  message?: string;
  /** `passed/total` of tests and asserts, when there are any. */
  checks?: string;
}

/** An iteration header of a data-driven or repeated run. */
export interface IterationHeader {
  kind: "iteration";
  index: number;
  total: number;
  data: Record<string, unknown>;
}

export type RunListEntry = ({ kind: "row" } & RunRow) | IterationHeader;

export type RunFilterValue = "all" | "fail" | "pass" | "skip";

function folderOf(item: string): string {
  const i = item.lastIndexOf("/");
  return i < 0 ? "" : item.slice(0, i);
}

/** "3/4": asserts count as tests, like Bruno's runner. */
function checks(e: Extract<RunEventDto, { type: "passed" | "failed" }>): string | undefined {
  const all = [...e.asserts.map((a) => a.passed), ...e.tests.map((t) => t.passed)];
  return all.length ? `${all.filter(Boolean).length}/${all.length}` : undefined;
}

/**
 * Rows in run order: a `started` event opens a running row, its result event finishes it
 * (rows are keyed by result index, so a request run twice has two rows). Iteration headers
 * come before their rows.
 */
export function buildEntries(events: RunEventDto[]): RunListEntry[] {
  const out: RunListEntry[] = [];
  for (const e of events) {
    if (e.type === "iterationStarted") {
      out.push({ kind: "iteration", index: e.index, total: e.total, data: e.data });
    } else if (e.type === "started") {
      out.push({
        kind: "row",
        index: -1,
        item: e.item,
        name: e.name,
        method: e.method,
        folder: folderOf(e.item),
        state: "running",
        iteration: e.iteration,
      });
    } else if (e.type === "passed" || e.type === "failed" || e.type === "skipped") {
      let row = [...out]
        .reverse()
        .find((r): r is { kind: "row" } & RunRow => r.kind === "row" && r.item === e.item && r.state === "running");
      if (!row) {
        row = {
          kind: "row",
          index: e.index,
          item: e.item,
          name: e.item.replace(/^.*\//, "").replace(/\.bru$/, ""),
          method: "",
          folder: folderOf(e.item),
          state: "running",
          iteration: e.iteration,
        };
        out.push(row);
      }
      row.index = e.index;
      if (e.type === "passed") {
        Object.assign(row, { state: "pass", status: e.status, ms: e.elapsedMs, size: e.size, checks: checks(e) });
      } else if (e.type === "failed") {
        Object.assign(row, { state: "fail", status: e.status, ms: e.elapsedMs, message: e.error, checks: checks(e) });
      } else {
        Object.assign(row, { state: "skip", message: e.reason });
      }
    }
  }
  return out;
}

export function buildRows(events: RunEventDto[]): RunRow[] {
  return buildEntries(events).filter((e): e is { kind: "row" } & RunRow => e.kind === "row");
}

/** Names on two or more rows: only those rows show their folder (to tell them apart). */
export function duplicateNames(rows: RunRow[]): Set<string> {
  const seen = new Set<string>();
  const dup = new Set<string>();
  for (const r of rows) {
    if (seen.has(r.name)) dup.add(r.name);
    seen.add(r.name);
  }
  return dup;
}

/** Counts for the filter; running rows count only in All. */
export function filterCounts(rows: RunRow[]): Record<RunFilterValue, number> {
  return {
    all: rows.length,
    fail: rows.filter((r) => r.state === "fail").length,
    pass: rows.filter((r) => r.state === "pass").length,
    skip: rows.filter((r) => r.state === "skip").length,
  };
}

export function matchesFilter(row: RunRow, filter: RunFilterValue, name: string): boolean {
  if (filter !== "all" && row.state !== filter) return false;
  const q = name.trim().toLowerCase();
  return !q || row.name.toLowerCase().includes(q) || row.item.toLowerCase().includes(q);
}
