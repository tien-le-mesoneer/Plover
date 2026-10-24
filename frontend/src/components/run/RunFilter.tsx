import type { KeyboardEvent } from "react";
import type { RunFilterValue } from "../../lib/runRows";

const ORDER: { value: RunFilterValue; label: string }[] = [
  { value: "all", label: "All" },
  { value: "fail", label: "Fail" },
  { value: "pass", label: "Pass" },
  { value: "skip", label: "Skip" },
];

/** `All N | Fail N | Pass N | Skip N`, like Bruno's runner; the counts live only here. */
export function RunFilter({
  counts,
  value,
  onChange,
}: {
  counts: Record<RunFilterValue, number>;
  value: RunFilterValue;
  onChange: (v: RunFilterValue) => void;
}) {
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
    const at = ORDER.findIndex((o) => o.value === value);
    const next = ORDER[(at + (e.key === "ArrowRight" ? 1 : ORDER.length - 1)) % ORDER.length]!;
    e.preventDefault();
    onChange(next.value);
    e.currentTarget.querySelector<HTMLButtonElement>(`[data-value="${next.value}"]`)?.focus();
  };
  return (
    <div className="segmented run-filter" role="radiogroup" aria-label="Show results" onKeyDown={onKeyDown}>
      {ORDER.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          data-value={o.value}
          aria-checked={value === o.value}
          aria-label={`${o.label} ${counts[o.value]}`}
          tabIndex={value === o.value ? 0 : -1}
          className={`filter-${o.value}`}
          onClick={() => onChange(o.value)}
        >
          {o.label} <span className="num">{counts[o.value]}</span>
        </button>
      ))}
    </div>
  );
}
