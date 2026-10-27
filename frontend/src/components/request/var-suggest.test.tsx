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
    { name: "host", source: "environment", value: "https://example.test", masked: false, truncated: false, shadows: [] },
    { name: "apiKey", source: "environment", value: null, masked: true, truncated: false, shadows: [] },
    { name: "plain", source: "collection", value: "visible-plain-value", masked: false, truncated: false, shadows: [] },
    { name: "process.env.HOME", source: "processEnv", value: null, masked: true, truncated: false, shadows: [] },
  ],
  setLater: [{ name: "token", by: "auth/login.bru" }],
};
const draft = { name: "r" } as HttpRequestDto;

beforeEach(() => {
  useTabs.setState({ tabs: [{ key: "c1:a.bru", collectionId: "c1", item: "a.bru", saved: draft, draft, hash: "h", sending: false }] });
  useWorkspace.setState({ activeEnv: { c1: "Dev" } });
  useVars.setState({ byTab: { "c1:a.bru": dto } });
});

afterEach(() => cleanup());

function Field({ initial = "", label = "URL", onKeyDown }: { initial?: string; label?: string; onKeyDown?: (e: React.KeyboardEvent) => void }) {
  const [value, setValue] = useState(initial);
  return <VarInput tabKey="c1:a.bru" aria-label={label} value={value} onChange={(e) => setValue(e.target.value)} onKeyDown={onKeyDown} />;
}

/** Types `text` at the end, as the browser does: new value and caret, then the change. */
function type(input: HTMLInputElement, text: string) {
  const value = input.value + text;
  fireEvent.change(input, { target: { value, selectionStart: value.length, selectionEnd: value.length } });
  input.setSelectionRange(value.length, value.length);
  fireEvent.keyUp(input, { key: text.slice(-1) });
}

const input = (label = "URL") => screen.getByLabelText(label) as HTMLInputElement;

describe("variable-name autocomplete", () => {
  it("typing_open_braces_opens_list", () => {
    render(<Field />);
    type(input(), "{{");
    const list = screen.getByRole("listbox");
    expect(input().getAttribute("role")).toBe("combobox");
    expect(input().getAttribute("aria-expanded")).toBe("true");
    expect(input().getAttribute("aria-controls")).toBe(list.id);
  });

  it("filter_narrows_as_you_type", () => {
    render(<Field />);
    type(input(), "{{ho");
    expect(screen.getAllByRole("option")[0]!.textContent).toContain("host");
    type(input(), "zzq");
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("enter_inserts_name_with_closing_braces", () => {
    render(<Field initial="https://" />);
    type(input(), "{{ho");
    fireEvent.keyDown(input(), { key: "Enter" });
    expect(input().value).toBe("https://{{host}}");
    expect(input().selectionStart).toBe(16);
    expect(screen.queryByRole("listbox")).toBeNull();
    cleanup();
    render(<Field initial="{{}}" />);
    // Typing "ho" between the braces: the caret ends up after "ho".
    fireEvent.change(input(), { target: { value: "{{ho}}" } });
    input().setSelectionRange(4, 4);
    fireEvent.keyUp(input(), { key: "o" });
    expect(screen.getByRole("listbox")).toBeTruthy();
    fireEvent.keyDown(input(), { key: "Tab" });
    expect(input().value).toBe("{{host}}");
  });

  it("arrow_keys_move_active_option", () => {
    render(<Field />);
    type(input(), "{{");
    const options = screen.getAllByRole("option");
    expect(input().getAttribute("aria-activedescendant")).toBe(options[0]!.id);
    fireEvent.keyDown(input(), { key: "ArrowDown" });
    expect(input().getAttribute("aria-activedescendant")).toBe(options[1]!.id);
    expect(options[1]!.getAttribute("aria-selected")).toBe("true");
    fireEvent.keyDown(input(), { key: "ArrowUp" });
    expect(input().getAttribute("aria-activedescendant")).toBe(options[0]!.id);
  });

  it("secret_shows_name_only", () => {
    render(<Field />);
    type(input(), "{{");
    const apiKey = screen.getByRole("option", { name: /apiKey/ });
    expect(apiKey.textContent).toContain("Environment: Dev");
    // No value is ever shown in the list, secret or not.
    expect(screen.getByRole("listbox").textContent).not.toContain("visible-plain-value");
  });

  it("set_later_names_labelled", () => {
    render(<Field />);
    type(input(), "{{to");
    expect(screen.getByRole("option", { name: /token/ }).textContent).toContain("set later");
  });

  it("process_env_names_excluded", () => {
    render(<Field />);
    type(input(), "{{");
    expect(screen.getByRole("listbox").textContent).not.toContain("process.env");
  });

  it("esc_closes_list_without_cancelling_send", () => {
    const outer = vi.fn();
    render(
      <div onKeyDown={(e) => !e.defaultPrevented && outer(e.key)}>
        <Field />
      </div>,
    );
    type(input(), "{{");
    fireEvent.keyDown(input(), { key: "Escape" });
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(outer).not.toHaveBeenCalled();
    fireEvent.keyDown(input(), { key: "Escape" });
    expect(outer).toHaveBeenCalledWith("Escape");
  });

  it("works_in_params_headers_auth_and_form_fields", () => {
    for (const label of ["Value of id", "Value of Authorization", "Token", "Value of field"]) {
      render(<Field label={label} />);
      type(input(label), "{{h");
      expect(screen.getByRole("listbox")).toBeTruthy();
      cleanup();
    }
  });

  it("var_card_hidden_while_list_open", () => {
    vi.useFakeTimers();
    render(<Field initial="{{host}} " />);
    input().setSelectionRange(3, 3);
    fireEvent.select(input());
    act(() => {
      vi.advanceTimersByTime(600);
    });
    expect(screen.getByRole("dialog")).toBeTruthy();
    type(input(), "{{h");
    expect(screen.getByRole("listbox")).toBeTruthy();
    expect(screen.queryByRole("dialog")).toBeNull();
    vi.useRealTimers();
  });

  it("moving the caret into a token does not open the list", () => {
    render(<Field initial="a {{host}} b" />);
    input().setSelectionRange(4, 4);
    fireEvent.click(input());
    fireEvent.keyUp(input(), { key: "ArrowRight" });
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("clicking an option keeps focus and inserts it", () => {
    render(<Field />);
    input().focus();
    type(input(), "{{pl");
    const option = screen.getByRole("option", { name: /plain/ });
    const down = new MouseEvent("mousedown", { bubbles: true, cancelable: true });
    act(() => {
      option.dispatchEvent(down);
    });
    expect(down.defaultPrevented).toBe(true);
    expect(input().value).toBe("{{plain}}");
  });
});
