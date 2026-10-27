// Runs before the first paint (a blocking classic script in <head>) so a dark OS never
// flashes the light theme; lib/appTheme.ts takes over once the app loads.
document.documentElement.dataset.theme = window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
