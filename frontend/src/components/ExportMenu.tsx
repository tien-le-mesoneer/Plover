import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import type { ReportFormat } from "../api/client";
import { api } from "../api/instance";
import { useWorkspace } from "../store/workspace";

const FORMATS: { format: ReportFormat; label: string }[] = [
  { format: "html", label: "HTML" },
  { format: "json", label: "JSON" },
  { format: "junit", label: "JUnit" },
];

/** Saves `blob` as `fileName` through a temporary `<a download>`. */
function download(blob: Blob, fileName: string): void {
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = fileName;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 0);
}

/**
 * "Export": the run's report as HTML, JSON or JUnit — the same file
 * `plv runs show <id> --reporter-*` writes. Disabled while the run is going.
 */
export function ExportMenu({
  collectionId,
  recordId,
  disabled,
}: {
  collectionId: string;
  recordId?: string;
  disabled: boolean;
}) {
  const nativeSave = useWorkspace((s) => s.capabilities?.nativeSave ?? false);
  const [open, setOpen] = useState(false);
  const [note, setNote] = useState<{ error: boolean; text: string }>();
  /** Fixed below the button, so the scrolling run panel does not clip it. */
  const [place, setPlace] = useState<{ right: number; top: number }>({ right: 8, top: 48 });
  const button = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const off = disabled || !recordId;

  useEffect(() => {
    if (!open) return;
    menu.current?.querySelector<HTMLElement>("button")?.focus();
    const onDown = (e: MouseEvent) => {
      const t = e.target as Node;
      if (!menu.current?.contains(t) && !button.current?.contains(t)) setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open]);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      setOpen(false);
      button.current?.focus();
    }
  };

  const exportAs = async (format: ReportFormat) => {
    setOpen(false);
    if (!recordId) return;
    try {
      if (nativeSave) {
        const path = await api().saveReport(collectionId, recordId, format);
        setNote(path ? { error: false, text: `Saved ${path}` } : undefined);
      } else {
        const { blob, fileName } = await api().downloadReport(collectionId, recordId, format);
        download(blob, fileName);
        setNote(undefined);
      }
    } catch (e) {
      setNote({ error: true, text: e instanceof Error ? e.message : String(e) });
    }
  };

  return (
    <span className="export-menu">
      <button
        ref={button}
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        disabled={off}
        title={off ? "Export is available when the run has finished" : "Export the run's report"}
        onClick={() => {
          const rect = button.current?.getBoundingClientRect();
          if (rect) setPlace({ right: Math.max(8, window.innerWidth - rect.right), top: rect.bottom + 4 });
          setOpen((o) => !o);
        }}
      >
        Export
      </button>
      {open && (
        <div
          ref={menu}
          className="menu"
          role="menu"
          aria-label="Export report as"
          style={{ right: place.right, top: place.top }}
          onKeyDown={onKeyDown}
        >
          {FORMATS.map((f) => (
            <button key={f.format} type="button" role="menuitem" onClick={() => void exportAs(f.format)}>
              {f.label}
            </button>
          ))}
        </div>
      )}
      {note && (
        <span role={note.error ? "alert" : "status"} className={note.error ? "err" : "muted"}>
          {note.text}
        </span>
      )}
    </span>
  );
}
