import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
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

beforeEach(() => {
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
    const tip = screen.getByRole("tooltip");
    expect(tip.textContent).toContain("https://example.test");
    expect(tip.textContent).toContain("Environment: Dev");
    expect(tip.textContent).toContain("overrides collection");
    expect(screen.getByLabelText("URL").getAttribute("aria-describedby")).toBe(tip.id);
  });

  it("hover_masks_secret", () => {
    const { container } = render(<Field initial="{{apiKey}}" />);
    hover(container, "var-defined");
    expect(screen.getByRole("tooltip").textContent).toContain("•••• secret");
  });

  it("set_later_message", () => {
    const { container } = render(<Field initial="{{token}}" />);
    hover(container, "var-setlater");
    expect(screen.getByRole("tooltip").textContent).toContain("Not set yet. Set at runtime by auth/login.bru.");
  });

  it("undefined_message", () => {
    const { container } = render(<Field initial="{{missing}}" />);
    hover(container, "var-undefined");
    expect(screen.getByRole("tooltip").textContent).toContain("Not defined in any scope.");
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
    expect(screen.queryByRole("tooltip")).toBeNull();
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(screen.getByRole("tooltip").textContent).toContain("https://example.test");
  });

  it("esc_hides_and_prevents_default", () => {
    const { container } = render(<Field initial="{{host}}" />);
    hover(container, "var-defined");
    const event = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    act(() => {
      screen.getByLabelText("URL").dispatchEvent(event);
    });
    expect(event.defaultPrevented).toBe(true);
    expect(screen.queryByRole("tooltip")).toBeNull();
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
});
