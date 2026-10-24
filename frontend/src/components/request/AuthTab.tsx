import type { InputHTMLAttributes } from "react";
import type { AuthDto } from "../../api/types";
import { VarInput } from "./VarInput";

const MODES = ["none", "inherit", "basic", "bearer"] as const;

interface Props {
  auth: AuthDto;
  onChange: (auth: AuthDto) => void;
  /** The request tab: `{{variables}}` in username and token are underlined and explained. */
  tabKey?: string;
}

export function AuthTab({ auth, onChange, tabKey }: Props) {
  // The password field stays a plain password input: its mirror would put the text in the page.
  const Field = (props: InputHTMLAttributes<HTMLInputElement>) =>
    tabKey ? <VarInput tabKey={tabKey} {...props} /> : <input {...props} />;
  if (auth.mode === "unsupported") {
    return (
      <p className="muted">
        Auth mode <code>{auth.name}</code> is kept in the file but not editable or applied by this
        version.
      </p>
    );
  }
  const select = (mode: (typeof MODES)[number]) => {
    switch (mode) {
      case "basic":
        return onChange({ mode, username: "", password: "" });
      case "bearer":
        return onChange({ mode, token: "" });
      default:
        return onChange({ mode });
    }
  };
  return (
    <div className="form">
      <label>
        Mode
        <select value={auth.mode} onChange={(e) => select(e.target.value as (typeof MODES)[number])}>
          {MODES.map((m) => (
            <option key={m}>{m}</option>
          ))}
        </select>
      </label>
      {auth.mode === "inherit" && <p className="muted">Uses the folder or collection auth.</p>}
      {auth.mode === "basic" && (
        <>
          <label>
            Username
            {Field({ value: auth.username, onChange: (e) => onChange({ ...auth, username: e.target.value }) })}
          </label>
          <label>
            Password
            <input
              type="password"
              value={auth.password}
              onChange={(e) => onChange({ ...auth, password: e.target.value })}
            />
          </label>
        </>
      )}
      {auth.mode === "bearer" && (
        <label>
          Token
          {Field({ value: auth.token, onChange: (e) => onChange({ ...auth, token: e.target.value }) })}
        </label>
      )}
    </div>
  );
}
