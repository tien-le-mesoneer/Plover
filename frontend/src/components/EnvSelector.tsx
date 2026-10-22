import { useState } from "react";
import { useWorkspace } from "../store/workspace";
import { EnvEditor } from "./EnvEditor";

export function EnvSelector({ collectionId }: { collectionId: string | null }) {
  const { environments, activeEnv, selectEnv } = useWorkspace();
  const [editing, setEditing] = useState(false);
  if (!collectionId) return null;
  const envs = environments[collectionId] ?? [];
  const current = activeEnv[collectionId] ?? null;
  return (
    <div className="env-selector">
      <select
        aria-label="Environment"
        value={current ?? ""}
        onChange={(e) => selectEnv(collectionId, e.target.value || null)}
      >
        <option value="">No environment</option>
        {envs.map((e) => (
          <option key={e.name} value={e.name}>
            {e.name}
          </option>
        ))}
      </select>
      <button disabled={!current} onClick={() => setEditing(true)}>
        Edit
      </button>
      {editing && current && (
        <EnvEditor collectionId={collectionId} name={current} onClose={() => setEditing(false)} />
      )}
    </div>
  );
}
