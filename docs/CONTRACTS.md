# Contracts

Six workers build Rebuffer in parallel. This file is the only place where their
work touches. **Nothing here changes without the coordinator.** If a contract is
wrong, send an `escalation` — do not fix it locally, because your fix silently
breaks whoever else reads it.

## File ownership

Write only inside your own paths. Reading anything is fine.

| Worker | Owns |
|---|---|
| W1 store | `src-tauri/src/store/**`, `src-tauri/migrations/**` |
| W2 clipboard | `src-tauri/src/clipboard/**` |
| W3 window+hotkey | `src-tauri/src/window/**`, `src-tauri/src/hotkey/**` |
| W4 platform | `src-tauri/src/settings.rs`, `tray.rs`, `logging.rs`, `shell.rs` |
| W5 UI kit | `src/lib/components/**`, `src/lib/styles/**`, `src/lib/mock.ts` |
| W6 wiring | `src/lib/ipc.ts`, `src/lib/stores/**`, `src/routes/**` |

Coordinator-owned, read-only to everyone: `src-tauri/src/lib.rs`, `commands.rs`,
`model.rs`, `capture.rs`, `error.rs`, `src/lib/types.ts`, `tauri.conf.json`,
`Cargo.toml`, `package.json`, `vite.config.ts`, this file.

Adding a dependency is a coordinator request, not a local edit — parallel writes
to `Cargo.toml` corrupt it.

## Rust seams

- `clipboard` produces exactly one `capture::Capture`; `store::Store::insert_capture`
  is its only consumer. Neither side reaches past that type.
- The store computes the content hash, because the store owns the dedup rule.
- Nothing outside `store/` opens the database or writes under `blobs/`.
- Nothing outside `clipboard/` calls a Win32 clipboard API.
- Every fallible function returns `error::AppResult<T>`.
- Replace `todo!()` in your own stubs. Leaving another module's `todo!()` alone is
  correct — it is not your file.

## Frontend seams

W6 owns data, W5 owns pixels. A component never calls `invoke`; a store never
renders. `src/routes/Popup.svelte` (W6) is where the two meet.

### Component API — W5 implements exactly these

```ts
// Card.svelte
{ item: ItemDto, selected: boolean, focused: boolean, zoom: number,
  showAge: boolean, formatLabelSize: 'off'|'small'|'medium'|'large',
  animateGifs: boolean }
// events: onactivate(item), oncontextmenu(item, x, y), ontoggle(item, mode:'single'|'ctrl'|'shift')
// The root element carries `draggable="true"` and `data-id={item.id}`. Card
// itself has NO dragstart handler: W6 delegates one listener on the grid
// viewport, reads data-id, and hands off to the Rust `begin_drag`, which owns
// the real OLE drag. One listener beats 10,000.

// Grid.svelte — virtualized; renders group headers itself when `grouped`
{ items: ItemDto[], zoom: number, grouped: boolean,
  selectedIds: Set<number>, focusedId: number | null,
  showAge: boolean, formatLabelSize: string, animateGifs: boolean }
// Grid forwards showAge, formatLabelSize and animateGifs straight to Card. It
// does not interpret them — a display flag that stopped at the Grid could never
// reach the thing it describes, which is how animateGifs stayed dead.
// events: same three, forwarded from Card

// Tabs.svelte      { active: TabId, counts: Record<TabId, number> } -> onselect(tab)
// Toolbar.svelte   { query: string, sort: Sort, filter: Filter, facets: Facet[] }
//                  -> onquery(s), onsort(s), onfilter(f), onadd()
// ZoomDial.svelte  { value: number /* 1..5 */ } -> onchange(n)
// ContextMenu.svelte { item: ItemDto, x: number, y: number } -> onaction(id: MenuAction)
// StatusBar.svelte { itemCount: number, totalBytes: number }
// EmptyState.svelte { tab: TabId }
// Skeleton.svelte  { count: number }
```

`MenuAction` is
`'copy'|'copyPlain'|'pin'|'unpin'|'open'|'openWith'|'saveAs'|'reveal'|'rename'|'delete'`.

Svelte 5 runes only: `$props()`, `$state`, `$derived`, `$effect`. Callback props,
not `createEventDispatcher`.

### Store API — W6 implements exactly these

```ts
// stores/items.svelte.ts
items.list: ItemDto[]        // current page, reactive
items.loading: boolean
items.load(tab, sort, query): Promise<void>
items.loadMore(): Promise<void>
items.facets: Facet[]

// stores/selection.svelte.ts
selection.ids: Set<number>
selection.focusedId: number | null
selection.toggle(id, mode), selection.clear(), selection.all(ids)
selection.moveFocus(dx, dy, columns)

// stores/settings.svelte.ts
settings.current: Settings
settings.patch(p: SettingsPatch): Promise<void>
```

W5 develops against `src/lib/mock.ts`, which exports
`mockItems(n: number): ItemDto[]` covering every kind and sub-kind, including a
missing reference and a 7-line text item. Nothing outside W5 imports it.

## Design language

Dark liquid glass. Tokens live in `src/lib/styles/tokens.css` as CSS custom
properties; no component hardcodes a color, radius, or duration. Animate
`transform` and `opacity` only — this grid holds 10,000 nodes. Honour
`prefers-reduced-motion` and `settings.appearance.reduceMotion`.

Do not rely on `backdrop-filter` for the window backdrop: WebView2 blurs against
the page, not the desktop. The desktop blur comes from `window-vibrancy` on the
Rust side. `backdrop-filter` on inner panels is fine.

## Definition of done

`cargo check` clean for Rust workers, `npx tsc --noEmit && npx svelte-check` clean
for frontend workers. No `todo!()` left in your own files, no `unwrap()` on
anything that can fail in production, no `console.log`. Then send `worker_done`.
