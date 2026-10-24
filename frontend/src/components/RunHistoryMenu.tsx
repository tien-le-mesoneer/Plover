import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { formatDuration } from "../lib/format";
import { useRuns } from "../store/runs";
import { Modal } from "./Modal";

function when(ms: number): string {
  return new Date(ms).toLocaleString();
}

/** "History": the collection's saved runs (app and `plv`); choosing one opens it read-only. */
export function RunHistoryMenu({ collectionId }: { collectionId: string }) {
  const { history, loadHistory, openHistory, clearHistory } = useRuns();
  const [open, setOpen] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const [error, setError] = useState<string>();
  /** Fixed above the button, so the scrolling run panel does not clip it. */
  const [place, setPlace] = useState<{ right: number; bottom: number }>({ right: 8, bottom: 48 });
  const button = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const runs = history[collectionId] ?? [];

  useEffect(() => {
    if (!open) return;
    void loadHistory(collectionId);
    const opener = button.current;
    const onDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (panel.current?.contains(target) || opener?.contains(target)) return;
      setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    return () => {
      document.removeEventListener("mousedown", onDown);
      opener?.focus();
    };
  }, [open, collectionId, loadHistory]);

  useEffect(() => {
    if (!open) return;
    (panel.current?.querySelector<HTMLElement>("button:not([disabled])") ?? panel.current)?.focus();
  }, [open, runs.length]);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      setOpen(false);
    }
  };
  const choose = async (recordId: string) => {
    setOpen(false);
    try {
      await openHistory(collectionId, recordId);
      setError(undefined);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <span className="history-menu">
      <button
        ref={button}
        type="button"
        aria-haspopup="dialog"
        aria-expanded={open}
        onClick={() => {
          const rect = button.current?.getBoundingClientRect();
          if (rect) setPlace({ right: Math.max(8, window.innerWidth - rect.right), bottom: window.innerHeight - rect.top + 4 });
          setOpen((o) => !o);
        }}
      >
        History
      </button>
      {error && <span className="err">{error}</span>}
      {open && (
        <div
          ref={panel}
          className="popover history-popover"
          role="dialog"
          aria-label="Run history"
          tabIndex={-1}
          style={place}
          onKeyDown={onKeyDown}
        >
          {runs.length === 0 ? (
            <p className="muted">No saved runs yet.</p>
          ) : (
            <ul className="recent-list">
              {runs.map((r) => (
                <li key={r.recordId}>
                  <button type="button" className="recent-item" onClick={() => void choose(r.recordId)}>
                    <span className="recent-name">{when(r.startedAtMs)}</span>
                    <span className="recent-tag">{r.source}</span>
                    <span className="recent-path">
                      {r.target === "." ? "collection" : r.target}
                      {r.env ? ` · ${r.env}` : ""} · {r.report.passed} passed, {r.report.failed} failed, {r.report.skipped} skipped ·{" "}
                      {formatDuration(r.report.elapsedMs)}
                      {r.outcome === "cancelled" ? " · cancelled" : ""}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
          <button type="button" disabled={runs.length === 0} onClick={() => setConfirm(true)}>
            Clear history
          </button>
        </div>
      )}
      {confirm && (
        <Modal title="Clear run history" onDismiss={() => setConfirm(false)}>
          <h3>Delete the saved runs of this collection?</h3>
          <p>Runs saved by the app and by plv are deleted. This cannot be undone.</p>
          <div className="actions">
            <button onClick={() => setConfirm(false)}>Cancel</button>
            <button
              className="primary"
              onClick={() => {
                setConfirm(false);
                setOpen(false);
                void clearHistory(collectionId);
              }}
            >
              Clear history
            </button>
          </div>
        </Modal>
      )}
    </span>
  );
}
