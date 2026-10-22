import type { ApiClient } from "./client";

let current: ApiClient | null = null;

/** Set once at startup (and by tests); stores call `api()`. */
export function setApi(client: ApiClient): void {
  current = client;
}

export function api(): ApiClient {
  if (!current) throw new Error("ApiClient not initialised");
  return current;
}
