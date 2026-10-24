import { create } from "zustand";
import { api } from "../api/instance";
import type { VarsDto } from "../api/types";
import { useRuns } from "./runs";
import { useTabs, type Tab } from "./tabs";
import { useWorkspace } from "./workspace";

/** Where each request tab's `{{variables}}` come from (`POST …/vars`), for underlines and hovers. */
export interface VarsState {
  byTab: Record<string, VarsDto | undefined>;
  refresh(tabKey: string): Promise<void>;
}

/** Only the latest answer per tab is kept. */
const latest = new Map<string, number>();
let calls = 0;

export const useVars = create<VarsState>()((set) => ({
  byTab: {},
  async refresh(tabKey) {
    const tab = useTabs.getState().tabs.find((t) => t.key === tabKey);
    if (!tab) return;
    const call = ++calls;
    latest.set(tabKey, call);
    const env = useWorkspace.getState().activeEnv[tab.collectionId] ?? null;
    try {
      const dto = await api().vars(tab.collectionId, { item: tab.item, draft: tab.draft, env });
      if (latest.get(tabKey) === call) set((s) => ({ byTab: { ...s.byTab, [tabKey]: dto } }));
    } catch {
      // Keep the last answer; the hover is a hint, not a gate.
    }
  },
}));

/** Debounce after editing a request's `vars:pre-request`. */
const VARS_PRE_DELAY = 300;

/**
 * Refreshes tabs when what `/vars` reports may have changed: a tab opened, a send
 * finished, a run of its collection ended, its environment was switched or saved, or
 * (debounced) its `vars:pre-request` was edited. Returns the unsubscribe.
 */
export function watchVars(): () => void {
  const timers = new Map<string, ReturnType<typeof setTimeout>>();
  const refresh = (key: string) => void useVars.getState().refresh(key);
  const refreshCollection = (id: string) =>
    useTabs.getState().tabs.filter((t) => t.collectionId === id).forEach((t) => refresh(t.key));

  const onTabs = (tabs: Tab[], before: Tab[]) => {
    const old = new Map(before.map((t) => [t.key, t]));
    for (const t of tabs) {
      const prev = old.get(t.key);
      if (!prev || (prev.sending && !t.sending)) refresh(t.key);
      else if (prev.draft.varsPre !== t.draft.varsPre) {
        clearTimeout(timers.get(t.key));
        timers.set(
          t.key,
          setTimeout(() => {
            timers.delete(t.key);
            refresh(t.key);
          }, VARS_PRE_DELAY),
        );
      }
    }
    const open = new Set(tabs.map((t) => t.key));
    const gone = before.filter((t) => !open.has(t.key)).map((t) => t.key);
    if (gone.length) {
      useVars.setState((s) => ({ byTab: Object.fromEntries(Object.entries(s.byTab).filter(([k]) => !gone.includes(k))) }));
    }
  };
  onTabs(useTabs.getState().tabs, []);
  const unTabs = useTabs.subscribe((s, prev) => {
    if (s.tabs !== prev.tabs) onTabs(s.tabs, prev.tabs);
  });
  const unWorkspace = useWorkspace.subscribe((s, prev) => {
    const ids = new Set([...Object.keys(s.activeEnv), ...Object.keys(s.environments)]);
    for (const id of ids) {
      if (s.activeEnv[id] !== prev.activeEnv[id] || s.environments[id] !== prev.environments[id]) refreshCollection(id);
    }
  });
  const unRuns = useRuns.subscribe((s, prev) => {
    for (const [id, run] of Object.entries(s.runs)) {
      if (run.status !== "running" && prev.runs[id]?.status === "running") refreshCollection(run.collectionId);
    }
  });
  return () => {
    unTabs();
    unWorkspace();
    unRuns();
    timers.forEach(clearTimeout);
  };
}
