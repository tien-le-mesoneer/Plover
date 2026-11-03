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

/**
 * The page's session token, from the first source that has one:
 * 1. a launch code (`#code=`), traded once;
 * 2. a `#token=` in the address;
 * 3. the server itself, when `plv serve` runs without `--require-token` (the plain
 *    address opens the app; this also replaces a stored token from an earlier server);
 * 4. the token this tab stored earlier.
 */
export async function resolveSessionToken(deps: {
  loc: Location;
  storage: Storage;
  exchange: (code: string) => Promise<string>;
  open: () => Promise<string>;
  history?: Pick<History, "replaceState">;
}): Promise<string | null> {
  const { loc, storage, history = window.history } = deps;
  const code = takeLaunchCode(loc, history);
  if (code) {
    try {
      const token = await deps.exchange(code);
      storeToken(token, storage);
      return token;
    } catch {
      // Used or expired code: try the other sources.
    }
  }
  const fromHash = new URLSearchParams(loc.hash.replace(/^#/, "")).get("token");
  if (fromHash) return takeTokenFromLocation(loc, storage, history);
  try {
    const token = await deps.open();
    storeToken(token, storage);
    return token;
  } catch {
    // The server asks for a token: use the one this tab has, if any.
  }
  return storage.getItem(STORAGE_KEY);
}
