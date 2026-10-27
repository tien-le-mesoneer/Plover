import { create } from "zustand";
import { api } from "../api/instance";
import type { SendMetaDto, SendRecordDto } from "../api/types";

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** The History tab: past UI sends (masked by the server), newest first. */
export interface HistoryState {
  list: SendMetaDto[];
  /** Collection ID shown; `null` = all collections. */
  filter: string | null;
  selected: string | null;
  record: SendRecordDto | null;
  error?: string;
  load(): Promise<void>;
  setFilter(collectionId: string | null): Promise<void>;
  select(id: string): Promise<void>;
  /** Deletes the sends of the current filter; resolves to how many. */
  clear(): Promise<number>;
}

/** Only the latest list answer is kept. */
let calls = 0;

export const useHistory = create<HistoryState>()((set, get) => ({
  list: [],
  filter: null,
  selected: null,
  record: null,
  async load() {
    const call = ++calls;
    try {
      const list = await api().sends(get().filter);
      if (call === calls) set({ list, error: undefined });
    } catch (e) {
      if (call === calls) set({ error: message(e) });
    }
  },
  async setFilter(filter) {
    set({ filter, selected: null, record: null });
    await get().load();
  },
  async select(id) {
    set({ selected: id, record: null, error: undefined });
    try {
      const record = await api().sendRecord(id);
      if (get().selected === id) set({ record });
    } catch (e) {
      if (get().selected === id) set({ error: message(e) });
    }
  },
  async clear() {
    const { removed } = await api().clearSends(get().filter);
    set({ selected: null, record: null });
    await get().load();
    return removed;
  },
}));
