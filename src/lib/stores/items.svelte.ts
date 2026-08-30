// OWNER: worker W6. Current-view paging at 200 items with loadMore, FTS
// search debounced 120 ms, extension facets and tab counts refreshed with the
// filter, and live reconciliation against item-added / item-updated /
// items-deleted — items are patched in place so a copy never makes the grid
// jump under the cursor.

import {
  getExtensionFacets,
  getTabCounts,
  listItems,
  onItemAdded,
  onItemsDeleted,
  onItemsUpdated,
  searchItems,
} from '../ipc'
import {
  TAB_FILTERS,
  type Facet,
  type Filter,
  type ItemDto,
  type Sort,
  type TabCounts,
  type TabId,
} from '../types'

const PAGE_SIZE = 200
const SEARCH_DEBOUNCE_MS = 120

const displayName = (i: ItemDto): string => i.title ?? i.previewText ?? i.ext ?? ''

type BufferedEvent =
  | { kind: 'added'; item: ItemDto }
  | { kind: 'updated'; ids: number[] }
  | { kind: 'deleted'; ids: number[] }

class ItemsStore {
  list = $state<ItemDto[]>([])
  loading = $state(false)
  hasMore = $state(false)
  facets = $state<Facet[]>([])
  counts = $state<TabCounts | null>(null)

  private filter: Filter = TAB_FILTERS.all
  private sort: Sort = 'newest'
  private query = ''
  private timer: ReturnType<typeof setTimeout> | undefined
  private running = false
  private rerun = false
  private loadingMore = false
  private waiters: Array<{ resolve: () => void; reject: (e: unknown) => void }> = []
  private buffered: BufferedEvent[] = []

  /** The primary API: a tab is just a preset filter. Debounced 120 ms. */
  load(tab: TabId, sort: Sort, query: string): Promise<void> {
    this.filter = TAB_FILTERS[tab]
    this.sort = sort
    this.query = query.trim()
    return this.schedule()
  }

  /** Same as load, for toolbar refinements (kind / extension) that do not
   * map onto a preset tab. */
  applyFilter(filter: Filter, sort: Sort, query: string): Promise<void> {
    this.filter = filter
    this.sort = sort
    this.query = query.trim()
    return this.schedule()
  }

  async loadMore(): Promise<void> {
    if (this.loading || this.loadingMore || !this.hasMore) return
    this.loadingMore = true
    const { filter, sort, query } = this
    try {
      if (query) {
        const limit = this.list.length + PAGE_SIZE
        const raw = await searchItems(query, filter, limit)
        this.list = raw.slice(0, limit)
        this.hasMore = raw.length > limit
      } else {
        const offset = this.list.length
        const raw = await listItems(filter, sort, offset, PAGE_SIZE)
        if (raw.length === 0) {
          this.hasMore = false
          return
        }
        this.list = [...this.list, ...raw]
        this.hasMore = raw.length >= PAGE_SIZE
      }
    } finally {
      this.loadingMore = false
    }
  }

  // -------------------------------------------------------------------------
  // internals
  // -------------------------------------------------------------------------

  private schedule(): Promise<void> {
    if (this.timer !== undefined) clearTimeout(this.timer)
    return new Promise((resolve, reject) => {
      this.waiters.push({ resolve, reject })
      this.timer = setTimeout(() => {
        this.timer = undefined
        void this.run()
      }, SEARCH_DEBOUNCE_MS)
    })
  }

  private async run(): Promise<void> {
    if (this.running) {
      this.rerun = true
      return
    }
    this.running = true
    this.loading = true
    const { filter, sort, query } = this
    try {
      const raw = query
        ? await searchItems(query, filter, PAGE_SIZE + 1)
        : await listItems(filter, sort, 0, PAGE_SIZE + 1)
      this.list = raw.slice(0, PAGE_SIZE)
      this.hasMore = raw.length > PAGE_SIZE
      for (const w of this.waiters) w.resolve()
      this.waiters = []
    } catch (e) {
      for (const w of this.waiters) w.reject(e)
      this.waiters = []
    } finally {
      this.loading = false
      this.running = false
      void this.refreshMeta()
      this.flush()
      if (this.rerun) {
        this.rerun = false
        void this.run()
      }
    }
  }

  /** Silent refetch of the current page. The old list stays visible until the
   * fresh one arrives, so nothing jumps. */
  private async refreshPage(): Promise<void> {
    const { filter, sort, query } = this
    try {
      const limit = Math.max(PAGE_SIZE + 1, this.list.length + PAGE_SIZE)
      const raw = query
        ? await searchItems(query, filter, limit)
        : await listItems(filter, sort, 0, limit)
      this.list = raw.slice(0, limit)
      this.hasMore = raw.length > limit
    } catch {
      // Keep the current list; the next explicit load will correct it.
    }
  }

  private async refreshMeta(): Promise<void> {
    const [facets, counts] = await Promise.allSettled([
      getExtensionFacets(this.filter),
      getTabCounts(),
    ])
    if (facets.status === 'fulfilled') this.facets = facets.value
    if (counts.status === 'fulfilled') this.counts = counts.value
  }

  // -- live events -----------------------------------------------------------

  private busy(): boolean {
    return this.running || this.timer !== undefined
  }

  private onAdded(item: ItemDto): void {
    if (this.busy()) {
      this.buffered.push({ kind: 'added', item })
      return
    }
    if (this.query) {
      // Rank order is unknowable client-side; a debounced silent re-search is
      // the least disruptive way to include a fresh capture.
      void this.schedule()
      return
    }
    const idx = this.list.findIndex((i) => i.id === item.id)
    if (idx >= 0) {
      // Dedup bump: same row, newer timestamp — refresh and re-sort in place.
      this.list[idx] = item
      this.resort(idx)
    } else if (this.matchesFilter(item)) {
      this.insertSorted(item)
    }
    void this.refreshMeta()
  }

  private onUpdated(ids: number[]): void {
    if (this.busy()) {
      this.buffered.push({ kind: 'updated', ids })
      return
    }
    void this.refreshMeta()
    if (ids.some((id) => this.list.some((i) => i.id === id))) void this.refreshPage()
  }

  private onDeleted(ids: number[]): void {
    if (this.busy()) {
      this.buffered.push({ kind: 'deleted', ids })
      return
    }
    if (ids.length === 0) {
      // clear_history signal: anything in the current view may be gone, and
      // the payload carries no ids — refresh silently and keep the old list
      // visible until the fresh page lands.
      void this.refreshMeta()
      void this.refreshPage()
      return
    }
    const gone = new Set(ids)
    const before = this.list.length
    this.list = this.list.filter((i) => !gone.has(i.id))
    if (this.list.length !== before) {
      void this.refreshMeta()
      if (this.hasMore) void this.refreshPage()
    }
  }

  private flush(): void {
    const pending = this.buffered
    this.buffered = []
    for (const e of pending) {
      if (e.kind === 'added') this.onAdded(e.item)
      else if (e.kind === 'updated') this.onUpdated(e.ids)
      else this.onDeleted(e.ids)
    }
  }

  // -- in-place ordering -----------------------------------------------------

  private matchesFilter(item: ItemDto): boolean {
    const f = this.filter
    if (f.pinnedOnly && !item.pinned) return false
    if (f.kind !== null && item.kind !== f.kind) return false
    if (f.subKind !== null && item.subKind !== f.subKind) return false
    if (f.ext !== null && (item.ext ?? '').toUpperCase() !== f.ext.toUpperCase()) return false
    return true
  }

  private compare(a: ItemDto, b: ItemDto): number {
    switch (this.sort) {
      case 'newest':
        return b.createdAt - a.createdAt || b.id - a.id
      case 'oldest':
        return a.createdAt - b.createdAt || a.id - b.id
      case 'nameAsc':
        return displayName(a).localeCompare(displayName(b)) || a.id - b.id
      case 'nameDesc':
        return displayName(b).localeCompare(displayName(a)) || b.id - a.id
      case 'sizeAsc':
        return a.byteSize - b.byteSize || a.id - b.id
      case 'sizeDesc':
        return b.byteSize - a.byteSize || b.id - a.id
    }
  }

  private sortedIndex(item: ItemDto): number {
    let lo = 0
    let hi = this.list.length
    while (lo < hi) {
      const mid = (lo + hi) >>> 1
      if (this.compare(this.list[mid]!, item) <= 0) lo = mid + 1
      else hi = mid
    }
    return lo
  }

  private insertSorted(item: ItemDto): void {
    const at = this.sortedIndex(item)
    if (at >= PAGE_SIZE) return // belongs on a later page
    this.list.splice(at, 0, item)
    if (this.list.length > PAGE_SIZE) this.list.length = PAGE_SIZE
  }

  private resort(idx: number): void {
    const item = this.list[idx]!
    this.list.splice(idx, 1)
    this.insertSorted(item)
  }

  /** Wires the live event listeners once; idempotent per module instance. */
  subscribe(): void {
    void onItemAdded((item) => this.onAdded(item))
    void onItemsUpdated((ids) => this.onUpdated(ids))
    void onItemsDeleted((ids) => this.onDeleted(ids))
  }
}

export const items = new ItemsStore()
items.subscribe()