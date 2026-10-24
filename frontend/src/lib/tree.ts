import type { TreeNodeDto } from "../api/types";

/** Folder IDs of a collection tree, depth first; the root (`""`) is not included. */
export function folderIds(node: TreeNodeDto | undefined): string[] {
  const out: string[] = [];
  const walk = (n: TreeNodeDto) => {
    for (const child of n.children ?? []) {
      if (child.kind !== "folder") continue;
      out.push(child.id);
      walk(child);
    }
  };
  if (node) walk(node);
  return out;
}
