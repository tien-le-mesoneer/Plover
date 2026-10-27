import { useEffect, useRef, type KeyboardEvent } from "react";
import { useHistory } from "../../store/history";
import { HISTORY_BANNER, useTabs } from "../../store/tabs";
import { useWorkspace } from "../../store/workspace";
import { formatDuration } from "../../lib/format";
import { OutcomeDetail } from "../RunResultDetail";

const time = (ms: number) =>
  new Date(ms).toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", second: "2-digit" });

/**
 * Past UI sends (masked by the server), newest first: a collection filter and Clear on
 * top, the list on the left, the selected send read only on the right with Open request
 * and Open as new request.
 */
export function HistoryTab() {
  const { list, filter, selected, record, error, load, setFilter, select, clear } = useHistory();
  const collections = useWorkspace((s) => s.collections);
  const listRef = useRef<HTMLUListElement>(null);

  useEffect(() => {
    void load();
  }, [load]);

  // Collections to filter by: open ones and any the list names.
  const names = new Map<string, string>();
  for (const c of collections) names.set(c.id, c.name);
  for (const m of list) if (!names.has(m.collectionId)) names.set(m.collectionId, m.collectionName);
  const open = record ? collections.find((c) => c.id === record.meta.collectionId && c.available) : undefined;
  const openReason = !record
    ? ""
    : record.meta.item === null
      ? "A scratch request has no file"
      : !open
        ? "Its collection is not open"
        : "";

  const onClear = async () => {
    const scope = filter ? (names.get(filter) ?? "this collection") : "all collections";
    const n = list.length;
    if (!window.confirm(`Clear ${n} send${n === 1 ? "" : "s"} of ${scope} from history?`)) return;
    await clear();
  };

  const onKeyDown = (e: KeyboardEvent<HTMLLIElement>, index: number) => {
    const options = listRef.current?.querySelectorAll<HTMLElement>('[role="option"]');
    const move = (i: number) => {
      e.preventDefault();
      options?.[i]?.focus();
    };
    if (e.key === "ArrowDown") move(Math.min(index + 1, list.length - 1));
    else if (e.key === "ArrowUp") move(Math.max(index - 1, 0));
    else if (e.key === "Home") move(0);
    else if (e.key === "End") move(list.length - 1);
    else if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      void select(list[index]!.id);
    }
  };

  return (
    <div className="history-tab">
      <header className="history-head">
        <strong>History</strong>
        <label className="history-filter">
          <span className="sr-only">Collection</span>
          <select aria-label="Collection" value={filter ?? ""} onChange={(e) => void setFilter(e.target.value || null)}>
            <option value="">All collections</option>
            {[...names].map(([id, name]) => (
              <option key={id} value={id}>
                {name}
              </option>
            ))}
          </select>
        </label>
        <span className="muted">Last 100 sends, secrets masked</span>
        <span className="spacer" />
        <button type="button" aria-label="Clear history" disabled={list.length === 0} onClick={() => void onClear()}>
          Clear
        </button>
      </header>
      {error && <p className="err">{error}</p>}
      <div className="history-body">
        <ul ref={listRef} className="history-list" role="listbox" aria-label="Past sends">
          {list.length === 0 && <li className="muted history-empty">No sends yet.</li>}
          {list.map((m, i) => (
            <li
              key={m.id}
              role="option"
              aria-selected={m.id === selected}
              tabIndex={m.id === selected || (!selected && i === 0) ? 0 : -1}
              className={`history-row ${m.id === selected ? "selected" : ""}`}
              onClick={() => void select(m.id)}
              onKeyDown={(e) => onKeyDown(e, i)}
            >
              <span className={`badge method-${m.method.toLowerCase()}`}>{m.method}</span>
              <span className="history-name">{m.name}</span>
              <span className={`result-${m.status}`}>{m.httpStatus ?? m.status}</span>
              <span className="history-meta muted">
                {m.collectionName} · {time(m.sentAtMs)} · {formatDuration(m.durationMs)}
              </span>
            </li>
          ))}
        </ul>
        <section className="history-detail" aria-label="Send detail">
          {!selected && <p className="muted center">Select a send to see its request and response.</p>}
          {selected && !record && !error && (
            <p className="muted center" aria-busy="true">
              Loading…
            </p>
          )}
          {record && (
            <>
              <div className="history-actions">
                <span className="history-url mono" title={record.meta.url}>
                  {record.meta.url}
                </span>
                <span className="spacer" />
                <button
                  type="button"
                  aria-label="Open request"
                  title={openReason || "Open the request file in a tab"}
                  disabled={openReason !== ""}
                  onClick={() =>
                    void useTabs
                      .getState()
                      .open(record.meta.collectionId, record.meta.item!)
                      .catch(() => undefined)
                  }
                >
                  Open request
                </button>
                <button
                  type="button"
                  aria-label="Open as new request"
                  title={open ? "A new unsaved request with this send's request" : "Its collection is not open"}
                  disabled={!open || !record.detail.request}
                  onClick={() =>
                    record.detail.request &&
                    useTabs.getState().openScratchFrom(record.meta.collectionId, record.detail.request, HISTORY_BANNER)
                  }
                >
                  Open as new request
                </button>
              </div>
              <OutcomeDetail data={record.detail} collectionId={record.meta.collectionId} />
            </>
          )}
        </section>
      </div>
    </div>
  );
}
