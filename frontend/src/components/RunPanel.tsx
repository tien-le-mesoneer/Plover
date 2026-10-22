import type { RunEventDto, RunReportDto } from "../api/types";
import { formatDuration, formatSize } from "../lib/format";
import { useRuns } from "../store/runs";

interface Row {
  item: string;
  name: string;
  method: string;
  outcome?: RunEventDto;
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

/** "tests 3/4" for a finished row (asserts count as tests, like Bruno's runner). */
function testCount(e: Extract<RunEventDto, { type: "passed" | "failed" }>): string {
  const all = [...e.asserts.map((a) => a.passed), ...e.tests.map((t) => t.passed)];
  return all.length ? `tests ${all.filter(Boolean).length}/${all.length}` : "";
}

function checks(r: RunReportDto): string {
  const parts = [];
  const tests = r.testsPassed + r.testsFailed;
  const asserts = r.assertsPassed + r.assertsFailed;
  if (tests) parts.push(`tests ${r.testsPassed}/${tests}`);
  if (asserts) parts.push(`asserts ${r.assertsPassed}/${asserts}`);
  return parts.join(", ");
}

/** The most recent collection run, with live rows and a Cancel button. */
export function RunPanel({ onClose }: { onClose: () => void }) {
  const { runs, order, cancel } = useRuns();
  const run = runs[order[order.length - 1] ?? ""];
  if (!run) return null;
  return (
    <section className="run-panel" aria-label="Run">
      <header>
        <strong>Run: {run.label}</strong>
        <span className="muted">{run.status}</span>
        {run.report && (
          <span>
            {run.report.passed} passed, {run.report.failed} failed, {run.report.skipped} skipped
            {checks(run.report) && ` · ${checks(run.report)}`} ({formatDuration(run.report.elapsedMs)})
          </span>
        )}
        {run.error && <span className="err">{run.error}</span>}
        <span className="spacer" />
        {run.status === "running" && <button onClick={() => cancel(run.runId)}>Cancel</button>}
        <button className="icon" title="Close run panel" aria-label="Close run panel" onClick={onClose}>
          ×
        </button>
      </header>
      <table>
        <tbody>
          {rows(run.events).map((r, i) => {
            const o = r.outcome;
            return (
              <tr key={i}>
                <td className={o?.type ?? "pending"}>
                  {o?.type === "passed" ? "✓" : o?.type === "failed" ? "✗" : o?.type === "skipped" ? "–" : "…"}
                </td>
                <td className={`badge method-${r.method.toLowerCase()}`}>{r.method}</td>
                <td>{r.name}</td>
                <td>{o?.type === "passed" ? o.status : o?.type === "failed" ? (o.status ?? "") : ""}</td>
                <td>{o?.type === "passed" ? `${formatDuration(o.elapsedMs)} · ${formatSize(o.size)}` : ""}</td>
                <td>{o && (o.type === "passed" || o.type === "failed") ? testCount(o) : ""}</td>
                <td className="muted">{o?.type === "failed" ? o.error : o?.type === "skipped" ? o.reason : ""}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </section>
  );
}
