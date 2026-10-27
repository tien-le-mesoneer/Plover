import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import type { CollectionSummaryDto, TreeNodeDto } from "../api/types";
import { useTabs, tabKey } from "../store/tabs";
import { useWorkspace } from "../store/workspace";
import { ContextMenu } from "./ContextMenu";
import { AlertIcon, ChevronIcon, CloseIcon, PlayIcon, RefreshIcon, SearchIcon } from "./icons";
import { ImportHarDialog } from "./ImportHarDialog";
import type { MenuItem } from "./Menu";
import { Modal } from "./Modal";
import { shortPath } from "./OpenCollectionPopover";

interface Props {
  onRun?: (collectionId: string, item: string, label: string) => void;
}

/** An open context menu: where, what, and the element that gets focus back. */
interface OpenMenu {
  at: { x: number; y: number };
  label: string;
  items: MenuItem[];
  returnTo: HTMLElement | null;
}

/** Opens a folder's or collection's menu (`folder` "" is the collection root). */
type OpenFolderMenu = (
  collectionId: string,
  folder: string,
  label: string,
  at: { x: number; y: number },
  returnTo: HTMLElement | null,
) => void;

/**
 * Keeps requests whose name (or "METHOD name") contains `query`, folders whose name
 * contains it (with everything inside), and the folders leading to any match.
 */
export function filterTree(node: TreeNodeDto, query: string): TreeNodeDto | null {
  const q = query.trim().toLowerCase();
  if (!q) return node;
  const hit = (n: TreeNodeDto) =>
    n.name.toLowerCase().includes(q) || `${n.method ?? ""} ${n.name}`.toLowerCase().includes(q);
  const children = (node.children ?? [])
    .map((child) => (child.kind === "folder" ? (hit(child) ? child : filterTree(child, q)) : hit(child) ? child : null))
    .filter((c): c is TreeNodeDto => c !== null);
  return children.length > 0 ? { ...node, children } : null;
}

function folderIds(node: TreeNodeDto): string[] {
  return (node.children ?? []).flatMap((c) => (c.kind === "folder" ? [c.id, ...folderIds(c)] : []));
}

export function Sidebar({ onRun }: Props) {
  const { collections, trees, reload, filter, setFilter, close, retry } = useWorkspace();
  const filterInput = useRef<HTMLInputElement>(null);
  const [confirmClose, setConfirmClose] = useState<CollectionSummaryDto | null>(null);
  const [menu, setMenu] = useState<OpenMenu | null>(null);
  const [importing, setImporting] = useState<{ collectionId: string; folder: string } | null>(null);

  const openMenu: OpenFolderMenu = (collectionId, folder, label, at, returnTo) => {
    const items: MenuItem[] = [];
    if (onRun) {
      items.push({
        label: folder ? "Run folder" : "Run collection",
        onSelect: () => onRun(collectionId, folder, label),
      });
    }
    items.push({ label: "Import HAR…", onSelect: () => setImporting({ collectionId, folder }) });
    setMenu({ at, label, items, returnTo });
  };
  const closeMenu = () => {
    menu?.returnTo?.focus();
    setMenu(null);
  };

  // Cmd/Ctrl+P: jump to the request filter (instead of the browser's print dialog).
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "p") {
        e.preventDefault();
        filterInput.current?.focus();
        filterInput.current?.select();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const closeCollection = (c: CollectionSummaryDto) => {
    const { tabs, isDirty } = useTabs.getState();
    if (tabs.some((t) => t.collectionId === c.id && isDirty(t.key))) setConfirmClose(c);
    else void doClose(c.id);
  };
  const doClose = async (id: string) => {
    setConfirmClose(null);
    const { tabs, close: closeTab } = useTabs.getState();
    tabs.filter((t) => t.collectionId === id).forEach((t) => closeTab(t.key));
    await close(id);
  };

  return (
    <aside className="sidebar" aria-label="Collections">
      {collections.length > 0 && (
        <div className="filter">
          <SearchIcon />
          <input
            ref={filterInput}
            type="search"
            aria-label="Filter requests"
            aria-keyshortcuts="Meta+P Control+P"
            placeholder="Filter requests  ⌘P"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape" && filter) {
                e.preventDefault();
                e.stopPropagation();
                setFilter("");
              }
            }}
          />
        </div>
      )}
      {collections.map((c) =>
        c.available ? (
          <div key={c.id} className="collection">
            <div
              className="collection-header"
              title={c.root}
              onContextMenu={(e) => {
                e.preventDefault();
                openMenu(c.id, "", c.name, { x: e.clientX, y: e.clientY }, null);
              }}
            >
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
              <button
                className="icon"
                title="Close collection (Plover reopens it next time unless closed)"
                aria-label={`Close collection ${c.name}`}
                onClick={() => closeCollection(c)}>
                <CloseIcon />
              </button>
            </div>
            {trees[c.id] && (
              <CollectionTree
                collectionId={c.id}
                name={c.name}
                root={trees[c.id]!}
                filter={filter}
                onRun={onRun}
                onMenu={openMenu}
              />
            )}
          </div>
        ) : (
          <div key={c.id} className="collection unavailable">
            <div className="collection-header" title={`${c.root}\n${c.error ?? "not available"}`}>
              <AlertIcon />
              <span className="collection-name">{c.name}</span>
              <span className="spacer" />
              <button type="button" className="text" aria-label={`Retry opening ${c.name}`} onClick={() => void retry(c.id)}>
                Retry
              </button>
              <button type="button" className="text" aria-label={`Remove ${c.name}`} onClick={() => void close(c.id)}>
                Remove
              </button>
            </div>
            <p className="unavailable-reason" title={c.error ?? undefined}>
              Not found at <span className="mono">{shortPath(c.root)}</span> — moved or deleted?
            </p>
          </div>
        ),
      )}
      {menu && <ContextMenu at={menu.at} items={menu.items} label={menu.label} onClose={closeMenu} />}
      {importing && (
        <ImportHarDialog
          collectionId={importing.collectionId}
          initialFolder={importing.folder}
          onClose={() => setImporting(null)}
        />
      )}
      {confirmClose && (
        <Modal title="Close collection" onDismiss={() => setConfirmClose(null)}>
          <h3>Close {confirmClose.name}?</h3>
          <p>Some of its requests have unsaved edits. Closing discards them.</p>
          <div className="actions">
            <button onClick={() => setConfirmClose(null)}>Cancel</button>
            <button className="primary" onClick={() => void doClose(confirmClose.id)}>
              Discard and close
            </button>
          </div>
        </Modal>
      )}
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
  filter,
  onRun,
  onMenu,
}: {
  collectionId: string;
  name: string;
  root: TreeNodeDto;
  filter: string;
  onRun?: Props["onRun"];
  onMenu: OpenFolderMenu;
}) {
  const saved = useWorkspace((s) => s.expanded[collectionId]);
  const toggleExpanded = useWorkspace((s) => s.toggleExpanded);
  const [focusId, setFocusId] = useState<string | null>(null);
  const { open: openTab, active } = useTabs();
  const refs = useRef(new Map<string, HTMLLIElement>());
  const filtering = filter.trim() !== "";
  const shown = useMemo(() => filterTree(root, filter), [root, filter]);
  // While filtering every folder on the way to a match is open; otherwise the remembered state.
  const expanded = useMemo(
    () => new Set(filtering ? (shown ? folderIds(shown) : []) : (saved ?? [])),
    [filtering, shown, saved],
  );
  const items = visibleItems(shown?.children ?? [], expanded);
  const current = items.some((i) => i.node.id === focusId) ? focusId : (items[0]?.node.id ?? null);

  const toggle = (id: string, open?: boolean) => {
    if (!filtering) toggleExpanded(collectionId, id, open);
  };
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
    if (isFolder && ((e.key === "F10" && e.shiftKey) || e.key === "ContextMenu")) {
      e.preventDefault();
      e.stopPropagation();
      const rect = e.currentTarget.getBoundingClientRect();
      onMenu(collectionId, node.id, node.name, { x: rect.left + 16, y: rect.bottom }, e.currentTarget);
      return;
    }
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

  if (filtering && items.length === 0) return <p className="muted no-matches">No matching requests</p>;
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
              aria-keyshortcuts={onRun ? "Control+Enter Meta+Enter Shift+F10" : "Shift+F10"}
              onClick={() => {
                focus(node.id);
                toggle(node.id);
              }}
              onContextMenu={(e) => {
                e.preventDefault();
                focus(node.id);
                onMenu(collectionId, node.id, node.name, { x: e.clientX, y: e.clientY }, e.currentTarget);
              }}
              aria-haspopup="menu"
              title={
                onRun
                  ? "Enter: expand · Ctrl/Cmd+Enter or ▶: run this folder · Shift+F10: more"
                  : "Enter: expand · Shift+F10: more"
              }
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
