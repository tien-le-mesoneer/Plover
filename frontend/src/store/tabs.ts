import { create } from "zustand";
import { ApiError } from "../api/client";
import { api } from "../api/instance";
import type { ConflictDto, HttpRequestDto, SendResultDto } from "../api/types";
import { blankRequest } from "../lib/request";
import { useWorkspace } from "./workspace";

export interface Tab {
  /** `collectionId + ':' + item`, or `collectionId + ':scratch:' + n` for a scratch tab. */
  key: string;
  collectionId: string;
  /** The request file; `null` for a scratch tab, which has no file until it is saved. */
  item: string | null;
  saved: HttpRequestDto;
  draft: HttpRequestDto;
  hash: string;
  /** The last send: response (if any), tests, asserts, console output, script error. */
  result?: SendResultDto;
  sending: boolean;
  error?: string;
  /** Set when a save found the file changed on disk. */
  conflict?: ConflictDto;
}

export interface TabsState {
  tabs: Tab[];
  active: string | null;
  /** The scratch tab whose "Save request" dialog is open. */
  saveDialog: string | null;
  /** Save, or for a scratch tab open the dialog that picks its folder and name. */
  requestSave(key: string): void;
  closeSaveDialog(): void;
  open(collectionId: string, item: string): Promise<void>;
  close(key: string): void;
  activate(key: string): void;
  edit(key: string, f: (d: HttpRequestDto) => HttpRequestDto): void;
  isDirty(key: string): boolean;
  save(key: string): Promise<"saved" | "conflict">;
  resolveConflict(key: string, choice: "reload" | "overwrite"): Promise<void>;
  send(key: string): Promise<void>;
  /** Opens an unsaved "Untitled" request in `collectionId`; returns its key. */
  newScratch(collectionId: string): string;
  /** Writes a scratch tab to a new file and turns it into that file's tab. */
  saveScratch(key: string, folder: string, name: string): Promise<"saved" | "exists">;
  /** Fills method, URL, query, headers, body and auth from a pasted curl; returns the draft before. */
  applyCurl(key: string, parsed: HttpRequestDto): HttpRequestDto;
  /** Some scratch tab has content that would be lost. */
  hasScratchContent(): boolean;
}

export const tabKey = (collectionId: string, item: string) => `${collectionId}:${item}`;

let scratchCount = 0;

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
    if (tab.item === null) throw new Error("a scratch request is saved with saveScratch");
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
    saveDialog: null,

    requestSave(key) {
      const tab = find(key);
      if (!tab) return;
      if (tab.item === null) set({ saveDialog: key });
      else void get().save(key).catch(() => {});
    },

    closeSaveDialog() {
      set({ saveDialog: null });
    },

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
      if (!tab?.conflict || tab.item === null) return;
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
        const result = await api().send(tab.collectionId, tab.item, tab.draft, env);
        update(key, { result, sending: false });
      } catch (e) {
        update(key, { sending: false, error: message(e), result: undefined });
      }
    },
  
    newScratch(collectionId) {
      scratchCount += 1;
      const key = `${collectionId}:scratch:${scratchCount}`;
      const request = blankRequest("Untitled");
      set((s) => ({
        tabs: [
          ...s.tabs,
          { key, collectionId, item: null, saved: request, draft: request, hash: "", sending: false },
        ],
        active: key,
      }));
      return key;
    },

    async saveScratch(key, folder, name) {
      const tab = find(key);
      if (!tab || tab.item !== null) return "saved";
      let created;
      try {
        created = await api().createRequest(tab.collectionId, folder, name, tab.draft);
      } catch (e) {
        if (e instanceof ApiError && e.code === "exists") return "exists";
        throw e;
      }
      const fileKey = tabKey(tab.collectionId, created.item);
      set((s) => ({
        tabs: s.tabs
          .filter((t) => t.key !== fileKey)
          .map((t) =>
            t.key === key
              ? {
                  ...t,
                  key: fileKey,
                  item: created.item,
                  saved: created.request,
                  draft: created.request,
                  hash: created.hash,
                  error: undefined,
                }
              : t,
          ),
        active: s.active === key ? fileKey : s.active,
      }));
      void useWorkspace.getState().refreshTree(tab.collectionId).catch(() => {});
      return "saved";
    },

    applyCurl(key, parsed) {
      const tab = find(key);
      if (!tab) return parsed;
      const previous = tab.draft;
      update(key, {
        draft: {
          ...previous,
          method: parsed.method,
          url: parsed.url,
          query: parsed.query,
          headers: parsed.headers,
          body: parsed.body,
          auth: parsed.auth,
        },
      });
      return previous;
    },

    hasScratchContent() {
      return get().tabs.some((t) => t.item === null && get().isDirty(t.key));
    },
  };
});
