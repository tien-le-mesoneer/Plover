import { useState } from "react";
import { useWorkspace } from "../store/workspace";
import { EnvEditor } from "./EnvEditor";

/**
 * The top bar's environment control: `<collection> · <env ▾>` for the collection of the
 * active tab, so a switch between collections with environments of the same name is visible.
 */
export function EnvSelector({ collectionId }: { collectionId: string | null }) {
  const { environments, activeEnv, selectEnv, collections } = useWorkspace();
  const [editing, setEditing] = useState(false);
  if (!collectionId) return null;
  const envs = environments[collectionId] ?? [];
  const current = activeEnv[collectionId] ?? null;
  const name = collections.find((c) => c.id === collectionId)?.name ?? collectionId;
  return (
    <div className="env-selector">
      <span className="env-collection" title={name}>
        {name}
      </span>
      <span className="env-sep" aria-hidden="true">
        ·
      </span>
      <select
        aria-label={`Environment for ${name}`}
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
