import { useState } from "react";
import type { KeyValueDto } from "../api/types";
import { useWorkspace } from "../store/workspace";
import { KeyValueTable } from "./request/KeyValueTable";

interface Props {
  collectionId: string;
  name: string;
  onClose: () => void;
}

/** Edits an environment's variables (saved to its .bru file) and secret values (memory only). */
export function EnvEditor({ collectionId, name, onClose }: Props) {
  const env = useWorkspace((s) => s.environments[collectionId]?.find((e) => e.name === name));
  const { saveEnvironment, setSecrets } = useWorkspace();
  const [vars, setVars] = useState<KeyValueDto[]>(env?.vars ?? []);
  const [secrets, setSecretValues] = useState<Record<string, string>>({});
  const [status, setStatus] = useState<string | null>(null);
  if (!env) return null;

  const save = async () => {
    const result = await saveEnvironment(collectionId, name, vars);
    if (Object.keys(secrets).length > 0) await setSecrets(collectionId, name, secrets);
    if (result === "saved") {
      onClose();
      return;
    }
    const fresh = useWorkspace.getState().environments[collectionId]?.find((e) => e.name === name);
    setVars(fresh?.vars ?? []);
    setStatus("The file changed on disk and was reloaded; re-apply your edits and save again.");
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" role="dialog" aria-label={`Environment ${name}`} onClick={(e) => e.stopPropagation()}>
        <h3>Environment: {name}</h3>
        <KeyValueTable rows={vars} onChange={setVars} />
        {env.secretNames.length > 0 && (
          <>
            <h4>Secrets</h4>
            <p className="muted">Kept in memory for this session only; never written to disk.</p>
            {env.secretNames.map((secret) => (
              <label key={secret} className="secret">
                {secret}
                <input
                  type="password"
                  value={secrets[secret] ?? ""}
                  onChange={(e) => setSecretValues({ ...secrets, [secret]: e.target.value })}
                />
              </label>
            ))}
          </>
        )}
        {status && <p className="banner warn">{status}</p>}
        <div className="actions">
          <button onClick={onClose}>Cancel</button>
          <button className="primary" onClick={() => void save()}>
            Save
          </button>
        </div>
      </div>
    </div>
  );
}
