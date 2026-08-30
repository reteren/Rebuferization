# W6 handoff — frontend wiring (task_69e317f4f9a2)

All four DONE gates pass from C:\Rebuferization:
`npx tsc --noEmit` clean, `npx svelte-check` 0 errors / 0 warnings, `npx vite build`
succeeds, no `console.log` anywhere in W6-owned files.

## What landed

- `src/lib/ipc.ts` — one typed wrapper per command in commands.rs (27 commands,
  including the coordinator-added `get_tab_counts`), typed `listen` helpers for
  all 6 events in EVENTS, camelCase args identical to Rust parameter names.
  The only frontend file importing `@tauri-apps/api`.
- `src/lib/stores/items.svelte.ts` — contract shape (`list`, `loading`, `load`,
  `loadMore`, `facets`); paging at 200 with `hasMore`; FTS search debounced
  120 ms; facets + tab counts (`counts`, extra field) refreshed with the filter;
  live reconcile of item-added (dedup bump re-sorts in place), item-updated
  (silent page refetch only when the id is in view), items-deleted (in-place
  removal + backfill). Events arriving mid-fetch are buffered and flushed after
  the load, so a copy never refetches the page the grid is showing.
- `src/lib/stores/selection.svelte.ts` — contract API + `syncOrder` (popup feeds
  current page ids). Anchor is the last single click; shift ranges and
  `moveFocus(dx, dy, columns)` follow the visible layout.
- `src/lib/stores/settings.svelte.ts` — `DEFAULT_SETTINGS` mirroring
  settings.rs defaults so the UI renders pre-load; `current`, `patch`,
  `init()` (load once), `reload()` (after relocate/import), re-sync on
  `settings-changed`.
- `src/routes/Popup.svelte` — SPEC 6.1 layout; keyboard (arrows move focus,
  Enter copies + closes per closeOnCopy, Esc closes, Ctrl+A/Ctrl+click/Shift+click
  select, printable chars focus search); Shift+click → paste_to_previous_window
  plain text; Ctrl+wheel zooms (non-passive listener, persisted via
  window.zoomStep); drag-out via delegated dragstart → begin_drag with the
  full selection; context menu incl. rename overlay and save-as; storage-warning
  banner; settings gear.
- `src/routes/Settings.svelte` — General / Storage / Appearance / Privacy /
  Data / About. Every control patches a real field. Hotkey recorder rejects
  Win-chords Windows owns with an inline explanation and warns on bare-key and
  Ctrl+C-style chords. Usage meter split by kind with "Clean now (N days)".
  Blocklist editor, export/import (.rbx) with progress, relocate, clear-history.

## Assumptions / gaps worth knowing

- `storage-warning` payload is typed `string` (nothing emits it yet in Rust;
  janitor will) — adjust ipc.ts if W1 emits a struct.
- Drag-out needs W5's Card `draggable` + `data-id` — coordinator made it
  binding in CONTRACTS.md; Card has no dragstart handler, Popup delegates.
- Search paging: `search_items` has no offset, so loadMore grows the limit.
- Arrow-key column math mirrors Grid's TILE_W/pitch but subtracts the wrapper
  padding — approximate; harmless if off by a column.
- No "reset" command exists; Data > Clear history = `run_cleanup_now(0)`.
- Blocked-processes editor is text entry (no running-process picker command).
- Item-updated gives ids only (no get-by-id command), so in-view updates
  refetch the current page silently — old list stays until the fresh one lands.