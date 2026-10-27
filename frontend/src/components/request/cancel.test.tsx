import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../../api/client";
import { setApi } from "../../api/instance";
import type { HttpRequestDto, TreeNodeDto } from "../../api/types";
import App from "../../App";
import { useTabs } from "../../store/tabs";
import { useWorkspace } from "../../store/workspace";

vi.mock("../../editor/CodeEditor", () => ({ CodeEditor: () => null }));
vi.mock("../../store/startup", () => ({ startUp: async () => undefined }));

const tree: TreeNodeDto = { kind: "folder", id: "", name: "Demo", method: null, seq: null, error: null, children: [] };
const request = {
  name: "slow",
  seq: 1,
  kind: "http",
  method: "GET",
  url: "http://example.test/slow",
  query: [],
  pathParams: [],
  headers: [],
  body: { mode: "none" },
  otherBodies: [],
  auth: { mode: "none" },
  varsPre: [],
  varsPost: [],
  scriptPre: null,
  scriptPost: null,
  tests: null,
  asserts: [],
  settings: { followRedirects: null, maxRedirects: null, timeoutMs: null },
} as HttpRequestDto;

let client: Record<string, ReturnType<typeof vi.fn>>;

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setInterval", "clearInterval", "setTimeout", "clearTimeout", "Date"] });
  client = {
    tree: vi.fn(async () => tree),
    subscribeEvents: vi.fn(() => ({ close() {} })),
    send: vi.fn(
      (_i: string, _it: string, _d: unknown, _e: unknown, opts: { signal: AbortSignal }) =>
        new Promise((_, reject) =>
          opts.signal.addEventListener("abort", () => reject(new DOMException("aborted", "AbortError"))),
        ),
    ),
    cancelSend: vi.fn(async () => {}),
    vars: vi.fn(async () => ({ vars: [], setLater: [] })),
  };
  setApi(client as unknown as ApiClient);
  useWorkspace.setState({
    collections: [{ id: "c1", name: "Demo", root: "/x", available: true, error: null }],
    trees: { c1: tree },
    environments: {},
    activeEnv: {},
    activeCollection: "c1",
    capabilities: null,
  });
  useTabs.setState({
    tabs: [{ key: "c1:slow.bru", collectionId: "c1", item: "slow.bru", saved: request, draft: request, hash: "h", sending: false }],
    runTabs: [],
    active: "c1:slow.bru",
    saveDialog: null,
  });
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

async function startSend() {
  render(<App hasToken />);
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: /^Send/ }));
  });
}

describe("cancel a send", () => {
  it("button_shows_cancel_with_timer", async () => {
    await startSend();
    await act(async () => {
      vi.advanceTimersByTime(3200);
    });
    const button = screen.getByRole("button", { name: "Cancel request" });
    expect(button.textContent).toBe("Cancel · 3.2s");
    expect((button as HTMLButtonElement).disabled).toBe(false);
  });

  it("click_cancel_shows_banner", async () => {
    await startSend();
    await act(async () => {
      vi.advanceTimersByTime(1500);
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Cancel request" }));
    });
    expect(client.cancelSend).toHaveBeenCalled();
    expect(screen.getByText("Cancelled after 1.5s")).toBeTruthy();
    expect(screen.getByRole("button", { name: /^Send/ })).toBeTruthy();
  });

  it("esc_cancels_active_send", async () => {
    await startSend();
    await act(async () => {
      fireEvent.keyDown(window, { key: "Escape" });
    });
    expect(client.cancelSend).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("button", { name: "Cancel request" })).toBeNull();
  });

  it("esc_does_nothing_with_modal_open", async () => {
    await startSend();
    const dialog = document.createElement("div");
    dialog.setAttribute("aria-modal", "true");
    document.body.appendChild(dialog);
    await act(async () => {
      fireEvent.keyDown(window, { key: "Escape" });
    });
    dialog.remove();
    expect(client.cancelSend).not.toHaveBeenCalled();
  });

  it("esc_ignored_when_default_prevented", async () => {
    await startSend();
    const event = new KeyboardEvent("keydown", { key: "Escape", cancelable: true });
    event.preventDefault();
    await act(async () => {
      window.dispatchEvent(event);
    });
    expect(client.cancelSend).not.toHaveBeenCalled();
  });

  it("cancel_is_not_send_colour", async () => {
    await startSend();
    const button = screen.getByRole("button", { name: "Cancel request" });
    expect(button.classList.contains("send")).toBe(false);
  });

  it("send_and_cancel_share_slot", async () => {
    await startSend();
    const cancel = screen.getByRole("button", { name: "Cancel request" });
    expect(cancel.parentElement?.className).toBe("send-slot");
    expect(cancel.parentElement?.children).toHaveLength(1);
    const css = readFileSync(resolve(__dirname, "../../styles.css"), "utf8");
    expect(css).toMatch(/\.send-slot\s*\{[^}]*inline-size:\s*9\.5em/);
  });
});
