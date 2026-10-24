import type { KeyboardEvent } from "react";
import { useTabs } from "../store/tabs";
import { CloseIcon } from "./icons";

/** Asks before discarding unsaved edits; returns whether the tab was closed. */
export function closeTabWithConfirm(key: string): boolean {
  const { tabs, isDirty, close } = useTabs.getState();
  const tab = tabs.find((t) => t.key === key);
  if (!tab) return false;
  const question =
    tab.item === null ? "Discard this request?" : `Discard unsaved changes to "${tab.draft.name}"?`;
  if (isDirty(key) && !window.confirm(question)) return false;
  close(key);
  return true;
}

/**
 * Open request tabs. One tab stop (the active tab); ←/→/Home/End switch tabs,
 * Delete closes the focused one.
 */
export function RequestTabs() {
  const { tabs, active, activate, isDirty } = useTabs();
  if (tabs.length === 0) return null;

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>, index: number) => {
    const last = tabs.length - 1;
    let next: number | null = null;
    if (e.key === "ArrowRight") next = index === last ? 0 : index + 1;
    else if (e.key === "ArrowLeft") next = index === 0 ? last : index - 1;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = last;
    else if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      closeTabWithConfirm(tabs[index]!.key);
      return;
    }
    if (next === null) return;
    e.preventDefault();
    activate(tabs[next]!.key);
    const strip = e.currentTarget.parentElement;
    strip?.querySelectorAll<HTMLElement>('[role="tab"]')[next]?.focus();
  };

  return (
    <div className="tabbar" role="tablist" aria-label="Open requests">
      {tabs.map((t, index) => {
        const dirty = isDirty(t.key);
        const selected = t.key === active;
        return (
          <div
            key={t.key}
            role="tab"
            aria-selected={selected}
            aria-label={`${t.draft.method} ${t.draft.name}${dirty ? ", unsaved changes" : ""}`}
            aria-keyshortcuts="Delete"
            tabIndex={selected ? 0 : -1}
            className={`tab ${selected ? "active" : ""}`}
            title={t.item ?? "Unsaved request"}
            onClick={() => activate(t.key)}
            onKeyDown={(e) => onKeyDown(e, index)}
            onAuxClick={(e) => e.button === 1 && closeTabWithConfirm(t.key)}
          >
            <span className={`badge method-${t.draft.method.toLowerCase()}`}>{t.draft.method}</span>
            <span className="tab-name">{t.draft.name}</span>
            {dirty && <span className="dirty" aria-hidden="true" title="Unsaved changes" />}
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
