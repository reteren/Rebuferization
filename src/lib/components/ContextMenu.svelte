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
    onaction?: (id: MenuAction) => void
  }

  let { item, x, y, onaction }: Props = $props()

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
</script>

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