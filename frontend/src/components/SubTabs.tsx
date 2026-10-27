import type { KeyboardEvent, ReactNode } from "react";

export interface SubTab<T extends string> {
  id: T;
  label: ReactNode;
  /** Full accessible name, e.g. "Tests, 2 of 3 passed". */
  name: string;
}

/** A tablist with a roving tab stop; ←/→/Home/End move and select. */
export function SubTabs<T extends string>({
  tabs,
  value,
  onChange,
  label,
  end,
}: {
  tabs: SubTab<T>[];
  value: T;
  onChange: (id: T) => void;
  label: string;
  /** Controls after the tabs, outside the tablist (the split toggle). */
  end?: ReactNode;
}) {
  const onKeyDown = (e: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const last = tabs.length - 1;
    const next =
      e.key === "ArrowRight" ? (index === last ? 0 : index + 1)
      : e.key === "ArrowLeft" ? (index === 0 ? last : index - 1)
      : e.key === "Home" ? 0
      : e.key === "End" ? last
      : null;
    if (next === null) return;
    e.preventDefault();
    onChange(tabs[next]!.id);
    const buttons = e.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>('[role="tab"]');
    buttons?.[next]?.focus();
  };
  const list = (
    <div className="subtabs" role="tablist" aria-label={label}>
      {tabs.map((t, i) => (
        <button
          key={t.id}
          type="button"
          role="tab"
          aria-selected={t.id === value}
          aria-label={t.name}
          tabIndex={t.id === value ? 0 : -1}
          className={t.id === value ? "active" : ""}
          onClick={() => onChange(t.id)}
          onKeyDown={(e) => onKeyDown(e, i)}
        >
          {t.label}
        </button>
      ))}
    </div>
  );
  if (!end) return list;
  return (
    <div className="subtabs-bar">
      {list}
      <div className="subtabs-end">{end}</div>
    </div>
  );
}
