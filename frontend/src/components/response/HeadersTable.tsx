import { CornerActions } from "../CornerActions";
import { CopyIcon } from "../icons";

/** Response headers; the copy-all icon (corner) and each row's copy button show on hover and keyboard focus. */
export function HeadersTable({ headers, onCopy }: { headers: [string, string][]; onCopy: (text: string) => void }) {
  return (
    <div className="headers-view has-corner">
      <CornerActions>
        <button
          type="button"
          className="icon"
          aria-label="Copy all headers"
          title="Copy all headers"
          onClick={() => onCopy(headers.map(([k, v]) => `${k}: ${v}`).join("\n"))}
        >
          <CopyIcon />
        </button>
      </CornerActions>
      <table className="headers" aria-label="Response headers">
        <thead>
          <tr>
            <th scope="col">Header</th>
            <th scope="col">Value</th>
            <th scope="col">
              <span className="sr-only">Copy</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {headers.map(([k, v], i) => (
            <tr key={i} className="header-row">
              <th scope="row">{k}</th>
              <td>{v}</td>
              <td className="row-copy">
                <button type="button" className="icon" aria-label={`Copy ${k}`} title={`Copy ${k}`} onClick={() => onCopy(v)}>
                  <CopyIcon />
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
