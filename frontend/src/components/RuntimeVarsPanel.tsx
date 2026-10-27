import { useCallback, useEffect, useRef, useState, type KeyboardEvent } from "react";
import { api } from "../api/instance";
import type { VarDto } from "../api/types";
import { useRuns } from "../store/runs";
import { useTabs } from "../store/tabs";
import { useVars } from "../store/vars";
import { useWorkspace } from "../store/workspace";
import { CopyIcon, TrashIcon, VariableIcon } from "./icons";
import { Modal } from "./Modal";

/**
 * The top bar's `{x}` button: the collection's runtime variables (set by scripts and Save
 * as variable), with masked values shown as `••••`. A row can be removed, and Clear all
 * asks first. There is no reveal.
 */
export function RuntimeVarsPanel({ collectionId }: { collectionId: string | null }) {
  const env = useWorkspace((s) => (collectionId ? (s.activeEnv[collectionId] ?? null) : null));
  const [open, setOpen] = useState(false);
  const [vars, setVars] = useState<VarDto[] | null>(null);
  const [confirm, setConfirm] = useState(false);
  const [error, setError] = useState<string>();
  /** Fixed below the button, so the top bar does not clip it. */
  const [place, setPlace] = useState<{ right: number; top: number }>({ right: 8, top: 48 });
  const button = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);

  const load = useCallback(async () => {
    if (!collectionId) return;
    try {
      setVars((await api().runtimeVars(collectionId, env)).vars);
      setError(undefined);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [collectionId, env]);

  useEffect(() => {
    if (!open) return;
    // A send or run of this collection may have changed its runtime variables.
    const unTabs = useTabs.subscribe((s, prev) => {
      const was = new Map(prev.tabs.map((t) => [t.key, t.sending]));
      if (s.tabs.some((t) => t.collectionId === collectionId && was.get(t.key) && !t.sending)) void load();
    });
    const unRuns = useRuns.subscribe((s, prev) => {
      const ended = Object.entries(s.runs).some(
        ([id, r]) => r.collectionId === collectionId && r.status !== "running" && prev.runs[id]?.status === "running",
      );
      if (ended) void load();
    });
    const opener = button.current;
    const onDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (panel.current?.contains(target) || opener?.contains(target)) return;
      if ((target as Element).closest?.('[aria-modal="true"]')) return;
      setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    return () => {
      unTabs();
      unRuns();
      document.removeEventListener("mousedown", onDown);
    };
  }, [open, load, collectionId]);

  // Focus moves into the panel, so Esc and Tab work from the keyboard right away.
  useEffect(() => {
    if (open) panel.current?.focus();
  }, [open]);

  if (!collectionId) return null;

  const close = () => {
    setOpen(false);
    button.current?.focus();
  };
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      close();
    }
  };
  const changed = async (action: Promise<void>) => {
    try {
      await action;
      useVars.getState().refreshCollection(collectionId);
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };
  const count = vars?.length ?? 0;

  return (
    <span className="runtime-vars">
      <button
        ref={button}
        type="button"
        className="icon"
        aria-label="Runtime variables"
        title="Runtime variables"
        aria-haspopup="dialog"
        aria-expanded={open}
        onClick={() => {
          const rect = button.current?.getBoundingClientRect();
          if (rect) setPlace({ right: Math.max(8, window.innerWidth - rect.right), top: rect.bottom + 4 });
          if (!open) void load();
          setOpen((o) => !o);
        }}
      >
        <VariableIcon />
      </button>
      {open && (
        <div
          ref={panel}
          className="popover runtime-vars-panel"
          role="dialog"
          aria-label="Runtime variables"
          tabIndex={-1}
          style={{ right: place.right, top: place.top }}
          onKeyDown={onKeyDown}
        >
          <div className="popover-label">Runtime variables</div>
          {error && (
            <p role="alert" className="field-error">
              {error}
            </p>
          )}
          {vars && count === 0 && <p className="muted">No runtime variables. Scripts and Save as variable add them.</p>}
          {count > 0 && (
            <ul className="runtime-vars-list">
              {vars!.map((v) => (
                <li key={v.name}>
                  <span className="mono runtime-var-name" title={v.name}>
                    {v.name}
                  </span>
                  {v.masked ? (
                    <span className="mono muted" aria-label="masked">
                      ••••
                    </span>
                  ) : (
                    <span className="mono runtime-var-value" title={v.value ?? ""}>
                      {v.value}
                      {v.truncated ? "…" : ""}
                    </span>
                  )}
                  <button
                    type="button"
                    className="icon"
                    aria-label={`Copy name ${v.name}`}
                    title="Copy name"
                    onClick={() => void navigator.clipboard?.writeText(v.name)}
                  >
                    <CopyIcon />
                  </button>
                  <button
                    type="button"
                    className="icon"
                    aria-label={`Remove ${v.name}`}
                    title="Remove"
                    onClick={() => void changed(api().deleteRuntimeVar(collectionId, v.name))}
                  >
                    <TrashIcon />
                  </button>
                </li>
              ))}
            </ul>
          )}
          <div className="actions">
            <button type="button" disabled={count === 0} onClick={() => setConfirm(true)}>
              Clear all
            </button>
          </div>
        </div>
      )}
      {confirm && (
        <Modal title="Clear runtime variables" onDismiss={() => setConfirm(false)}>
          <h3>
            Clear {count} runtime variable{count === 1 ? "" : "s"}?
          </h3>
          <p>
            {count === 1
              ? "Requests that use it see it as undefined until a script or Save as variable sets it again."
              : "Requests that use them see them as undefined until a script or Save as variable sets them again."}
          </p>
          <div className="actions">
            <button type="button" onClick={() => setConfirm(false)}>
              Cancel
            </button>
            <button
              type="button"
              className="primary"
              onClick={() => {
                setConfirm(false);
                void changed(api().clearRuntimeVars(collectionId));
              }}
            >
              Clear
            </button>
          </div>
        </Modal>
      )}
    </span>
  );
}
