// Remembered per-collection layout (open tabs, expanded folders, environment), kept by
// the server so it survives restarts — the desktop webview gets a new origin each launch,
// so browser storage would not. Unsaved drafts are not remembered.
import { api } from "../api/instance";
import type { UiStateDto } from "../api/types";
import { useTabs } from "./tabs";
import { useWorkspace } from "./workspace";

const SAVE_DELAY_MS = 500;

/** Applies one collection's remembered layout; resolves to its active tab key, if reopened. */
async function apply(id: string, ui: UiStateDto): Promise<string | null> {
  const names = new Set((useWorkspace.getState().environments[id] ?? []).map((e) => e.name));
  useWorkspace.setState((s) => ({
    expanded: { ...s.expanded, [id]: ui.expanded },
    collapsed: { ...s.collapsed, [id]: ui.collapsed },
    activeEnv: ui.env !== null && names.has(ui.env) ? { ...s.activeEnv, [id]: ui.env } : s.activeEnv,
  }));
  for (const item of ui.tabs) {
    try {
      await useTabs.getState().open(id, item);
    } catch {
      // The request no longer exists (renamed, deleted): drop it.
    }
  }
  const key = `${id}:${ui.active}`;
  return ui.active !== null && useTabs.getState().tabs.some((t) => t.key === key) ? key : null;
}

async function remembered(): Promise<Record<string, UiStateDto>> {
  try {
    return await api().uiState();
  } catch {
    return {};
  }
}

/** Reopens remembered tabs (missing requests are dropped), folders and environments. */
export async function restoreUiState(): Promise<void> {
  const saved = await remembered();
  let active: string | null = null;
  for (const c of useWorkspace.getState().collections) {
    const ui = saved[c.id];
    if (!c.available || !ui) continue;
    const key = await apply(c.id, ui);
    active ??= key;
  }
  if (active) useTabs.getState().activate(active);
}

function snapshot(): Record<string, UiStateDto> {
  const { collections, expanded, collapsed, activeEnv } = useWorkspace.getState();
  const { tabs, active } = useTabs.getState();
  const out: Record<string, UiStateDto> = {};
  for (const c of collections) {
    if (!c.available) continue;
    // Scratch tabs are never remembered: a pasted token must not reach disk.
    const own = tabs.filter(
      (t): t is typeof t & { item: string } => t.collectionId === c.id && t.item !== null,
    );
    out[c.id] = {
      tabs: own.map((t) => t.item),
      // Only the globally active tab is marked, so a restore re-selects it.
      active: own.find((t) => t.key === active)?.item ?? null,
      expanded: expanded[c.id] ?? [],
      env: activeEnv[c.id] ?? null,
      collapsed: collapsed[c.id] ?? false,
    };
  }
  return out;
}

/**
 * Saves layout changes (debounced, per collection). A collection that becomes available
 * later (opened, reopened from Recent, retried) first gets its remembered layout back, so
 * an empty layout never overwrites it. Returns a function that stops saving.
 */
export function startUiPersistence(): () => void {
  const last = new Map(Object.entries(snapshot()).map(([id, ui]) => [id, JSON.stringify(ui)]));
  const timers = new Map<string, ReturnType<typeof setTimeout>>();
  const restoring = new Set<string>();
  let stopped = false;

  const check = () => {
    for (const [id, ui] of Object.entries(snapshot())) {
      if (restoring.has(id)) continue;
      if (!last.has(id)) {
        restoring.add(id);
        void remembered()
          .then((saved) => (saved[id] ? apply(id, saved[id]) : null))
          .then((key) => {
            if (key) useTabs.getState().activate(key);
          })
          .finally(() => {
            restoring.delete(id);
            const now = snapshot()[id];
            if (now) last.set(id, JSON.stringify(now));
          });
        continue;
      }
      const text = JSON.stringify(ui);
      if (last.get(id) === text) continue;
      last.set(id, text);
      clearTimeout(timers.get(id));
      timers.set(
        id,
        setTimeout(() => {
          timers.delete(id);
          if (stopped) return;
          void api()
            .saveUiState(id, ui)
            .catch(() => undefined);
        }, SAVE_DELAY_MS),
      );
    }
    // A closed collection is forgotten here, so reopening it restores its layout again.
    for (const id of [...last.keys()]) if (!(id in snapshot())) last.delete(id);
  };
  const unsubscribe = [useWorkspace.subscribe(check), useTabs.subscribe(check)];
  return () => {
    stopped = true;
    unsubscribe.forEach((u) => u());
    timers.forEach((t) => clearTimeout(t));
  };
}
