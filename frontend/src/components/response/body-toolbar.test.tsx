import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ResponseDto, SendResultDto } from "../../api/types";
import { ResponseViewer } from "../ResponseViewer";

const run = vi.fn(async () => undefined);
const fakeEditor = { focus: vi.fn(), getAction: vi.fn(() => ({ run })) };

vi.mock("../../editor/CodeEditor", () => ({
  CodeEditor: (p: { value: string; onReady?: (e: unknown) => void }) => {
    p.onReady?.(fakeEditor);
    return <pre data-testid="editor">{p.value}</pre>;
  },
}));

let writeText: ReturnType<typeof vi.fn>;

const signers = { signers: [{ email: "a@example.test" }, { email: "b@example.test" }, { email: "c@example.test" }] };

function response(contentType: string, bodyText: string): ResponseDto {
  return {
    status: 200,
    headers: [["content-type", contentType]],
    elapsedMs: 3,
    size: bodyText.length,
    sizeComplete: true,
    bodyText,
    bodyBase64: null,
    truncated: false,
  };
}

function result(r: ResponseDto): SendResultDto {
  return { response: r, tests: [], asserts: [], console: [], scriptError: null, scriptedRequests: [], skipped: null };
}

const editorText = () => screen.getByTestId("editor").textContent;
const filterInput = () => screen.getByRole("textbox", { name: "Filter with JSONPath" }) as HTMLInputElement;

beforeEach(() => {
  writeText = vi.fn(async () => undefined);
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  run.mockClear();
});

afterEach(cleanup);

describe("response body toolbar", () => {
  it("filter_changes_body_and_copy", async () => {
    render(<ResponseViewer result={result(response("application/json", JSON.stringify(signers)))} sending={false} />);
    fireEvent.change(filterInput(), { target: { value: "$.signers[*].email" } });
    const filtered = JSON.stringify(["a@example.test", "b@example.test", "c@example.test"], null, 2);
    expect(editorText()).toBe(filtered);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Copy body" }));
    });
    expect(writeText).toHaveBeenCalledWith(filtered);
  });

  it("invalid_filter_shows_error_and_keeps_body", () => {
    render(<ResponseViewer result={result(response("application/json", JSON.stringify(signers)))} sending={false} />);
    fireEvent.change(filterInput(), { target: { value: "$.signers[?(@.x)]" } });
    expect(screen.getByText(/^Unsupported at 10/)).toBeTruthy();
    expect(filterInput().getAttribute("aria-invalid")).toBe("true");
    expect(editorText()).toBe(JSON.stringify(signers, null, 2));
  });

  it("raw shows the body as sent", () => {
    render(<ResponseViewer result={result(response("application/json", '{"a":1}'))} sending={false} />);
    expect(editorText()).toBe('{\n  "a": 1\n}');
    fireEvent.click(screen.getByRole("radio", { name: "Raw" }));
    expect(editorText()).toBe('{"a":1}');
  });

  it("json_content_type_with_html_body", () => {
    render(<ResponseViewer result={result(response("application/json", "<html>Bad gateway</html>"))} sending={false} />);
    expect(filterInput().disabled).toBe(true);
    expect(screen.getByText("Body is not valid JSON")).toBeTruthy();
    expect(screen.getByRole("radio", { name: "Raw" }).getAttribute("aria-checked")).toBe("true");
    expect((screen.getByRole("radio", { name: "Pretty" }) as HTMLButtonElement).disabled).toBe(true);
    expect(editorText()).toBe("<html>Bad gateway</html>");
  });

  it("preview_html_is_sandboxed", () => {
    render(<ResponseViewer result={result(response("text/html; charset=utf-8", "<h1>Hi</h1><script>x()</script>"))} sending={false} />);
    fireEvent.click(screen.getByRole("radio", { name: "Preview" }));
    const frame = screen.getByTitle("Response preview") as HTMLIFrameElement;
    expect(frame.getAttribute("sandbox")).toBe("");
    expect(frame.getAttribute("sandbox")).not.toContain("allow-scripts");
  });

  it("preview_disabled_for_plain_text", () => {
    render(<ResponseViewer result={result(response("text/plain", "hello"))} sending={false} />);
    const preview = screen.getByRole("radio", { name: "Preview" }) as HTMLButtonElement;
    expect(preview.disabled).toBe(true);
    expect(preview.title).toBe("Preview shows HTML, images and PDFs");
    expect(filterInput().disabled).toBe(true);
    expect(screen.queryByText("Body is not valid JSON")).toBeNull();
  });

  it("cmd_f_opens_find", () => {
    render(<ResponseViewer result={result(response("application/json", '{"a":1}'))} sending={false} />);
    fireEvent.keyDown(screen.getByRole("radio", { name: "Pretty" }), { key: "f", metaKey: true });
    expect(fakeEditor.getAction).toHaveBeenCalledWith("actions.find");
    expect(run).toHaveBeenCalled();
  });
});
