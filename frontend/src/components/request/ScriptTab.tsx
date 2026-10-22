import { useState } from "react";
import { CodeEditor } from "../../editor/CodeEditor";

const PHASES = [
  { key: "pre", label: "Pre Request" },
  { key: "post", label: "Post Response" },
] as const;

/** `script:pre-request` / `script:post-response`. An emptied editor removes the block. */
export function ScriptTab({
  pre,
  post,
  onChange,
}: {
  pre: string | null;
  post: string | null;
  onChange: (phase: "pre" | "post", script: string | null) => void;
}) {
  const [phase, setPhase] = useState<"pre" | "post">(pre || !post ? "pre" : "post");
  const value = (phase === "pre" ? pre : post) ?? "";
  return (
    <div className="script-tab">
      <nav className="subtabs">
        {PHASES.map((p) => (
          <button key={p.key} className={phase === p.key ? "active" : ""} onClick={() => setPhase(p.key)}>
            {p.label}
            {(p.key === "pre" ? pre : post) ? <sup>•</sup> : null}
          </button>
        ))}
      </nav>
      <div className="editor-box" key={phase}>
        <CodeEditor value={value} language="javascript" onChange={(v) => onChange(phase, v === "" ? null : v)} />
      </div>
    </div>
  );
}
