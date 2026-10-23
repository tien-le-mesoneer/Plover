import { create } from "zustand";
import { ApiError } from "../api/client";
import { api } from "../api/instance";
import type {
  CapabilitiesDto,
  CollectionSummaryDto,
  EnvironmentDto,
  KeyValueDto,
  RecentDto,
  TreeNodeDto,
} from "../api/types";

export interface WorkspaceState {
  /** Open collections, then remembered ones that could not be reopened (`available: false`). */
  collections: CollectionSummaryDto[];
  trees: Record<string, TreeNodeDto>;
  environments: Record<string, EnvironmentDto[]>;
  /** Selected environment per collection; null = no environment. */
  activeEnv: Record<string, string | null>;
  /** `null` until loaded; web mode has no native features and memory-only secrets. */
  capabilities: CapabilitiesDto | null;
  /** The collection last opened (here or natively); used when no tab is active. */
  activeCollection: string | null;
  /** Expanded folder IDs per collection, in the order they were expanded. */
  expanded: Record<string, string[]>;
  /** Sidebar request filter (case-insensitive substring). */
  filter: string;
  recent: RecentDto[];
  /** The "Open collection" popover. */
  pickerOpen: boolean;
  error?: string;
  init(): Promise<void>;
  open(path: string): Promise<void>;
  /** Closes a collection (or drops an unavailable one); it stays in the recent list. */
  close(id: string): Promise<void>;
  /** Tries an unavailable collection's root again. */
  retry(id: string): Promise<void>;
  loadRecent(): Promise<void>;
  removeRecent(root: string): Promise<void>;
  toggleExpanded(id: string, folder: string, open?: boolean): void;
  setFilter(filter: string): void;
  setPickerOpen(open: boolean): void;
  reload(id: string): Promise<void>;
  refreshTree(id: string): Promise<void>;
  selectEnv(id: string, name: string | null): void;
  saveEnvironment(id: string, name: string, vars: KeyValueDto[]): Promise<"saved" | "conflict">;
  /**
   * Sends only the given names (`null` clears), then refreshes which secrets are set
   * (and where) and the capabilities. Resolves to warnings for values kept in memory.
   */
  setSecrets(id: string, name: string, values: Record<string, string | null>): Promise<string[]>;
  /** Opens this session in the default browser (desktop only). */
  openInBrowser(): Promise<void>;
  /** A collection the desktop shell opened: add and select it, keeping tabs and drafts. */
  openedElsewhere(id: string): Promise<void>;
  clearError(): void;
}

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export const useWorkspace = create<WorkspaceState>()((set, get) => {
  async function load(summary: CollectionSummaryDto) {
    const [tree, environments] = await Promise.all([
      api().tree(summary.id),
      api().environments(summary.id),
    ]);
    set((s) => ({
      collections: s.collections.some((c) => c.id === summary.id)
        ? s.collections.map((c) => (c.id === summary.id ? summary : c))
        : [...s.collections, summary],
      trees: { ...s.trees, [summary.id]: tree },
      environments: { ...s.environments, [summary.id]: environments },
      activeEnv:
        summary.id in s.activeEnv
          ? s.activeEnv
          : { ...s.activeEnv, [summary.id]: environments[0]?.name ?? null },
    }));
  }

  return {
    collections: [],
    trees: {},
    environments: {},
    activeEnv: {},
    capabilities: null,
    activeCollection: null,
    expanded: {},
    filter: "",
    recent: [],
    pickerOpen: false,

    async init() {
      try {
        set({ capabilities: await api().capabilities() });
      } catch (e) {
        set({ error: message(e) });
      }
      try {
        for (const summary of await api().listCollections()) {
          if (summary.available) await load(summary);
          else
            set((s) => ({
              collections: s.collections.some((c) => c.id === summary.id)
                ? s.collections
                : [...s.collections, summary],
            }));
        }
      } catch (e) {
        set({ error: message(e) });
      }
      await get().loadRecent();
    },

    async close(id) {
      try {
        await api().closeCollection(id);
      } catch (e) {
        set({ error: message(e) });
        return;
      }
      set((s) => {
        const without = <T,>(r: Record<string, T>) => Object.fromEntries(Object.entries(r).filter(([k]) => k !== id));
        return {
          collections: s.collections.filter((c) => c.id !== id),
          trees: without(s.trees),
          environments: without(s.environments),
          activeEnv: without(s.activeEnv),
          expanded: without(s.expanded),
          activeCollection: s.activeCollection === id ? null : s.activeCollection,
        };
      });
      await get().loadRecent();
    },

    async retry(id) {
      const entry = get().collections.find((c) => c.id === id);
      if (entry) await get().open(entry.root);
    },

    async loadRecent() {
      try {
        set({ recent: await api().recent() });
      } catch {
        // The recent list is a convenience; ignore failures.
      }
    },

    async removeRecent(root) {
      try {
        await api().removeRecent(root);
      } catch (e) {
        set({ error: message(e) });
      }
      await get().loadRecent();
    },

    toggleExpanded(id, folder, open) {
      set((s) => {
        const current = s.expanded[id] ?? [];
        const isOpen = current.includes(folder);
        const next = (open ?? !isOpen) ? (isOpen ? current : [...current, folder]) : current.filter((f) => f !== folder);
        return next === current ? s : { expanded: { ...s.expanded, [id]: next } };
      });
    },

    setFilter(filter) {
      set({ filter });
    },

    setPickerOpen(pickerOpen) {
      set({ pickerOpen });
    },

    async openInBrowser() {
      try {
        await api().openInBrowser();
      } catch (e) {
        set({ error: message(e) });
      }
    },

    async openedElsewhere(id) {
      try {
        const summary = (await api().listCollections()).find((c) => c.id === id);
        if (!summary) return;
        await load(summary);
        set({ activeCollection: id });
      } catch (e) {
        set({ error: message(e) });
      }
    },

    async open(path) {
      try {
        set({ error: undefined });
        const summary = await api().openCollection(path);
        await load(summary);
        set({ activeCollection: summary.id });
        void get().loadRecent();
      } catch (e) {
        set({ error: message(e) });
      }
    },

    async reload(id) {
      try {
        const [tree, environments] = await Promise.all([api().reload(id), api().environments(id)]);
        set((s) => ({
          trees: { ...s.trees, [id]: tree },
          environments: { ...s.environments, [id]: environments },
        }));
      } catch (e) {
        set({ error: message(e) });
      }
    },

    async refreshTree(id) {
      const tree = await api().tree(id);
      set((s) => ({ trees: { ...s.trees, [id]: tree } }));
    },

    selectEnv(id, name) {
      set((s) => ({ activeEnv: { ...s.activeEnv, [id]: name } }));
    },

    async saveEnvironment(id, name, vars) {
      const env = get().environments[id]?.find((e) => e.name === name);
      if (!env) return "conflict";
      try {
        await api().saveEnvironment(id, name, vars, env.hash);
        const environments = await api().environments(id);
        set((s) => ({ environments: { ...s.environments, [id]: environments } }));
        return "saved";
      } catch (e) {
        if (e instanceof ApiError && e.code === "conflict") {
          const environments = await api().environments(id);
          set((s) => ({
            environments: { ...s.environments, [id]: environments },
            error: `Environment "${name}" changed on disk; reloaded it. Re-apply your edits.`,
          }));
          return "conflict";
        }
        set({ error: message(e) });
        throw e;
      }
    },

    async setSecrets(id, name, values) {
      const { warnings } = await api().setSecrets(id, name, values);
      const [environments, capabilities] = await Promise.all([api().environments(id), api().capabilities()]);
      set((s) => ({ environments: { ...s.environments, [id]: environments }, capabilities }));
      return warnings;
    },

    clearError() {
      set({ error: undefined });
    },
  };
});
