import type { VarInfo } from "./varTokens";

const SOURCES: Record<string, string> = {
  processEnv: "Process env",
  iteration: "Data row",
  runtime: "Runtime",
  request: "Request",
  folder: "Folder",
  environment: "Environment",
  collection: "Collection",
};

/** The copy-value shortcut as this platform writes it. */
export function copyShortcut(): string {
  return /Mac|iPhone|iPad/.test(globalThis.navigator?.platform ?? "") ? "⌘⇧C" : "Ctrl+Shift+C";
}

/** `{{name}}` was copied (the live note). */
export const copiedValueNote = (name: string) => `Copied value of {{${name}}}`;

/** "Environment: Dev", "Runtime", … */
export function sourceLabel(source: string, env: string | null): string {
  const label = SOURCES[source] ?? source;
  return source === "environment" && env ? `${label}: ${env}` : label;
}

export interface HoverLine {
  /** Shown as code; `•••• secret` when masked. */
  value?: string;
  masked?: boolean;
  note: string;
}

/** The hover card lines for one variable, shared by inputs and the body editor. */
export function hoverLines(info: VarInfo, env: string | null): HoverLine[] {
  if (info.kind === "setLater") return [{ note: `Not set yet. Set at runtime by ${info.setBy}.` }];
  if (info.kind === "undefined") return [{ note: "Not defined in any scope." }];
  const lines: HoverLine[] = [
    info.masked
      ? { value: "•••• secret", masked: true, note: "" }
      : { value: info.value ?? "", note: info.truncated ? "(cut to 1 KiB)" : "" },
    { note: sourceLabel(info.source, env) },
  ];
  if (info.shadows.length) {
    lines.push({ note: `overrides ${info.shadows.map((s) => sourceLabel(s, null).toLowerCase()).join(", ")}` });
  }
  return lines;
}
