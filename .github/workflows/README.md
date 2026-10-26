# Workflows

`ci.yml` runs on pushes to `main` and on pull requests, on three runners:

| Runner | Checks | Bundles (uploaded as artifacts) |
|---|---|---|
| `macos-14` (arm64) | all | `.app`, `.dmg` |
| `windows-latest` (x64) | all | NSIS installer (`.exe`) |
| `ubuntu-22.04` (x64) | all, smoke under `xvfb-run` | `.deb`, `.AppImage` |

"All checks": frontend typecheck/lint/test/build/no-CDN, script-library bundle check,
`cargo test --workspace`, CLI-only build tests, `cargo clippy -D warnings`, then
`tauri build` and a `PLOVER_DESKTOP_SMOKE=1` run of the desktop binary (starts the
server, opens the window, checks `/api/health`, exits).

Bundles are unsigned (decision P4-6). `PLOVER_KEYCHAIN_SERVICE=dev.plover.ci` keeps any
keychain access away from real entries.

`release.yml` runs on `v*` tags (and by hand): it builds `plv` archives and desktop
bundles on macOS (universal), Windows and Linux, then drafts a GitHub release with
`SHA256SUMS`. A manual run only uploads the packages as artifacts. See
[Publishing a release](../../docs/development.md#publishing-a-release).
