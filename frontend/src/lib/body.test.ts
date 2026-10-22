import { describe, expect, it } from "vitest";
import type { HttpRequestDto } from "../api/types";
import { switchBodyMode } from "./body";

const draft = (patch: Partial<HttpRequestDto>) => ({ body: { mode: "none" }, otherBodies: [], ...patch }) as HttpRequestDto;

describe("switchBodyMode", () => {
  it("json -> none -> json restores the JSON text", () => {
    const start = draft({ body: { mode: "json", text: '{"a":1}' } });
    const none = switchBodyMode(start, "none");
    expect(none.body).toEqual({ mode: "none" });
    expect(none.otherBodies).toEqual([{ mode: "json", text: '{"a":1}' }]);
    const back = switchBodyMode(none, "json");
    expect(back.body).toEqual({ mode: "json", text: '{"a":1}' });
    expect(back.otherBodies).toEqual([]);
  });

  it("keeps each mode's own content and starts new modes empty", () => {
    let d = draft({ body: { mode: "json", text: "j" }, otherBodies: [{ mode: "xml", text: "<x/>" }] });
    d = switchBodyMode(d, "xml");
    expect(d.body).toEqual({ mode: "xml", text: "<x/>" });
    d = switchBodyMode(d, "formUrlEncoded");
    expect(d.body).toEqual({ mode: "formUrlEncoded", fields: [] });
    expect(d.otherBodies).toEqual([{ mode: "json", text: "j" }, { mode: "xml", text: "<x/>" }]);
  });
});
