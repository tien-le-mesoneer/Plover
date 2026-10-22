const STORAGE_KEY = "bru-rs.token";

/**
 * The server opens the UI at `/#token=<token>`. The fragment never reaches
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
