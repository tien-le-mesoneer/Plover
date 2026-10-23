// The page's startup sequence: load collections, restore the remembered layout, start
// saving layout changes, then tell the server how it went (`POST /api/client-ready`). The
// report is sent even when a step fails, with the error, so a stuck start (the desktop
// smoke test waits for this) says why instead of timing out silently.
import { api } from "../api/instance";
import { restoreUiState, startUiPersistence } from "./uiState";
import { useWorkspace } from "./workspace";

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** Resolves to the function that stops saving layout changes (if startup got that far). */
export async function startUp(isCancelled: () => boolean = () => false): Promise<(() => void) | undefined> {
  let stop: (() => void) | undefined;
  let failure: string | undefined;
  try {
    await useWorkspace.getState().init();
    await restoreUiState();
    // Save layout changes only after the remembered layout is back, so a restart
    // never overwrites it with an empty one.
    if (!isCancelled()) stop = startUiPersistence();
  } catch (e) {
    failure = message(e);
    useWorkspace.setState({ error: failure });
  } finally {
    // init() keeps its own failures (capabilities, collections) as the visible error.
    const error = failure ?? useWorkspace.getState().error ?? undefined;
    await api()
      .clientReady(error)
      .catch(() => undefined);
  }
  return stop;
}
