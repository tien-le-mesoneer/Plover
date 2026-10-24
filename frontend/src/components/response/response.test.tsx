import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ResponseDto, SendResultDto } from "../../api/types";
import { downloadName } from "../../lib/download";
import { ResponseViewer } from "../ResponseViewer";

vi.mock("../../editor/CodeEditor", () => ({ CodeEditor: () => null }));

let writeText: ReturnType<typeof vi.fn>;

function response(headers: [string, string][] = [["content-type", "application/json"]], bodyText = '{"a":1}'): ResponseDto {
  return { status: 200, headers, elapsedMs: 312, size: 1229, sizeComplete: true, bodyText, bodyBase64: null, truncated: false };
}

function result(r: ResponseDto | null = response(), extra: Partial<SendResultDto> = {}): SendResultDto {
  return { response: r, tests: [], asserts: [], console: [], scriptError: null, scriptedRequests: [], skipped: null, ...extra };
}

beforeEach(() => {
  writeText = vi.fn(async () => undefined);
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("response status line", () => {
  it("shows status, time and size with tabular figures", () => {
    render(<ResponseViewer result={result()} sending={false} />);
    expect(screen.getByText("200 OK").className).toContain("status-ok");
    expect(screen.getByText("312 ms · 1.2 KB").className).toContain("num");
  });

  it("copy_body_icon_copies_and_announces", async () => {
    render(<ResponseViewer result={result()} sending={false} />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Copy body" }));
    });
    expect(writeText).toHaveBeenCalledWith('{"a":1}');
    expect(screen.getByText("Copied").getAttribute("aria-live")).toBe("polite");
  });

  it("save_to_file_names_json", () => {
    const createObjectURL = vi.fn(() => "blob:x");
    Object.assign(URL, { createObjectURL, revokeObjectURL: vi.fn() });
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});
    render(<ResponseViewer result={result()} sending={false} name="get document" />);
    fireEvent.click(screen.getByRole("button", { name: "Save body to file" }));
    expect(createObjectURL).toHaveBeenCalled();
    expect(click).toHaveBeenCalled();
    expect(downloadName("get document", response())).toBe("get-document.json");
    expect(downloadName("page", response([["content-type", "text/html"]]))).toBe("page.html");
    expect(downloadName("x", response([]))).toBe("x.bin");
  });

  it("shows the checks summary as n/m", () => {
    render(
      <ResponseViewer
        result={result(response(), { tests: [{ name: "t", passed: false, error: "boom" }, { name: "u", passed: true, error: null }] })}
        sending={false}
      />,
    );
    expect(screen.getByRole("img", { name: "Checks: 1 of 2 passed" }).textContent).toBe("1/2");
  });
});

describe("pinned headers", () => {
  it("pinned_headers_only_when_present", async () => {
    const { unmount } = render(<ResponseViewer result={result()} sending={false} />);
    expect(screen.queryByRole("list", { name: "Pinned headers" })).toBeNull();
    unmount();
    render(
      <ResponseViewer
        result={result(response([["content-type", "application/json"], ["X-Request-Id", "7f3c-a91"]]))}
        sending={false}
      />,
    );
    const list = screen.getByRole("list", { name: "Pinned headers" });
    expect(within(list).getAllByRole("listitem")).toHaveLength(1);
    await act(async () => {
      fireEvent.click(within(list).getByRole("button", { name: "Copy x-request-id" }));
    });
    expect(writeText).toHaveBeenCalledWith("7f3c-a91");
  });

  it("pinned_order_is_fixed", () => {
    render(
      <ResponseViewer
        result={result(response([["location", "/d/1"], ["traceparent", "00-abc"], ["x-request-id", "r1"]]))}
        sending={false}
      />,
    );
    const names = within(screen.getByRole("list", { name: "Pinned headers" }))
      .getAllByRole("listitem")
      .map((li) => li.querySelector(".muted")?.textContent);
    expect(names).toEqual(["x-request-id", "traceparent", "location"]);
  });
});

describe("headers tab", () => {
  const open = () => {
    render(<ResponseViewer result={result(response([["content-type", "application/json"], ["etag", "W/1"]]))} sending={false} />);
    fireEvent.click(screen.getByRole("tab", { name: /Response headers/ }));
  };

  it("headers_copy_on_focus", async () => {
    open();
    const copyEtag = screen.getByRole("button", { name: "Copy etag" });
    copyEtag.focus();
    expect(document.activeElement).toBe(copyEtag);
    await act(async () => {
      fireEvent.click(copyEtag);
    });
    expect(writeText).toHaveBeenCalledWith("W/1");
  });

  it("copy_all_headers", async () => {
    open();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Copy all" }));
    });
    expect(writeText).toHaveBeenCalledWith("content-type: application/json\netag: W/1");
  });
});

it("subtab_order_and_counts", () => {
  render(
    <ResponseViewer
      result={result(response(), { console: [{ level: "log", args: ["x"] }] as never })}
      request={{ method: "GET", url: "http://x", headers: [], body: { kind: "none" } } as never}
      sending={false}
    />,
  );
  expect(screen.getAllByRole("tab").map((t) => t.textContent)).toEqual(["Body", "Headers1", "Request", "Tests", "Console1"]);
});
