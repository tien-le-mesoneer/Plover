import { useEffect, useRef, useState, type KeyboardEvent, type RefObject } from "react";
import { api } from "../api/instance";
import { useWorkspace } from "../store/workspace";
import { CloseIcon, FolderIcon } from "./icons";

/** `/Users/me/work/demo` → `~/work/demo`-style: keep the last two segments when long. */
export function shortPath(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  if (parts.length <= 3) return path;
  const sep = path.includes("\\") && !path.includes("/") ? "\\" : "/";
  return `…${sep}${parts.slice(-2).join(sep)}`;
}

interface Props {
  /** The "+" button: clicks on it don't count as "outside", and focus returns to it. */
  anchor: RefObject<HTMLButtonElement | null>;
  onClose: () => void;
}

/**
 * Open a collection: native folder picker (desktop), a pasted path, or a recent one.
 * A non-modal dialog: focus moves in, Escape or a click outside closes it, and focus
 * returns to the "+" button.
 */
export function OpenCollectionPopover({ anchor, onClose }: Props) {
  const { open, recent, removeRecent, capabilities } = useWorkspace();
  const nativePicker = capabilities?.nativePicker ?? false;
  const [path, setPath] = useState("");
  const panel = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const opener = anchor.current;
    panel.current?.querySelector<HTMLElement>("button, input")?.focus();
    const onDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (panel.current?.contains(target) || anchor.current?.contains(target)) return;
      onClose();
    };
    document.addEventListener("mousedown", onDown);
    return () => {
      document.removeEventListener("mousedown", onDown);
      opener?.focus();
    };
  }, [anchor, onClose]);

  const openPath = (p: string) => {
    onClose();
    void open(p);
  };
  const browse = async () => {
    const picked = await api().pickFolder();
    if (picked) openPath(picked);
  };
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onClose();
    }
  };

  return (
    <div ref={panel} className="popover open-popover" role="dialog" aria-label="Open collection" onKeyDown={onKeyDown}>
      {nativePicker && (
        <>
          <button type="button" className="primary wide" onClick={() => void browse()}>
            <FolderIcon /> Browse for folder…
          </button>
          <div className="popover-label">or paste a path</div>
        </>
      )}
      {!nativePicker && <div className="popover-label">Path to a folder with a bruno.json</div>}
      <form
        className="path-form"
        onSubmit={(e) => {
          e.preventDefault();
          if (path.trim()) openPath(path.trim());
        }}
      >
        <input
          aria-label="Collection path"
          placeholder="~/projects/my-api"
          spellCheck={false}
          value={path}
          onChange={(e) => setPath(e.target.value)}
        />
        <button type="submit" disabled={!path.trim()} title="Open">
          Open
        </button>
      </form>
      {recent.length > 0 && (
        <>
          <div className="popover-label">Recent</div>
          <ul className="recent-list">
            {recent.map((r) => (
              <li key={r.root}>
                <button
                  type="button"
                  className="recent-item"
                  title={r.path}
                  aria-label={`${r.name} ${r.path}${r.open ? " (open)" : ""}`}
                  onClick={() => openPath(r.path)}
                >
                  <span className="recent-name">{r.name}</span>
                  <span className="recent-path">{shortPath(r.path)}</span>
                  {r.open && <span className="recent-tag">open</span>}
                </button>
                <button
                  type="button"
                  className="icon"
                  title="Remove from recent"
                  aria-label={`Remove ${r.name} from recent`}
                  onClick={() => void removeRecent(r.root)}
                >
                  <CloseIcon />
                </button>
              </li>
            ))}
          </ul>
        </>
      )}
    </div>
  );
}
