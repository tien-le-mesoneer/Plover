import { useState } from "react";
import { api } from "../api/instance";
import { useWorkspace } from "../store/workspace";

/** Absolute path input, plus a native folder picker in the desktop app. */
export function OpenCollectionDialog() {
  const [path, setPath] = useState("");
  const open = useWorkspace((s) => s.open);
  const nativePicker = useWorkspace((s) => s.capabilities?.nativePicker ?? false);
  const browse = async () => {
    const picked = await api().pickFolder();
    if (picked) await open(picked);
  };
  return (
    <form
      className="open-collection"
      onSubmit={(e) => {
        e.preventDefault();
        if (path.trim()) void open(path.trim()).then(() => setPath(""));
      }}
    >
      <input
        aria-label="Collection path"
        placeholder="/absolute/path/to/collection"
        value={path}
        onChange={(e) => setPath(e.target.value)}
      />
      {nativePicker && (
        <button type="button" onClick={() => void browse()}>
          Browse…
        </button>
      )}
      <button type="submit">Open</button>
    </form>
  );
}
