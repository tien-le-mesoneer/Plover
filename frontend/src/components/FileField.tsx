import { useId, useState } from "react";

interface Props {
  label: string;
  accept: string;
  /** Help text read with the field (its element ID). */
  describedBy?: string;
  /** The picked file, or `undefined` when the pick was cancelled. */
  onFile: (file: File | undefined) => void;
}

/**
 * A file picker that looks like the other buttons: the native input stays in the page
 * (visually hidden, labelled, in the Tab order, showing its focus on the button), and a
 * second label is the visible "Choose file…" button. The picked name sits beside it.
 */
export function FileField({ label, accept, describedBy, onFile }: Props) {
  const id = useId();
  const [name, setName] = useState<string>();
  return (
    <div className="field file-field">
      <label id={`${id}-label`} htmlFor={id}>
        {label}
      </label>
      <span className="file-row">
        <input
          id={id}
          type="file"
          className="sr-only"
          accept={accept}
          aria-labelledby={`${id}-label`}
          aria-describedby={[describedBy, `${id}-name`].filter(Boolean).join(" ")}
          onChange={(e) => {
            const file = e.target.files?.[0];
            setName(file?.name);
            onFile(file);
          }}
        />
        <label htmlFor={id} className="file-button" aria-hidden="true">
          Choose file…
        </label>
        <span id={`${id}-name`} className="file-name">
          {name ?? "No file chosen"}
        </span>
      </span>
    </div>
  );
}
