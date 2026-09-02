<script lang="ts">
  import type { ItemDto } from '../types'

  export type MenuAction =
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

  interface Props {
    item: ItemDto
    x: number
    y: number
    /// The box of the card the menu was opened on. A link's full address is
    /// shown against this rather than against the cursor, so the plaque sits
    /// over the grid above the tile instead of on top of it.
    anchor?: DOMRect | null
    onaction?: (id: MenuAction) => void
  }

  let { item, x, y, anchor = null, onaction }: Props = $props()

  interface Entry {
    id: MenuAction
    label: string
    danger?: boolean
  }

  const entries = $derived.by((): Entry[] => {
    const out: Entry[] = [{ id: 'copy', label: 'Copy' }]
    if (item.subKind === 'rich') out.push({ id: 'copyPlain', label: 'Copy as plain text' })
    if (item.kind === 'image' || item.kind === 'video' || item.kind === 'file') {
      out.push({ id: 'open', label: 'Open' })
      out.push({ id: 'openWith', label: 'Open with' })
    }
    out.push(item.pinned ? { id: 'unpin', label: 'Unpin' } : { id: 'pin', label: 'Pin' })
    out.push({ id: 'saveAs', label: 'Save as' })
    out.push({ id: 'reveal', label: 'Show in folder' })
    out.push({ id: 'rename', label: 'Rename' })
    out.push({ id: 'delete', label: 'Delete', danger: true })
    return out
  })

  let menuEl = $state<HTMLDivElement | null>(null)
  let dx = $state(0)
  let dy = $state(0)

  $effect(() => {
    const node = menuEl
    if (!node) return
    const r = node.getBoundingClientRect()
    dx = x + r.width + 8 > window.innerWidth ? r.width + 8 : 0
    dy = y + r.height + 8 > window.innerHeight ? r.height + 8 : 0
  })

  // A card can only show a link as a hostname, so the one thing a right-click
  // on it cannot tell you is where it actually points. The plaque says it in
  // full, and it lives and dies with the menu: opening one elsewhere, clicking
  // away, Esc, or the popup closing all take it with them.
  const linkUrl = $derived(item.subKind === 'link' ? item.previewText : null)

  const MARGIN = 8

  let plaqueEl = $state<HTMLDivElement | null>(null)
  let px = $state(0)
  let py = $state(0)
  let placed = $state(false)

  $effect(() => {
    const node = plaqueEl
    const a = anchor
    if (!node || !a) return
    void linkUrl
    // Measured, not guessed: the plaque wraps to as many lines as the address
    // needs, so its height is only known once it is in the DOM.
    const r = node.getBoundingClientRect()
    px = Math.min(
      Math.max(MARGIN, a.left + a.width / 2 - r.width / 2),
      Math.max(MARGIN, window.innerWidth - r.width - MARGIN),
    )
    const above = a.top - MARGIN - r.height
    // Above the tile is the point — it keeps the card itself visible. Only a
    // card near the top of the grid, with no room, gets it underneath.
    py = above >= MARGIN ? above : Math.min(a.bottom + MARGIN, window.innerHeight - r.height - MARGIN)
    placed = true
  })
</script>

{#if linkUrl && anchor}
  <div class="link-plaque" class:placed bind:this={plaqueEl} style="left:{px}px; top:{py}px" role="tooltip">
    {linkUrl}
  </div>
{/if}

<div
  class="menu"
  bind:this={menuEl}
  style="left:{x}px; top:{y}px; translate:{-dx}px {-dy}px"
  role="menu"
  tabindex="-1"
  aria-label="Item actions"
  oncontextmenu={(e) => e.preventDefault()}
>
  {#each entries as e}
    <button
      type="button"
      role="menuitem"
      class="item"
      class:danger={e.danger}
      onclick={() => onaction?.(e.id)}
    >
      {e.label}
    </button>
  {/each}
</div>

<style>
  /* Same body as the menu, so the two read as one thing opened together. It
     never takes a click: the address is there to be read, and a click anywhere
     outside the menu is a dismissal. Rendered at 0,0 and only revealed once
     measured, so it is never seen in the wrong place first. */
  .link-plaque {
    position: fixed;
    z-index: 1001;
    max-width: min(420px, calc(100vw - 16px));
    padding: 7px 10px;
    border-radius: var(--r-md);
    background: color-mix(in srgb, var(--surface-3) 88%, transparent);
    border: 1px solid var(--border-2);
    box-shadow: var(--shadow-3);
    backdrop-filter: blur(var(--glass-blur)) saturate(1.3);
    color: var(--text-1);
    font-family: var(--font-mono);
    font-size: var(--fs-xs);
    line-height: 1.45;
    text-align: left;
    /* A URL has no spaces to break at, so it must be allowed to break
       anywhere; five lines is more than any address worth reading. */
    overflow-wrap: anywhere;
    display: -webkit-box;
    line-clamp: 5;
    -webkit-line-clamp: 5;
    -webkit-box-orient: vertical;
    overflow: hidden;
    pointer-events: none;
    user-select: none;
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease-out);
  }

  .link-plaque.placed {
    opacity: 1;
    animation: menu-in var(--dur-fast) var(--ease-out);
  }

  .menu {
    position: fixed;
    z-index: 1000;
    min-width: 200px;
    padding: 5px;
    border-radius: var(--r-md);
    background: color-mix(in srgb, var(--surface-3) 88%, transparent);
    border: 1px solid var(--border-2);
    box-shadow: var(--shadow-3);
    backdrop-filter: blur(var(--glass-blur)) saturate(1.3);
    animation: menu-in var(--dur-fast) var(--ease-out);
    transform-origin: top left;
  }

  @keyframes menu-in {
    from {
      opacity: 0;
      transform: scale(0.96);
    }
    to {
      opacity: 1;
      transform: scale(1);
    }
  }

  .item {
    display: block;
    width: 100%;
    text-align: left;
    padding: 7px 10px;
    border: none;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--text-1);
    font-size: var(--fs-sm);
    cursor: pointer;
    white-space: nowrap;
  }

  .item:hover {
    background: var(--accent-soft);
  }

  .item.danger {
    color: var(--danger);
  }

  .item.danger:hover {
    background: var(--danger-soft);
  }
</style>