<script lang="ts">
  // OWNER: worker W6. The popup: where the items/selection/settings stores
  // meet W5's component kit. No component calls invoke; this file is the only
  // consumer of both sides.

  import { untrack } from 'svelte'
  import { open, save } from '@tauri-apps/plugin-dialog'

  import ContextMenu from '../lib/components/ContextMenu.svelte'
  import EmptyState from '../lib/components/EmptyState.svelte'
  import Grid from '../lib/components/Grid.svelte'
  import Skeleton from '../lib/components/Skeleton.svelte'
  import StatusBar from '../lib/components/StatusBar.svelte'
  import Tabs from '../lib/components/Tabs.svelte'
  import Toolbar from '../lib/components/Toolbar.svelte'
  import ZoomDial from '../lib/components/ZoomDial.svelte'

  import {
    addFiles,
    beginDrag,
    copyToClipboard,
    deleteItems,
    getCurrentClipboardId,
    getStorageStats,
    hidePopup,
    onClipboardCurrent,
    onStorageWarning,
    openItem,
    openItemWith,
    pasteToPreviousWindow,
    popupReady,
    renameItem,
    saveItemAs,
    setPinned,
    showInFolder,
    showSettingsWindow,
    type UnlistenFn,
  } from '../lib/ipc'
  import { items } from '../lib/stores/items.svelte'
  import { selection } from '../lib/stores/selection.svelte'
  import { settings } from '../lib/stores/settings.svelte'
  import {
    TAB_FILTERS,
    type Filter,
    type ItemDto,
    type Sort,
    type StorageStats,
    type StorageWarning,
    type TabId,
  } from '../lib/types'

  type MenuAction =
    | 'copy'
    | 'copyPlain'
    | 'pin'
    | 'unpin'
    | 'open'
    | 'openWith'
    | 'saveAs'
    | 'reveal'
    | 'rename'
    | 'delete'

  // Tile widths per zoom step — must mirror Grid.svelte's TILE_W so arrow
  // navigation lands on the same columns the grid renders.
  const TILE_W: Record<number, number> = { 1: 72, 2: 92, 3: 116, 4: 148, 5: 188 }
  const GRID_PAD = 16
  const GRID_GAP = 10
  const VIEWPORT_SIDE_PAD = 12

  let activeTab = $state<TabId>('all')
  let sort = $state<Sort>('newest')
  let query = $state('')
  let filter = $state<Filter>(TAB_FILTERS.all)
  let zoom = $state(3)
  let gridEl = $state<HTMLElement | null>(null)
  let gridWidth = $state(0)
  let stats = $state<StorageStats | null>(null)
  let warning = $state<{ text: string; kind: 'warn' | 'report' } | null>(null)
  let error = $state<string | null>(null)
  let contextMenu = $state<{ item: ItemDto; x: number; y: number } | null>(null)
  let renameTarget = $state<ItemDto | null>(null)
  let renameValue = $state('')

  const showSkeleton = $derived(items.loading && items.list.length === 0)
  const empty = $derived(!items.loading && items.list.length === 0)
  const grouped = $derived(sort === 'newest' || sort === 'oldest')
  const columns = $derived.by(() => {
    const tileW = TILE_W[zoom] ?? 116
    const inner = Math.max(0, gridWidth - VIEWPORT_SIDE_PAD * 2 - GRID_PAD * 2 + GRID_GAP)
    return Math.max(1, Math.floor(inner / (tileW + GRID_GAP)))
  })
  const tabCounts = $derived<Record<TabId, number>>(
    items.counts ?? {
      all: 0,
      images: 0,
      text: 0,
      links: 0,
      files: 0,
      references: 0,
      pinned: 0,
    },
  )

  let currentClipboardId = $state<number | null>(null)

  $effect(() => {
    // Runs once: untrack keeps tab/sort/query changes from re-firing this
    // block, which would stack a new storage-warning listener per keystroke.
    untrack(() => {
      void settings.init()
      void items.load(activeTab, sort, query)
      void refreshStats()
      void popupReady()
    })
    let unlistenCurrent: UnlistenFn | null = null
    // Which item the clipboard holds now. Asked once at startup, then kept
    // current by the event — the backend knows at exactly two moments, a
    // capture landing and a write we made ourselves.
    untrack(() => {
      void getCurrentClipboardId().then((id) => {
        currentClipboardId = id
      })
    })
    void onClipboardCurrent((id) => {
      currentClipboardId = id
    }).then((fn) => {
      unlistenCurrent = fn
    })

    let unlisten: UnlistenFn | null = null
    void onStorageWarning((w) => {
      warning = {
        text: storageWarningText(w),
        kind: w.removedItems > 0 ? 'report' : 'warn',
      }
      if (w.removedItems > 0) {
        // The janitor prunes without emitting items-deleted; the banner is
        // the only signal that the view may be stale. Refetch silently.
        items.refreshView()
      }
    }).then((fn) => {
      unlisten = fn
    })
    return () => {
      unlisten?.()
      unlistenCurrent?.()
    }
  })

  $effect(() => {
    zoom = settings.current.window.zoomStep
  })

  $effect(() => {
    const ids = items.list.map((i) => i.id)
    selection.syncOrder(ids)
    // Prune selection of ids the view no longer contains — a janitor prune,
    // a clear_history, or a refetch after deletion. The loaded page is the
    // only thing that can stay selected; anything else is a ghost that would
    // ride along on the next copy or delete.
    const alive = new Set(ids)
    if ([...selection.ids].some((id) => !alive.has(id))) {
      selection.ids = new Set([...selection.ids].filter((id) => alive.has(id)))
    }
    const f = selection.focusedId
    if (f !== null && items.list.length > 0 && !alive.has(f)) {
      selection.focusedId = null
    }
  })

  $effect(() => {
    void items.counts
    void refreshStats()
  })

  $effect(() => {
    const el = gridEl
    if (!el) return
    const onWheel = (e: WheelEvent): void => {
      if (!e.ctrlKey) return
      e.preventDefault()
      const dir = e.deltaY < 0 ? 1 : -1
      const next = Math.max(1, Math.min(5, zoom + dir))
      if (next !== zoom) setZoom(next)
    }
    // Scroll events do not bubble, but they do propagate through capture, so
    // this catches whichever descendant is the real scroller (the virtualized
    // grid's own overflow container, which is what actually scrolls). loadMore
    // guards against re-entry, so repeated events in the threshold zone are
    // harmless.
    const onScrollCapture = (e: Event): void => {
      const scroller = e.target as HTMLElement | null
      // The menu must not float over a different set of cards than the one it
      // was opened for; any grid scroll dismisses it.
      if (contextMenu) contextMenu = null
      if (!scroller) return
      const overflow = scroller.scrollHeight - scroller.clientHeight
      if (overflow < 100) return
      if (scroller.scrollTop + scroller.clientHeight >= scroller.scrollHeight - 600) {
        void items.loadMore()
      }
    }
    el.addEventListener('wheel', onWheel, { passive: false })
    el.addEventListener('scroll', onScrollCapture, true)
    return () => {
      el.removeEventListener('wheel', onWheel)
      el.removeEventListener('scroll', onScrollCapture, true)
    }
  })

  $effect(() => {
    const onKey = (e: KeyboardEvent): void => {
      const t = e.target as HTMLElement | null
      if (t instanceof HTMLInputElement || t instanceof HTMLTextAreaElement) {
        if (t.id === 'rename-input') {
          if (e.key === 'Enter') {
            e.preventDefault()
            void doRename()
          } else if (e.key === 'Escape') {
            e.preventDefault()
            renameTarget = null
          }
          return
        }
        if (e.key === 'Escape') {
          e.preventDefault()
          void hidePopup()
        } else if (e.key === 'Enter') {
          e.preventDefault()
          copySelection()
        }
        return
      }
      if (t instanceof HTMLButtonElement) {
        if (e.key === 'Escape') {
          e.preventDefault()
          void hidePopup()
        }
        return
      }
      if (e.key === 'Escape') {
        e.preventDefault()
        if (contextMenu) contextMenu = null
        else void hidePopup()
        return
      }
      if (e.ctrlKey && !e.altKey && !e.metaKey && (e.key === 'a' || e.key === 'A')) {
        e.preventDefault()
        selection.all(items.list.map((i) => i.id))
        return
      }
      if (e.key === 'Enter') {
        e.preventDefault()
        copySelection()
        return
      }
      if (e.key.startsWith('Arrow')) {
        e.preventDefault()
        const dx = e.key === 'ArrowLeft' ? -1 : e.key === 'ArrowRight' ? 1 : 0
        const dy = e.key === 'ArrowUp' ? -1 : e.key === 'ArrowDown' ? 1 : 0
        // Arrow-down at the bottom of the loaded page loads the next page so
        // keyboard navigation never dead-ends at the page boundary.
        if (dy > 0 && items.hasMore && selection.focusedId !== null) {
          const idx = items.list.findIndex((i) => i.id === selection.focusedId)
          if (idx >= items.list.length - columns) void items.loadMore()
        }
        selection.moveFocus(dx, dy, columns)
        return
      }
      if (!e.ctrlKey && !e.altKey && !e.metaKey && e.key.length === 1) {
        focusSearch()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })

  // While the context menu is open, dismiss it on any interaction that is not
  // with the menu itself: a left or right click anywhere outside, the popup
  // losing focus, being hidden or minimised, or the document going invisible.
  // Esc is handled by the keydown listener above. The menu element carries
  // class="menu", so clicks on its own items pass through to their action.
  $effect(() => {
    if (!contextMenu) return
    const insideMenu = (t: EventTarget | null): boolean =>
      t instanceof HTMLElement && t.closest('.menu') !== null
    const close = (): void => {
      contextMenu = null
    }
    const onPointerDown = (e: PointerEvent): void => {
      if (!insideMenu(e.target)) close()
    }
    const onContextMenu = (e: MouseEvent): void => {
      // Never let the native menu appear over ours, and close on a right-click
      // outside the menu (a right-click on another card re-opens it there via
      // the card's own handler, which is the usual desktop behaviour).
      e.preventDefault()
      if (!insideMenu(e.target)) close()
    }
    const onBlur = (): void => close()
    const onVisibility = (): void => {
      if (document.visibilityState === 'hidden') close()
    }
    window.addEventListener('pointerdown', onPointerDown, true)
    window.addEventListener('contextmenu', onContextMenu, true)
    window.addEventListener('blur', onBlur)
    document.addEventListener('visibilitychange', onVisibility)
    return () => {
      window.removeEventListener('pointerdown', onPointerDown, true)
      window.removeEventListener('contextmenu', onContextMenu, true)
      window.removeEventListener('blur', onBlur)
      document.removeEventListener('visibilitychange', onVisibility)
    }
  })

  function refreshStats(): Promise<void> {
    return getStorageStats()
      .then((s) => {
        stats = s
      })
      .catch(() => {
        // Backend not reachable yet (parallel build); retry on next event.
      })
  }

  function fmtBytes(n: number): string {
    if (n < 1024) return `${n} B`
    const units = ['KB', 'MB', 'GB', 'TB']
    let v = n
    let u = -1
    do {
      v /= 1024
      u++
    } while (v >= 1024 && u < units.length - 1)
    return `${v.toFixed(v >= 100 ? 0 : v >= 10 ? 1 : 2)} ${units[u]}`
  }

  /** The 90% pre-deletion warning and the post-prune report read differently:
   * one tells the user what is about to happen, the other what already did. */
  function storageWarningText(w: StorageWarning): string {
    if (w.removedItems > 0) {
      return `Clipboard store full — removed ${w.removedItems} old items to free ${fmtBytes(w.freedBytes)}.`
    }
    if (w.capBytes && w.capBytes > 0) {
      const pct = Math.round((w.usedBytes / w.capBytes) * 100)
      return `Clipboard store is ${pct}% full (${fmtBytes(w.usedBytes)} of ${fmtBytes(w.capBytes)}). Older items will be pruned automatically.`
    }
    return `Clipboard store is at ${fmtBytes(w.usedBytes)}.`
  }

  function setZoom(n: number): void {
    zoom = n
    void settings.patch({ window: { zoomStep: n } })
  }

  function onZoomChange(n: number): void {
    setZoom(n)
  }

  // -- toolbar / tabs ---------------------------------------------------------

  function onTabSelect(t: TabId): void {
    activeTab = t
    filter = TAB_FILTERS[t]
    selection.clear()
    void items.load(t, sort, query)
  }

  function onToolbarFilter(f: Filter): void {
    filter = f
    for (const t of Object.keys(TAB_FILTERS) as TabId[]) {
      if (sameFilter(TAB_FILTERS[t], f)) {
        activeTab = t
        break
      }
    }
    selection.clear()
    void items.applyFilter(f, sort, query)
  }

  function sameFilter(a: Filter, b: Filter): boolean {
    return (
      a.kind === b.kind &&
      a.ext === b.ext &&
      a.pinnedOnly === b.pinnedOnly &&
      a.subKind === b.subKind &&
      a.referencesOnly === b.referencesOnly
    )
  }

  function onQuery(q: string): void {
    query = q
    selection.clear()
    // applyFilter keeps the toolbar's kind/ext filter — load() would reset it
    // to the tab preset and silently drop what the user picked.
    void items.applyFilter(filter, sort, q)
  }

  function onSort(s: Sort): void {
    sort = s
    void items.applyFilter(filter, sort, query)
  }

  async function onAdd(): Promise<void> {
    const picked = await open({ multiple: true, directory: false, title: 'Add to Rebuffer' })
    if (picked && picked.length > 0) {
      try {
        await addFiles(picked)
      } catch (err) {
        error = String(err)
      }
    }
  }

  function focusSearch(): void {
    document.querySelector<HTMLInputElement>('#rebuffer-toolbar input')?.focus()
  }

  // -- grid -------------------------------------------------------------------

  function onGridActivate(item: ItemDto): void {
    // Double-click is an explicit copy: the item alone, or the whole
    // selection when the double-clicked card is part of one.
    const ids =
      selection.ids.size > 0 && selection.ids.has(item.id) ? [...selection.ids] : [item.id]
    copyItem(ids)
  }

  function onGridToggle(item: ItemDto, mode: 'single' | 'ctrl' | 'shift'): void {
    if (mode === 'single') {
      // Plain click with nothing selected is the quick-copy gesture; with a
      // selection it single-selects (and sets the shift-range anchor) so a
      // user can pick one item without ever copying it.
      if (selection.ids.size === 0) {
        copyItem([item.id])
        return
      }
      selection.toggle(item.id, 'single')
      return
    }
    // Ctrl+click toggles, Shift+click extends the range from the anchor.
    // Modifiers never copy: plain-text paste lives on the context menu
    // (Copy as plain text), where it cannot be confused with range selection.
    selection.toggle(item.id, mode)
  }

  function onCardContextMenu(item: ItemDto, x: number, y: number): void {
    contextMenu = { item, x, y }
  }

  /** The single copy path. With closeOnCopy the popup is about to vanish back
   * into the previously focused window, so the paste-back command is used:
   * it writes the clipboard, hides the popup, and — when behavior.autoPaste
   * is on — injects Ctrl+V there (SPEC §5.4). With closeOnCopy off the copy
   * must not disturb the user's window, so only the clipboard write happens.
   * `plain` defaults to behavior.pasteAsPlainText. */
  function copyItem(ids: number[], plain?: boolean): void {
    const p = settings.current.behavior.closeOnCopy
      ? pasteToPreviousWindow(ids, plain ?? settings.current.behavior.pasteAsPlainText)
      : copyToClipboard(ids, plain ?? settings.current.behavior.pasteAsPlainText)
    void p.catch((err) => {
      error = String(err)
    })
  }

  function copySelection(): void {
    if (selection.ids.size > 0) {
      copyItem([...selection.ids])
      return
    }
    const id = selection.focusedId
    if (id !== null) copyItem([id])
  }

  /** Surfaces a rejected command in the banner instead of dropping it. */
  function guarded(p: Promise<unknown>): void {
    void p.catch((err) => {
      error = String(err)
    })
  }

  function onDragStart(e: DragEvent): void {
    const card = (e.target as HTMLElement | null)?.closest?.('[data-id]') as HTMLElement | null
    const fromId = card ? Number(card.dataset.id) : NaN
    let ids: number[]
    if (!Number.isNaN(fromId) && selection.ids.has(fromId)) ids = [...selection.ids]
    else if (!Number.isNaN(fromId)) ids = [fromId]
    else if (selection.focusedId !== null) ids = [selection.focusedId]
    else return
    e.preventDefault()
    guarded(beginDrag(ids))
  }

  // -- context menu / rename ---------------------------------------------------

  async function onMenuAction(action: MenuAction): Promise<void> {
    const menu = contextMenu
    contextMenu = null
    if (!menu) return
    const item = menu.item
    const multi =
      selection.ids.size > 0 && selection.ids.has(item.id) ? [...selection.ids] : [item.id]
    switch (action) {
      case 'copy':
        copyItem(multi)
        break
      case 'copyPlain':
        guarded(copyToClipboard(multi, true))
        break
      case 'pin':
        guarded(setPinned(multi, true))
        break
      case 'unpin':
        guarded(setPinned(multi, false))
        break
      case 'open':
        guarded(openItem(item.id))
        break
      case 'openWith':
        guarded(openItemWith(item.id))
        break
      case 'reveal':
        guarded(showInFolder(item.id))
        break
      case 'rename':
        renameTarget = item
        renameValue = item.title ?? ''
        break
      case 'delete':
        try {
          await deleteItems(multi)
        } catch (err) {
          error = String(err)
        }
        selection.clear()
        break
      case 'saveAs': {
        const target = await save({
          title: 'Save item as…',
          defaultPath: suggestName(item),
          filters: [{ name: 'All files', extensions: ['*'] }],
        })
        if (target) {
          try {
            await saveItemAs(item.id, target)
          } catch (err) {
            error = String(err)
          }
        }
        break
      }
    }
  }

  function suggestName(item: ItemDto): string {
    const base = item.title || `rebuffer-${item.id}`
    const ext = item.ext?.toLowerCase() ?? (item.kind === 'text' ? 'txt' : '')
    return ext ? `${base}.${ext}` : base
  }

  async function doRename(): Promise<void> {
    const target = renameTarget
    if (!target) return
    const title = renameValue.trim()
    renameTarget = null
    if (title === (target.title ?? '')) return
    try {
      await renameItem(target.id, title)
    } catch (err) {
      error = String(err)
    }
  }
</script>

<div class="popup">
  {#if error}
    <div class="banner error" role="alert">
      <span>{error}</span>
      <button onclick={() => { error = null }}>Dismiss</button>
    </div>
  {/if}
  {#if warning}
    <div class="banner" class:report={warning.kind === 'report'} role="alert">
      <span>{warning.text}</span>
      <button onclick={() => { warning = null }}>Dismiss</button>
    </div>
  {/if}

  <header class="header">
    <div id="rebuffer-toolbar">
      <Toolbar
        {query}
        {sort}
        {filter}
        facets={items.facets}
        onquery={onQuery}
        onsort={onSort}
        onfilter={onToolbarFilter}
        onadd={onAdd}
      />
    </div>
    <Tabs active={activeTab} counts={tabCounts} onselect={onTabSelect} />
  </header>

  <div
    class="grid-viewport"
    role="group"
    aria-label="Clipboard items"
    bind:this={gridEl}
    bind:clientWidth={gridWidth}
    ondragstart={onDragStart}
  >
    {#if showSkeleton}
      <Skeleton count={24} />
    {:else if empty}
      <EmptyState tab={activeTab} />
    {:else}
      <Grid
        items={items.list}
        {zoom}
        {grouped}
        selectedIds={selection.ids}
        focusedId={selection.focusedId}
        showAge={settings.current.appearance.showAge}
        formatLabelSize={settings.current.appearance.formatLabelSize}
        animateGifs={settings.current.appearance.animateGifs}
        currentId={currentClipboardId}
        onactivate={onGridActivate}
        oncontextmenu={onCardContextMenu}
        ontoggle={onGridToggle}
      />
    {/if}
  </div>

  <footer class="footer">
    <StatusBar itemCount={tabCounts.all} totalBytes={stats?.totalBytes ?? 0} />
    <div class="corner">
      <button
        class="icon-btn"
        aria-label="Open settings"
        title="Settings"
        onclick={() => void showSettingsWindow()}
      >
        <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
          <path
            fill="currentColor"
            d="M8 5a3 3 0 1 0 0 6 3 3 0 0 0 0-6Zm5.9 3c0-.24-.02-.47-.06-.7l1.4-1.1-.9-1.6-1.66.66a5.9 5.9 0 0 0-1.2-.7L11.3 3H9.5l-.28 1.66a5.9 5.9 0 0 0-1.2.7l-1.66-.66-.9 1.6 1.4 1.1c-.04.23-.06.47-.06.7 0 .24.02.47.06.7l-1.4 1.1.9 1.6 1.66-.66c.36.29.76.53 1.2.7l.28 1.66h1.8l.28-1.66c.44-.17.84-.41 1.2-.7l1.66.66.9-1.6-1.4-1.1c.04-.23.06-.46.06-.7Z"
          />
        </svg>
      </button>
      <ZoomDial value={zoom} onchange={onZoomChange} />
    </div>
  </footer>

  {#if contextMenu}
    <ContextMenu
      item={contextMenu.item}
      x={contextMenu.x}
      y={contextMenu.y}
      onaction={onMenuAction}
    />
  {/if}

  {#if renameTarget}
    <div class="rename-overlay" role="dialog" aria-label="Rename item">
      <div class="rename-box">
        <input
          id="rename-input"
          bind:value={renameValue}
          placeholder="Display title"
          spellcheck="false"
        />
        <div class="rename-actions">
          <button class="primary" onclick={() => void doRename()}>Rename</button>
          <button onclick={() => { renameTarget = null }}>Cancel</button>
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .popup {
    height: 100vh;
    display: flex;
    flex-direction: column;
    background: var(--bg, rgba(18, 20, 24, 0.92));
    color: var(--text-1, #e8eaf0);
    font-family: var(--font-ui, 'Segoe UI', system-ui, sans-serif);
    font-size: 13px;
  }

  .header {
    padding: 10px 12px 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .grid-viewport {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 4px 12px 8px;
  }

  .footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 6px 12px;
    border-top: 1px solid var(--border-1, rgba(255, 255, 255, 0.08));
  }

  .corner {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .icon-btn {
    display: grid;
    place-items: center;
    padding: 6px;
    border: none;
    border-radius: 8px;
    background: transparent;
    color: var(--text-2, #9aa3b2);
    cursor: pointer;
  }

  .icon-btn:hover {
    color: var(--text-1, #e8eaf0);
    background: var(--hover, rgba(255, 255, 255, 0.06));
  }

  .banner {
    position: fixed;
    top: 10px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 60;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 14px;
    border-radius: 10px;
    background: var(--accent, #7aa2ff);
    color: #0b0d10;
    font-weight: 600;
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.35);
  }

  .banner.report {
    background: #9ece6a;
  }

  .banner.error {
    top: 56px;
    background: #f7768e;
  }

  .banner.error {
    background: #f7768e;
  }

  .banner button {
    border: none;
    border-radius: 6px;
    padding: 3px 10px;
    background: rgba(0, 0, 0, 0.18);
    color: inherit;
    font-weight: 600;
    cursor: pointer;
  }

  .rename-overlay {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    background: rgba(5, 6, 8, 0.45);
    z-index: 50;
  }

  .rename-box {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-width: 320px;
    padding: 16px;
    border-radius: 14px;
    background: var(--panel, #1c1f26);
    border: 1px solid var(--border-1, rgba(255, 255, 255, 0.1));
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.5);
  }

  .rename-box input {
    border: 1px solid var(--border-1, rgba(255, 255, 255, 0.12));
    border-radius: 8px;
    padding: 8px 10px;
    background: var(--input-bg, rgba(255, 255, 255, 0.05));
    color: var(--text-1, #e8eaf0);
    font: inherit;
    outline: none;
  }

  .rename-box input:focus {
    border-color: var(--accent, #7aa2ff);
  }

  .rename-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  .rename-actions button {
    border: 1px solid var(--border-1, rgba(255, 255, 255, 0.12));
    border-radius: 8px;
    padding: 6px 14px;
    background: transparent;
    color: var(--text-1, #e8eaf0);
    font: inherit;
    cursor: pointer;
  }

  .rename-actions button.primary {
    background: var(--accent, #7aa2ff);
    border-color: transparent;
    color: #0b0d10;
    font-weight: 600;
  }
</style>