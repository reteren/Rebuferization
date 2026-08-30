// OWNER: worker W6. Selection + focus for the grid.
//
// The anchor is the last *single* click, not the last item touched: ctrl-clicks
// and arrows never move it, so a shift-click always selects the range from the
// last plain click. The store knows the current view order (fed by the popup
// via syncOrder) so shift ranges and arrow moves follow the visible layout.

type ToggleMode = 'single' | 'ctrl' | 'shift'

class SelectionStore {
  ids = $state<Set<number>>(new Set())
  focusedId = $state<number | null>(null)

  private anchorId: number | null = null
  private order: number[] = []

  /** Keep in sync with the current page (Popup feeds it when the list changes). */
  syncOrder(ids: number[]): void {
    this.order = ids
  }

  toggle(id: number, mode: ToggleMode): void {
    this.focusedId = id
    if (mode === 'single') {
      this.ids = new Set([id])
      this.anchorId = id
      return
    }
    if (mode === 'ctrl') {
      const next = new Set(this.ids)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      this.ids = next
      return
    }
    // shift: range from the anchor to this item, in view order
    let anchor = this.anchorId
    if (anchor === null || !this.order.includes(anchor)) anchor = this.focusedId ?? id
    const a = this.order.indexOf(anchor)
    const b = this.order.indexOf(id)
    if (a < 0 || b < 0) {
      this.ids = new Set([id])
      return
    }
    const lo = Math.min(a, b)
    const hi = Math.max(a, b)
    this.ids = new Set(this.order.slice(lo, hi + 1))
  }

  clear(): void {
    this.ids = new Set()
  }

  /** Selects everything currently in view. */
  all(ids: number[]): void {
    this.ids = new Set(ids)
  }

  /** Moves focus across a grid of `columns` columns; never touches selection. */
  moveFocus(dx: number, dy: number, columns: number): void {
    if (this.order.length === 0) return
    let idx = this.focusedId === null ? 0 : this.order.indexOf(this.focusedId)
    if (idx < 0) idx = 0
    const cols = Math.max(1, columns)
    let target = idx + dy * cols + dx
    target = Math.max(0, Math.min(target, this.order.length - 1))
    const next = this.order[target]
    if (next !== undefined) this.focusedId = next
  }
}

export const selection = new SelectionStore()