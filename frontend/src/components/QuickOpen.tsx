import { useId, useMemo, useState, type KeyboardEvent } from "react";
import type { TreeNodeDto } from "../api/types";
import { rank, type QuickItem } from "../lib/fuzzy";
import { tabKey, useTabs } from "../store/tabs";
import { useWorkspace } from "../store/workspace";
import { Modal } from "./Modal";

interface Props {
  open: boolean;
  onClose: () => void;
}

/** Every request of the open collections, in tree order. */
function quickItems(trees: Record<string, TreeNodeDto>, names: Record<string, string>): QuickItem[] {
  const out: QuickItem[] = [];
  for (const [collectionId, root] of Object.entries(trees)) {
    const walk = (node: TreeNodeDto, path: string[]) => {
      for (const child of node.children ?? []) {
        if (child.kind === "folder") walk(child, [...path, child.name]);
        else if (child.kind === "request")
          out.push({
            collectionId,
            item: child.id,
            method: child.method ?? "",
            name: child.name,
            path: path.join("/"),
            collection: names[collectionId] ?? root.name,
          });
      }
    };
    walk(root, []);
  }
  return out;
}

/** Cmd/Ctrl+K: find a request by name, folder or collection and open it. */
export function QuickOpen({ open, onClose }: Props) {
  const trees = useWorkspace((s) => s.trees);
  const collections = useWorkspace((s) => s.collections);
  const recent = useTabs((s) => s.recent);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const listId = useId();

  const items = useMemo(
    () => quickItems(trees, Object.fromEntries(collections.map((c) => [c.id, c.name]))),
    [trees, collections],
  );
  const results = useMemo(() => {
    if (query.trim()) return rank(query, items);
    const byKey = new Map(items.map((i) => [tabKey(i.collectionId, i.item), i]));
    return recent.flatMap((k) => byKey.get(k) ?? []).slice(0, 50);
  }, [query, items, recent]);

  if (!open) return null;
  const selected = Math.min(active, Math.max(0, results.length - 1));
  const optionId = (i: number) => `${listId}-${i}`;
  const choose = (item: QuickItem | undefined) => {
    if (!item) return;
    onClose();
    void useTabs.getState().open(item.collectionId, item.item);
  };
  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive(Math.min(selected + 1, results.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive(Math.max(selected - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      choose(results[selected]);
    }
  };

  return (
    <Modal title="Quick open" onDismiss={onClose}>
      <input
        className="quick-input"
        role="combobox"
        aria-label="Find a request"
        aria-expanded={results.length > 0}
        aria-controls={listId}
        aria-autocomplete="list"
        aria-activedescendant={results.length > 0 ? optionId(selected) : undefined}
        placeholder="Request name, folder or collection"
        spellCheck={false}
        value={query}
        onChange={(e) => {
          setQuery(e.target.value);
          setActive(0);
        }}
        onKeyDown={onKeyDown}
      />
      <ul className="quick-list" role="listbox" id={listId} aria-label="Requests">
        {results.map((item, i) => (
          <li
            key={`${item.collectionId}:${item.item}`}
            id={optionId(i)}
            role="option"
            aria-selected={i === selected}
            className={i === selected ? "selected" : ""}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => choose(item)}
          >
            <span className={`badge method-${item.method.toLowerCase()}`}>{item.method}</span>
            <span className="quick-name">{item.name}</span>
            <span className="quick-where muted">
              {item.path ? `${item.path} · ` : ""}
              {item.collection}
            </span>
          </li>
        ))}
      </ul>
      {results.length === 0 && (
        <p className="muted">{query.trim() ? "No request matches." : "Requests you open show here. Type to search."}</p>
      )}
    </Modal>
  );
}
