<script lang="ts">
  import type { TabId } from '../types'

  const TABS: TabId[] = ['all', 'images', 'text', 'links', 'files', 'pinned']
  const LABELS: Record<TabId, string> = {
    all: 'All',
    images: 'Images',
    text: 'Text',
    links: 'Links',
    files: 'Files',
    pinned: 'Pinned',
  }

  let {
    active,
    counts,
    onselect,
  }: {
    active: TabId
    counts: Record<TabId, number>
    onselect?: (tab: TabId) => void
  } = $props()
</script>

<div class="tabs" role="tablist" aria-label="Filter by type">
  {#each TABS as tab}
    <button
      type="button"
      role="tab"
      class="tab"
      class:active={tab === active}
      aria-selected={tab === active}
      onclick={() => onselect?.(tab)}
    >
      <span class="label">{LABELS[tab]}</span>
      <span class="count">{counts[tab].toLocaleString('en-US')}</span>
    </button>
  {/each}
</div>

<style>
  .tabs {
    display: flex;
    gap: 2px;
  }

  .tab {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    border: none;
    border-radius: var(--r-pill);
    background: transparent;
    color: var(--text-2);
    font-size: var(--fs-sm);
    font-weight: 550;
    cursor: pointer;
  }

  .tab::after {
    content: '';
    position: absolute;
    inset: 0;
    border-radius: inherit;
    background: var(--surface-2);
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease-out);
    pointer-events: none;
  }

  .tab:hover::after {
    opacity: 1;
  }

  .tab.active {
    color: var(--text-1);
  }

  .tab.active::after {
    background: var(--accent-soft);
    opacity: 1;
  }

  .label {
    position: relative;
    z-index: 1;
  }

  .count {
    position: relative;
    z-index: 1;
    font-size: var(--fs-2xs);
    font-weight: 600;
    color: var(--text-3);
    background: var(--overlay-1);
    border-radius: var(--r-pill);
    padding: 1px 6px;
  }

  .tab.active .count {
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }
</style>