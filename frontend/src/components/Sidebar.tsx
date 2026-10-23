import { useRef, useState, type KeyboardEvent } from "react";
import type { TreeNodeDto } from "../api/types";
import { useTabs, tabKey } from "../store/tabs";
import { useWorkspace } from "../store/workspace";
import { AlertIcon, ChevronIcon, PlayIcon, RefreshIcon } from "./icons";
import { OpenCollectionDialog } from "./OpenCollectionDialog";

interface Props {
  onRun?: (collectionId: string, item: string, label: string) => void;
}

export function Sidebar({ onRun }: Props) {
  const { collections, trees, reload } = useWorkspace();
  return (
    <aside className="sidebar" aria-label="Collections">
      <OpenCollectionDialog />
      {collections.map((c) => (
        <div key={c.id} className="collection">
          <div className="collection-header" title={c.root}>
            <strong className="collection-name">{c.name}</strong>
            <span className="spacer" />
            {onRun && (
              <button
                className="icon"
                title="Run collection"
                aria-label="Run collection"
                onClick={() => onRun(c.id, "", c.name)}>
                <PlayIcon />
              </button>
            )}
            <button
              className="icon"
              title="Reload from disk"
              aria-label="Reload from disk"
              onClick={() => void reload(c.id)}>
              <RefreshIcon />
            </button>
          </div>
          {trees[c.id] && <CollectionTree collectionId={c.id} name={c.name} root={trees[c.id]!} onRun={onRun} />}
        </div>
      ))}
    </aside>
  );
}

interface Visible {
  node: TreeNodeDto;
  level: number;
  parent: string | null;
}

function visibleItems(nodes: TreeNodeDto[], expanded: Set<string>, level = 1, parent: string | null = null): Visible[] {
  return nodes.flatMap((node) => [
    { node, level, parent },
    ...(node.kind === "folder" && expanded.has(node.id)
      ? visibleItems(node.children ?? [], expanded, level + 1, node.id)
      : []),
  ]);
}

/**
 * One collection as an ARIA tree: a single tab stop (roving tabindex), ↑/↓ to move,
 * →/← to expand/collapse or go to child/parent, Enter to open a request or toggle a
 * folder, Ctrl/Cmd+Enter to run a folder.
 */
function CollectionTree({
  collectionId,
  name,
  root,
  onRun,
}: {
  collectionId: string;
  name: string;
  root: TreeNodeDto;
  onRun?: Props["onRun"];
}) {
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [focusId, setFocusId] = useState<string | null>(null);
  const { open: openTab, active } = useTabs();
  const refs = useRef(new Map<string, HTMLLIElement>());
  const items = visibleItems(root.children ?? [], expanded);
  const current = items.some((i) => i.node.id === focusId) ? focusId : (items[0]?.node.id ?? null);

  const toggle = (id: string, open?: boolean) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (open ?? !next.has(id)) next.add(id);
      else next.delete(id);
      return next;
    });
  const focus = (id: string | null | undefined) => {
    if (!id) return;
    setFocusId(id);
    refs.current.get(id)?.focus();
  };
  const run = (node: TreeNodeDto) => onRun?.(collectionId, node.id, node.name);
  const activate = (node: TreeNodeDto) => {
    if (node.kind === "folder") toggle(node.id);
    else if (node.kind === "request") void openTab(collectionId, node.id);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLLIElement>, index: number) => {
    const { node, parent } = items[index]!;
    const isFolder = node.kind === "folder";
    const isOpen = isFolder && expanded.has(node.id);
    switch (e.key) {
      case "ArrowDown":
        focus(items[index + 1]?.node.id);
        break;
      case "ArrowUp":
        focus(items[index - 1]?.node.id);
        break;
      case "Home":
        focus(items[0]?.node.id);
        break;
      case "End":
        focus(items[items.length - 1]?.node.id);
        break;
      case "ArrowRight":
        if (isFolder && !isOpen) toggle(node.id, true);
        else if (isOpen) focus(items[index + 1]?.node.id);
        break;
      case "ArrowLeft":
        if (isOpen) toggle(node.id, false);
        else focus(parent);
        break;
      case "Enter":
      case " ":
        if (e.metaKey || e.ctrlKey) {
          // On a folder: run it. On a request: leave it to the app-wide "send" shortcut.
          if (!isFolder) return;
          run(node);
        } else activate(node);
        break;
      default:
        return;
    }
    e.preventDefault();
    e.stopPropagation();
  };

  return (
    <ul className="tree" role="tree" aria-label={name}>
      {items.map((item, index) => {
        const { node, level } = item;
        const common = {
          ref: (el: HTMLLIElement | null) => {
            if (el) refs.current.set(node.id, el);
            else refs.current.delete(node.id);
          },
          role: "treeitem",
          "aria-level": level,
          tabIndex: node.id === current ? 0 : -1,
          style: { paddingLeft: `${4 + (level - 1) * 14}px` },
          onFocus: () => setFocusId(node.id),
          onKeyDown: (e: KeyboardEvent<HTMLLIElement>) => onKeyDown(e, index),
        } as const;
        if (node.kind === "folder") {
          const isOpen = expanded.has(node.id);
          return (
            <li
              key={node.id}
              {...common}
              className="node folder"
              aria-label={node.name}
              aria-expanded={isOpen}
              aria-keyshortcuts={onRun ? "Control+Enter Meta+Enter" : undefined}
              onClick={() => {
                focus(node.id);
                toggle(node.id);
              }}
              onContextMenu={(e) => {
                if (!onRun) return;
                e.preventDefault();
                run(node);
              }}
              title={onRun ? "Enter: expand · Ctrl/Cmd+Enter or ▶: run this folder" : undefined}
            >
              <span className="caret">
                <ChevronIcon open={isOpen} />
              </span>
              <span className="node-name">{node.name}</span>
              {onRun && (
                <button
                  type="button"
                  className="icon node-action"
                  tabIndex={-1}
                  title="Run folder"
                  aria-label={`Run folder ${node.name}`}
                  onClick={(e) => {
                    e.stopPropagation();
                    run(node);
                  }}
                >
                  <PlayIcon />
                </button>
              )}
            </li>
          );
        }
        if (node.kind === "invalid") {
          return (
            <li
              key={node.id}
              {...common}
              className="node invalid"
              aria-label={`${node.name} (invalid: ${node.error ?? "could not be parsed"})`}
              aria-disabled="true"
              title={node.error ?? "invalid file"}
            >
              <span className="caret">
                <AlertIcon />
              </span>
              <span className="node-name">{node.name}</span>
            </li>
          );
        }
        const selected = active === tabKey(collectionId, node.id);
        return (
          <li
            key={node.id}
            {...common}
            className={`node request ${selected ? "selected" : ""}`}
            aria-label={`${node.method ?? ""} ${node.name}`.trim()}
            aria-selected={selected}
            onClick={() => {
              focus(node.id);
              activate(node);
            }}
          >
            <span className={`badge method-${(node.method ?? "").toLowerCase()}`}>{node.method}</span>
            <span className="node-name">{node.name}</span>
          </li>
        );
      })}
    </ul>
  );
}
