<script lang="ts">
  import { untrack } from 'svelte'
  import Card from './Card.svelte'
  import type { ItemDto } from '../types'

  interface Props {
    items: ItemDto[]
    zoom: number
    grouped: boolean
    selectedIds: Set<number>
    focusedId: number | null
    showAge: boolean
    formatLabelSize: string
    animateGifs: boolean
    onactivate?: (item: ItemDto) => void
    oncontextmenu?: (item: ItemDto, x: number, y: number) => void
    ontoggle?: (item: ItemDto, mode: 'single' | 'ctrl' | 'shift') => void
  }

  let {
    items,
    zoom,
    grouped,
    selectedIds,
    focusedId,
    showAge,
    formatLabelSize,
    animateGifs,
    onactivate,
    oncontextmenu,
    ontoggle,
  }: Props = $props()

  const TILE_W = [72, 92, 116, 148, 188] as const
  const TILE_H = [96, 122, 154, 196, 250] as const
  const OVERSCAN_PX = 320

  let el = $state<HTMLDivElement | null>(null)
  let scrollTop = $state(0)
  let clientWidth = $state(0)
  let clientHeight = $state(0)
  let gap = $state(10)
  let pad = $state(16)
  let headerH = $state(32)

  $effect(() => {
    const node = el
    if (!node) return
    const cs = getComputedStyle(node)
    gap = parseFloat(cs.getPropertyValue('--grid-gap')) || 10
    pad = parseFloat(cs.getPropertyValue('--grid-pad')) || 16
    headerH = parseFloat(cs.getPropertyValue('--group-header-h')) || 32
  })

  const z = $derived(Math.min(5, Math.max(1, Math.round(zoom))))
  const tileW = $derived(TILE_W[z - 1] ?? 116)
  const tileH = $derived(TILE_H[z - 1] ?? 154)
  const pitchW = $derived(tileW + gap)
  const pitchH = $derived(tileH + gap)
  const columns = $derived(
    clientWidth > 0 ? Math.max(1, Math.floor((clientWidth - pad * 2 + gap) / pitchW)) : 1,
  )

  const labelSize = $derived(
    formatLabelSize === 'off' || formatLabelSize === 'small' || formatLabelSize === 'medium' || formatLabelSize === 'large'
      ? formatLabelSize
      : 'medium',
  )

  function groupLabel(ts: number): string {
    const d = new Date(ts)
    const now = new Date()
    const day = Date.UTC(d.getFullYear(), d.getMonth(), d.getDate())
    const today = Date.UTC(now.getFullYear(), now.getMonth(), now.getDate())
    if (day === today) return 'Today'
    if (day === today - 86_400_000) return 'Yesterday'
    const opts: Intl.DateTimeFormatOptions =
      d.getFullYear() === now.getFullYear()
        ? { day: 'numeric', month: 'long' }
        : { day: 'numeric', month: 'long', year: 'numeric' }
    return d.toLocaleDateString('en-GB', opts)
  }

  interface GroupInfo {
    key: string
    label: string
    items: ItemDto[]
    top: number
    rows: number
    height: number
  }

  const groups = $derived.by((): GroupInfo[] => {
    if (!grouped) return []
    const map = new Map<string, ItemDto[]>()
    for (const it of items) {
      const label = groupLabel(it.createdAt)
      const arr = map.get(label)
      if (arr) arr.push(it)
      else map.set(label, [it])
    }
    const out: GroupInfo[] = []
    let y = pad
    for (const [label, arr] of map) {
      const rows = Math.ceil(arr.length / columns)
      const height = headerH + rows * pitchH - gap
      out.push({ key: label, label, items: arr, top: y, rows, height })
      y += height + gap
    }
    return out
  })

  const totalHeight = $derived.by(() => {
    if (grouped) {
      let h = pad
      for (const g of groups) h += g.height + gap
      return h + pad
    }
    const rows = Math.ceil(items.length / columns)
    return pad * 2 + Math.max(0, rows * pitchH - gap)
  })

  const rowOf = $derived.by(() => {
    const map = new Map<number, number>()
    if (grouped) {
      for (const g of groups) {
        const base = g.top + headerH
        let i = 0
        for (const it of g.items) {
          map.set(it.id, base + Math.floor(i / columns) * pitchH)
          i++
        }
      }
    } else {
      let i = 0
      for (const it of items) {
        map.set(it.id, pad + Math.floor(i / columns) * pitchH)
        i++
      }
    }
    return map
  })

  interface VCell {
    id: number
    item: ItemDto
    x: number
    y: number
  }

  interface HeaderCell {
    key: string
    label: string
    top: number
    pinned: boolean
  }

  // Every group top and every card coordinate below is in the same document
  // space as the spacer: cards and headers are positioned directly inside the
  // spacer, so nothing is double-translated the way nesting a card inside a
  // translated group element was.
  //
  // The pinned header is the one CSS `position: sticky` used to provide: the
  // last group whose top has scrolled past the viewport top. Its header is
  // translated to sit at the viewport top and floats above the cards, which
  // scroll underneath it. Because the decision is made from the group tops
  // (not from a sticky layout box), it stays correct for every group, not just
  // the first, and after zoom re-layouts.
  const pinnedGroup = $derived.by((): GroupInfo | null => {
    if (!grouped || groups.length === 0) return null
    let last: GroupInfo | null = null
    for (const g of groups) {
      if (g.top <= scrollTop) last = g
      else break
    }
    return last
  })

  const visibleCards = $derived.by((): VCell[] => {
    const viewTop = scrollTop - OVERSCAN_PX
    const viewBottom = scrollTop + clientHeight + OVERSCAN_PX
    const cards: VCell[] = []
    if (grouped) {
      for (const g of groups) {
        if (g.top + g.height < viewTop || g.top > viewBottom) continue
        const base = g.top + headerH
        const firstRow = Math.max(0, Math.floor((viewTop - base) / pitchH))
        const lastRow = Math.min(g.rows - 1, Math.floor((viewBottom - base) / pitchH))
        const start = firstRow * columns
        const end = Math.min(g.items.length, (lastRow + 1) * columns)
        for (let i = start; i < end; i++) {
          const it = g.items[i]
          if (!it) continue
          cards.push({
            id: it.id,
            item: it,
            x: pad + (i % columns) * pitchW,
            y: base + Math.floor(i / columns) * pitchH,
          })
        }
      }
    } else if (items.length > 0 && clientHeight > 0) {
      const firstRow = Math.max(0, Math.floor((viewTop - pad) / pitchH))
      const lastRow = Math.min(Math.ceil(items.length / columns) - 1, Math.floor((viewBottom - pad) / pitchH))
      for (let r = firstRow; r <= lastRow; r++) {
        for (let c = 0; c < columns; c++) {
          const it = items[r * columns + c]
          if (!it) break
          cards.push({ id: it.id, item: it, x: pad + c * pitchW, y: pad + r * pitchH })
        }
      }
    }
    return cards
  })

  const visibleHeaders = $derived.by((): HeaderCell[] => {
    if (!grouped) return []
    const viewTop = scrollTop - OVERSCAN_PX
    const viewBottom = scrollTop + clientHeight + OVERSCAN_PX
    const headers: HeaderCell[] = []
    for (const g of groups) {
      if (g.top + headerH < viewTop || g.top > viewBottom) continue
      headers.push({ key: g.key, label: g.label, top: g.top, pinned: g.key === pinnedGroup?.key })
    }
    // Once we have scrolled far past the pinned group its header's natural
    // position is above the overscan window, so the loop above did not render
    // it; render it pinned-only so the viewport-top header is never missing.
    if (pinnedGroup && pinnedGroup.top + headerH < viewTop) {
      headers.push({ key: pinnedGroup.key, label: pinnedGroup.label, top: pinnedGroup.top, pinned: true })
    }
    return headers
  })

  $effect(() => {
    const id = focusedId
    const node = el
    if (id == null || !node) return
    const top = rowOf.get(id)
    if (top == null) return
    const st = untrack(() => node.scrollTop)
    const vh = untrack(() => node.clientHeight)
    if (top < st || top + tileH > st + vh) {
      node.scrollTop = Math.max(0, top - vh / 3)
    }
  })

  function onScroll(): void {
    if (el) scrollTop = el.scrollTop
  }
</script>

<div
  class="grid"
  bind:this={el}
  bind:clientWidth={clientWidth}
  bind:clientHeight={clientHeight}
  role="grid"
  aria-rowcount={items.length}
  onscroll={onScroll}
>
  {#if items.length > 0}
    <div class="spacer" style="height:{totalHeight}px">
      {#each visibleHeaders as h (h.key)}
        <div
          class="group-header"
          class:pinned={h.pinned}
          style="top:{h.top}px;{h.pinned ? `transform: translateY(${scrollTop - h.top}px);` : ''}"
        >
          {h.label}
        </div>
      {/each}
      {#each visibleCards as c (c.id)}
        <Card
          item={c.item}
          selected={selectedIds.has(c.item.id)}
          focused={c.item.id === focusedId}
          zoom={z}
          showAge={showAge}
          formatLabelSize={labelSize}
          animateGifs={animateGifs}
          style="position:absolute; left:{c.x}px; top:{c.y}px; width:{tileW}px; height:{tileH}px"
          onactivate={onactivate}
          oncontextmenu={oncontextmenu}
          ontoggle={ontoggle}
        />
      {/each}
    </div>
  {/if}
</div>

<style>
  .grid {
    position: relative;
    height: 100%;
    overflow-y: auto;
    overflow-x: hidden;
    overscroll-behavior: contain;
    contain: layout paint;
  }

  .spacer {
    position: relative;
    width: 100%;
  }

  .group-header {
    position: absolute;
    left: 0;
    top: 0;
    z-index: 6;
    height: var(--group-header-h);
    display: flex;
    align-items: center;
    padding: 0 4px;
    font-size: var(--fs-xs);
    font-weight: 600;
    letter-spacing: 0.05em;
    color: var(--text-2);
    background: color-mix(in srgb, var(--bg-0) 62%, transparent);
    backdrop-filter: blur(var(--glass-blur));
    user-select: none;
    -webkit-user-select: none;
  }

  .group-header.pinned {
    z-index: 7;
    will-change: transform;
  }
</style>
