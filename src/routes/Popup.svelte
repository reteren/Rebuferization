<script lang="ts">
  // OWNER: worker W6. The popup: where the items/selection/settings stores
  // meet W5's component kit. No component calls invoke; this file is the only
  // consumer of both sides.

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
    getStorageStats,
    hidePopup,
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
  let contextMenu = $state<{ item: ItemDto; x: number; y: number } | null>(null)
  let renameTarget = $state<ItemDto | null>(null)
  let renameValue = $state('')

  let lastToggleAt = 0

  const showSkeleton = $derived(items.loading && items.list.length === 0)
  const empty = $derived(!items.loading && items.list.length === 0)
  const grouped = $derived(sort === 'newest' || sort === 'oldest')
  const columns = $derived.by(() => {
    const tileW = TILE_W[zoom] ?? 116
    const inner = Math.max(0, gridWidth - VIEWPORT_SIDE_PAD * 2 - GRID_PAD * 2 + GRID_GAP)
    return Math.max(1, Math.floor(inner / (tileW + GRID_GAP)))
  })
  const tabCounts = $derived<Record<TabId, number>>(
    items.counts ?? { all: 0, images: 0, text: 0, links: 0, files: 0, pinned: 0 },
  )

  $effect(() => {
    void settings.init()
    void items.load(activeTab, sort, query)
    void refreshStats()
    void popupReady()
    void onStorageWarning((w) => {
      warning = {
        text: storageWarningText(w),
        kind: w.removedItems > 0 ? 'report' : 'warn',
      }
    })
  })

  $effect(() => {
    zoom = settings.current.window.zoomStep
  })

  $effect(() => {
    selection.syncOrder(items.list.map((i) => i.id))
  })

  $effect(() => {
    const f = selection.focusedId
    if (f !== null && items.list.length > 0 && !items.list.some((i) => i.id === f)) {
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
    el.addEventListener('wheel', onWheel, { passive: false })
    return () => el.removeEventListener('wheel', onWheel)
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
          copyFocused()
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
        copyFocused()
        return
      }
      if (e.key.startsWith('Arrow')) {
        e.preventDefault()
        const dx = e.key === 'ArrowLeft' ? -1 : e.key === 'ArrowRight' ? 1 : 0
        const dy = e.key === 'ArrowUp' ? -1 : e.key === 'ArrowDown' ? 1 : 0
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
      a.kind === b.kind && a.ext === b.ext && a.pinnedOnly === b.pinnedOnly && a.subKind === b.subKind
    )
  }

  function onQuery(q: string): void {
    query = q
    selection.clear()
    void items.load(activeTab, sort, q)
  }

  function onSort(s: Sort): void {
    sort = s
    void items.load(activeTab, sort, query)
  }

  async function onAdd(): Promise<void> {
    const picked = await open({ multiple: true, directory: false, title: 'Add to Rebuffer' })
    if (picked && picked.length > 0) {
      try {
        await addFiles(picked)
      } catch {
        // Store not ready (parallel build) — nothing to reconcile.
      }
    }
  }

  function focusSearch(): void {
    document.querySelector<HTMLInputElement>('#rebuffer-toolbar input')?.focus()
  }

  // -- grid -------------------------------------------------------------------

  function onGridActivate(item: ItemDto): void {
    if (Date.now() - lastToggleAt < 40) return
    copyItem([item.id], false)
  }

  function onGridToggle(item: ItemDto, mode: 'single' | 'ctrl' | 'shift'): void {
    lastToggleAt = Date.now()
    selection.toggle(item.id, mode)
    if (mode === 'shift') void pasteToPreviousWindow([item.id], true)
  }

  function onCardContextMenu(item: ItemDto, x: number, y: number): void {
    contextMenu = { item, x, y }
  }

  function copyItem(ids: number[], plain: boolean): void {
    void copyToClipboard(ids, plain).then(() => {
      if (settings.current.behavior.closeOnCopy) void hidePopup()
    })
  }

  function copyFocused(): void {
    const id = selection.focusedId
    if (id !== null) copyItem([id], false)
  }

  function onGridScroll(e: Event): void {
    const el = e.target as HTMLElement | null
    if (!el) return
    const overflow = el.scrollHeight - el.clientHeight
    if (overflow < 100) return
    if (el.scrollTop + el.clientHeight >= el.scrollHeight - 600) void items.loadMore()
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
    void beginDrag(ids)
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
        copyItem(multi, false)
        break
      case 'copyPlain':
        void copyToClipboard(multi, true)
        break
      case 'pin':
        void setPinned(multi, true)
        break
      case 'unpin':
        void setPinned(multi, false)
        break
      case 'open':
        void openItem(item.id)
        break
      case 'openWith':
        void openItemWith(item.id)
        break
      case 'reveal':
        void showInFolder(item.id)
        break
      case 'rename':
        renameTarget = item
        renameValue = item.title ?? ''
        break
      case 'delete':
        await deleteItems(multi)
        selection.clear()
        break
      case 'saveAs': {
        const target = await save({
          title: 'Save item as…',
          defaultPath: suggestName(item),
          filters: [{ name: 'All files', extensions: ['*'] }],
        })
        if (target) await saveItemAs(item.id, target)
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
    } catch {
      // Backend not ready (parallel build).
    }
  }
</script>

<div class="popup">
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
    onscroll={onGridScroll}
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
        onactivate={onGridActivate}
        oncontextmenu={onCardContextMenu}
        ontoggle={onGridToggle}
      />
    {/if}
  </div>

  <footer class="footer">
    <StatusBar itemCount={tabCounts.all} totalBytes={stats?.totalBytes ?? 0} />
    <div class="corner">
      <button class="icon-btn" aria-label="Open settings" onclick={() => void showSettingsWindow()}>
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