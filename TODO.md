# TODO

Summary of the 2026-11-01 → 2026-11-03 session. Open items first.

## Open

- [ ] Release v0.2.3 so the installed app gets the folded-JSON item count (`65fe49c`, on `main`, CI green)
- [ ] URL bar: scroll back to the end on window resize
- [ ] Shift+Alt+F is caught in Text/XML bodies, where it does nothing
- [ ] History tab "Clear history" button: use `.danger` (needs an in-app confirm instead of `window.confirm`)
- [ ] KeyValueTable inputs sit ~22px right of the heading (checkbox column)
- [ ] Popovers still use 8px padding
- [ ] Run and History tabs do not come back after a page reload
- [ ] Folded-JSON count: check speed on a very large body (tens of thousands of entries) and in the desktop app
- [ ] Token-free web: test the Vite dev server flow live
- [ ] Hover states, Quick open, keyboard file picker: verified only by CSS/DOM tests, not in a browser
- [ ] `plv serve` picks a random port; use `--port 4225` for a bookmarkable URL (consider a fixed default)
- [ ] GitGuardian curl fixtures: decide whether to change the tests

## Done

### v0.2.1 (released, Latest at the time)
- [x] Long URL scrolls to its end; full URL on hover (URL bar, sidebar, tabs, run results)
- [x] Prettify JSON body (`{ }` button, Shift+Alt+F), keeps `{{var}}` as written
- [x] No red squiggle under an unquoted `{{var}}` in a JSON body
- [x] Component consistency: button types, toolbar heights, `.primary`/`.danger`, selected-row style, one floating surface, one table style, field labels, file picker

### v0.2.2 (released, Latest, installed: `plv 0.2.2`, `~/Applications/Plover.app` 0.2.2)
- [x] Enter in the response search goes to the next match (no Copy value/Copy path menu)
- [x] `{{var}}` hover card stays open while the pointer moves onto it
- [x] A new tab always opens at the end of the tab strip
- [x] Selects draw one muted chevron (method dropdown looks right in the browser)
- [x] `plv serve` opens at its plain address, no token (`--require-token` restores it; desktop keeps tokens)

### After v0.2.2 (on `main`, not released)
- [x] A folded JSON array/object shows its size (`[ 12 items⋯`, `{ 3 keys⋯`)

## How-to notes
- New request from curl: sidebar **New request** button → paste `curl …` into the URL field
- Release: `scripts/check-version.sh vX.Y.Z`, bump 9 files, tag, push, then `gh release edit vX.Y.Z --draft=false --latest`
- Installing `plv` on macOS: `rm` the old binary before `cp` (overwriting a signed binary gets SIGKILL)
