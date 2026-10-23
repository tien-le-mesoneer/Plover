import type { ServerEvent } from "../api/types";

export interface EventDeps {
  anyDirty(): boolean;
  reload(): void;
  /** Shows the in-app "discard unsaved changes and reload?" dialog. */
  askReload(): void;
  openedElsewhere(id: string): Promise<void>;
}

/** What the page does for each server notification. */
export function handleServerEvent(event: ServerEvent, deps: EventDeps): void {
  switch (event.type) {
    case "collection_opened":
      void deps.openedElsewhere(event.id);
      break;
    case "reload_requested":
      if (deps.anyDirty()) deps.askReload();
      else deps.reload();
      break;
  }
}
