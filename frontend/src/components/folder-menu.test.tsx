import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../api/client";
import { setApi } from "../api/instance";
import type { TreeNodeDto } from "../api/types";
import { useTabs } from "../store/tabs";
import { useWorkspace } from "../store/workspace";
import { Sidebar } from "./Sidebar";

const folder = (id: string, name: string, children: TreeNodeDto[] = []): TreeNodeDto => ({
  kind: "folder",
  id,
  name,
  method: null,
  seq: null,
  error: null,
  children,
});

const tree = folder("", "c", [folder("auth", "auth", [folder("auth/v2", "v2")])]);

let client: Record<string, ReturnType<typeof vi.fn>>;

beforeEach(() => {
  client = {
    previewHar: vi.fn(async () => [
      {
        index: 0,
        method: "POST",
        url: "https://api.example.test/v1/login",
        host: "api.example.test",
        status: 200,
        mime: "application/json",
        size: 10,
        hasSecrets: false,
        hidden: null,
        duplicates: 1,
      },
    ]),
    planHar: vi.fn(async () => ({ env: null, secretNames: [], requestNames: [], notices: [] })),
    importHar: vi.fn(async () => ({ items: ["auth/v2/POST v1-login.bru"], secretNames: [], env: null, notices: [], failedAt: null })),
    reload: vi.fn(async () => tree),
    environments: vi.fn(async () => []),
  };
  setApi(client as unknown as ApiClient);
  useWorkspace.setState({
    collections: [{ id: "c1", name: "demo", root: "/x", available: true, error: null }],
    expanded: { c1: ["auth"] },
    filter: "",
    trees: { c1: tree },
    environments: { c1: [] },
    activeEnv: {},
    capabilities: null,
  });
  useTabs.setState({ tabs: [], active: null });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const row = (name: string) => screen.getByRole("treeitem", { name });

/** Picks a HAR file in the open dialog and waits for its entries. */
async function loadHar() {
  const dialog = await screen.findByRole("dialog");
  const file = new File(['{"log":{"entries":[]}}'], "flow.har", { type: "application/json" });
  fireEvent.change(within(dialog).getByLabelText("HAR file"), { target: { files: [file] } });
  await within(dialog).findByRole("table");
  return dialog;
}

describe("folder context menu", () => {
  it("right_click_folder_opens_menu", () => {
    const onRun = vi.fn();
    render(<Sidebar onRun={onRun} />);
    fireEvent.contextMenu(row("v2"), { clientX: 40, clientY: 60 });
    const menu = screen.getByRole("menu", { name: "v2" });
    expect(within(menu).getAllByRole("menuitem").map((i) => i.textContent)).toEqual(["Run folder", "Import HAR…"]);
    expect(document.activeElement?.textContent).toBe("Run folder");
    expect(onRun).not.toHaveBeenCalled();
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Run folder" }));
    expect(onRun).toHaveBeenCalledWith("c1", "auth/v2", "v2");
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("import_har_from_folder_preselects_it", async () => {
    render(<Sidebar onRun={() => {}} />);
    fireEvent.contextMenu(row("v2"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Import HAR…" }));
    const dialog = await loadHar();
    const select = within(dialog).getByRole("combobox", { name: "Import into" }) as HTMLSelectElement;
    expect(select.value).toBe("auth/v2");
    const go = within(dialog).getByRole("button", { name: "Import" }) as HTMLButtonElement;
    await waitFor(() => expect(go.disabled).toBe(false), { timeout: 3000 });
    fireEvent.click(go);
    await waitFor(() => expect(client.importHar).toHaveBeenCalled());
    expect(client.importHar!.mock.calls[0]![1]).toMatchObject({ folder: "auth/v2" });
  });

  it("shift_f10_opens_menu_for_focused_folder", () => {
    render(<Sidebar onRun={() => {}} />);
    const v2 = row("v2");
    v2.focus();
    fireEvent.keyDown(v2, { key: "F10", shiftKey: true });
    expect(screen.getByRole("menu", { name: "v2" })).toBeTruthy();
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    fireEvent.keyDown(v2, { key: "ContextMenu" });
    expect(screen.getByRole("menu", { name: "v2" })).toBeTruthy();
  });

  it("context_menu_escape_runs_nothing", () => {
    const onRun = vi.fn();
    render(<Sidebar onRun={onRun} />);
    const v2 = row("v2");
    v2.focus();
    fireEvent.keyDown(v2, { key: "F10", shiftKey: true });
    const menu = screen.getByRole("menu");
    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(document.activeElement?.textContent).toBe("Import HAR…");
    fireEvent.keyDown(menu, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(document.activeElement).toBe(v2);
    expect(onRun).not.toHaveBeenCalled();
  });

  it("collection_header_menu_imports_into_root", async () => {
    const onRun = vi.fn();
    render(<Sidebar onRun={onRun} />);
    fireEvent.contextMenu(screen.getByTitle("/x"));
    const menu = screen.getByRole("menu", { name: "demo" });
    expect(within(menu).getAllByRole("menuitem").map((i) => i.textContent)).toEqual(["Run collection", "Import HAR…"]);
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Import HAR…" }));
    const dialog = await loadHar();
    expect((within(dialog).getByRole("combobox", { name: "Import into" }) as HTMLSelectElement).value).toBe("");
  });

  it("without_run_the_folder_menu_has_import_only", () => {
    render(<Sidebar />);
    fireEvent.contextMenu(row("v2"));
    expect(within(screen.getByRole("menu")).getAllByRole("menuitem").map((i) => i.textContent)).toEqual(["Import HAR…"]);
  });
});
