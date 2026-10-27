import { useLayoutEffect, useRef } from "react";
import type { VarSuggestion } from "../../lib/varSuggest";

interface Props {
  id: string;
  optionId: (index: number) => string;
  items: VarSuggestion[];
  active: number;
  onPick: (item: VarSuggestion) => void;
  /** Where the open `{{` is on screen; read after render, the list sits under it. */
  anchor: () => { left: number; bottom: number };
}

/**
 * The variable names offered after `{{`: a listbox the field controls (the field keeps
 * focus and points at the active option). Names and scopes only, never values.
 */
export function VarSuggest({ id, optionId, items, active, onPick, anchor }: Props) {
  const list = useRef<HTMLUListElement>(null);
  useLayoutEffect(() => {
    const el = list.current;
    if (!el) return;
    const a = anchor();
    el.style.left = `${a.left}px`;
    el.style.top = `${a.bottom + 4}px`;
    el.querySelector<HTMLElement>('[aria-selected="true"]')?.scrollIntoView?.({ block: "nearest" });
  });
  return (
    <ul ref={list} id={id} role="listbox" aria-label="Variables" className="var-suggest">
      {items.map((item, i) => (
        <li
          key={item.name}
          id={optionId(i)}
          role="option"
          aria-selected={i === active}
          className={i === active ? "active" : ""}
          // mousedown, not click: the field must keep focus (blur would close the list).
          onMouseDown={(e) => {
            e.preventDefault();
            onPick(item);
          }}
        >
          <span className="var-suggest-name">{item.name}</span>
          <span className="var-suggest-scope">{item.label}</span>
        </li>
      ))}
    </ul>
  );
}
