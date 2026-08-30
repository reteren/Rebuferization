# REVIEW-FRONTEND — W27 adversarial review of the frontend

Reviewed: `src/lib/ipc.ts`, `src/lib/stores/**`, `src/routes/Popup.svelte`,
`src/routes/Settings.svelte`, `src/lib/components/Grid.svelte` and
`Card.svelte`, against docs/CONTRACTS.md, docs/SPEC.md §6.4/6.5/§9, and
`src-tauri/src/commands.rs` (the authoritative IPC surface). Read-only; no
source was modified.

**Confidence: high on the static paths, medium on anything that needs the
app running.** I could not check without launching the app: which element
actually receives the wheel under finding 4 (the layout facts are certain,
the browser's resolution of `height: 100%` in this exact flex tree is not),
real FTS ranking and LIMIT truncation against a populated store, and the
user-visible frequency of the timing-dependent races (findings 6-8 are
possible by construction and cheap to fix, but their frequency needs a live
app). Everything else — IPC names, event names, serialized field names, the
selection/anchor arithmetic, the virtualized row math — was checked
line-by-line against the Rust side.

Findings, most severe first.

---

### 1. Shift-click pastes into the previous window instead of selecting a range — SPEC §6.5's range selection is impossible, and this is the only path that ever auto-pastes

`src/routes/Popup.svelte:306-310`:

```ts
function onGridToggle(item: ItemDto, mode: 'single' | 'ctrl' | 'shift'): void {
  lastToggleAt = Date.now()
  selection.toggle(item.id, mode)
  if (mode === 'shift') void pasteToPreviousWindow([item.id], true)
}
```

Every shift-click runs `pasteToPreviousWindow([item.id], true)`, which
writes the item to the clipboard, hides the popup, and — with
`behavior.autoPaste` on — injects Ctrl+V into the previously focused window
(commands.rs:82-95). SPEC §6.5 says "Shift+click selects a range"; the
selection store computes the range correctly (`selection.toggle`,
selection.svelte.ts:36-48), and then the popup closes before the user can
do anything with it.

Failure sequence: popup open over Word -> user shift-clicks a card to
extend a selection -> the popup vanishes, Word receives a plain-text paste
of that one item, and the selection the user was building is discarded.
Range selection via shift is unreachable in every path.

Secondary defect in the same line: `pasteToPreviousWindow` is called from
nowhere else in the frontend (grep: Popup.svelte:309 is the only call
site). Every intended copy path — plain click, Enter, context-menu Copy
(Popup.svelte:316-320, 361) — goes through `copyToClipboard`, which never
auto-pastes. So the `autoPaste` setting (SPEC §5.4) only ever fires through
this broken shift-click path; the feature is effectively dead in the flows
users are meant to use.

### 2. A plain click copies and closes the popup; single selection is impossible — the contract's `single` toggle mode never fires

Card.svelte:83-93 (`handleClick`): a plain click calls `onactivate`, which
Popup.svelte:301-304 routes to `copyItem` -> `copyToClipboard` +
`hidePopup` (with `closeOnCopy`, the default). Nothing ever calls
`ontoggle(..., 'single')`; that mode is dead code across both files.

Consequences, both demonstrable:

- **You cannot select one item without copying it.** SPEC §6.5's model is
  "arrows move focus, Enter copies"; CONTRACTS.md defines
  `ontoggle(item, mode: 'single' | ...)` for plain clicks. As written, a
  plain click on any card replaces the clipboard and closes the popup.
  The only non-destructive select gesture is Ctrl+click, and the keyboard
  copy path (`Enter` -> `copyFocused`, Popup.svelte:182-186) does nothing
  unless `focusedId` was set by a previous Ctrl+click.
- **Accidental clipboard replacement.** One stray click on a card replaces
  the user's clipboard with that card's content and dismisses the popup.
  For a clipboard-history tool this is the highest-visibility failure
  mode, and it happens on every single click.
- The 40 ms `lastToggleAt` guard in `onGridActivate` (Popup.svelte:302) is
  dead code: `lastToggleAt` is only written by `onGridToggle`, and
  `onGridToggle` and `onGridActivate` are mutually exclusive in Card, so
  the guard can never trigger.

Sequence: popup opens -> user clicks a card to select it -> clipboard is
overwritten, popup closes. Every time.

### 3. Drag-out is dead: the contract's `data-id` and `draggable="true"` exist nowhere

CONTRACTS.md states the Card root carries `draggable="true"` and
`data-id={item.id}`, and that W6 delegates one dragstart listener on the
grid viewport that reads `data-id` and hands off to `begin_drag`.

- Card.svelte:108-120 — the root div has `class`, `style`, `role`,
  `tabindex` and handlers, but **no `draggable` and no `data-id`**.
- Grid.svelte:233-245 renders `<Card ... style="...">` — no `data-id`.
- Popup.svelte:335-345 (`onDragStart`) does `closest('[data-id]')`, which
  is always `null`, so `beginDrag` is never reached. And even if it were,
  `dragstart` only fires on draggable elements (or selections); cards are
  not draggable, so the handler never fires at all.

Sequence: user drags a card toward Explorer/Discord -> nothing happens;
SPEC §6.8's OLE drag never starts. The Rust side (`begin_drag`, the whole
CF_HDROP machinery) is unreachable.

### 4. Paging is dead: `loadMore`'s only trigger is a scroll listener on a container that never scrolls

Popup.svelte:442-450 puts `onscroll={onGridScroll}` on `.grid-viewport`
(overflow: auto, Popup.svelte:531-536). Inside it, Grid.svelte renders
`.grid` with `height: 100%; overflow-y: auto` (Grid.svelte:253-261) and
its own `bind:this`/`onscroll` (Grid.svelte:212-225) that drives the
virtualization. The viewport is a definite-height flex item (`flex: 1;
min-height: 0`), so the grid resolves `height: 100%` to exactly the
viewport's height; the scrollable content (the `totalHeight` spacer) lives
inside `.grid`. All scrolling happens on the inner element, which never
overflows the outer one — the outer viewport's `scrollTop` stays 0 and
`onGridScroll` either never fires or always sees `scrollHeight -
clientHeight < 100` and returns at Popup.svelte:331.

Sequence: 200 items loaded, `hasMore = true`; the user scrolls to the
bottom of the grid (the inner container scrolls fine — the grid keeps
rendering), `onGridScroll` never passes its threshold, `items.loadMore()`
never runs, the app silently caps at 200 items. The same holds for search
(results capped at 200), because `loadMore`'s search branch (items.svelte.ts:75-79)
is behind the same dead trigger.

The search branch is otherwise coherent (it grows the LIMIT rather than
OFFSET, matching the FTS query in queries.rs which has no OFFSET — so
paging by re-fetching with a larger limit is the correct shape), and the
list branch's exact-boundary case does not loop (a fetch returning exactly
200 keeps `hasMore` true; the next fetch returns 0 and clears it).

### 5. Sorting or searching silently discards the toolbar's kind/ext filter

Popup.svelte:273-282:

```ts
function onQuery(q: string): void {
  query = q
  selection.clear()
  void items.load(activeTab, sort, q)      // resets this.filter = TAB_FILTERS[activeTab]
}
function onSort(s: Sort): void {
  sort = s
  void items.load(activeTab, sort, query)  // same
}
```

`items.load(tab, ...)` sets `this.filter = TAB_FILTERS[tab]`
(items.svelte.ts:54-59), discarding any filter the user applied through the
toolbar. The toolbar filter only survives via `applyFilter`
(`onToolbarFilter`, Popup.svelte:255-265), which is never called again by
the query/sort handlers.

Sequence: All tab, filter set to `kind: image, ext: PNG` via the toolbar
(facets populated) -> user types in the search box or changes sort -> the
grid refetches with the All-tab preset and now shows every kind, while the
Toolbar's filter dropdown still renders PNG as active (its `filter` prop is
the untouched local state). The UI and the result set disagree until the
user re-picks the filter. The same happens after a search result arrives
mid-query (onAdded's `schedule()` re-run uses the same stale preset).

### 6. Event reconciliation can drop an item that loadMore will then never fetch — vanished cards

items.svelte.ts:274-279 (`insertSorted`):

```ts
if (at >= PAGE_SIZE) return          // belongs on a later page
this.list.splice(at, 0, item)
if (this.list.length > PAGE_SIZE) this.list.length = PAGE_SIZE
```

When the page is full (200) and a new item sorts inside it, the item that
was at index 199 is dropped from the array. `loadMore` (items.svelte.ts:81-88)
fetches with `offset = this.list.length` (= 200), so the dropped item,
which sits at rank 199 in the store, is skipped by every subsequent fetch.
`hasMore` stays true and no later event refreshes the page (a dedup bump
only calls `refreshMeta`, never `refreshPage`).

Sequence: 200 items loaded on All/newest -> a new capture arrives
(ITEM_ADDED) -> `onAdded` -> `insertSorted` at index 0 -> the 200th-ranked
item is dropped from the array but not from the store -> the user scrolls
to the bottom, loadMore fetches offset 200, the dropped item is never seen
again until a full reload (tab switch). Same outcome when a re-captured
item bumps via `resort` (items.svelte.ts:281-285).

Related race, same mechanism: `busy()` (items.svelte.ts:168-170) does not
include `loadingMore` or an in-flight `refreshPage`. An ITEM_ADDED arriving
during `refreshPage` runs `insertSorted` against the old array, and when
the refetch lands it overwrites `this.list` with a snapshot that predates
the insert — the fresh item vanishes. An ITEMS_DELETED arriving during
`refreshPage` is applied to the old array, then the refetch overwrites it
with a snapshot that still contains the deleted id — a ghost card. Both are
narrow windows, but both are the exact "event arrives mid-fetch" case the
buffering design exists to handle, and both end in a card that is wrong
until the next full reload.

### 7. The janitor never emits `items-deleted` — pruned items stay on screen as ghost cards

`ITEMS_DELETED` is emitted in exactly two places (commands.rs:114 for
`delete_items`, commands.rs:273 for `clear_history`). The janitor — size-cap
prune, retention prune, startup integrity sweep (store/janitor.rs) — emits
only `STORAGE_WARNING` and `STORE_PROGRESS`. The popup reconciles deletions
only from `onItemsDeleted` (items.svelte.ts:203-223).

Sequence: popup open on All; the store hits its size cap; the janitor
prunes 40 old items; the popup gets the storage-warning banner ("removed 40
items") but the grid still shows all 40 cards. Clicking one copies a
stale id — `write_items` skips missing ids, so the user gets either a
partial copy or (if every id is stale) an error with no UI feedback (see
finding 9). The banner's `removedItems > 0` branch (Popup.svelte:226-229)
reports the prune without reconciling the grid.

### 8. Selection ghosts and the scope of Ctrl+A

- **Deleted ids are never pruned from `selection.ids`.** `onDeleted`
  (items.svelte.ts:216-222) filters the list but nothing touches the
  selection store; Popup clears selection only in the delete menu path
  (Popup.svelte:384). After a janitor prune or a `clear_history` from the
  settings window, `selection.ids` holds ids that no longer exist. The
  next Enter/context-menu copy or delete then sends those ids to Rust —
  and `delete_items`/`write_items` tolerate missing ids by skipping them,
  so the failure is silent (and combined with finding 9, invisible).
- **Ctrl+A selects only the loaded page** (Popup.svelte:177-181,
  `selection.all(items.list.map(...))`). SPEC §6.5's "select all in view"
  is satisfied by the letter, but with paging dead (finding 4) "in view"
  means the first 200 of, say, 12,000 — the user hits Ctrl+A, then Copy,
  and the clipboard receives 200 items' worth of concatenation while the
  status bar reports 12,000. Whether that is a bug or a feature is a
  product call; the mismatch deserves one.
- The shift-range anchor (selection.svelte.ts:37-38) falls back to the
  focused item when the anchor is gone from the current order — which is
  the correct degradation after a filter change — and Ctrl+click never
  moves the anchor, per its own comment. That part of the model is sound.

### 9. Error handling in the popup: every mutating command is fire-and-forget — a failed copy, delete, pin, or open is completely silent

Popup.svelte awaits with no `.catch` on: `copyItem` (316-320, and its
`.then` only hides the popup on success), `copyPlain` (361), `setPinned`
(364, 367), `openItem` (370), `openItemWith` (373), `showInFolder` (376),
`deleteItems` (383), `saveItemAs` (392), `beginDrag` (344), and the
shift-click paste (309). The only try/catch in the file is `doRename`
(410-414), which swallows the error into nothing. Tauri commands reject
with a string; an unhandled rejection produces no user-visible feedback of
any kind.

Sequence: the user presses Enter or clicks a card while the clipboard
listener is mid-write — `writer.rs` returns `AppError::ClipboardBusy` — or
the item's blob is missing; the command rejects; nothing happens, no
banner, the popup stays open, and the user has no idea the copy failed
(the common reaction is "the app is broken" or a re-click). The settings
window does this right (every action has a catch that surfaces `error`,
Settings.svelte:123-127, 210-236, 260-323); the popup does none of it.

### 10. The settings progress banner never clears

Settings.svelte:89-91 sets `progress` from every `store-progress` event.
The janitor's final event for relocate/export/import is
`phase: "completed"` (janitor.rs, several emits), and nothing in the
frontend ever resets `progress` to null. The banner (Settings.svelte:327-337)
stays pinned to the top of the window indefinitely after any relocate,
export, or import finishes.

Sequence: Data tab -> Import -> the archive imports; the progress bar
reaches "completed"; the banner remains over the UI for the rest of the
window's life.

### 11. Every `listen()` result is dropped — the unlisten contract is ignored in all six call sites

`onItemAdded/onItemsUpdated/onItemsDeleted` (items.svelte.ts:288-292),
`onStorageWarning` (Popup.svelte:99), `onStoreProgress` (Settings.svelte:89),
`onSettingsChanged` (settings.svelte.ts:86) all discard the returned
`UnlistenFn`. Today this is benign: each webview is a separate module
instance and the popup webview persists across show/hide, so the count
stays at one listener per window — the "every copy fires N handlers"
scenario does not accumulate in normal use. The failure mode is structural:
if the popup window is ever recreated (window re-creation, dev HMR, a
future reload-on-resize), every handler duplicates, and nothing in the code
can prevent it because the unlisten handles were thrown away at subscribe
time. The fix is a one-line-per-site change; the contract's return value
should not be ignored in a codebase that otherwise honors contracts.

### 12. Keyboard navigation stops at the page boundary

`selection.moveFocus` (selection.svelte.ts:60-69) clamps to
`order.length - 1`, and `order` is only the loaded page. Arrow-Down at the
bottom of page 1 does nothing and never triggers `loadMore`, so with paging
dead (finding 4) the keyboard cannot reach beyond the first 200 items at
all, and even with paging fixed the arrow path would still require a wheel
scroll to advance pages. Minor beside finding 4, but it is the same
feature users will hit first.

---

## Verified clean (checked, found nothing wrong)

- **IPC contract drift: none found.** Every wrapper in ipc.ts was checked
  against commands.rs: argument names (camelCase) match for all 26
  commands, including `olderThanDays`/`includePinned`/`plainText`; every
  return type is declared and used (`CleanupResult` for
  `run_cleanup_now`/`clear_history`, `Settings` for `update_settings`,
  `ItemDto[]` for `add_files`); event names in types.ts `EVENTS` match
  model.rs `events` verbatim, and the payload shapes (ItemDto for
  item-added, `number[]` for item-updated/items-deleted, Settings for
  settings-changed, StoreProgress/StorageWarning) match the emissions in
  commands.rs and janitor.rs. `ItemDto` fields in types.ts mirror
  model.rs:77-99 one-for-one (camelCase), including `copyCount` and
  `fileNames`, which the SPEC's IPC section predates.
- **Column math between Popup and Grid agrees.** Popup's derived `columns`
  (Popup.svelte:85-89: `gridWidth - 24 - 32 + 10` over `tileW + 10`) and
  Grid's (Grid.svelte:58-60: `clientWidth - pad*2 + gap`) reduce to the
  same expression because the grid's `clientWidth` is the viewport's minus
  its 12 px side padding and `--grid-pad`/`--grid-gap` default to 16/10.
  Zoom rounding (`Math.round`, Grid.svelte:53) matches the integer zoom the
  wheel produces. `moveFocus` therefore lands on the columns the grid
  renders.
- **Grouped-mode virtualization and sticky headers.** Row math for groups
  (Grid.svelte:91-109) uses `ceil(items/columns)` with the header height
  folded in; the sticky header is pushed out exactly when its section's
  bottom reaches the viewport top, so headers do not overlap or outlive
  their group; short groups (fewer items than one row) compute height
  correctly; switching to a flat sort renders one section with no header
  (Grid.svelte:196). The virtualization culling respects the overscan.
- **Search paging shape.** `search_items` (queries.rs:235-300) has no
  OFFSET — LIMIT-only — and the store's grow-the-limit approach
  (items.svelte.ts:75-79) is the correct counterpart; the empty-query
  fallback to newest-first (queries.rs:244) is never reachable from the
  frontend, which trims the query before testing it.
- **loadMore's double-fetch guard.** `loadingMore` plus `hasMore` make the
  re-entrant scroll handler (once it is attached to the right element)
  idempotent; the exact-boundary case terminates.
- **Dedup in the store.** `onAdded` checks for an existing id before
  inserting (items.svelte.ts:183-190), so an item both inserted by the
  event and present in a concurrent fetch snapshot cannot double-render.
- **Settings error surfacing.** All of Settings.svelte's mutating actions
  (relocate, import, export, cleanup, clear/reset, capture toggle, every
  `patch`) route failures into the `error` banner — the popup should copy
  this pattern, not the reverse.

---

## What this review could not establish

Whether finding 4's inner-scroll claim holds in the built app (the flex
resolution of `height: 100%`), the real-world frequency of findings 6-8,
FTS rank-order stability between successive `search_items` calls (a
re-rank between loadMore calls would reshuffle the page), and any WebView2
scrollbar-visibility side effect of the double scroll container. All four
are one manual run away from confirmation.