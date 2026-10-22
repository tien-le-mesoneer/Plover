import type { RunEventDto } from "../api/types";
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
            {run.report.passed} passed, {run.report.failed} failed, {run.report.skipped} skipped (
            {formatDuration(run.report.elapsedMs)})
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
                <td>{o?.type === "passed" ? o.status : ""}</td>
                <td>{o?.type === "passed" ? `${formatDuration(o.elapsedMs)} · ${formatSize(o.size)}` : ""}</td>
                <td className="muted">{o?.type === "failed" ? o.error : o?.type === "skipped" ? o.reason : ""}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </section>
  );
}
