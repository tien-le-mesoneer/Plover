import { useId, useState, type FormEvent } from "react";
import type { TreeNodeDto } from "../api/types";
import { useTabs } from "../store/tabs";
import { useWorkspace } from "../store/workspace";
import { Modal } from "./Modal";

/** Folder IDs of a tree, depth first (the root is `""`). */
function folders(node: TreeNodeDto | undefined): string[] {
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

/** `POST orders` from `POST {{host}}/api/orders?page=2`: the method and the last path segment. */
export function suggestedName(method: string, url: string): string {
  const path = (url.split(/[?#]/)[0] ?? "").replace(/^[a-z][a-z0-9+.-]*:\/\/[^/]*/i, "");
  const last = path
    .split("/")
    .filter((s) => s && !s.includes("{{"))
    .pop();
  if (!last) return `${method} request`;
  try {
    return `${method} ${decodeURIComponent(last)}`;
  } catch {
    return `${method} ${last}`;
  }
}

/** Saves a scratch tab as a new request file: pick a folder (root allowed) and a name. */
export function SaveRequestDialog({ tabKey, onClose }: { tabKey: string; onClose: () => void }) {
  const tab = useTabs((s) => s.tabs.find((t) => t.key === tabKey));
  const saveScratch = useTabs((s) => s.saveScratch);
  const collectionId = tab?.collectionId ?? "";
  const collection = useWorkspace((s) => s.collections.find((c) => c.id === collectionId));
  const tree = useWorkspace((s) => s.trees[collectionId]);
  const [folder, setFolder] = useState("");
  const [name, setName] = useState(() => (tab ? suggestedName(tab.draft.method, tab.draft.url) : ""));
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const errorId = useId();
  if (!tab) return null;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!name.trim()) {
      setError("Give the request a name");
      return;
    }
    setBusy(true);
    try {
      const result = await saveScratch(tabKey, folder, name.trim());
      if (result === "exists") setError("A request with this name already exists");
      else onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal title="Save request" onDismiss={onClose}>
      <form className="save-request" onSubmit={(e) => void submit(e)}>
        <h3>Save request</h3>
        <label>
          <span>Folder</span>
          <select value={folder} onChange={(e) => setFolder(e.target.value)}>
            <option value="">{collection?.name ?? "Collection"} (root)</option>
            {folders(tree).map((id) => (
              <option key={id} value={id}>
                {id}
              </option>
            ))}
          </select>
        </label>
        <label>
          <span>Name</span>
          <input
            value={name}
            spellCheck={false}
            aria-invalid={error ? true : undefined}
            aria-describedby={error ? errorId : undefined}
            onChange={(e) => {
              setName(e.target.value);
              setError(null);
            }}
          />
        </label>
        {error && (
          <p id={errorId} className="field-error" role="alert">
            {error}
          </p>
        )}
        <div className="actions">
          <button type="button" onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className="primary" disabled={busy}>
            Save request
          </button>
        </div>
      </form>
    </Modal>
  );
}
