export type ThemeName = "plover-light" | "plover-dark";

export interface SchemeQuery {
  matches: boolean;
  addEventListener(type: "change", listener: (e: { matches: boolean }) => void): void;
  removeEventListener(type: "change", listener: (e: { matches: boolean }) => void): void;
}

export const themeFor = (dark: boolean): ThemeName => (dark ? "plover-dark" : "plover-light");

/** Applies the theme for the current scheme now and on every system change. */
export function watchColorScheme(query: SchemeQuery, apply: (theme: ThemeName) => void): () => void {
  apply(themeFor(query.matches));
  const listener = (e: { matches: boolean }) => apply(themeFor(e.matches));
  query.addEventListener("change", listener);
  return () => query.removeEventListener("change", listener);
}
