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
  /** The running send's ID (for `cancelSend`) and when it started (`Date.now()`). */
  sendId?: string;
  sendStartedAt?: number;
  /** Set when the last send was cancelled: how long it had run. */
  cancelledAfterMs?: number;
  error?: string;
  /** Set when a save found the file changed on disk. */
  conflict?: ConflictDto;
}

/** A collection or folder run shown in its own tab (never remembered across restarts). */
export interface RunTab {
  kind: "run";
  /** `collectionId + ':run:' + n`. */
  key: string;
  collectionId: string;
  /** The run shown (a live run ID or a saved run's record ID); `null` before any run. */
  runId: string | null;
  label: string;
}

export interface TabsState {
  tabs: Tab[];
  /** Run tabs, after the request tabs in the tab strip; they share `active`. */
  runTabs: RunTab[];
  active: string | null;
  /** Request tab keys opened during this page life, newest first (quick open). */
  recent: string[];
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
  /** Stops the tab's running send on the server and drops its result. */
  cancel(key: string): Promise<void>;
  /** Opens an unsaved "Untitled" request in `collectionId`; returns its key. */
  newScratch(collectionId: string): string;
  /** Writes a scratch tab to a new file and turns it into that file's tab. */
  saveScratch(key: string, folder: string, name: string): Promise<"saved" | "exists">;
  /** Fills method, URL, query, headers, body and auth from a pasted curl; returns the draft before. */
  applyCurl(key: string, parsed: HttpRequestDto): HttpRequestDto;
  /** Some scratch tab has content that would be lost. */
  hasScratchContent(): boolean;
  /** Opens a tab showing `runId` (or an empty one for the collection's runs) and focuses it. */
  openRun(collectionId: string, runId: string | null, label: string): string;
  /** Shows another run in an existing run tab (Re-run, a previous run). */
  retargetRun(key: string, runId: string, label?: string): void;
}

export const tabKey = (collectionId: string, item: string) => `${collectionId}:${item}`;

let scratchCount = 0;
let runTabCount = 0;

/** Fetches of running sends, by send ID, so `cancel` can abort them. */
const inFlight = new Map<string, AbortController>();

function newSendId(): string {
  return globalThis.crypto?.randomUUID?.() ?? `s-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}

const isCancel = (e: unknown) =>
  (e instanceof ApiError && e.code === "cancelled") || (e instanceof DOMException && e.name === "AbortError");

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
    runTabs: [],
    active: null,
    recent: [],
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
      set((s) => ({ recent: [key, ...s.recent.filter((k) => k !== key)].slice(0, 50) }));
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
        const all = [...s.tabs.map((t) => t.key), ...s.runTabs.map((t) => t.key)];
        const index = all.indexOf(key);
        if (index < 0) return s;
        const rest = all.filter((k) => k !== key);
        const active = s.active === key ? (rest[index] ?? rest[index - 1] ?? null) : s.active;
        return {
          tabs: s.tabs.filter((t) => t.key !== key),
          runTabs: s.runTabs.filter((t) => t.key !== key),
          active,
        };
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
      const sendId = newSendId();
      const controller = new AbortController();
      inFlight.set(sendId, controller);
      const startedAt = Date.now();
      update(key, { sending: true, sendId, sendStartedAt: startedAt, cancelledAfterMs: undefined, error: undefined });
      const env = useWorkspace.getState().activeEnv[tab.collectionId] ?? null;
      // A cancel (or a newer send) already settled this tab: its answer is not shown.
      const current = () => find(key)?.sendId === sendId;
      const done = { sending: false, sendId: undefined, sendStartedAt: undefined };
      try {
        const result = await api().send(tab.collectionId, tab.item, tab.draft, env, {
          sendId,
          signal: controller.signal,
        });
        if (current()) update(key, { ...done, result });
      } catch (e) {
        if (!current()) return;
        if (isCancel(e)) update(key, { ...done, result: undefined, cancelledAfterMs: Date.now() - startedAt });
        else update(key, { ...done, error: message(e), result: undefined });
      } finally {
        inFlight.delete(sendId);
      }
    },

    async cancel(key) {
      const tab = find(key);
      const sendId = tab?.sendId;
      if (!tab?.sending || !sendId) return;
      try {
        await api().cancelSend(tab.collectionId, sendId);
      } catch {
        // The fetch is aborted below either way.
      }
      // The send may have finished while the cancel was on its way: keep that result.
      const now = find(key);
      if (now?.sendId !== sendId) return;
      inFlight.get(sendId)?.abort();
      update(key, {
        sending: false,
        sendId: undefined,
        sendStartedAt: undefined,
        result: undefined,
        cancelledAfterMs: Date.now() - (now.sendStartedAt ?? Date.now()),
      });
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

    openRun(collectionId, runId, label) {
      const existing = get().runTabs.find((t) => t.collectionId === collectionId && t.runId === runId);
      if (existing) {
        set({ active: existing.key });
        return existing.key;
      }
      runTabCount += 1;
      const key = `${collectionId}:run:${runTabCount}`;
      set((s) => ({ runTabs: [...s.runTabs, { kind: "run", key, collectionId, runId, label }], active: key }));
      return key;
    },

    retargetRun(key, runId, label) {
      set((s) => ({
        runTabs: s.runTabs.map((t) => (t.key === key ? { ...t, runId, label: label ?? t.label } : t)),
      }));
    },
  };
});
