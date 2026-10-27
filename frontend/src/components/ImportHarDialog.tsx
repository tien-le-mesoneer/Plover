import { useEffect, useMemo, useState } from "react";
import { api } from "../api/instance";
import type { HarImportBody, HarImportResultDto, HarPlanDto, HarRowDto } from "../api/types";
import { folderIds } from "../lib/tree";
import { useWorkspace } from "../store/workspace";
import { AlertIcon } from "./icons";
import { Modal } from "./Modal";

/** The server's request body limit; larger files are refused before upload. */
export const MAX_HAR_BYTES = 64 * 1024 * 1024;
const PLAN_DELAY_MS = 300;

function readText(file: File): Promise<string> {
  if (typeof file.text === "function") return file.text();
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error);
    reader.readAsText(file);
  });
}

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/**
 * Import a HAR file into a folder: pick the entries (assets and preflights hidden by
 * default), see the secret variables it will create (names only), then import.
 */
export function ImportHarDialog({
  collectionId,
  onClose,
  initialFolder,
}: {
  collectionId: string;
  onClose: () => void;
  /** The folder the import goes into at first (a folder's context menu); the root otherwise. */
  initialFolder?: string;
}) {
  const tree = useWorkspace((s) => s.trees[collectionId]);
  const collectionName = useWorkspace((s) => s.collections.find((c) => c.id === collectionId)?.name);
  const envs = useWorkspace((s) => s.environments[collectionId] ?? []);
  const activeEnv = useWorkspace((s) => s.activeEnv[collectionId] ?? null);
  const [har, setHar] = useState<string | null>(null);
  const [rows, setRows] = useState<HarRowDto[]>([]);
  const [ticked, setTicked] = useState<Set<number>>(new Set());
  const [hostsOff, setHostsOff] = useState<Set<string>>(new Set());
  const [showHidden, setShowHidden] = useState(false);
  const [folder, setFolder] = useState(initialFolder ?? "");
  const [newFolder, setNewFolder] = useState("");
  const [env, setEnv] = useState<string>(activeEnv ?? "");
  // The plan for one request body (its key); a plan for an older body is not shown.
  const [planned, setPlanned] = useState<{ key: string; plan: HarPlanDto } | null>(null);
  const [importing, setImporting] = useState(false);
  const [result, setResult] = useState<HarImportResultDto | null>(null);
  const [error, setError] = useState<string | null>(null);

  const hosts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const r of rows) counts.set(r.host, (counts.get(r.host) ?? 0) + r.duplicates);
    return [...counts.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
  }, [rows]);
  const visible = rows.filter((r) => !hostsOff.has(r.host) && (showHidden || r.hidden === null));
  const entries = visible.filter((r) => ticked.has(r.index)).map((r) => r.index);
  const hiddenCount = rows.filter((r) => r.hidden !== null && !hostsOff.has(r.host)).length;
  const body: HarImportBody | null = har
    ? { har, entries, folder, newFolder: newFolder.trim() || null, env: env || null }
    : null;
  const bodyKey = body ? JSON.stringify({ ...body, har: har?.length }) : "";

  const plan = planned?.key === bodyKey ? planned.plan : null;
  const planning = entries.length > 0 && plan === null;

  useEffect(() => {
    if (!body || body.entries.length === 0) return;
    let stale = false;
    const timer = setTimeout(() => {
      api()
        .planHar(collectionId, body)
        .then((p) => !stale && setPlanned({ key: bodyKey, plan: p }))
        .catch((e) => !stale && setError(message(e)));
    }, PLAN_DELAY_MS);
    return () => {
      stale = true;
      clearTimeout(timer);
    };
    // `bodyKey` stands for `body` (the HAR text itself only changes with the file).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [bodyKey, collectionId]);

  const onFile = async (file: File | undefined) => {
    setError(null);
    setResult(null);
    setRows([]);
    setHar(null);
    if (!file) return;
    if (file.size > MAX_HAR_BYTES) {
      setError("This HAR file is larger than 64 MB. Save a shorter capture, or one without response content.");
      return;
    }
    try {
      const text = await readText(file);
      const preview = await api().previewHar(collectionId, text);
      setHar(text);
      setRows(preview);
      setTicked(new Set(preview.filter((r) => r.hidden === null).map((r) => r.index)));
      setHostsOff(new Set());
      const first = preview[0]?.host;
      setNewFolder(first ? `captured-${first}` : "captured");
    } catch (e) {
      setError(message(e));
    }
  };

  const toggle = (index: number) =>
    setTicked((s) => {
      const next = new Set(s);
      if (next.has(index)) next.delete(index);
      else next.add(index);
      return next;
    });

  const toggleHost = (host: string) =>
    setHostsOff((s) => {
      const next = new Set(s);
      if (next.has(host)) next.delete(host);
      else next.add(host);
      return next;
    });

  const runImport = async () => {
    if (!body) return;
    setImporting(true);
    setError(null);
    try {
      const done = await api().importHar(collectionId, body);
      setResult(done);
      await useWorkspace.getState().reload(collectionId);
    } catch (e) {
      setError(message(e));
    } finally {
      setImporting(false);
    }
  };

  const target = newFolder.trim() ? (folder ? `${folder}/${newFolder.trim()}` : newFolder.trim()) : folder || "the root";
  const envLabel = env || "Imported";

  return (
    <Modal title="Import HAR" onDismiss={onClose}>
      <div className="import-har">
        <h3>Import HAR</h3>
        <label>
          <span>HAR file</span>
          <input type="file" accept=".har,application/json" onChange={(e) => void onFile(e.target.files?.[0])} />
        </label>
        {error && (
          <p className="field-error" role="alert">
            {error}
          </p>
        )}
        {rows.length > 0 && (
          <>
            <div className="har-hosts" role="group" aria-label="Hosts">
              {hosts.map(([host, count]) => (
                <button
                  key={host || "(none)"}
                  type="button"
                  className="chip"
                  aria-pressed={!hostsOff.has(host)}
                  onClick={() => toggleHost(host)}
                >
                  {host || "(no host)"} <span className="muted">{count}</span>
                </button>
              ))}
            </div>
            <label className="inline">
              <input type="checkbox" checked={showHidden} onChange={(e) => setShowHidden(e.target.checked)} />
              Show hidden ({hiddenCount}): images, scripts, styles, fonts, media, source maps, preflights
            </label>
            <div className="har-table">
              <table>
                <thead>
                  <tr>
                    <th scope="col">
                      <span className="visually-hidden">Import</span>
                    </th>
                    <th scope="col">Method</th>
                    <th scope="col">URL</th>
                    <th scope="col">Status</th>
                    <th scope="col">Type</th>
                  </tr>
                </thead>
                <tbody>
                  {visible.map((r) => (
                    <tr key={r.index} className={r.hidden ? "muted" : undefined}>
                      <td>
                        <input
                          type="checkbox"
                          aria-label={`Import ${r.method} ${r.url}`}
                          checked={ticked.has(r.index)}
                          onChange={() => toggle(r.index)}
                        />
                      </td>
                      <td>
                        <span className={`badge method-${r.method.toLowerCase()}`}>{r.method}</span>
                      </td>
                      <td className="har-url">
                        <span>{r.url}</span>
                        {r.duplicates > 1 && <span className="muted"> ×{r.duplicates}</span>}
                        {r.hasSecrets && (
                          <span className="har-secret" title="Has credentials; they become secret variables">
                            <AlertIcon />
                          </span>
                        )}
                      </td>
                      <td className="num">{r.status ?? "—"}</td>
                      <td>{r.hidden ?? r.mime}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <div className="har-target">
              <label>
                <span>Import into</span>
                <select value={folder} onChange={(e) => setFolder(e.target.value)}>
                  <option value="">{collectionName ?? "Collection"} (root)</option>
                  {folderIds(tree).map((id) => (
                    <option key={id} value={id}>
                      {id}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                <span>New subfolder</span>
                <input value={newFolder} spellCheck={false} onChange={(e) => setNewFolder(e.target.value)} />
              </label>
              <label>
                <span>Environment</span>
                <select value={env} onChange={(e) => setEnv(e.target.value)}>
                  {envs.map((e) => (
                    <option key={e.name} value={e.name}>
                      {e.name}
                    </option>
                  ))}
                  {!envs.some((e) => e.name === "Imported") && <option value="">Imported (new)</option>}
                </select>
              </label>
            </div>
            {result ? (
              <p role="status">
                Imported {result.items.length} request{result.items.length === 1 ? "" : "s"} into {target}.
                {result.secretNames.length > 0 &&
                  ` Secret variables in ${result.env}: ${result.secretNames.join(", ")}.`}
                {result.failedAt && ` Stopped at ${result.failedAt}. Files written before it stay.`}
              </p>
            ) : (
              <p className="muted" aria-live="polite">
                {entries.length === 0
                  ? "Tick the requests to import."
                  : planning || !plan
                    ? "Checking which secret variables this creates…"
                    : plan.secretNames.length > 0
                      ? `These secret variables will be created in ${plan.env}: ${plan.secretNames.join(", ")}`
                      : "No secret variables will be created."}
              </p>
            )}
            {(result?.notices ?? plan?.notices ?? []).map((n, i) => (
              <p key={i} className="muted">
                {n}
              </p>
            ))}
          </>
        )}
        <div className="actions">
          <button type="button" onClick={onClose}>
            {result ? "Close" : "Cancel"}
          </button>
          {!result && (
            <button
              type="button"
              className="primary"
              disabled={!body || entries.length === 0 || planning || importing}
              title={`Import into ${target}; secrets go to ${envLabel}`}
              onClick={() => void runImport()}
            >
              Import
            </button>
          )}
        </div>
      </div>
    </Modal>
  );
}
