import type { HttpRequestDto } from "../api/types";

/** A new, empty request: `GET`, no URL, auth inherited (Bruno's default for new requests). */
export function blankRequest(name = ""): HttpRequestDto {
  return {
    name,
    seq: 0,
    kind: "http",
    method: "GET",
    url: "",
    query: [],
    pathParams: [],
    headers: [],
    body: { mode: "none" },
    otherBodies: [],
    auth: { mode: "inherit" },
    varsPre: [],
    varsPost: [],
    scriptPre: null,
    scriptPost: null,
    tests: null,
    asserts: [],
    settings: { followRedirects: null, maxRedirects: null, timeoutMs: null },
  };
}
