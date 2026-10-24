import type { RunEventDto, RunReportDto } from "../api/types";
import { formatDuration, formatSize } from "../lib/format";
import { shownRun, useRuns } from "../store/runs";
import { CloseIcon } from "./icons";
import { RunHistoryMenu } from "./RunHistoryMenu";
import { RunResultDetail } from "./RunResultDetail";

interface Row {
  item: string;
  name: string;
  method: string;
  outcome?: RunEventDto;
}

/** The result index of a finished row (events carry it; a request run twice has two). */
function resultIndex(r: Row): number | undefined {
  const o = r.outcome;
  return o && (o.type === "passed" || o.type === "failed" || o.type === "skipped") ? o.index : undefined;
}

function rows(events: RunEventDto[]): Row[] {
  const out: Row[] = [];
  for (const e of events) {
    if (e.type === "started") out.push({ item: e.item, name: e.name, method: e.method });
    else if (e.type === "passed" || e.type === "failed" || e.type === "skipped") {
      const row = [...out].reverse().find((r) => r.item === e.item && !r.outcome);
      if (row) row.outcome = e;
      else out.push({ item: e.item, name: e.item, method: "", outcome: e });
    }
  }
  return out;
}

/** "3/4" for a finished row (asserts count as tests, like Bruno's runner). */
function testCount(e: Extract<RunEventDto, { type: "passed" | "failed" }>): string {
  const all = [...e.asserts.map((a) => a.passed), ...e.tests.map((t) => t.passed)];
  return all.length ? `${all.filter(Boolean).length}/${all.length}` : "";
}

function checks(r: RunReportDto): string {
  const parts = [];
  const tests = r.testsPassed + r.testsFailed;
  const asserts = r.assertsPassed + r.assertsFailed;
  if (tests) parts.push(`tests ${r.testsPassed}/${tests}`);
  if (asserts) parts.push(`asserts ${r.assertsPassed}/${asserts}`);
  return parts.join(", ");
}

const RESULT_TEXT = { passed: "✓ Passed", failed: "✗ Failed", skipped: "– Skipped" } as const;

/**
 * The shown collection run (the newest, or one opened from History) with live rows, a
 * Cancel button, and the detail of the selected row.
 */
export function RunPanel({ onClose, collectionId }: { onClose: () => void; collectionId?: string | null }) {
  const state = useRuns();
  const { cancel, select, closeHistory } = state;
  const run = shownRun(state);
  const historyFor = run?.collectionId ?? collectionId ?? null;
  if (!run) {
    return (
      <section className="run-panel" aria-label="Run">
        <header>
          <strong>Runs</strong>
          <span className="muted">No run in this session</span>
          <span className="spacer" />
          {historyFor && <RunHistoryMenu collectionId={historyFor} />}
          <button className="icon" title="Close run panel" aria-label="Close run panel" onClick={onClose}>
            <CloseIcon />
          </button>
        </header>
      </section>
    );
  }
  const saved = run.source === "history";
  return (
    <section className="run-panel" aria-label="Run">
      <header>
        <strong>Run: {run.label}</strong>
        <span className="muted">{saved ? `saved · ${run.status}` : run.status}</span>
        {run.report && (
          <span>
            {run.report.passed} passed, {run.report.failed} failed, {run.report.skipped} skipped
            {checks(run.report) && ` · ${checks(run.report)}`} ({formatDuration(run.report.elapsedMs)})
          </span>
        )}
        {run.error && <span className="err">{run.error}</span>}
        <span className="spacer" />
        {!saved && run.status === "running" && <button onClick={() => cancel(run.runId)}>Cancel</button>}
        {saved && <button onClick={closeHistory}>Back to latest run</button>}
        {historyFor && <RunHistoryMenu collectionId={historyFor} />}
        <button className="icon" title="Close run panel" aria-label="Close run panel" onClick={onClose}>
          <CloseIcon />
        </button>
      </header>
      <div className="run-body">
        <table aria-label="Run results">
          <thead>
            <tr>
              <th scope="col">Result</th>
              <th scope="col">Method</th>
              <th scope="col">Name</th>
              <th scope="col">Status</th>
              <th scope="col">Time · Size</th>
              <th scope="col">Checks</th>
              <th scope="col">Message</th>
            </tr>
          </thead>
          <tbody>
            {rows(run.events).map((r, i) => {
              const o = r.outcome;
              const index = resultIndex(r);
              const selectable = index !== undefined;
              const open = () => index !== undefined && select(run.runId, index);
              return (
                <tr
                  key={i}
                  className={selectable ? "selectable" : undefined}
                  tabIndex={selectable ? 0 : undefined}
                  aria-selected={selectable ? run.selected === index : undefined}
                  onClick={open}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" || e.key === " ") {
                      e.preventDefault();
                      open();
                    }
                  }}
                >
                  <td className={o?.type ?? "pending"}>
                    {o?.type === "passed" || o?.type === "failed" || o?.type === "skipped" ? RESULT_TEXT[o.type] : "… Running"}
                  </td>
                  <td className={`badge method-${r.method.toLowerCase()}`}>{r.method}</td>
                  <td>{r.name}</td>
                  <td className="num">{o?.type === "passed" ? o.status : o?.type === "failed" ? (o.status ?? "") : ""}</td>
                  <td className="num">{o?.type === "passed" ? `${formatDuration(o.elapsedMs)} · ${formatSize(o.size)}` : ""}</td>
                  <td className="num">{o && (o.type === "passed" || o.type === "failed") ? testCount(o) : ""}</td>
                  <td className={o?.type === "failed" ? "err message" : "muted message"}>
                    {o?.type === "failed" ? o.error : o?.type === "skipped" ? o.reason : ""}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
        <RunResultDetail runId={run.runId} />
      </div>
    </section>
  );
}
