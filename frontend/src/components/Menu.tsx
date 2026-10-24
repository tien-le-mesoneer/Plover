import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";

function menuEntries(el: HTMLElement | null): HTMLButtonElement[] {
  return [...(el?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]:not([disabled])') ?? [])];
}

export interface MenuItem {
  label: string;
  onSelect: () => void;
  disabled?: boolean;
  /** Shown after the label, e.g. a shortcut. */
  hint?: string;
  title?: string;
}

/**
 * A button that opens a menu. Arrow keys, Home and End move between items; Esc or Tab
 * closes it and puts focus back on the button; choosing an item closes it first.
 */
export function Menu({
  label,
  children,
  items,
  className,
}: {
  /** The button's accessible name (and tooltip). */
  label: string;
  /** The button's content (an icon). */
  children: ReactNode;
  items: MenuItem[];
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);

  const entries = () => menuEntries(menu.current);

  useEffect(() => {
    if (!open) return;
    menuEntries(menu.current)[0]?.focus();
    const onDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (!menu.current?.contains(target) && !button.current?.contains(target)) setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open]);

  const close = () => {
    setOpen(false);
    button.current?.focus();
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const list = entries();
    const at = list.indexOf(document.activeElement as HTMLButtonElement);
    let next: number | null = null;
    if (e.key === "ArrowDown") next = (at + 1) % list.length;
    else if (e.key === "ArrowUp") next = (at - 1 + list.length) % list.length;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = list.length - 1;
    else if (e.key === "Escape" || e.key === "Tab") {
      e.preventDefault();
      e.stopPropagation();
      close();
      return;
    }
    if (next === null) return;
    e.preventDefault();
    list[next]?.focus();
  };

  return (
    <span className={`menu-button ${className ?? ""}`}>
      <button
        ref={button}
        type="button"
        className="icon"
        aria-label={label}
        title={label}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown" && !open) {
            e.preventDefault();
            setOpen(true);
          }
        }}
      >
        {children}
      </button>
      {open && (
        <div ref={menu} className="menu" role="menu" aria-label={label} onKeyDown={onKeyDown}>
          {items.map((item) => (
            <button
              key={item.label}
              type="button"
              role="menuitem"
              tabIndex={-1}
              disabled={item.disabled}
              title={item.title}
              onClick={() => {
                close();
                item.onSelect();
              }}
            >
              <span>{item.label}</span>
              {item.hint && <kbd>{item.hint}</kbd>}
            </button>
          ))}
        </div>
      )}
    </span>
  );
}
