import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../api/client";
import { ApiError } from "../api/client";
import { setApi } from "../api/instance";
import type { HttpRequestDto, TreeNodeDto } from "../api/types";
import { useTabs } from "../store/tabs";
import { useWorkspace } from "../store/workspace";
import App from "../App";
import { closeTabWithConfirm } from "./RequestTabs";
import { SaveRequestDialog } from "./SaveRequestDialog";
import { SidebarHeader } from "./SidebarHeader";

vi.mock("../editor/CodeEditor", () => ({ CodeEditor: () => null }));
vi.mock("../store/startup", () => ({ startUp: async () => undefined }));

const tree: TreeNodeDto = {
  kind: "folder",
  id: "",
  name: "Demo",
  method: null,
  seq: null,
  error: null,
  children: [
    {
      kind: "folder",
      id: "orders",
      name: "orders",
      method: null,
      seq: null,
      error: null,
      children: [{ kind: "folder", id: "orders/v2", name: "v2", method: null, seq: null, error: null, children: [] }],
    },
  ],
};

let client: Record<string, ReturnType<typeof vi.fn>>;

beforeEach(() => {
  client = {
    createRequest: vi.fn(async (_id: string, folder: string, name: string, request: HttpRequestDto) => ({
      item: `${folder ? `${folder}/` : ""}${name}.bru`,
      request: { ...request, name },
      hash: "h1",
    })),
    tree: vi.fn(async () => tree),
    subscribeEvents: vi.fn(() => ({ close() {} })),
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
  useTabs.setState({ tabs: [], active: null, saveDialog: null });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("scratch requests", () => {
  it("new request button disabled without collection", () => {
    useWorkspace.setState({ collections: [], activeCollection: null });
    render(<SidebarHeader />);
    const button = screen.getByRole("button", { name: "New request" });
    expect((button as HTMLButtonElement).disabled).toBe(true);
    expect(button.getAttribute("title")).toBe("Open a collection first");
  });

  it("new request opens Untitled tab", () => {
    render(<SidebarHeader />);
    fireEvent.click(screen.getByRole("button", { name: "New request" }));
    const tabs = useTabs.getState().tabs;
    expect(tabs).toHaveLength(1);
    expect(tabs[0]!.item).toBeNull();
    expect(tabs[0]!.collectionId).toBe("c1");
    expect(tabs[0]!.draft.name).toBe("Untitled");
  });

  it("save on scratch opens dialog with prefilled name", async () => {
    const key = useTabs.getState().newScratch("c1");
    useTabs.getState().edit(key, (d) => ({ ...d, method: "POST", url: "{{host}}/api/orders?page=2" }));
    render(<App hasToken />);
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    const dialog = await screen.findByRole("dialog", { name: "Save request" });
    expect(dialog).toBeTruthy();
    expect((screen.getByLabelText("Name") as HTMLInputElement).value).toBe("POST orders");
    const folder = screen.getByLabelText("Folder") as HTMLSelectElement;
    expect(Array.from(folder.options).map((o) => o.textContent)).toEqual(["Demo (root)", "orders", "orders/v2"]);
    fireEvent.change(folder, { target: { value: "orders" } });
    fireEvent.click(screen.getByRole("button", { name: "Save request" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(client.createRequest).toHaveBeenCalledWith("c1", "orders", "POST orders", expect.anything());
    expect(useTabs.getState().tabs[0]!.item).toBe("orders/POST orders.bru");
  });

  it("cmd+s on scratch opens the dialog instead of saving", async () => {
    const key = useTabs.getState().newScratch("c1");
    useTabs.getState().edit(key, (d) => ({ ...d, url: "http://x" }));
    render(<App hasToken />);
    fireEvent.keyDown(window, { key: "s", metaKey: true });
    expect(await screen.findByRole("dialog", { name: "Save request" })).toBeTruthy();
  });

  it("exists shows inline error", async () => {
    client.createRequest!.mockRejectedValue(new ApiError(409, "exists", {}, "exists"));
    const key = useTabs.getState().newScratch("c1");
    useTabs.getState().edit(key, (d) => ({ ...d, url: "http://x/a" }));
    render(<SaveRequestDialog tabKey={key} onClose={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "Save request" }));
    const error = await screen.findByText("A request with this name already exists");
    const name = screen.getByLabelText("Name");
    expect(name.getAttribute("aria-invalid")).toBe("true");
    expect(name.getAttribute("aria-describedby")).toBe(error.id);
    expect(useTabs.getState().tabs[0]!.item).toBeNull();
  });

  it("closing scratch with content asks Discard this request?", () => {
    const key = useTabs.getState().newScratch("c1");
    const confirm = vi.spyOn(window, "confirm");
    // Empty scratch tabs close without asking.
    expect(closeTabWithConfirm(key)).toBe(true);
    expect(confirm).not.toHaveBeenCalled();
    const full = useTabs.getState().newScratch("c1");
    useTabs.getState().edit(full, (d) => ({ ...d, url: "http://x" }));
    confirm.mockReturnValueOnce(false);
    expect(closeTabWithConfirm(full)).toBe(false);
    expect(confirm).toHaveBeenLastCalledWith("Discard this request?");
    expect(useTabs.getState().tabs).toHaveLength(1);
    confirm.mockReturnValueOnce(true);
    expect(closeTabWithConfirm(full)).toBe(true);
    expect(useTabs.getState().tabs).toHaveLength(0);
  });

  it("beforeunload set only when scratch has content", () => {
    const add = vi.spyOn(window, "addEventListener");
    const key = useTabs.getState().newScratch("c1");
    render(<App hasToken />);
    const registered = () => add.mock.calls.filter(([type]) => type === "beforeunload").length;
    expect(registered()).toBe(0);
    act(() => useTabs.getState().edit(key, (d) => ({ ...d, url: "http://x" })));
    expect(registered()).toBe(1);
  });

  it("dialog fields are labelled and focus returns to the trigger", async () => {
    const key = useTabs.getState().newScratch("c1");
    useTabs.getState().edit(key, (d) => ({ ...d, url: "http://x" }));
    render(<App hasToken />);
    const save = screen.getByRole("button", { name: "Save" });
    save.focus();
    fireEvent.click(save);
    await screen.findByRole("dialog", { name: "Save request" });
    expect(screen.getByLabelText("Folder").tagName).toBe("SELECT");
    expect(screen.getByLabelText("Name").tagName).toBe("INPUT");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(document.activeElement).toBe(save);
  });
});
