import type { KeyValueDto } from "../api/types";

// Bruno keeps the query string inside the URL (that is what gets sent) and
// mirrors it in the Params table. These helpers keep both in sync without
// encoding anything, so `{{vars}}` survive untouched.

function splitUrl(url: string): { base: string; query: string | null; hash: string } {
  const hashAt = url.indexOf("#");
  const hash = hashAt >= 0 ? url.slice(hashAt) : "";
  const rest = hashAt >= 0 ? url.slice(0, hashAt) : url;
  const qAt = rest.indexOf("?");
  return qAt >= 0
    ? { base: rest.slice(0, qAt), query: rest.slice(qAt + 1), hash }
    : { base: rest, query: null, hash };
}

const row = (name: string, value: string): KeyValueDto => ({ name, value, enabled: true, local: false });

/** Replaces the URL's query string with the enabled params. */
export function urlWithQuery(url: string, params: KeyValueDto[]): string {
  const { base, hash } = splitUrl(url);
  const enabled = params.filter((p) => p.enabled && p.name !== "");
  if (enabled.length === 0) return base + hash;
  return `${base}?${enabled.map((p) => `${p.name}=${p.value}`).join("&")}${hash}`;
}

/** Params parsed from the URL, followed by the existing disabled rows. */
export function syncQueryFromUrl(url: string, existing: KeyValueDto[]): KeyValueDto[] {
  const { query } = splitUrl(url);
  const parsed =
    query === null || query === ""
      ? []
      : query.split("&").map((part) => {
          const eq = part.indexOf("=");
          return eq >= 0 ? row(part.slice(0, eq), part.slice(eq + 1)) : row(part, "");
        });
  if (parsed.length === 0) return [];
  return [...parsed, ...existing.filter((p) => !p.enabled)];
}

/** `:name` segments in the URL path (not in the query string). */
export function pathParamNames(url: string): string[] {
  const { base } = splitUrl(url);
  const names: string[] = [];
  for (const segment of base.split("/")) {
    if (segment.startsWith(":") && segment.length > 1) names.push(segment.slice(1));
  }
  return names;
}

/** One row per `:name` in the URL, keeping values of rows that still exist. */
export function syncPathParams(url: string, existing: KeyValueDto[]): KeyValueDto[] {
  return pathParamNames(url).map(
    (name) => existing.find((p) => p.name === name) ?? row(name, ""),
  );
}
