import { useCallback, useEffect, useRef } from "react";
import { useWorkspace } from "../store/workspace";
import { LogoMark, PlusIcon } from "./icons";
import { OpenCollectionPopover } from "./OpenCollectionPopover";

/** "◆ Plover" and the "+" (Open collection, Cmd/Ctrl+O) above the sidebar. */
export function SidebarHeader() {
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
    </div>
  );
}
