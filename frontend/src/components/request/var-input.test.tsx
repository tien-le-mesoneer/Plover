import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../../api/client";
import { setApi } from "../../api/instance";
import type { HttpRequestDto, VarsDto } from "../../api/types";
import { useTabs } from "../../store/tabs";
import { useVars } from "../../store/vars";
import { useWorkspace } from "../../store/workspace";
import { VarInput } from "./VarInput";

const dto: VarsDto = {
  vars: [
    { name: "host", source: "environment", value: "https://example.test", masked: false, truncated: false, shadows: ["collection"] },
    { name: "apiKey", source: "environment", value: null, masked: true, truncated: false, shadows: [] },
  ],
  setLater: [{ name: "token", by: "auth/login.bru" }],
};

const draft = { name: "r" } as HttpRequestDto;

let writeText: ReturnType<typeof vi.fn>;
let resolveText: ReturnType<typeof vi.fn>;

beforeEach(() => {
  writeText = vi.fn(async () => undefined);
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  resolveText = vi.fn(async (_id: string, body: { text: string; reveal: boolean }) =>
    body.text === "{{apiKey}}"
      ? { text: body.reveal ? "key-1234" : null, containsSecret: true }
      : { text: body.text.replace("{{host}}", "api.test"), containsSecret: false },
  );
  setApi({ resolveText } as unknown as ApiClient);
  useTabs.setState({ tabs: [{ key: "c1:a.bru", collectionId: "c1", item: "a.bru", saved: draft, draft, hash: "h", sending: false }] });
  useWorkspace.setState({ activeEnv: { c1: "Dev" } });
  useVars.setState({ byTab: { "c1:a.bru": dto } });
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

function Field({ initial, onPaste, onKeyDown }: { initial: string; onPaste?: () => void; onKeyDown?: (e: React.KeyboardEvent) => void }) {
  const [value, setValue] = useState(initial);
  return (
    <VarInput
      tabKey="c1:a.bru"
      aria-label="URL"
      value={value}
      onChange={(e) => setValue(e.target.value)}
      onPaste={onPaste}
      onKeyDown={onKeyDown}
    />
  );
}

/** jsdom has no layout: give one mirror span a box and point at it. */
function hover(container: HTMLElement, cls: string) {
  const span = container.querySelector<HTMLElement>(`.var-mirror .${cls}`)!;
  span.getBoundingClientRect = () => ({ left: 10, right: 60, top: 0, bottom: 20, width: 50, height: 20, x: 10, y: 0, toJSON: () => ({}) });
  fireEvent.mouseMove(screen.getByLabelText("URL"), { clientX: 20, clientY: 10 });
}

describe("VarInput", () => {
  it("mirror_draws_three_classes", () => {
    const { container } = render(<Field initial="{{host}}/{{token}}/{{missing}}" />);
    const mirror = container.querySelector(".var-mirror")!;
    expect(mirror.getAttribute("aria-hidden")).toBe("true");
    expect(mirror.querySelector(".var-defined")?.textContent).toBe("{{host}}");
    expect(mirror.querySelector(".var-setlater")?.textContent).toBe("{{token}}");
    expect(mirror.querySelector(".var-undefined")?.textContent).toBe("{{missing}}");
  });

  it("draws no class before the first answer", () => {
    useVars.setState({ byTab: {} });
    const { container } = render(<Field initial="{{missing}}" />);
    expect(container.querySelector(".var-undefined")).toBeNull();
  });

  it("hover_shows_value_and_source", () => {
    const { container } = render(<Field initial="{{host}}/x" />);
    hover(container, "var-defined");
    const tip = screen.getByRole("dialog");
    expect(tip.textContent).toContain("https://example.test");
    expect(tip.textContent).toContain("Environment: Dev");
    expect(tip.textContent).toContain("overrides collection");
    expect(screen.getByLabelText("URL").getAttribute("aria-describedby")).toBe(tip.id);
  });

  it("hover_masks_secret", () => {
    const { container } = render(<Field initial="{{apiKey}}" />);
    hover(container, "var-defined");
    expect(screen.getByRole("dialog").textContent).toContain("•••• secret");
  });

  it("set_later_message", () => {
    const { container } = render(<Field initial="{{token}}" />);
    hover(container, "var-setlater");
    expect(screen.getByRole("dialog").textContent).toContain("Not set yet. Set at runtime by auth/login.bru.");
  });

  it("undefined_message", () => {
    const { container } = render(<Field initial="{{missing}}" />);
    hover(container, "var-undefined");
    expect(screen.getByRole("dialog").textContent).toContain("Not defined in any scope.");
  });

  it("caret_inside_token_shows_after_500ms", () => {
    vi.useFakeTimers();
    render(<Field initial="a {{host}} b" />);
    const input = screen.getByLabelText("URL") as HTMLInputElement;
    input.focus();
    input.setSelectionRange(4, 4);
    fireEvent.select(input);
    act(() => {
      vi.advanceTimersByTime(499);
    });
    expect(screen.queryByRole("dialog")).toBeNull();
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(screen.getByRole("dialog").textContent).toContain("https://example.test");
  });

  it("esc_hides_and_prevents_default", () => {
    const { container } = render(<Field initial="{{host}}" />);
    hover(container, "var-defined");
    const event = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    act(() => {
      screen.getByLabelText("URL").dispatchEvent(event);
    });
    expect(event.defaultPrevented).toBe(true);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("paste_curl_still_works", () => {
    const onPaste = vi.fn();
    render(<Field initial="" onPaste={onPaste} />);
    fireEvent.paste(screen.getByLabelText("URL"), { clipboardData: { getData: () => "curl x" } });
    expect(onPaste).toHaveBeenCalledTimes(1);
  });

  it("input_value_and_selection_unchanged", () => {
    const onKeyDown = vi.fn();
    render(<Field initial="{{host}}" onKeyDown={onKeyDown} />);
    const input = screen.getByLabelText("URL") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "{{host}}/users" } });
    expect(input.value).toBe("{{host}}/users");
    input.setSelectionRange(2, 6);
    expect([input.selectionStart, input.selectionEnd]).toEqual([2, 6]);
    fireEvent.keyDown(input, { key: "a" });
    expect(onKeyDown).toHaveBeenCalled();
  });

  it("hover_card_is_dialog_not_tooltip", () => {
    const { container } = render(<Field initial="{{host}}" />);
    hover(container, "var-defined");
    const card = screen.getByRole("dialog", { name: "Variable host" });
    expect(screen.queryByRole("tooltip")).toBeNull();
    expect(card.querySelector('button[aria-label="Copy value"]')).toBeTruthy();
    expect(card.textContent).toMatch(/(⌘⇧C|Ctrl\+Shift\+C) copies the value/);
  });

  it("hover_card_copy_copies_value_only", async () => {
    const { container } = render(<Field initial="{{host}}" />);
    hover(container, "var-defined");
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Copy value" }));
    });
    expect(resolveText).toHaveBeenCalledWith("c1", expect.objectContaining({ text: "{{host}}", env: "Dev", reveal: false }));
    expect(writeText).toHaveBeenCalledWith("api.test");
    expect(screen.getAllByText("Copied value of {{host}}").length).toBeGreaterThan(0);
  });

  it("hover_card_stays_open_when_pointer_moves_to_it", () => {
    vi.useFakeTimers();
    const { container } = render(<Field initial="{{host}}" />);
    hover(container, "var-defined");
    fireEvent.mouseLeave(screen.getByLabelText("URL"));
    act(() => {
      vi.advanceTimersByTime(100);
    });
    fireEvent.mouseEnter(screen.getByRole("dialog"));
    act(() => {
      vi.advanceTimersByTime(500);
    });
    expect(screen.getByRole("dialog")).toBeTruthy();
    fireEvent.mouseLeave(screen.getByRole("dialog"));
    act(() => {
      vi.advanceTimersByTime(200);
    });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("hover_card_survives_the_way_from_token_to_card", async () => {
    vi.useFakeTimers();
    const { container } = render(<Field initial="{{host}}" />);
    hover(container, "var-defined");
    // Below the token but still over the field, on the way down to the card.
    fireEvent.mouseMove(screen.getByLabelText("URL"), { clientX: 20, clientY: 22 });
    fireEvent.mouseMove(screen.getByLabelText("URL"), { clientX: 20, clientY: 23 });
    act(() => {
      vi.advanceTimersByTime(100);
    });
    expect(screen.getByRole("dialog")).toBeTruthy();
    fireEvent.mouseEnter(screen.getByRole("dialog"));
    act(() => {
      vi.advanceTimersByTime(500);
    });
    vi.useRealTimers();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Copy value" }));
    });
    expect(writeText).toHaveBeenCalledWith("api.test");
  });

  it("hover_card_closes_after_pointer_rests_off_token", () => {
    vi.useFakeTimers();
    const { container } = render(<Field initial="{{host}}" />);
    hover(container, "var-defined");
    fireEvent.mouseMove(screen.getByLabelText("URL"), { clientX: 200, clientY: 10 });
    act(() => {
      vi.advanceTimersByTime(100);
    });
    // Moving on does not restart the wait.
    fireEvent.mouseMove(screen.getByLabelText("URL"), { clientX: 210, clientY: 10 });
    act(() => {
      vi.advanceTimersByTime(100);
    });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("secret_copy_asks_first", async () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    const { container } = render(<Field initial="{{apiKey}}" />);
    hover(container, "var-defined");
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Copy value" }));
    });
    expect(confirm).toHaveBeenCalled();
    expect(resolveText).toHaveBeenCalledTimes(1);
    expect(resolveText.mock.calls[0]![1].reveal).toBe(false);
    expect(writeText).not.toHaveBeenCalled();
  });

  it("caret_shortcut_copies_token_value", async () => {
    render(<Field initial="a {{host}} b" />);
    const input = screen.getByLabelText("URL") as HTMLInputElement;
    input.focus();
    input.setSelectionRange(4, 4);
    await act(async () => {
      fireEvent.keyDown(input, { key: "c", code: "KeyC", metaKey: true, shiftKey: true });
    });
    expect(writeText).toHaveBeenCalledWith("api.test");
    expect(screen.getByText("Copied value of {{host}}")).toBeTruthy();
  });

  it("no_copy_for_undefined_or_set_later", async () => {
    const { container, unmount } = render(<Field initial="{{token}}" />);
    hover(container, "var-setlater");
    expect(screen.queryByRole("button", { name: "Copy value" })).toBeNull();
    unmount();
    render(<Field initial="{{missing}}" />);
    const input = screen.getByLabelText("URL") as HTMLInputElement;
    input.setSelectionRange(3, 3);
    await act(async () => {
      fireEvent.keyDown(input, { key: "c", code: "KeyC", metaKey: true, shiftKey: true });
    });
    expect(resolveText).not.toHaveBeenCalled();
  });
});


/** jsdom has no layout: give an element a width, a content width and a settable scroll offset. */
function box(el: HTMLElement, client: number, scroll: number) {
  let left = 0;
  Object.defineProperty(el, "clientWidth", { configurable: true, value: client });
  Object.defineProperty(el, "scrollWidth", { configurable: true, value: scroll });
  Object.defineProperty(el, "scrollLeft", { configurable: true, get: () => left, set: (v: number) => (left = Math.min(v, scroll - client)) });
}

function TailField({ value }: { value: string }) {
  return <VarInput tabKey="c1:a.bru" aria-label="URL" tail value={value} onChange={() => {}} />;
}

const LONG = "{{host}}/users/1/orders?include=items&page=2";

describe("VarInput tail (URL bar)", () => {
  it("shows the end of a long value while not focused, mirror included", () => {
    const { container, rerender } = render(<TailField value="{{host}}" />);
    const input = screen.getByLabelText("URL");
    const mirror = container.querySelector<HTMLElement>(".var-mirror")!;
    box(input, 100, 400);
    box(mirror, 100, 400);
    rerender(<TailField value={LONG} />);
    expect(input.scrollLeft).toBe(300);
    expect(mirror.scrollLeft).toBe(300);
  });

  it("leaves the scroll alone while the field is being edited, and shows the end again on blur", () => {
    const { rerender } = render(<TailField value="{{host}}" />);
    const input = screen.getByLabelText("URL");
    box(input, 100, 400);
    input.focus();
    input.scrollLeft = 20;
    rerender(<TailField value={LONG} />);
    expect(input.scrollLeft).toBe(20);
    fireEvent.blur(input);
    expect(input.scrollLeft).toBe(300);
  });

  it("titles the field with the whole value only when it does not fit", () => {
    render(<TailField value={LONG} />);
    const input = screen.getByLabelText("URL");
    box(input, 100, 100);
    fireEvent.mouseEnter(input);
    expect(input.getAttribute("title")).toBeNull();
    box(input, 100, 400);
    fireEvent.mouseEnter(input);
    expect(input.getAttribute("title")).toBe(LONG);
  });

  it("drops the title while a variable card is open", () => {
    const { container } = render(<TailField value={LONG} />);
    const input = screen.getByLabelText("URL");
    box(input, 100, 400);
    fireEvent.mouseEnter(input);
    hover(container, "var-defined");
    expect(screen.getByRole("dialog")).toBeTruthy();
    expect(input.getAttribute("title")).toBeNull();
  });
});
