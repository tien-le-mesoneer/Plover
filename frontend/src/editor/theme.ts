export type ThemeName = "plover-light" | "plover-dark";

export const themeFor = (dark: boolean): ThemeName => (dark ? "plover-dark" : "plover-light");
