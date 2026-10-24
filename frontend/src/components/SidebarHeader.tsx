import { useCallback, useEffect, useRef, useState } from "react";
import { useTabs } from "../store/tabs";
import { useWorkspace } from "../store/workspace";
import { ImportIcon, LogoMark, NewRequestIcon, PlusIcon } from "./icons";
import { ImportHarDialog } from "./ImportHarDialog";
import { OpenCollectionPopover } from "./OpenCollectionPopover";

/** The collection a new request goes to: the active tab's, else the active collection. */
function targetCollection(): string | null {
  const { tabs, active } = useTabs.getState();
  const fromTab = tabs.find((t) => t.key === active)?.collectionId;
  const { collections, activeCollection } = useWorkspace.getState();
  const available = (id: string | null | undefined) =>
    id && collections.some((c) => c.id === id && c.available) ? id : null;
  return available(fromTab) ?? available(activeCollection) ?? collections.find((c) => c.available)?.id ?? null;
}

/**
 * "◆ Plover", "New request" (an unsaved scratch tab in the current collection) and the
 * "+" (Open collection, Cmd/Ctrl+O) above the sidebar.
 */
export function SidebarHeader() {
  // Subscribed so the button enables as soon as a collection is open.
  useWorkspace((s) => s.collections);
  useTabs((s) => s.active);
  const target = targetCollection();
  const [importing, setImporting] = useState<string | null>(null);
  const pickerOpen = useWorkspace((s) => s.pickerOpen);
  const setPickerOpen = useWorkspace((s) => s.setPickerOpen);
  const plus = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setPickerOpen(false), [setPickerOpen]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "o") {
        e.preventDefault();
        setPickerOpen(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [setPickerOpen]);

  return (
    <div className="brand">
      <span className="logo">
        <LogoMark /> Plover
      </span>
      <span className="spacer" />
      <button
        type="button"
        className="icon"
        title={target ? "New request" : "Open a collection first"}
        aria-label="New request"
        disabled={!target}
        onClick={() => target && useTabs.getState().newScratch(target)}
      >
        <NewRequestIcon />
      </button>
      <button
        type="button"
        className="icon"
        title={target ? "Import HAR…" : "Open a collection first"}
        aria-label="Import HAR"
        aria-haspopup="dialog"
        disabled={!target}
        onClick={() => setImporting(target)}
      >
        <ImportIcon />
      </button>
      <button
        ref={plus}
        type="button"
        className="icon"
        title="Open collection (⌘/Ctrl+O)"
        aria-label="Open collection"
        aria-haspopup="dialog"
        aria-expanded={pickerOpen}
        aria-keyshortcuts="Meta+O Control+O"
        onClick={() => setPickerOpen(!pickerOpen)}
      >
        <PlusIcon />
      </button>
      {pickerOpen && <OpenCollectionPopover anchor={plus} onClose={close} />}
      {importing && <ImportHarDialog collectionId={importing} onClose={() => setImporting(null)} />}
    </div>
  );
}
