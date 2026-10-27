import type { ThemeDto } from "../api/types";

/** What the user picked; `system` follows the OS light/dark setting. */
export type ThemeChoice = ThemeDto;
/** What is drawn: the `data-theme` value on `<html>` that styles.css keys the dark tokens on. */
export type ResolvedTheme = "light" | "dark";

export interface SchemeQuery {
  matches: boolean;
  addEventListener(type: "change", listener: (e: { matches: boolean }) => void): void;
  removeEventListener(type: "change", listener: (e: { matches: boolean }) => void): void;
}

const ORDER: ThemeChoice[] = ["system", "light", "dark"];
export const nextTheme = (c: ThemeChoice): ThemeChoice => ORDER[(ORDER.indexOf(c) + 1) % ORDER.length]!;

let choice: ThemeChoice = "system";
let systemDark = false;
let root: HTMLElement | undefined;
let resolved: ResolvedTheme = "light";
const listeners = new Set<(t: ResolvedTheme) => void>();

function apply() {
  const next: ResolvedTheme = choice === "system" ? (systemDark ? "dark" : "light") : choice;
  if (root) root.dataset.theme = next;
  if (next === resolved) return;
  resolved = next;
  for (const l of listeners) l(next);
}

/**
 * Applies the theme to `root` now (before the first render, so a dark system never
 * flashes light) and follows OS scheme changes while the choice is `system`.
 */
export function startTheme(
  query: SchemeQuery | undefined = window.matchMedia?.("(prefers-color-scheme: dark)"),
  target: HTMLElement = document.documentElement,
): () => void {
  root = target;
  choice = "system";
  systemDark = query?.matches ?? false;
  resolved = systemDark ? "dark" : "light";
  root.dataset.theme = resolved;
  const onChange = (e: { matches: boolean }) => {
    systemDark = e.matches;
    apply();
  };
  query?.addEventListener("change", onChange);
  return () => query?.removeEventListener("change", onChange);
}

export function setThemeChoice(c: ThemeChoice): void {
  choice = c;
  apply();
}

export const resolvedTheme = (): ResolvedTheme => resolved;

/** Calls `listener` whenever the drawn theme changes; returns the unsubscribe. */
export function onResolvedTheme(listener: (t: ResolvedTheme) => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}
