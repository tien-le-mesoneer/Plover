// Remembered per-collection layout (open tabs, expanded folders, environment), kept by
// the server so it survives restarts — the desktop webview gets a new origin each launch,
// so browser storage would not. Unsaved drafts are not remembered.
import { api } from "../api/instance";
import type { UiStateDto } from "../api/types";
import { useTabs } from "./tabs";
import { useWorkspace } from "./workspace";

const SAVE_DELAY_MS = 500;

/** Reopens remembered tabs (missing requests are dropped), folders and environments. */
export async function restoreUiState(): Promise<void> {
  let saved: Record<string, UiStateDto>;
  try {
    saved = await api().uiState();
  } catch {
    return;
  }
  const ws = useWorkspace.getState();
  let active: string | null = null;
  for (const c of ws.collections) {
    const ui = saved[c.id];
    if (!c.available || !ui) continue;
    const names = new Set((useWorkspace.getState().environments[c.id] ?? []).map((e) => e.name));
    useWorkspace.setState((s) => ({
      expanded: { ...s.expanded, [c.id]: ui.expanded },
      activeEnv: ui.env !== null && names.has(ui.env) ? { ...s.activeEnv, [c.id]: ui.env } : s.activeEnv,
    }));
    for (const item of ui.tabs) {
      try {
        await useTabs.getState().open(c.id, item);
      } catch {
        // The request no longer exists (renamed, deleted): drop it.
      }
    }
    const key = `${c.id}:${ui.active}`;
    if (ui.active !== null && !active && useTabs.getState().tabs.some((t) => t.key === key)) active = key;
  }
  if (active) useTabs.getState().activate(active);
}

function snapshot(): Record<string, UiStateDto> {
  const { collections, expanded, activeEnv } = useWorkspace.getState();
  const { tabs, active } = useTabs.getState();
  const out: Record<string, UiStateDto> = {};
  for (const c of collections) {
    if (!c.available) continue;
    const own = tabs.filter((t) => t.collectionId === c.id);
    out[c.id] = {
      tabs: own.map((t) => t.item),
      // Only the globally active tab is marked, so a restore re-selects it.
      active: own.find((t) => t.key === active)?.item ?? null,
      expanded: expanded[c.id] ?? [],
      env: activeEnv[c.id] ?? null,
    };
  }
  return out;
}

/** Saves layout changes (debounced, per collection). Returns a function that stops it. */
export function startUiPersistence(): () => void {
  const last = new Map(Object.entries(snapshot()).map(([id, ui]) => [id, JSON.stringify(ui)]));
  const timers = new Map<string, ReturnType<typeof setTimeout>>();
  const check = () => {
    for (const [id, ui] of Object.entries(snapshot())) {
      const text = JSON.stringify(ui);
      if (last.get(id) === text) continue;
      last.set(id, text);
      clearTimeout(timers.get(id));
      timers.set(
        id,
        setTimeout(() => {
          timers.delete(id);
          void api()
            .saveUiState(id, ui)
            .catch(() => undefined);
        }, SAVE_DELAY_MS),
      );
    }
  };
  const unsubscribe = [useWorkspace.subscribe(check), useTabs.subscribe(check)];
  return () => {
    unsubscribe.forEach((u) => u());
    timers.forEach((t) => clearTimeout(t));
  };
}
