import type { KeyboardEvent, ReactNode } from "react";
import { useRuns } from "../store/runs";
import { useTabs } from "../store/tabs";
import { ClockIcon, CloseIcon, PlayIcon } from "./icons";

/** Asks before discarding unsaved edits; returns whether the tab was closed. */
export function closeTabWithConfirm(key: string): boolean {
  const { tabs, runTabs, historyTab, isDirty, close } = useTabs.getState();
  if (runTabs.some((t) => t.key === key) || historyTab?.key === key) {
    close(key);
    return true;
  }
  const tab = tabs.find((t) => t.key === key);
  if (!tab) return false;
  const question =
    tab.item === null ? "Discard this request?" : `Discard unsaved changes to "${tab.draft.name}"?`;
  if (isDirty(key) && !window.confirm(question)) return false;
  close(key);
  return true;
}

interface Entry {
  key: string;
  name: string;
  title: string;
  content: ReactNode;
}

/**
 * Open request and run tabs. One tab stop (the active tab); ←/→/Home/End switch tabs,
 * Delete closes the focused one.
 */
export function RequestTabs() {
  const { tabs, runTabs, historyTab, active, activate, isDirty } = useTabs();
  const runs = useRuns((s) => s.runs);
  const entries: Entry[] = [
    ...tabs.map((t) => {
      const dirty = isDirty(t.key);
      return {
        key: t.key,
        name: `${t.draft.method} ${t.draft.name}${dirty ? ", unsaved changes" : ""}`,
        title: t.item ?? "Unsaved request",
        content: (
          <>
            <span className={`badge method-${t.draft.method.toLowerCase()}`}>{t.draft.method}</span>
            <span className="tab-name">{t.draft.name}</span>
            {dirty && <span className="dirty" role="img" aria-label="Unsaved changes" title="Unsaved changes" />}
          </>
        ),
      };
    }),
    ...runTabs.map((t) => {
      const run = t.runId ? runs[t.runId] : undefined;
      const failed = (run?.report?.failed ?? 0) > 0;
      const state = run?.status === "running" ? "running" : failed ? "failed" : run ? "done" : "";
      return {
        key: t.key,
        name: `Run ${t.label}${state ? `, ${state}` : ""}`,
        title: `Run · ${t.label}`,
        content: (
          <>
            <span className="tab-run-icon" aria-hidden="true">
              <PlayIcon />
            </span>
            <span className="tab-name">Run · {t.label}</span>
            {state === "running" && <span className="dirty" aria-hidden="true" />}
            {state === "failed" && (
              <span className="tab-failed" aria-hidden="true">
                ✕
              </span>
            )}
          </>
        ),
      };
    }),
    ...(historyTab
      ? [
          {
            key: historyTab.key,
            name: "History",
            title: "Past sends",
            content: (
              <>
                <span className="tab-run-icon" aria-hidden="true">
                  <ClockIcon />
                </span>
                <span className="tab-name">History</span>
              </>
            ),
          },
        ]
      : []),
  ];
  if (entries.length === 0) return null;

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>, index: number) => {
    const last = entries.length - 1;
    let next: number | null = null;
    if (e.key === "ArrowRight") next = index === last ? 0 : index + 1;
    else if (e.key === "ArrowLeft") next = index === 0 ? last : index - 1;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = last;
    else if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      closeTabWithConfirm(entries[index]!.key);
      return;
    }
    if (next === null) return;
    e.preventDefault();
    activate(entries[next]!.key);
    const strip = e.currentTarget.parentElement;
    strip?.querySelectorAll<HTMLElement>('[role="tab"]')[next]?.focus();
  };

  return (
    <div className="tabbar" role="tablist" aria-label="Open tabs">
      {entries.map((t, index) => {
        const selected = t.key === active;
        return (
          <div
            key={t.key}
            role="tab"
            aria-selected={selected}
            aria-label={t.name}
            aria-keyshortcuts="Delete"
            tabIndex={selected ? 0 : -1}
            className={`tab ${selected ? "active" : ""}`}
            title={t.title}
            onClick={() => activate(t.key)}
            onKeyDown={(e) => onKeyDown(e, index)}
            onAuxClick={(e) => e.button === 1 && closeTabWithConfirm(t.key)}
          >
            {t.content}
            <button
              className="icon"
              tabIndex={-1}
              title="Close (Delete when the tab is focused)"
              aria-label="Close"
              onClick={(e) => {
                e.stopPropagation();
                closeTabWithConfirm(t.key);
              }}
            >
              <CloseIcon />
            </button>
          </div>
        );
      })}
    </div>
  );
}
