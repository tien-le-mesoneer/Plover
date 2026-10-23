import { api } from "../api/instance";
import { useWorkspace } from "../store/workspace";
import { FolderIcon } from "./icons";

/** Main-area guidance when no request is open. */
export function EmptyState() {
  const hasCollections = useWorkspace((s) => s.collections.some((c) => c.available));
  const nativePicker = useWorkspace((s) => s.capabilities?.nativePicker ?? false);
  const setPickerOpen = useWorkspace((s) => s.setPickerOpen);
  const open = useWorkspace((s) => s.open);
  if (hasCollections) {
    return (
      <div className="empty-state">
        <h2>Pick a request in the sidebar</h2>
        <p className="muted">
          Use <kbd>↑</kbd>/<kbd>↓</kbd> to move through the tree and <kbd>Enter</kbd> to open a request.
          Then <kbd>⌘/Ctrl</kbd>+<kbd>Enter</kbd> sends it and <kbd>⌘/Ctrl</kbd>+<kbd>S</kbd> saves it;{" "}
          <kbd>⌘/Ctrl</kbd>+<kbd>P</kbd> filters requests.
        </p>
      </div>
    );
  }
  const browse = async () => {
    const picked = await api().pickFolder();
    if (picked) await open(picked);
  };
  return (
    <div className="empty-state">
      <h2>Open a Bruno collection</h2>
      <p className="muted">
        A collection is a folder with a <code>bruno.json</code> file and <code>.bru</code> requests. Plover reads
        and writes those files directly, so your changes show up in Git, and reopens your collections next time.
      </p>
      <div className="actions">
        {nativePicker && (
          <button type="button" className="primary" onClick={() => void browse()}>
            <FolderIcon /> Browse for folder…
          </button>
        )}
        <button type="button" className={nativePicker ? undefined : "primary"} onClick={() => setPickerOpen(true)}>
          Open collection… <kbd>⌘/Ctrl</kbd>+<kbd>O</kbd>
        </button>
      </div>
    </div>
  );
}
