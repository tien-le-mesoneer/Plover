import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../api/client";
import { setApi } from "../api/instance";
import type { HarRowDto, TreeNodeDto } from "../api/types";
import { useWorkspace } from "../store/workspace";
import { ImportHarDialog, MAX_HAR_BYTES } from "./ImportHarDialog";

const TOKEN = "tok-AAAA1111";

const row = (index: number, url: string, over: Partial<HarRowDto> = {}): HarRowDto => ({
  index,
  method: "GET",
  url,
  host: new URL(url).host,
  status: 200,
  mime: "application/json",
  size: 10,
  hasSecrets: false,
  hidden: null,
  duplicates: 1,
  ...over,
});

const rows: HarRowDto[] = [
  row(0, "https://app.example.test/", { mime: "text/html" }),
  row(1, "https://app.example.test/app.js", { hidden: "script", mime: "application/javascript" }),
  row(2, "https://api.example.test/v1/orders", { hasSecrets: true, duplicates: 3 }),
  row(5, "https://api.example.test/v1/me", { hasSecrets: true }),
  row(6, "https://cdn.example.test/logo.png", { hidden: "image", mime: "image/png" }),
];

const tree: TreeNodeDto = {
  kind: "folder",
  id: "",
  name: "Demo",
  method: null,
  seq: null,
  error: null,
  children: [{ kind: "folder", id: "api", name: "api", method: null, seq: null, error: null, children: [] }],
};

let client: Record<string, ReturnType<typeof vi.fn>>;

function harFile(text = `{"log":{"entries":[]},"x":"${TOKEN}"}`) {
  return new File([text], "flow.har", { type: "application/json" });
}

async function load(file = harFile()) {
  render(<ImportHarDialog collectionId="c1" onClose={() => {}} />);
  fireEvent.change(screen.getByLabelText("HAR file"), { target: { files: [file] } });
  await screen.findByRole("table");
}

const checked = () =>
  screen
    .getAllByRole("checkbox", { name: /^Import / })
    .filter((c) => (c as HTMLInputElement).checked)
    .map((c) => c.getAttribute("aria-label"));

beforeEach(() => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  client = {
    previewHar: vi.fn(async () => rows),
    planHar: vi.fn(async () => ({ env: "local", secretNames: ["authToken", "cookie"], requestNames: [], notices: [] })),
    importHar: vi.fn(async () => ({
      items: ["captured/GET v1-orders.bru", "captured/GET v1-me.bru"],
      secretNames: ["authToken", "cookie"],
      env: "local",
      notices: [],
      failedAt: null,
    })),
    reload: vi.fn(async () => tree),
    environments: vi.fn(async () => []),
  };
  setApi(client as unknown as ApiClient);
  useWorkspace.setState({
    collections: [{ id: "c1", name: "Demo", root: "/x", available: true, error: null }],
    trees: { c1: tree },
    environments: { c1: [{ name: "local", vars: [], secretNames: [], secretsSet: [], hash: "h" }] },
    activeEnv: { c1: "local" },
  });
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("Import HAR dialog", () => {
  it("assets hidden and unticked by default", async () => {
    await load();
    expect(client.previewHar).toHaveBeenCalledWith("c1", expect.stringContaining('"entries"'));
    const table = screen.getByRole("table");
    expect(within(table).queryByText("https://app.example.test/app.js")).toBeNull();
    expect(within(table).queryByText("https://cdn.example.test/logo.png")).toBeNull();
    expect(checked()).toEqual([
      "Import GET https://app.example.test/",
      "Import GET https://api.example.test/v1/orders",
      "Import GET https://api.example.test/v1/me",
    ]);
    expect(within(table).getByText("×3")).toBeTruthy();
    expect(within(table).getAllByTitle("Has credentials; they become secret variables")).toHaveLength(2);
  });

  it("domain chip filters rows", async () => {
    await load();
    const chips = screen.getAllByRole("button", { pressed: true }).map((b) => b.textContent);
    // Hosts ordered by request count.
    expect(chips[0]).toContain("api.example.test");
    fireEvent.click(screen.getByRole("button", { name: /api\.example\.test/ }));
    expect(within(screen.getByRole("table")).queryByText("https://api.example.test/v1/me")).toBeNull();
    expect(checked()).toEqual(["Import GET https://app.example.test/"]);
  });

  it("show hidden reveals rows", async () => {
    await load();
    fireEvent.click(screen.getByLabelText(/Show hidden/));
    const table = screen.getByRole("table");
    expect(within(table).getByText("https://app.example.test/app.js")).toBeTruthy();
    expect(within(table).getByText("script")).toBeTruthy();
    // Revealed rows stay unticked until chosen.
    expect(checked()).toHaveLength(3);
  });

  it("import sends ticked entries folder newFolder env", async () => {
    await load();
    fireEvent.click(screen.getByRole("checkbox", { name: "Import GET https://app.example.test/" }));
    fireEvent.change(screen.getByLabelText("Import into"), { target: { value: "api" } });
    expect((screen.getByLabelText("New subfolder") as HTMLInputElement).value).toBe("captured-app.example.test");
    fireEvent.change(screen.getByLabelText("New subfolder"), { target: { value: "captured" } });
    await act(async () => {
      vi.advanceTimersByTime(400);
    });
    await waitFor(() => expect((screen.getByRole("button", { name: "Import" }) as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(screen.getByRole("button", { name: "Import" }));
    await waitFor(() => expect(client.importHar).toHaveBeenCalled());
    expect(client.importHar).toHaveBeenCalledWith("c1", {
      har: expect.any(String),
      entries: [2, 5],
      folder: "api",
      newFolder: "captured",
      env: "local",
    });
    await waitFor(() => expect(client.reload).toHaveBeenCalledWith("c1"));
  });

  it("secret names shown before import", async () => {
    await load();
    await act(async () => {
      vi.advanceTimersByTime(400);
    });
    expect(await screen.findByText("These secret variables will be created in local: authToken, cookie")).toBeTruthy();
    expect(client.planHar).toHaveBeenCalledWith("c1", expect.objectContaining({ entries: [0, 2, 5], env: "local" }));
    expect(client.importHar).not.toHaveBeenCalled();
    expect(document.body.textContent).not.toContain(TOKEN);
  });

  it("result lists secret names never values", async () => {
    await load();
    await act(async () => {
      vi.advanceTimersByTime(400);
    });
    await waitFor(() => expect((screen.getByRole("button", { name: "Import" }) as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(screen.getByRole("button", { name: "Import" }));
    expect(await screen.findByText(/Imported 2 requests/)).toBeTruthy();
    expect(screen.getByText(/authToken, cookie/)).toBeTruthy();
    expect(document.body.textContent).not.toContain(TOKEN);
  });

  it("failedAt shown", async () => {
    client.importHar!.mockResolvedValue({
      items: ["a.bru"],
      secretNames: [],
      env: "Imported",
      notices: [],
      failedAt: "request `GET x`: `x.bru` already exists",
    });
    await load();
    await act(async () => {
      vi.advanceTimersByTime(400);
    });
    await waitFor(() => expect((screen.getByRole("button", { name: "Import" }) as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(screen.getByRole("button", { name: "Import" }));
    expect(await screen.findByText(/Stopped at request `GET x`/)).toBeTruthy();
  });

  it("file over 64 MiB rejected client side with message", async () => {
    render(<ImportHarDialog collectionId="c1" onClose={() => {}} />);
    const big = harFile();
    Object.defineProperty(big, "size", { value: MAX_HAR_BYTES + 1 });
    fireEvent.change(screen.getByLabelText("HAR file"), { target: { files: [big] } });
    expect(await screen.findByText(/larger than 64 MB/)).toBeTruthy();
    expect(client.previewHar).not.toHaveBeenCalled();
  });
});
