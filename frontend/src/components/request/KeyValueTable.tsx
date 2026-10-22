import type { KeyValueDto } from "../../api/types";
import { Banner } from "../Banner";

interface Props {
  rows: KeyValueDto[];
  onChange: (rows: KeyValueDto[]) => void;
  /** Names come from elsewhere (path params): no add, delete or rename. */
  fixedNames?: boolean;
  namePlaceholder?: string;
  valuePlaceholder?: string;
}

const empty = (): KeyValueDto => ({ name: "", value: "", enabled: true, local: false });

/** Editable name/value rows; the checkbox maps to `~` (disabled) in `.bru`. */
export function KeyValueTable({ rows, onChange, fixedNames, namePlaceholder, valuePlaceholder }: Props) {
  const set = (index: number, patch: Partial<KeyValueDto>) =>
    onChange(rows.map((r, i) => (i === index ? { ...r, ...patch } : r)));
  const remove = (index: number) => onChange(rows.filter((_, i) => i !== index));
  const display = fixedNames ? rows : [...rows, empty()];
  const tildeNames = rows.some((r) => r.name.startsWith("~"));

  return (
    <>
    {tildeNames && (
      <Banner kind="warn" role="note">
        A name starting with <code>~</code> means &ldquo;disabled&rdquo; in <code>.bru</code>; it is saved
        as a disabled entry without the <code>~</code>.
      </Banner>
    )}
    <table className="kv">
      <tbody>
        {display.map((row, i) => {
          const isNew = i === rows.length;
          const change = (patch: Partial<KeyValueDto>) =>
            isNew ? onChange([...rows, { ...empty(), ...patch }]) : set(i, patch);
          return (
            <tr key={i} className={row.enabled ? "" : "disabled"}>
              <td className="kv-check">
                {!isNew && (
                  <input
                    type="checkbox"
                    aria-label="enabled"
                    checked={row.enabled}
                    onChange={(e) => set(i, { enabled: e.target.checked })}
                  />
                )}
              </td>
              <td>
                <input
                  value={row.name}
                  placeholder={namePlaceholder ?? "name"}
                  readOnly={fixedNames}
                  onChange={(e) => change({ name: e.target.value })}
                />
              </td>
              <td>
                <input
                  value={row.value}
                  placeholder={valuePlaceholder ?? "value"}
                  onChange={(e) => change({ value: e.target.value })}
                />
              </td>
              <td className="kv-del">
                {!isNew && !fixedNames && (
                  <button className="icon" title="Remove" aria-label="Remove" onClick={() => remove(i)}>
                    ×
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
