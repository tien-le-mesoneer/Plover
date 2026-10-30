import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { FileField } from "./FileField";

afterEach(cleanup);

describe("FileField", () => {
  it("keeps_the_native_input_labelled_and_focusable_behind_a_normal_button", () => {
    render(<FileField label="HAR file" accept=".har" onFile={vi.fn()} />);
    const input = screen.getByLabelText("HAR file") as HTMLInputElement;
    expect(input.type).toBe("file");
    expect(input.className).toContain("sr-only");
    expect(input.tabIndex).not.toBe(-1);
    // The visible control opens the same input and looks like any other button.
    const button = screen.getByText("Choose file…");
    expect(button.tagName).toBe("LABEL");
    expect((button as HTMLLabelElement).htmlFor).toBe(input.id);
    expect(screen.getByText("No file chosen")).toBeTruthy();
  });

  it("shows_the_picked_file_name_and_hands_over_the_file", async () => {
    const onFile = vi.fn();
    render(<FileField label="Data file" accept=".csv" describedBy="help" onFile={onFile} />);
    const input = screen.getByLabelText("Data file") as HTMLInputElement;
    const file = new File(["a,b"], "rows.csv", { type: "text/csv" });
    await act(async () => {
      fireEvent.change(input, { target: { files: [file] } });
    });
    expect(onFile).toHaveBeenCalledWith(file);
    expect(screen.getByText("rows.csv")).toBeTruthy();
    // Screen readers hear the help text and the chosen name with the field.
    const described = input.getAttribute("aria-describedby")!.split(" ");
    expect(described).toContain("help");
    expect(described.map((id) => document.getElementById(id)?.textContent)).toContain("rows.csv");
  });
});
