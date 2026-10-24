import { useState } from "react";
import { formatDuration } from "../../lib/format";
import { buildEntries, filterCounts, type RunFilterValue } from "../../lib/runRows";
import { useRuns } from "../../store/runs";
import { useTabs, type RunTab as RunTabState } from "../../store/tabs";
import { useWorkspace } from "../../store/workspace";
import { ExportMenu } from "../ExportMenu";
import { RunHistoryMenu } from "../RunHistoryMenu";
import { RunResultDetail } from "../RunResultDetail";
import { RunWithDataDialog } from "../RunWithDataDialog";
import { RunFilter } from "./RunFilter";
import { RunList } from "./RunList";

function when(ms: number | undefined): string {
  return ms ? new Date(ms).toLocaleTimeString() : "";
}

/**
 * A run in its own tab: header (title, environment, duration; Previous runs, Export,
 * Re-run, Run with data…), the All/Fail/Pass/Skip filter with a name filter, then the
 * results and the selected result's detail side by side.
 */
export function RunTab({ tab }: { tab: RunTabState }) {
  const run = useRuns((s) => (tab.runId ? s.runs[tab.runId] : undefined));
  const { start, cancel, select } = useRuns();
  const retarget = useTabs((s) => s.retargetRun);
  const envName = useWorkspace((s) => s.activeEnv[tab.collectionId] ?? null);
  const [filter, setFilter] = useState<RunFilterValue>("all");
  const [name, setName] = useState("");
  const [withData, setWithData] = useState(false);
  const [startedAt] = useState(() => Date.now());

  const entries = run ? buildEntries(run.events) : [];
  const counts = filterCounts(entries.flatMap((e) => (e.kind === "row" ? [e] : [])));
  const saved = run?.source === "history";
  const item = run?.item ?? "";
  const rerun = () => {
    const runId = start(tab.collectionId, item, run?.env ?? envName, tab.label);
    retarget(tab.key, runId);
  };

  return (
    <section className="run-tab" aria-label={`Run ${tab.label}`}>
      <header className="run-header">
        <div className="run-title">
          <strong>Run · {run?.label ?? tab.label}</strong>
          <span className="muted num">
            {[
              run?.env ?? envName ?? "no environment",
              saved ? "saved run" : when(startedAt),
              run?.report ? formatDuration(run.report.elapsedMs) : run?.status === "running" ? "running…" : null,
            ]
              .filter(Boolean)
              .join(" · ")}
          </span>
          {run?.status === "cancelled" && <span className="muted">cancelled</span>}
          {run?.error && <span className="err">{run.error}</span>}
        </div>
        <span className="spacer" />
        <RunHistoryMenu collectionId={tab.collectionId} onChoose={(recordId) => retarget(tab.key, recordId)} />
        <ExportMenu collectionId={tab.collectionId} recordId={run?.recordId} disabled={!run || run.status === "running"} />
        {run?.status === "running" && !saved ? (
          <button type="button" onClick={() => cancel(run.runId)}>
            Cancel
          </button>
        ) : (
          <button type="button" onClick={rerun}>
            Re-run
          </button>
        )}
        <button type="button" disabled={run?.status === "running"} onClick={() => setWithData(true)}>
          Run with data…
        </button>
      </header>
      {run ? (
        <>
          <div className="run-filters">
            <RunFilter counts={counts} value={filter} onChange={setFilter} />
            <input
              type="search"
              className="run-name-filter"
              aria-label="Filter results by name"
              placeholder="Filter by name"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </div>
          <div className="run-body">
            <div className="run-list-pane">
              <RunList
                entries={entries}
                events={run.events}
                filter={filter}
                name={name}
                selected={run.selected}
                onSelect={(index) => select(run.runId, index)}
              />
            </div>
            <div className="run-detail-pane">
              {run.detail ? (
                <RunResultDetail runId={run.runId} />
              ) : (
                <p className="muted center">Pick a result to see its request and response.</p>
              )}
            </div>
          </div>
        </>
      ) : (
        <p className="muted center">No run in this tab yet. Re-run starts one, or pick a previous run.</p>
      )}
      {withData && (
        <RunWithDataDialog
          collectionId={tab.collectionId}
          item={item}
          label={tab.label}
          env={run?.env ?? envName}
          onStarted={(runId) => retarget(tab.key, runId)}
          onClose={() => setWithData(false)}
        />
      )}
    </section>
  );
}
