import { create } from "zustand";
import { ApiError } from "../api/client";
import { api } from "../api/instance";
import type {
  CollectionSummaryDto,
  EnvironmentDto,
  KeyValueDto,
  TreeNodeDto,
} from "../api/types";

export interface WorkspaceState {
  collections: CollectionSummaryDto[];
  trees: Record<string, TreeNodeDto>;
  environments: Record<string, EnvironmentDto[]>;
  /** Selected environment per collection; null = no environment. */
  activeEnv: Record<string, string | null>;
  error?: string;
  init(): Promise<void>;
  open(path: string): Promise<void>;
  reload(id: string): Promise<void>;
  refreshTree(id: string): Promise<void>;
  selectEnv(id: string, name: string | null): void;
  saveEnvironment(id: string, name: string, vars: KeyValueDto[]): Promise<"saved" | "conflict">;
  setSecrets(id: string, name: string, values: Record<string, string>): Promise<void>;
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
        ? s.collections
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

    async init() {
      try {
        for (const summary of await api().listCollections()) await load(summary);
      } catch (e) {
        set({ error: message(e) });
      }
    },

    async open(path) {
      try {
        set({ error: undefined });
        await load(await api().openCollection(path));
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
      await api().setSecrets(id, name, values);
    },

    clearError() {
      set({ error: undefined });
    },
  };
});
