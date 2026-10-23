# Persisted collections + sidebar header redesign

**Goal:** collections opened in `plv serve` or the desktop app come back after a restart
(with their open tabs, expanded folders and selected environment), and the top-left
"Plover + path input + Open" block becomes a compact header with a "+" popover and a
request filter.

## Decisions

| ID | Decision |
|---|---|
| S1 | The server owns the state (`plover_core::app_state`), not browser storage: the desktop webview gets a new random port (origin) on every launch, so `localStorage` would be empty. |
| S2 | File: `state.json` in the per-user config dir — macOS `~/Library/Application Support/Plover`, Windows `%APPDATA%\Plover`, Linux `$XDG_CONFIG_HOME/plover` (else `~/.config/plover`). `PLOVER_STATE_DIR` overrides it; tests always set it. Computed from env vars, no new dependency. |
| S3 | Schema `{version: 1, open: [{root, openedAt}], recent: [{root, name, lastOpened}] (≤10, newest first), ui: {<root>: UiState}}`. Roots are the collection's canonical root as a string; collection IDs stay `sha256(root)[..16]`, so an unavailable root keeps the same ID. Only paths and UI layout — never secrets or variable values. |
| S4 | Writes are atomic (temp file + rename, the store's helper). A missing file is an empty state; an unreadable or unparsable one (or an unknown `version`) is renamed to `state.json.bak-<unix-ms>`, logged once, and replaced by an empty state. |
| S5 | Only the server persists (`ServerConfig::state_dir`). `plv run` and tests without a state dir never read or write it. |
| S6 | On start the server reopens `open` in saved order; a root that fails to load becomes an *unavailable* entry (reason kept) instead of failing startup. `plv serve PATH` opens PATH after the restore. Unavailable entries are listed after the available ones. |
| S7 | Closing a collection removes it from `open` and keeps it in `recent`. Removing an unavailable entry is the same `DELETE /api/collections/{id}`. Retry = `POST /api/collections {path: root}`. |
| S8 | UI state per collection: `{tabs: string[] (≤50), active: string|null, expanded: string[] (≤1000), env: string|null}`, each string ≤1024 chars, request body ≤64 KiB (413). Restored after the collections load; items that no longer exist are dropped silently. Saved debounced (500 ms). **Unsaved drafts are not persisted.** |
| S9 | Header: "◆ Plover" + "+" (Open collection, `⌘/Ctrl+O`) in the top bar above the sidebar. "+" opens a non-modal dialog popover: Browse for folder… (desktop only), "or paste a path" + ↵, Recent (click opens, × removes). Focus moves in, Esc and outside click close it, focus returns to "+". |
| S10 | "Filter requests" box at the top of the sidebar: case-insensitive substring match on request and folder names; matching requests show with their ancestor folders expanded; `⌘/Ctrl+P` focuses it, Esc clears it. |
| S11 | The old `OpenCollectionDialog` form is removed; the empty state points to "+", `⌘/Ctrl+O` and Browse. |

## Files

- Core: `crates/core/src/app_state.rs` (new), `workspace.rs` (optional `AppStateStore`, `close`, unavailable entries, restore), `store.rs` (`write_atomic` made `pub(crate)`).
- Server: `lib.rs` (`ServerConfig::state_dir`, restore on start), `api/collections.rs` (close, list with unavailable), `api/app_state.rs` (recent + ui-state handlers), `api/dto.rs` (+ `CollectionSummaryDto.available/error`, `RecentDto`, `UiStateDto`), `api/extract.rs` (413 code), regenerated `frontend/src/api/types.ts`.
- CLI/desktop: `plv serve` and the desktop app pass `app_state::default_dir()`.
- Frontend: `components/SidebarHeader.tsx` + `OpenCollectionPopover.tsx` (new), `Sidebar.tsx` (filter, close, unavailable rows, expansion from the store), `store/workspace.ts` (recent, close, retry, expanded, filter, ui-state restore/save), `App.tsx` (⌘O/⌘P, header), `EmptyState.tsx`, `styles.css`; `OpenCollectionDialog.tsx` deleted.

## API

| Route | Result |
|---|---|
| `GET /api/collections` | `CollectionSummaryDto[]` with `available` and `error` |
| `DELETE /api/collections/{id}` | 204; closes an open collection or drops an unavailable one |
| `GET /api/recent` | `RecentDto[]` (`root, name, lastOpened, open`) |
| `DELETE /api/recent?root=…` | 204 |
| `GET /api/ui-state` | `Record<collectionId, UiStateDto>` for listed collections |
| `PUT /api/ui-state/{id}` | 204; 400 invalid shape, 404 unknown collection, 413 > 64 KiB |

All behind the token and Host/Origin checks like every other route.

## Tests

- Core `app_state`: round trip, missing file, corrupt file → backup + empty, unknown version → backup, recent cap 10 + newest first + dedupe, open order, close keeps recent, ui-state per root, no write without a dir.
- Core workspace: restore with a missing root → unavailable entry with reason; close; retry after the folder appears.
- Server: restore on start (available + unavailable listed), close, recent list/remove, ui-state round trip, 413, invalid shape 400, token required on the new routes.
- CLI: `plv serve` with a temp `PLOVER_STATE_DIR` reopens a collection opened in a previous run.
- Frontend: popover focus in / Esc / focus back to "+", recent click opens, filter narrows and expands, `⌘O`/`⌘P`, ui-state restore opens tabs and drops missing ones, close button.
- Live (agent-browser): two collections, restart with the same state dir → both back with tabs and expansion; delete a folder → unavailable entry; 1440×900 and 960×700, light and dark.
