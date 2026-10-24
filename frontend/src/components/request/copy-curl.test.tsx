import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../../api/client";
import { setApi } from "../../api/instance";
import { blankRequest } from "../../lib/request";
import { useTabs } from "../../store/tabs";
import { useWorkspace } from "../../store/workspace";
import { CopyCurlMenu } from "./CopyCurlMenu";

let client: Record<string, ReturnType<typeof vi.fn>>;
let writeText: ReturnType<typeof vi.fn>;

beforeEach(() => {
  client = {
    copyCurl: vi.fn(async () => ({ text: "curl -X GET 'x'", containsSecret: false, omitted: [] })),
  };
  setApi(client as unknown as ApiClient);
  writeText = vi.fn(async () => undefined);
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  useWorkspace.setState({ activeEnv: { c1: "local" } });
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

describe("Copy as cURL", () => {
  it("copy as written writes text to clipboard", async () => {
    render(<CopyCurlMenu tabKey="c1:a.bru" />);
    fireEvent.click(screen.getByRole("button", { name: "Copy as cURL" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("curl -X GET 'x'"));
    expect(client.copyCurl).toHaveBeenCalledWith("c1", {
      item: "a.bru",
      draft: expect.objectContaining({ url: "{{host}}/a" }),
      env: "local",
      resolve: false,
    });
    expect(await screen.findByText("Copied")).toBeTruthy();
  });

  it("filled in with secret asks before copying", async () => {
    client.copyCurl!.mockResolvedValue({ text: "curl secret", containsSecret: true, omitted: [] });
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    render(<CopyCurlMenu tabKey="c1:a.bru" />);
    fireEvent.click(screen.getByRole("button", { name: "More copy options" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "With variables filled in" }));
    await waitFor(() => expect(confirm).toHaveBeenCalledWith("This command contains secret values. Copy anyway?"));
    expect(client.copyCurl).toHaveBeenCalledWith("c1", expect.objectContaining({ resolve: true }));
    expect(writeText).not.toHaveBeenCalled();
    confirm.mockReturnValue(true);
    fireEvent.click(screen.getByRole("button", { name: "More copy options" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "With variables filled in" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("curl secret"));
  });

  it("omitted parts toast", async () => {
    client.copyCurl!.mockResolvedValue({ text: "curl x", containsSecret: false, omitted: ["auth mode `oauth2`"] });
    render(<CopyCurlMenu tabKey="c1:a.bru" />);
    fireEvent.click(screen.getByRole("button", { name: "Copy as cURL" }));
    expect(await screen.findByText("Copied without auth mode `oauth2`")).toBeTruthy();
  });

  it("works on scratch tab", async () => {
    const key = useTabs.getState().newScratch("c1");
    useTabs.getState().edit(key, (d) => ({ ...d, url: "http://x" }));
    render(<CopyCurlMenu tabKey={key} />);
    fireEvent.click(screen.getByRole("button", { name: "Copy as cURL" }));
    await waitFor(() =>
      expect(client.copyCurl).toHaveBeenCalledWith("c1", {
        item: null,
        draft: expect.objectContaining({ url: "http://x" }),
        env: "local",
        resolve: false,
      }),
    );
  });
});
