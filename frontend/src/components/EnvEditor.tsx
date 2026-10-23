import { useState } from "react";
import type { KeyValueDto } from "../api/types";
import { useWorkspace } from "../store/workspace";
import { Banner } from "./Banner";
import { Modal } from "./Modal";
import { KeyValueTable } from "./request/KeyValueTable";

interface Props {
  collectionId: string;
  name: string;
  onClose: () => void;
}

/**
 * Edits an environment's variables (saved to its .bru file) and secret values
 * (OS keychain, or memory when unavailable). Only secrets the user changed are
 * sent; others keep their value.
 */
export function EnvEditor({ collectionId, name, onClose }: Props) {
  const env = useWorkspace((s) => s.environments[collectionId]?.find((e) => e.name === name));
  const { saveEnvironment, setSecrets } = useWorkspace();
  const keychain = useWorkspace((s) => s.capabilities?.secretBackend === "keychain");
  const [vars, setVars] = useState<KeyValueDto[]>(env?.vars ?? []);
  // Only touched secrets: a string sets it, null clears it.
  const [edits, setEdits] = useState<Record<string, string | null>>({});
  const [status, setStatus] = useState<string | null>(null);
  if (!env) return null;

  const memoryOnly = env.secretsSet.filter((s) => s.backend === "memory").map((s) => s.name);
  const dirty = Object.keys(edits).length > 0 || JSON.stringify(vars) !== JSON.stringify(env.vars);
  // Escape, a backdrop click or Cancel keep unsaved edits unless the user confirms.
  const dismiss = () => {
    if (!dirty || window.confirm(`Discard unsaved changes to environment "${name}"?`)) onClose();
  };

  const save = async () => {
    const warnings = Object.keys(edits).length > 0 ? await setSecrets(collectionId, name, edits) : [];
    const result = await saveEnvironment(collectionId, name, vars);
    if (result === "saved" && warnings.length === 0) {
      onClose();
      return;
    }
    if (result === "saved") {
      // Stay open so the user sees which values did not reach the keychain.
      setEdits({});
      setStatus(warnings.join("\n"));
      return;
    }
    const fresh = useWorkspace.getState().environments[collectionId]?.find((e) => e.name === name);
    setVars(fresh?.vars ?? []);
    setEdits({});
    setStatus("The file changed on disk and was reloaded; re-apply your edits and save again.");
  };

  return (
    <Modal title={`Environment ${name}`} onDismiss={dismiss}>
        <h3>Environment: {name}</h3>
        <KeyValueTable rows={vars} onChange={setVars} />
        {env.secretNames.length > 0 && (
          <>
            <h4>Secrets</h4>
            <p className="muted">
              {keychain
                ? "Stored in the OS keychain; never written to collection files."
                : "Memory only — values are lost when Plover quits; never written to disk."}
            </p>
            {memoryOnly.length > 0 && (
              <Banner kind="warn" role="alert">
                Memory only: {memoryOnly.join(", ")} — these values are lost when Plover quits.
              </Banner>
            )}
            {env.secretNames.map((secret) => {
              const stored = env.secretsSet.find((s) => s.name === secret);
              const isSet = stored !== undefined;
              const edit = edits[secret];
              const state =
                edit === null ? "will be cleared" : edit !== undefined ? "changed" : isSet ? "set" : "not set";
              return (
                <label key={secret} className="secret">
                  <span>
                    {secret} <span className="muted">({state})</span>
                    {stored && (
                      <span className={`badge ${stored.backend}`}>
                        {stored.backend === "keychain" ? "keychain" : "memory only"}
                      </span>
                    )}
                  </span>
                  <span className="secret-row">
                    <input
                      type="password"
                      aria-label={`Secret ${secret}`}
                      placeholder={isSet ? "•••••• (unchanged)" : ""}
                      value={edit ?? ""}
                      onChange={(e) => setEdits({ ...edits, [secret]: e.target.value })}
                    />
                    {isSet && (
                      <button type="button" onClick={() => setEdits({ ...edits, [secret]: null })}>
                        Clear
                      </button>
                    )}
                  </span>
                </label>
              );
            })}
          </>
        )}
        {status && (
          <Banner kind="warn" role="status">
            {status}
          </Banner>
        )}
        <div className="actions">
          <button onClick={dismiss}>Cancel</button>
          <button className="primary" onClick={() => void save()}>
            Save
          </button>
        </div>
    </Modal>
  );
}
