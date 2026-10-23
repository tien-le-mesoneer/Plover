import { useWorkspace } from "../store/workspace";

/** Main-area guidance when no request is open. */
export function EmptyState({ onOpen }: { onOpen: () => void }) {
  const hasCollections = useWorkspace((s) => s.collections.length > 0);
  if (hasCollections) {
    return (
      <div className="empty-state">
        <h2>Pick a request in the sidebar</h2>
        <p className="muted">
          Use <kbd>↑</kbd>/<kbd>↓</kbd> to move through the tree and <kbd>Enter</kbd> to open a request.
          Then <kbd>⌘/Ctrl</kbd>+<kbd>Enter</kbd> sends it and <kbd>⌘/Ctrl</kbd>+<kbd>S</kbd> saves it.
        </p>
      </div>
    );
  }
  return (
    <div className="empty-state">
      <h2>Open a Bruno collection</h2>
      <p className="muted">
        A collection is a folder with a <code>bruno.json</code> file and <code>.bru</code> requests. Plover reads
        and writes those files directly, so your changes show up in Git.
      </p>
      <button type="button" className="primary" onClick={onOpen}>
        Open collection…
      </button>
    </div>
  );
}
