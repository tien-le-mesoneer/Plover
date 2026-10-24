import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../../api/client";
import { ApiError } from "../../api/client";
import { setApi } from "../../api/instance";
import type { CurlParsedDto, HttpRequestDto } from "../../api/types";
import { blankRequest } from "../../lib/request";
import { useTabs } from "../../store/tabs";
import { useWorkspace } from "../../store/workspace";
import { RequestEditor } from "./RequestEditor";

vi.mock("../../editor/CodeEditor", () => ({ CodeEditor: () => null }));

const INSECURE = "`-k` not applied, TLS checking is a global setting";

function parsed(over: Partial<HttpRequestDto> = {}): CurlParsedDto {
  return {
    request: {
      ...blankRequest(),
      method: "POST",
      url: "https://api.example.test/orders",
      headers: [
        { name: "Authorization", value: "Bearer tok-PASTE-1", enabled: true, local: false },
        { name: "x-a", value: "1", enabled: true, local: false },
      ],
      body: { mode: "json", text: "{}" },
      auth: { mode: "none" },
      ...over,
    },
    notices: [{ option: "-k", reason: INSECURE }],
  };
}

let client: Record<string, ReturnType<typeof vi.fn>>;

function Editor() {
  const tab = useTabs((s) => s.tabs[0]);
  return tab ? <RequestEditor tab={tab} /> : null;
}

function paste(text: string) {
  const url = screen.getByLabelText("URL");
  return fireEvent.paste(url, { clipboardData: { getData: () => text } });
}

beforeEach(() => {
  client = {
    parseCurl: vi.fn(async () => parsed()),
    setSecrets: vi.fn(async () => ({ warnings: [] })),
    environments: vi.fn(async () => [
      { name: "local", vars: [], secretNames: ["authorization"], secretsSet: [], hash: "h" },
    ]),
    capabilities: vi.fn(async () => ({ desktop: false, nativePicker: false, openExternal: false, nativeSave: false, secretBackend: "memory" })),
  };
  setApi(client as unknown as ApiClient);
  useWorkspace.setState({
    collections: [{ id: "c1", name: "Demo", root: "/x", available: true, error: null }],
    environments: { c1: [{ name: "local", vars: [], secretNames: [], secretsSet: [], hash: "h" }] },
    activeEnv: { c1: "local" },
  });
  useTabs.setState({ tabs: [], active: null, saveDialog: null });
  const key = useTabs.getState().newScratch("c1");
  useTabs.getState().edit(key, (d) => ({ ...d, name: "keep me", scriptPre: "pre()" }));
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("pasting curl into the URL bar", () => {
  it("pasting curl fills the draft and shows banner", async () => {
    render(<Editor />);
    const event = paste("  curl -k https://api.example.test/orders");
    expect(event).toBe(false); // default prevented
    expect(client.parseCurl).toHaveBeenCalledWith("  curl -k https://api.example.test/orders");
    expect(await screen.findByText("Filled from cURL")).toBeTruthy();
    const draft = useTabs.getState().tabs[0]!.draft;
    expect(draft.method).toBe("POST");
    expect(draft.url).toBe("https://api.example.test/orders");
    expect(draft.name).toBe("keep me");
    expect(draft.scriptPre).toBe("pre()");
  });

  it("non-curl paste is normal", () => {
    render(<Editor />);
    expect(paste("https://example.test/a")).toBe(true);
    expect(client.parseCurl).not.toHaveBeenCalled();
  });

  it("notices listed with exact -k text", async () => {
    render(<Editor />);
    paste("curl -k x");
    await screen.findByText("Filled from cURL");
    expect(screen.getByText("Not imported:")).toBeTruthy();
    expect(screen.getByText("-k")).toBeTruthy();
    expect(screen.getByText(INSECURE, { exact: false })).toBeTruthy();
  });

  it("undo restores previous draft", async () => {
    render(<Editor />);
    const before = useTabs.getState().tabs[0]!.draft;
    paste("curl x");
    await screen.findByText("Filled from cURL");
    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    expect(useTabs.getState().tabs[0]!.draft).toEqual(before);
    expect(screen.queryByText("Filled from cURL")).toBeNull();
  });

  it("invalid curl shows error and leaves draft", async () => {
    client.parseCurl!.mockRejectedValue(new ApiError(400, "invalid_curl", {}, "the curl command has no URL"));
    render(<Editor />);
    const before = useTabs.getState().tabs[0]!.draft;
    paste("curl -H 'a: b'");
    expect(await screen.findByText(/the curl command has no URL/)).toBeTruthy();
    expect(useTabs.getState().tabs[0]!.draft).toEqual(before);
  });

  it("move to secret replaces header and calls setSecrets with value", async () => {
    render(<Editor />);
    paste("curl x");
    await screen.findByText("Filled from cURL");
    fireEvent.click(screen.getByRole("button", { name: "Move to secret" }));
    await waitFor(() => expect(client.setSecrets).toHaveBeenCalled());
    expect(client.setSecrets).toHaveBeenCalledWith("c1", "local", { authorization: "Bearer tok-PASTE-1" }, true);
    const headers = useTabs.getState().tabs[0]!.draft.headers;
    expect(headers[0]).toMatchObject({ name: "Authorization", value: "{{authorization}}" });
    expect(JSON.stringify(useTabs.getState().tabs[0]!.draft)).not.toContain("tok-PASTE-1");
    await waitFor(() => expect(screen.queryByRole("button", { name: "Move to secret" })).toBeNull());
  });

  it("move to secret numbers a taken name and needs an environment", async () => {
    useWorkspace.setState({
      environments: { c1: [{ name: "local", vars: [{ name: "cookie", value: "x", enabled: true, local: false }], secretNames: [], secretsSet: [], hash: "h" }] },
    });
    client.parseCurl!.mockResolvedValue(
      parsed({ headers: [{ name: "Cookie", value: "sid=c-SECRET", enabled: true, local: false }] }),
    );
    render(<Editor />);
    paste("curl x");
    await screen.findByText("Filled from cURL");
    fireEvent.click(screen.getByRole("button", { name: "Move to secret" }));
    await waitFor(() =>
      expect(client.setSecrets).toHaveBeenCalledWith("c1", "local", { cookie2: "sid=c-SECRET" }, true),
    );

    cleanup();
    useWorkspace.setState({ activeEnv: { c1: null } });
    render(<Editor />);
    paste("curl x");
    await screen.findByText("Filled from cURL");
    const move = screen.getByRole("button", { name: "Move to secret" }) as HTMLButtonElement;
    expect(move.disabled).toBe(true);
    expect(move.title).toBe("Select an environment first");
  });
});
