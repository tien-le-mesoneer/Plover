import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../../api/client";
import { setApi } from "../../api/instance";
import { blankRequest } from "../../lib/request";
import { useTabs } from "../../store/tabs";
import { useWorkspace } from "../../store/workspace";
import { RequestTabs } from "../RequestTabs";
import { RequestEditor } from "./RequestEditor";
import { RequestMenu } from "./RequestMenu";

vi.mock("../../editor/CodeEditor", () => ({ CodeEditor: () => null }));

let client: Record<string, ReturnType<typeof vi.fn>>;
let writeText: ReturnType<typeof vi.fn>;

beforeEach(() => {
  client = {
    copyCurl: vi.fn(async () => ({ text: "curl -X GET 'x'", containsSecret: false, omitted: [] })),
    resolvedUrl: vi.fn(async () => ({ url: "https://example.test/a?k=***", masked: true })),
    reveal: vi.fn(async () => undefined),
  };
  setApi(client as unknown as ApiClient);
  writeText = vi.fn(async () => undefined);
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  useWorkspace.setState({
    activeEnv: { c1: "local" },
    collections: [{ id: "c1", name: "Demo", root: "/work/demo", available: true, error: null }],
    capabilities: null,
    environments: {},
  });
  useTabs.setState({
    tabs: [
      {
        key: "c1:a.bru",
        collectionId: "c1",
        item: "a.bru",
        saved: blankRequest("a"),
        draft: { ...blankRequest("a"), url: "{{host}}/a" },
        hash: "h",
        sending: false,
      },
    ],
    active: "c1:a.bru",
  });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const openMenu = () => fireEvent.click(screen.getByRole("button", { name: "More request actions" }));
const items = () => screen.getAllByRole("menuitem").map((m) => m.textContent);

describe("request actions menu", () => {
  it("only_send_is_filled", () => {
    const tab = useTabs.getState().tabs[0]!;
    const { container } = render(<RequestEditor tab={tab} />);
    expect(container.querySelectorAll(".send")).toHaveLength(1);
    expect(screen.getByRole("button", { name: "Send" }).className).toBe("send");
    // The Send colour is used by the Send rules only.
    const css = readFileSync(resolve(process.cwd(), "src/styles.css"), "utf8");
    const rules = css.split("}").filter((r) => r.includes("var(--color-send"));
    expect(rules.length).toBeGreaterThan(0);
    for (const rule of rules) expect(rule.slice(0, rule.indexOf("{"))).toMatch(/\.send\b/);
  });

  it("menu_order_and_keyboard", () => {
    render(<RequestMenu tabKey="c1:a.bru" />);
    const button = screen.getByRole("button", { name: "More request actions" });
    expect(button.getAttribute("aria-haspopup")).toBe("menu");
    openMenu();
    expect(items()).toEqual([
      "Copy as cURL",
      "Copy as cURL (variables filled in)",
      "Copy resolved URL",
      "Save⌘S",
      "Copy file path",
    ]);
    const entries = screen.getAllByRole("menuitem");
    expect(document.activeElement).toBe(entries[0]);
    fireEvent.keyDown(entries[0]!, { key: "ArrowDown" });
    expect(document.activeElement).toBe(entries[1]);
    fireEvent.keyDown(entries[1]!, { key: "End" });
    expect(document.activeElement).toBe(entries[4]);
    fireEvent.keyDown(entries[4]!, { key: "ArrowDown" });
    expect(document.activeElement).toBe(entries[0]);
    fireEvent.keyDown(entries[0]!, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(document.activeElement).toBe(button);
  });

  it("copy as written writes text to the clipboard", async () => {
    render(<RequestMenu tabKey="c1:a.bru" />);
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy as cURL" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("curl -X GET 'x'"));
    expect(client.copyCurl).toHaveBeenCalledWith("c1", {
      item: "a.bru",
      draft: expect.objectContaining({ url: "{{host}}/a" }),
      env: "local",
      resolve: false,
    });
    expect(await screen.findByText("Copied")).toBeTruthy();
  });

  it("filled in with a secret asks before copying", async () => {
    client.copyCurl!.mockResolvedValue({ text: "curl secret", containsSecret: true, omitted: [] });
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    render(<RequestMenu tabKey="c1:a.bru" />);
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy as cURL (variables filled in)" }));
    await waitFor(() => expect(confirm).toHaveBeenCalledWith("This command contains secret values. Copy anyway?"));
    expect(client.copyCurl).toHaveBeenCalledWith("c1", expect.objectContaining({ resolve: true }));
    expect(writeText).not.toHaveBeenCalled();
  });

  it("omitted parts are named", async () => {
    client.copyCurl!.mockResolvedValue({ text: "curl x", containsSecret: false, omitted: ["auth mode `oauth2`"] });
    render(<RequestMenu tabKey="c1:a.bru" />);
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy as cURL" }));
    expect(await screen.findByText("Copied without auth mode `oauth2`")).toBeTruthy();
  });

  it("copy_resolved_url_writes_masked_text", async () => {
    render(<RequestMenu tabKey="c1:a.bru" />);
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy resolved URL" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("https://example.test/a?k=***"));
    expect(client.resolvedUrl).toHaveBeenCalledWith("c1", expect.objectContaining({ item: "a.bru", env: "local" }));
    expect(await screen.findByText("Copied (secret values as ***)")).toBeTruthy();
  });

  it("save_item_saves", () => {
    const requestSave = vi.fn();
    useTabs.setState({ requestSave });
    render(<RequestMenu tabKey="c1:a.bru" />);
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: /^Save/ }));
    expect(requestSave).toHaveBeenCalledWith("c1:a.bru");
  });

  it("copy file path in a browser, reveal in the desktop app", async () => {
    const { unmount } = render(<RequestMenu tabKey="c1:a.bru" />);
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy file path" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("/work/demo/a.bru"));
    unmount();
    useWorkspace.setState({
      capabilities: { desktop: true, nativePicker: true, openExternal: true, nativeSave: true, reveal: true, secretBackend: "keychain" },
    });
    render(<RequestMenu tabKey="c1:a.bru" />);
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Reveal .bru file" }));
    await waitFor(() => expect(client.reveal).toHaveBeenCalledWith("c1", "a.bru"));
  });

  it("scratch_menu_hides_file_items", async () => {
    const key = useTabs.getState().newScratch("c1");
    useTabs.getState().edit(key, (d) => ({ ...d, url: "http://x" }));
    render(<RequestMenu tabKey={key} />);
    openMenu();
    expect(items()).not.toContain("Copy file path");
    expect(items()).not.toContain("Reveal .bru file");
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy as cURL" }));
    await waitFor(() =>
      expect(client.copyCurl).toHaveBeenCalledWith("c1", expect.objectContaining({ item: null })),
    );
  });

  it("dirty_tab_shows_dot", () => {
    useTabs.getState().edit("c1:a.bru", (d) => ({ ...d, url: "http://changed" }));
    render(<RequestTabs />);
    expect(screen.getByRole("img", { name: "Unsaved changes" })).toBeTruthy();
  });
});
