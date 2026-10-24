import { useEffect, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import { api } from "../../api/instance";

const NAME = /^[A-Za-z_][\w.-]*$/;

/** The variable name a path suggests: its last key (`$.data.id` → `id`), or `value`. */
export function suggestedName(path: string): string {
  const m = /(?:\.([A-Za-z_][\w$]*)|\['([^']+)'\])(?:\[\d+\])*$/.exec(path);
  const key = m?.[1] ?? m?.[2] ?? "";
  return NAME.test(key) ? key : "value";
}

/** Copy value (strings without quotes), Copy path, or keep the value as a runtime variable. */
export function ValueMenu({
  at,
  path,
  value,
  collectionId,
  pathDisabled,
  onCopy,
  onSaved,
  onClose,
}: {
  at: { x: number; y: number };
  path: string;
  value: unknown;
  collectionId: string | null;
  /** Why Copy path is off (a JSONPath filter is active), if it is. */
  pathDisabled?: string;
  onCopy: (text: string) => void;
  onSaved?: (name: string) => void;
  onClose: () => void;
}) {
  const menu = useRef<HTMLDivElement>(null);
  const [naming, setNaming] = useState(false);
  const [name, setName] = useState(() => suggestedName(path));
  const [error, setError] = useState<string | null>(null);
  const valid = NAME.test(name);

  useEffect(() => {
    menu.current?.querySelector<HTMLElement>('[role="menuitem"]:not([disabled]), input')?.focus();
    const onDown = (e: MouseEvent) => {
      if (!menu.current?.contains(e.target as Node)) onClose();
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [onClose, naming]);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onClose();
      return;
    }
    if (naming || (e.key !== "ArrowDown" && e.key !== "ArrowUp")) return;
    const items = [...(menu.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]:not([disabled])') ?? [])];
    const i = items.indexOf(document.activeElement as HTMLButtonElement);
    e.preventDefault();
    items[(i + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length]?.focus();
  };

  const save = async (e: FormEvent) => {
    e.preventDefault();
    if (!valid || !collectionId) return;
    try {
      await api().setRuntimeVar(collectionId, name, value);
      onSaved?.(name);
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  return (
    <div
      ref={menu}
      className="value-menu"
      role="menu"
      aria-label={`Value at ${path}`}
      style={{ left: at.x, top: at.y + 4 }}
      onKeyDown={onKeyDown}
    >
      <div className="value-menu-path mono">{path}</div>
      {naming ? (
        <form onSubmit={(e) => void save(e)} className="value-menu-form">
          <label>
            <span>Runtime variable name</span>
            <input
              value={name}
              spellCheck={false}
              aria-invalid={!valid || undefined}
              aria-describedby="value-name-error"
              onChange={(e) => setName(e.target.value)}
            />
          </label>
          {(!valid || error) && (
            <p id="value-name-error" className="field-error">
              {error ?? "Start with a letter or _, then letters, digits, _ . -"}
            </p>
          )}
          <div className="actions">
            <button type="button" onClick={onClose}>
              Cancel
            </button>
            <button type="submit" className="primary" disabled={!valid || !collectionId}>
              Save variable
            </button>
          </div>
        </form>
      ) : (
        <>
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              onCopy(typeof value === "string" ? value : JSON.stringify(value, null, 2));
              onClose();
            }}
          >
            Copy value
          </button>
          <button
            type="button"
            role="menuitem"
            disabled={!!pathDisabled}
            title={pathDisabled}
            onClick={() => {
              onCopy(path);
              onClose();
            }}
          >
            Copy path
          </button>
          <button type="button" role="menuitem" disabled={!collectionId} onClick={() => setNaming(true)}>
            Save as runtime variable…
          </button>
        </>
      )}
    </div>
  );
}
