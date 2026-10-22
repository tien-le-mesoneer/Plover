import { useState } from "react";
import type { TreeNodeDto } from "../api/types";
import { useTabs, tabKey } from "../store/tabs";
import { useWorkspace } from "../store/workspace";
import { OpenCollectionDialog } from "./OpenCollectionDialog";

interface Props {
  onRun?: (collectionId: string, item: string, label: string) => void;
}

export function Sidebar({ onRun }: Props) {
  const { collections, trees, reload } = useWorkspace();
  return (
    <aside className="sidebar">
      <OpenCollectionDialog />
      {collections.map((c) => {
        const tree = trees[c.id];
        return (
          <div key={c.id} className="collection">
            <div className="collection-header" title={c.root}>
              <strong>{c.name}</strong>
              <span className="spacer" />
              {onRun && (
                <button
                  className="icon"
                  title="Run collection"
                  aria-label="Run collection"
                  onClick={() => onRun(c.id, "", c.name)}>
                  ▶
                </button>
              )}
              <button
                className="icon"
                title="Reload from disk"
                aria-label="Reload from disk"
                onClick={() => void reload(c.id)}>
                ⟳
              </button>
            </div>
            {tree?.children?.map((node) => (
              <Node key={node.id} collectionId={c.id} node={node} depth={0} onRun={onRun} />
            ))}
          </div>
        );
      })}
    </aside>
  );
}

function Node({
  collectionId,
  node,
  depth,
  onRun,
}: {
  collectionId: string;
  node: TreeNodeDto;
  depth: number;
  onRun?: Props["onRun"];
}) {
  const [open, setOpen] = useState(false);
  const { open: openTab, active } = useTabs();
  const indent = { paddingLeft: `${8 + depth * 14}px` };

  if (node.kind === "folder") {
    return (
      <>
        <div
          className="node folder"
          style={indent}
          onClick={() => setOpen(!open)}
          onContextMenu={(e) => {
            if (!onRun) return;
            e.preventDefault();
            onRun(collectionId, node.id, node.name);
          }}
          title="Click to expand; right-click to run this folder"
        >
          <span className="caret">{open ? "▾" : "▸"}</span> {node.name}
        </div>
        {open &&
          node.children?.map((child) => (
            <Node key={child.id} collectionId={collectionId} node={child} depth={depth + 1} onRun={onRun} />
          ))}
      </>
    );
  }
  if (node.kind === "invalid") {
    return (
      <div className="node invalid" style={indent} title={node.error ?? "invalid file"}>
        ⚠ {node.name}
      </div>
    );
  }
  const selected = active === tabKey(collectionId, node.id);
  return (
    <div
      className={`node request ${selected ? "selected" : ""}`}
      style={indent}
      onClick={() => void openTab(collectionId, node.id)}
    >
      <span className={`badge method-${(node.method ?? "").toLowerCase()}`}>{node.method}</span> {node.name}
    </div>
  );
}
