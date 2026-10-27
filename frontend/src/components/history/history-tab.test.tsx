import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../../api/client";
import { setApi } from "../../api/instance";
import type { RunResultDetailDto, SendMetaDto, SendRecordDto } from "../../api/types";
import { useHistory } from "../../store/history";
import { HISTORY_BANNER, useTabs } from "../../store/tabs";
import { snapshotForTest } from "../../store/uiState";
import { useWorkspace } from "../../store/workspace";
import { RequestTabs } from "../RequestTabs";
import { SidebarHeader } from "../SidebarHeader";
import { HistoryTab } from "./HistoryTab";

vi.mock("../../editor/CodeEditor", () => ({ CodeEditor: () => null }));

const meta = (id: string, over: Partial<SendMetaDto> = {}): SendMetaDto => ({
  id,
  sentAtMs: 1_790_000_000_000,
  collectionId: "c1",
  collectionName: "Demo",
  item: "users/me.bru",
  env: "dev",
  method: "GET",
  name: "me",
  url: "https://api.example.test/me",
  status: "pass",
  httpStatus: 200,
  durationMs: 42,
  ...over,
});

const detail: RunResultDetailDto = {
  index: 0,
  item: "users/me.bru",
  name: "me",
  status: "pass",
  error: null,
  skipReason: null,
  request: {
    method: "POST",
    url: "https://api.example.test/me?x=1",
    headers: [
      ["Authorization", "Bearer ***"],
      ["Accept", "application/json"],
    ],
    body: { kind: "text", text: '{"a":1}', base64: null, fields: [], parts: [], size: 7, truncated: false, omitted: null },
  },
  responseBody: null,
  result: {
    response: { status: 200, headers: [["content-type", "application/json"]], elapsedMs: 42, size: 7, sizeComplete: true, bodyText: '{"ok":1}', bodyBase64: null, truncated: false },
    tests: [],
    asserts: [],
    console: [],
    scriptError: null,
    scriptedRequests: [],
    skipped: null,
  },
};

let client: Record<string, ReturnType<typeof vi.fn>>;
const list = [meta("20261027T100000Z-aaaaaa", { name: "newest" }), meta("20261027T090000Z-bbbbbb", { collectionId: "c2", collectionName: "Other", item: null, name: "scratch" })];

beforeEach(() => {
  client = {
    sends: vi.fn(async (collection?: string | null) => (collection ? list.filter((m) => m.collectionId === collection) : list)),
    sendRecord: vi.fn(async (id: string): Promise<SendRecordDto> => ({ meta: list.find((m) => m.id === id)!, detail })),
    clearSends: vi.fn(async () => ({ removed: 2 })),
    readRequest: vi.fn(async () => ({ request: { name: "me", method: "GET" }, hash: "h" })),
    send: vi.fn(async () => ({ response: null, tests: [], asserts: [], console: [], scriptError: null, scriptedRequests: [], skipped: null })),
  };
  setApi(client as unknown as ApiClient);
  useWorkspace.setState({
    collections: [
      { id: "c1", name: "Demo", root: "/x", available: true, error: null },
      { id: "c2", name: "Other", root: "/y", available: true, error: null },
    ],
    activeEnv: {},
    activeCollection: "c1",
  });
  useTabs.setState({ tabs: [], runTabs: [], historyTab: null, active: null });
  useHistory.setState({ list: [], filter: null, selected: null, record: null });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const pastSends = () => screen.getByRole("listbox", { name: "Past sends" });
const rows = () => within(pastSends()).findAllByRole("option");

async function openHistory() {
  render(
    <>
      <SidebarHeader />
      <RequestTabs />
      <HistoryTab />
    </>,
  );
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "History" }));
  });
}

describe("history tab", () => {
  it("history_button_opens_single_tab", async () => {
    await openHistory();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "History" }));
    });
    expect(screen.getAllByRole("tab", { name: "History" })).toHaveLength(1);
    expect(useTabs.getState().active).toBe("history");
    expect(JSON.stringify(snapshotForTest())).not.toContain("history");
  });

  it("list_shows_newest_first_with_collection_filter", async () => {
    await openHistory();
    const options = await rows();
    expect(options.map((o) => o.textContent)).toEqual([expect.stringContaining("newest"), expect.stringContaining("scratch")]);
    await act(async () => {
      fireEvent.change(screen.getByRole("combobox", { name: "Collection" }), { target: { value: "c2" } });
    });
    expect(client.sends).toHaveBeenLastCalledWith("c2");
    expect(within(pastSends()).getAllByRole("option")).toHaveLength(1);
  });

  it("selecting_row_shows_request_and_response_read_only", async () => {
    await openHistory();
    const first = (await rows())[0]!;
    await act(async () => {
      fireEvent.click(first);
    });
    expect(first.getAttribute("aria-selected")).toBe("true");
    const panel = screen.getByRole("region", { name: "Send detail" });
    expect(within(panel).getByText("200 OK")).toBeTruthy();
    expect(within(panel).queryByLabelText("URL")).toBeNull();
    for (const box of within(panel).queryAllByRole("textbox")) expect(box.getAttribute("aria-label")).toBe("Filter with JSONPath");
  });

  it("open_request_disabled_for_scratch_or_missing_item", async () => {
    await openHistory();
    const options = await rows();
    await act(async () => {
      fireEvent.click(options[1]!);
    });
    const open = screen.getByRole("button", { name: "Open request" });
    expect(open.hasAttribute("disabled")).toBe(true);
    expect(open.getAttribute("title")).toBe("A scratch request has no file");
    await act(async () => {
      fireEvent.click(options[0]!);
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Open request" }));
    });
    expect(client.readRequest).toHaveBeenCalledWith("c1", "users/me.bru");
  });

  it("open_as_new_request_creates_scratch_with_banner", async () => {
    await openHistory();
    await act(async () => {
      fireEvent.click((await rows())[0]!);
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Open as new request" }));
    });
    const tab = useTabs.getState().tabs.at(-1)!;
    expect(tab.item).toBeNull();
    expect(tab.collectionId).toBe("c1");
    expect(tab.draft.method).toBe("POST");
    expect(tab.draft.url).toBe("https://api.example.test/me?x=1");
    expect(tab.draft.query).toEqual([{ name: "x", value: "1", enabled: true, local: false }]);
    expect(tab.draft.headers.map((h) => [h.name, h.value])).toEqual([
      ["Authorization", "Bearer ***"],
      ["Accept", "application/json"],
    ]);
    expect(tab.draft.body).toEqual({ mode: "text", text: '{"a":1}' });
    expect(tab.banner).toBe(HISTORY_BANNER);
    expect(HISTORY_BANNER).toBe("Opened from history. Values that were masked show as ***; replace them before sending.");
    expect(useTabs.getState().active).toBe(tab.key);
  });

  it("clear_asks_then_clears_filtered_scope", async () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValueOnce(false).mockReturnValueOnce(true);
    await openHistory();
    await rows();
    await act(async () => {
      fireEvent.change(screen.getByRole("combobox", { name: "Collection" }), { target: { value: "c1" } });
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Clear history" }));
    });
    expect(confirm).toHaveBeenCalledWith("Clear 1 send of Demo from history?");
    expect(client.clearSends).not.toHaveBeenCalled();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Clear history" }));
    });
    expect(client.clearSends).toHaveBeenCalledWith("c1");
  });

  it("send_refreshes_history", async () => {
    const key = useTabs.getState().newScratch("c1");
    const load = vi.spyOn(useHistory.getState(), "load");
    await act(async () => {
      await useTabs.getState().send(key);
    });
    await waitFor(() => expect(load).toHaveBeenCalled());
  });

  it("a11y", async () => {
    await openHistory();
    const listbox = screen.getByRole("listbox", { name: "Past sends" });
    const options = await within(listbox).findAllByRole("option");
    options[0]!.focus();
    fireEvent.keyDown(options[0]!, { key: "ArrowDown" });
    expect(document.activeElement).toBe(options[1]);
    await act(async () => {
      fireEvent.keyDown(options[1]!, { key: "Enter" });
    });
    expect(options[1]!.getAttribute("aria-selected")).toBe("true");
    fireEvent.keyDown(options[1]!, { key: "ArrowUp" });
    expect(document.activeElement).toBe(options[0]);
    for (const b of screen.getAllByRole("button")) {
      expect(b.getAttribute("aria-label") ?? b.textContent).toBeTruthy();
    }
  });
});
