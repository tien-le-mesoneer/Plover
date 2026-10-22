import { create } from "zustand";
import { ApiError } from "../api/client";
import { api } from "../api/instance";
import type { ConflictDto, HttpRequestDto, ResponseDto } from "../api/types";
import { useWorkspace } from "./workspace";

export interface Tab {
  /** `collectionId + ':' + item` */
  key: string;
  collectionId: string;
  item: string;
  saved: HttpRequestDto;
  draft: HttpRequestDto;
  hash: string;
  response?: ResponseDto;
  sending: boolean;
  error?: string;
  /** Set when a save found the file changed on disk. */
  conflict?: ConflictDto;
}

export interface TabsState {
  tabs: Tab[];
  active: string | null;
  open(collectionId: string, item: string): Promise<void>;
  close(key: string): void;
  activate(key: string): void;
  edit(key: string, f: (d: HttpRequestDto) => HttpRequestDto): void;
  isDirty(key: string): boolean;
  save(key: string): Promise<"saved" | "conflict">;
  resolveConflict(key: string, choice: "reload" | "overwrite"): Promise<void>;
  send(key: string): Promise<void>;
}

export const tabKey = (collectionId: string, item: string) => `${collectionId}:${item}`;

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export const useTabs = create<TabsState>()((set, get) => {
  const update = (key: string, patch: Partial<Tab> | ((t: Tab) => Partial<Tab>)) =>
    set((s) => ({
      tabs: s.tabs.map((t) =>
        t.key === key ? { ...t, ...(typeof patch === "function" ? patch(t) : patch) } : t,
      ),
    }));
  const find = (key: string) => get().tabs.find((t) => t.key === key);

  async function write(tab: Tab, hash: string): Promise<"saved" | "conflict"> {
    try {
      const saved = await api().saveRequest(tab.collectionId, tab.item, tab.draft, hash);
      // Adopt what the file now says (e.g. trimmed values), so "saved" matches disk.
      update(tab.key, {
        saved: saved.request,
        draft: saved.request,
        hash: saved.hash,
        conflict: undefined,
        error: undefined,
      });
      void useWorkspace.getState().refreshTree(tab.collectionId).catch(() => {});
      return "saved";
    } catch (e) {
      if (e instanceof ApiError && e.code === "conflict") {
        const current = (e.body as { current?: ConflictDto }).current;
        update(tab.key, { conflict: current });
        return "conflict";
      }
      update(tab.key, { error: message(e) });
      throw e;
    }
  }

  return {
    tabs: [],
    active: null,

    async open(collectionId, item) {
      const key = tabKey(collectionId, item);
      if (find(key)) {
        set({ active: key });
        return;
      }
      const stored = await api().readRequest(collectionId, item);
      if (find(key)) {
        set({ active: key });
        return;
      }
      set((s) => ({
        tabs: [
          ...s.tabs,
          {
            key,
            collectionId,
            item,
            saved: stored.request,
            draft: stored.request,
            hash: stored.hash,
            sending: false,
          },
        ],
        active: key,
      }));
    },

    close(key) {
      set((s) => {
        const index = s.tabs.findIndex((t) => t.key === key);
        if (index < 0) return s;
        const tabs = s.tabs.filter((t) => t.key !== key);
        const active =
          s.active === key ? (tabs[index] ?? tabs[index - 1] ?? null)?.key ?? null : s.active;
        return { tabs, active };
      });
    },

    activate(key) {
      set({ active: key });
    },

    edit(key, f) {
      update(key, (t) => ({ draft: f(t.draft) }));
    },

    isDirty(key) {
      const tab = find(key);
      return !!tab && JSON.stringify(tab.saved) !== JSON.stringify(tab.draft);
    },

    async save(key) {
      const tab = find(key);
      if (!tab) return "saved";
      return write(tab, tab.hash);
    },

    async resolveConflict(key, choice) {
      const tab = find(key);
      if (!tab?.conflict) return;
      if (choice === "overwrite") {
        await write(tab, tab.conflict.hash);
        return;
      }
      const stored = await api().readRequest(tab.collectionId, tab.item);
      update(key, {
        saved: stored.request,
        draft: stored.request,
        hash: stored.hash,
        conflict: undefined,
      });
    },

    async send(key) {
      const tab = find(key);
      if (!tab) return;
      update(key, { sending: true, error: undefined });
      const env = useWorkspace.getState().activeEnv[tab.collectionId] ?? null;
      try {
        const response = await api().send(tab.collectionId, tab.item, tab.draft, env);
        update(key, { response, sending: false });
      } catch (e) {
        update(key, { sending: false, error: message(e), response: undefined });
      }
    },
  };
});
