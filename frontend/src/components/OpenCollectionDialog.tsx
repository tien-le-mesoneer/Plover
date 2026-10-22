import { useState } from "react";
import { useWorkspace } from "../store/workspace";

/** Absolute path input; a native folder picker comes with the desktop app. */
export function OpenCollectionDialog() {
  const [path, setPath] = useState("");
  const open = useWorkspace((s) => s.open);
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
      <button type="submit">Open</button>
    </form>
  );
}
