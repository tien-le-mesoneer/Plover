import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { menuEntries, menuKeyDown, type MenuItem } from "./Menu";

/**
 * A menu at a point (right-click, Shift+F10 or the ContextMenu key), clamped to the
 * viewport. The first item gets focus; arrow keys, Home and End move as in `Menu`; Esc,
 * Tab and a click outside close it. `onClose` runs on every close, and the caller puts
 * focus back; choosing an item closes the menu first.
 */
export function ContextMenu({
  at,
  items,
  label,
  onClose,
}: {
  at: { x: number; y: number };
  items: MenuItem[];
  label: string;
  onClose: () => void;
}) {
  const menu = useRef<HTMLDivElement>(null);
  const [place, setPlace] = useState(at);
  /** The latest `onClose`, so a new callback each render does not move focus again. */
  const closeRef = useRef(onClose);
  useEffect(() => {
    closeRef.current = onClose;
  }, [onClose]);

  useLayoutEffect(() => {
    const rect = menu.current?.getBoundingClientRect();
    if (!rect) return;
    const x = Math.max(4, Math.min(at.x, window.innerWidth - rect.width - 4));
    const y = Math.max(4, Math.min(at.y, window.innerHeight - rect.height - 4));
    if (x !== at.x || y !== at.y) setPlace({ x, y });
  }, [at]);

  useEffect(() => {
    menuEntries(menu.current)[0]?.focus();
    const onDown = (e: MouseEvent) => {
      if (!menu.current?.contains(e.target as Node)) closeRef.current();
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, []);

  return (
    <div
      ref={menu}
      className="menu context-menu"
      role="menu"
      aria-label={label}
      style={{ left: place.x, top: place.y }}
      onKeyDown={(e) => menuKeyDown(e, menu.current, onClose)}
      onContextMenu={(e) => e.preventDefault()}
    >
      {items.map((item) => (
        <button
          key={item.label}
          type="button"
          role="menuitem"
          tabIndex={-1}
          disabled={item.disabled}
          title={item.title}
          onClick={() => {
            onClose();
            item.onSelect();
          }}
        >
          <span>{item.label}</span>
          {item.hint && <kbd>{item.hint}</kbd>}
        </button>
      ))}
    </div>
  );
}
