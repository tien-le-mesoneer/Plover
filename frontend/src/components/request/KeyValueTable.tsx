import type { KeyValueDto } from "../../api/types";
import { Banner } from "../Banner";
import { CloseIcon } from "../icons";

interface Props {
  rows: KeyValueDto[];
  onChange: (rows: KeyValueDto[]) => void;
  /** Names come from elsewhere (path params): no add, delete or rename. */
  fixedNames?: boolean;
  /** Column headers, also used in each input's accessible name. */
  nameLabel?: string;
  valueLabel?: string;
  /** A short example shown above the table instead of example placeholders. */
  hint?: string;
}

const empty = (): KeyValueDto => ({ name: "", value: "", enabled: true, local: false });

/** Editable name/value rows; the checkbox maps to `~` (disabled) in `.bru`. */
export function KeyValueTable({ rows, onChange, fixedNames, nameLabel = "Name", valueLabel = "Value", hint }: Props) {
  const set = (index: number, patch: Partial<KeyValueDto>) =>
    onChange(rows.map((r, i) => (i === index ? { ...r, ...patch } : r)));
  const remove = (index: number) => onChange(rows.filter((_, i) => i !== index));
  const display = fixedNames ? rows : [...rows, empty()];
  const tildeNames = rows.some((r) => r.name.startsWith("~"));
  const nameLower = nameLabel.toLowerCase();

  return (
    <>
    {tildeNames && (
      <Banner kind="warn" role="note">
        A name starting with <code>~</code> means &ldquo;disabled&rdquo; in <code>.bru</code>; it is saved
        as a disabled entry without the <code>~</code>.
      </Banner>
    )}
    {hint && <p className="kv-hint muted">{hint}</p>}
    <table className="kv">
      <thead>
        <tr>
          <th className="kv-check">
            <span className="sr-only">Enabled</span>
          </th>
          <th>{nameLabel}</th>
          <th>{valueLabel}</th>
          <th className="kv-del">
            <span className="sr-only">Remove</span>
          </th>
        </tr>
      </thead>
      <tbody>
        {display.map((row, i) => {
          const isNew = i === rows.length && !fixedNames;
          const change = (patch: Partial<KeyValueDto>) =>
            isNew ? onChange([...rows, { ...empty(), ...patch }]) : set(i, patch);
          const rowName = row.name || `row ${i + 1}`;
          return (
            <tr key={i} className={row.enabled ? "" : "disabled"}>
              <td className="kv-check">
                {!isNew && (
                  <input
                    type="checkbox"
                    aria-label={`Enable ${rowName}`}
                    checked={row.enabled}
                    onChange={(e) => set(i, { enabled: e.target.checked })}
                  />
                )}
              </td>
              <td>
                <input
                  value={row.name}
                  aria-label={isNew ? `New ${nameLower}` : `${nameLabel} of ${rowName}`}
                  placeholder={isNew ? `Add ${nameLower}` : ""}
                  readOnly={fixedNames}
                  spellCheck={false}
                  onChange={(e) => change({ name: e.target.value })}
                />
              </td>
              <td>
                <input
                  value={row.value}
                  aria-label={isNew ? `New ${valueLabel.toLowerCase()}` : `${valueLabel} of ${rowName}`}
                  spellCheck={false}
                  onChange={(e) => change({ value: e.target.value })}
                />
              </td>
              <td className="kv-del">
                {!isNew && !fixedNames && (
                  <button className="icon" title="Remove" aria-label="Remove" onClick={() => remove(i)}>
                    <CloseIcon />
                  </button>
                )}
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
    </>
  );
}
