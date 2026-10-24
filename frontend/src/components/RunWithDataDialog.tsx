import { useState, type ChangeEvent } from "react";
import type { DataFileDto, DataPreviewDto } from "../api/types";
import { api } from "../api/instance";
import { useRuns } from "../store/runs";
import { Modal } from "./Modal";

/** Same limit as the server (`data_file::MAX_DATA_FILE`). */
const MAX_DATA_FILE = 16 * 1024 * 1024;

interface Props {
  collectionId: string;
  /** Folder or request ID; `""` runs the whole collection. */
  item: string;
  label: string;
  env: string | null;
  onClose: () => void;
}

function formatOf(name: string): DataFileDto["format"] | null {
  const lower = name.toLowerCase();
  if (lower.endsWith(".csv")) return "csv";
  if (lower.endsWith(".json")) return "json";
  return null;
}

/**
 * "Run with data…": run once per row of a CSV/JSON file, or N times. The file is read in
 * the browser and never kept after the dialog closes (it can hold credentials); the
 * preview shows only the row count and column names.
 */
export function RunWithDataDialog({ collectionId, item, label, env, onClose }: Props) {
  const start = useRuns((s) => s.start);
  const [file, setFile] = useState<{ name: string; data: DataFileDto }>();
  const [preview, setPreview] = useState<DataPreviewDto>();
  const [error, setError] = useState<string>();
  const [count, setCount] = useState("1");

  const pick = async (e: ChangeEvent<HTMLInputElement>) => {
    const picked = e.target.files?.[0];
    setFile(undefined);
    setPreview(undefined);
    setError(undefined);
    if (!picked) return;
    const format = formatOf(picked.name);
    if (!format) {
      setError(`${picked.name}: choose a .csv or .json file`);
      return;
    }
    if (picked.size > MAX_DATA_FILE) {
      setError(`${picked.name}: file is larger than 16 MiB`);
      return;
    }
    const data = { format, text: await picked.text() };
    try {
      setPreview(await api().previewDataFile(collectionId, data));
      setFile({ name: picked.name, data });
    } catch (err) {
      setError((err instanceof Error ? err.message : String(err)).replace(/^data file/, picked.name));
    }
  };

  const iterations = Number(count);
  const countValid = Number.isInteger(iterations) && iterations >= 1;
  const canRun = file ? !!preview : !error && countValid;
  const run = () => {
    if (!canRun) return;
    start(collectionId, item, env, label, file ? { data: file.data } : { iterationCount: iterations });
    onClose();
  };

  return (
    <Modal title="Run with data" onDismiss={onClose}>
      <div className="run-with-data">
      <h3>Run with data: {label}</h3>
      <label>
        <span>Data file (CSV or JSON)</span>
        <input type="file" accept=".csv,.json" aria-describedby="data-file-help" onChange={(e) => void pick(e)} />
      </label>
      <p id="data-file-help" className="muted">
        One iteration per row; <code>{"{{column}}"}</code> reads the row. The file is not saved.
      </p>
      {preview && file && (
        <p role="status">
          {file.name}: {preview.rows} {preview.rows === 1 ? "row" : "rows"} · {preview.columns.join(", ")}
        </p>
      )}
      {error && (
        <p role="alert" className="err">
          {error}
        </p>
      )}
      <label>
        <span>Iterations</span>
        <input
          type="number"
          min={1}
          step={1}
          value={file ? String(preview?.rows ?? "") : count}
          disabled={!!file}
          aria-describedby={file ? "iterations-help" : undefined}
          onChange={(e) => setCount(e.target.value)}
        />
      </label>
      {file && preview && (
        <p id="iterations-help" className="muted">
          set by file ({preview.rows} rows)
        </p>
      )}
      <div className="actions">
        <button type="button" onClick={onClose}>
          Cancel
        </button>
        <button type="button" className="primary" disabled={!canRun} onClick={run}>
          Run
        </button>
      </div>
      </div>
    </Modal>
  );
}
