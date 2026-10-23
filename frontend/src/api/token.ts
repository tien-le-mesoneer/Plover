const STORAGE_KEY = "plover.token";

/**
 * URLs the app opens itself carry `#code=<single-use code>`; the page trades it for the
 * token (`exchangeLaunchCode`). This removes the code from the address bar at once.
 */
export function takeLaunchCode(
  loc: Location,
  history: Pick<History, "replaceState"> = window.history,
): string | null {
  const code = new URLSearchParams(loc.hash.replace(/^#/, "")).get("code");
  if (!code) return null;
  history.replaceState(null, "", loc.pathname + loc.search);
  return code;
}

/** Remembers a token obtained from a launch code for this tab. */
export function storeToken(token: string, storage: Storage): void {
  storage.setItem(STORAGE_KEY, token);
}

/**
 * `plv serve` prints a URL with `/#token=<token>` for copying. The fragment never reaches
 * the server or any log; we keep the token in sessionStorage (this tab only)
 * and remove it from the address bar.
 */
export function takeTokenFromLocation(
  loc: Location,
  storage: Storage,
  history: Pick<History, "replaceState"> = window.history,
): string | null {
  const fromHash = new URLSearchParams(loc.hash.replace(/^#/, "")).get("token");
  if (fromHash) {
    storage.setItem(STORAGE_KEY, fromHash);
    history.replaceState(null, "", loc.pathname + loc.search);
    return fromHash;
  }
  return storage.getItem(STORAGE_KEY);
}
