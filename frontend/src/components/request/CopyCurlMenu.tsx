import { useEffect, useRef, useState } from "react";
import { api } from "../../api/instance";
import { useTabs } from "../../store/tabs";
import { useWorkspace } from "../../store/workspace";
import { CopyIcon } from "../icons";

const NOTE_MS = 4000;

/**
 * "Copy as cURL" (as written, `{{variables}}` kept) with a menu for "With variables
 * filled in" (no scripts run; asks first when a secret value is in the command).
 */
export function CopyCurlMenu({ tabKey }: { tabKey: string }) {
  const [open, setOpen] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const menu = useRef<HTMLDivElement>(null);

  useEffect(() => () => clearTimeout(timer.current), []);
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!menu.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open]);

  const show = (text: string) => {
    setNote(text);
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setNote(null), NOTE_MS);
  };

  const copy = async (resolve: boolean) => {
    setOpen(false);
    const tab = useTabs.getState().tabs.find((t) => t.key === tabKey);
    if (!tab) return;
    const env = useWorkspace.getState().activeEnv[tab.collectionId] ?? null;
    try {
      const result = await api().copyCurl(tab.collectionId, {
        item: tab.item,
        draft: tab.draft,
        env,
        resolve,
      });
      if (result.containsSecret && !window.confirm("This command contains secret values. Copy anyway?")) return;
      await navigator.clipboard.writeText(result.text);
      show(result.omitted.length ? `Copied without ${result.omitted.join(", ")}` : "Copied");
    } catch (e) {
      show(`Could not copy: ${e instanceof Error ? e.message : String(e)}`);
    }
  };

  return (
    <div className="split-button" ref={menu}>
      <button type="button" title="Copy as cURL (variables as written)" onClick={() => void copy(false)}>
        <CopyIcon /> Copy as cURL
      </button>
      <button
        type="button"
        className="split-toggle"
        aria-label="More copy options"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
      >
        ▾
      </button>
      {open && (
        <div className="menu" role="menu">
          <button type="button" role="menuitem" onClick={() => void copy(false)}>
            As written
          </button>
          <button type="button" role="menuitem" onClick={() => void copy(true)}>
            With variables filled in
          </button>
        </div>
      )}
      <span className="copy-note" aria-live="polite">
        {note}
      </span>
    </div>
  );
}
