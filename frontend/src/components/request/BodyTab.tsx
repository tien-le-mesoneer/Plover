import type { BodyDto, MultipartFieldDto } from "../../api/types";
import { CodeEditor } from "../../editor/CodeEditor";
import { KeyValueTable } from "./KeyValueTable";

const MODES: { mode: Exclude<BodyDto["mode"], "unsupported">; label: string }[] = [
  { mode: "none", label: "No body" },
  { mode: "json", label: "JSON" },
  { mode: "text", label: "Text" },
  { mode: "xml", label: "XML" },
  { mode: "formUrlEncoded", label: "Form URL encoded" },
  { mode: "multipartForm", label: "Multipart form" },
];

interface Props {
  body: BodyDto;
  onChange: (body: BodyDto) => void;
  /** Switch modes; each mode keeps its own content (see `switchBodyMode`). */
  onModeChange: (mode: Exclude<BodyDto["mode"], "unsupported">) => void;
}

export function BodyTab({ body, onChange, onModeChange }: Props) {
  if (body.mode === "unsupported") {
    return (
      <p className="muted">
        Body mode <code>{body.name}</code> is kept in the file but not editable or sent by this version.
      </p>
    );
  }
  return (
    <div className="body-tab">
      <select aria-label="Body mode" value={body.mode} onChange={(e) => onModeChange(e.target.value as (typeof MODES)[number]["mode"])}>
        {MODES.map((m) => (
          <option key={m.mode} value={m.mode}>
            {m.label}
          </option>
        ))}
      </select>
      {(body.mode === "json" || body.mode === "text" || body.mode === "xml") && (
        <div className="editor-box">
          <CodeEditor
            value={body.text}
            language={body.mode === "text" ? "plaintext" : body.mode}
            onChange={(text) => onChange({ ...body, text })}
          />
        </div>
      )}
      {body.mode === "formUrlEncoded" && (
        <KeyValueTable rows={body.fields} onChange={(fields) => onChange({ ...body, fields })} />
      )}
      {body.mode === "multipartForm" && (
        <MultipartTable fields={body.fields} onChange={(fields) => onChange({ ...body, fields })} />
      )}
    </div>
  );
}

function MultipartTable({
  fields,
  onChange,
}: {
  fields: MultipartFieldDto[];
  onChange: (fields: MultipartFieldDto[]) => void;
}) {
  const set = (i: number, patch: Partial<MultipartFieldDto>) =>
    onChange(fields.map((f, j) => (j === i ? { ...f, ...patch } : f)));
  return (
    <table className="kv">
      <tbody>
        {fields.map((f, i) => (
          <tr key={i} className={f.enabled ? "" : "disabled"}>
            <td className="kv-check">
              <input type="checkbox" checked={f.enabled} onChange={(e) => set(i, { enabled: e.target.checked })} />
            </td>
            <td>
              <input value={f.name} placeholder="name" onChange={(e) => set(i, { name: e.target.value })} />
            </td>
            <td>
              <select
                value={f.value.type}
                onChange={(e) =>
                  set(i, {
                    value: e.target.value === "files" ? { type: "files", paths: [] } : { type: "text", value: "" },
                  })
                }
              >
                <option value="text">text</option>
                <option value="files">file</option>
              </select>
            </td>
            <td>
              {f.value.type === "text" ? (
                <input
                  value={f.value.value}
                  placeholder="value"
                  onChange={(e) => set(i, { value: { type: "text", value: e.target.value } })}
                />
              ) : (
                <input
                  value={f.value.paths.join("|")}
                  placeholder="path relative to the collection (a|b for several)"
                  onChange={(e) =>
                    set(i, { value: { type: "files", paths: e.target.value.split("|").filter(Boolean) } })
                  }
                />
              )}
            </td>
            <td className="kv-del">
              <button className="icon" title="Remove" onClick={() => onChange(fields.filter((_, j) => j !== i))}>
                ×
              </button>
            </td>
          </tr>
        ))}
        <tr>
          <td colSpan={5}>
            <button
              onClick={() =>
                onChange([...fields, { name: "", value: { type: "text", value: "" }, enabled: true, contentType: null }])
              }
            >
              Add field
            </button>
          </td>
        </tr>
      </tbody>
    </table>
  );
}
