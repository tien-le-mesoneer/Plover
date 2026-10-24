import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SendResultDto } from "../../api/types";
import { RENDER_CAP } from "../../lib/body";
import { ResponseViewer } from "../ResponseViewer";

let props: { value: string; foldLines?: number[]; onPick?: (o: number, at: { x: number; y: number }) => void } | undefined;

vi.mock("../../editor/CodeEditor", () => ({
  CodeEditor: (p: NonNullable<typeof props>) => {
    props = p;
    return <pre data-testid="editor" />;
  },
}));

let writeText: ReturnType<typeof vi.fn>;

function result(bodyText: string, contentType = "application/json"): SendResultDto {
  return {
    response: {
      status: 200,
      headers: [["content-type", contentType]],
      elapsedMs: 1,
      size: bodyText.length,
      sizeComplete: true,
      bodyText,
      bodyBase64: null,
      truncated: false,
    },
    tests: [],
    asserts: [],
    console: [],
    scriptError: null,
    scriptedRequests: [],
    skipped: null,
  };
}

const twoMiB = "x".repeat(2 * 1024 * 1024);

beforeEach(() => {
  writeText = vi.fn(async () => undefined);
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
});

afterEach(cleanup);

describe("large bodies", () => {
  it("renders_first_mib_and_copies_all", async () => {
    render(<ResponseViewer result={result(twoMiB, "text/plain")} sending={false} />);
    expect(props!.value.length).toBeLessThanOrEqual(RENDER_CAP);
    expect(screen.getByText(/Showing the first 1\.0 MB of 2\.0 MB/)).toBeTruthy();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Copy body" }));
    });
    expect(writeText).toHaveBeenCalledWith(twoMiB);
  });

  it("show_all_renders_everything", () => {
    render(<ResponseViewer result={result(twoMiB, "text/plain")} sending={false} />);
    fireEvent.click(screen.getByRole("button", { name: "Show all" }));
    expect(props!.value.length).toBe(twoMiB.length);
    expect(screen.queryByText(/Showing the first/)).toBeNull();
  });

  it("big arrays are folded, small ones are not", () => {
    const body = JSON.stringify({ big: Array.from({ length: 60 }, (_, i) => i), small: [1, 2, 3] });
    render(<ResponseViewer result={result(body)} sending={false} />);
    expect(props!.foldLines).toEqual([2]);
  });

  it("capped_text_is_not_reparsed_as_json", () => {
    // A JSON body over the cap: the value menu only maps offsets in the shown slice.
    const rows = Array.from({ length: 40000 }, (_, i) => ({ id: `row-${i}` }));
    render(<ResponseViewer result={result(JSON.stringify(rows))} sending={false} collectionId="c1" />);
    expect(props!.value.length).toBe(RENDER_CAP);
    act(() => props!.onPick!(props!.value.indexOf('"row-0"') + 1, { x: 0, y: 0 }));
    expect(screen.getByRole("menu", { name: "Value at $[0].id" })).toBeTruthy();
  });
});
