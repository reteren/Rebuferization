<script lang="ts">
  import type { TabId } from '../types'

  let { tab }: { tab: TabId } = $props()

  const COPY: Record<TabId, { title: string; hint: string }> = {
    all: { title: 'Nothing here yet', hint: 'Copy anything - text, images, files - and it lands here.' },
    images: { title: 'No images yet', hint: 'Screenshots and pictures you copy will show up here.' },
    text: { title: 'No text yet', hint: 'Text you copy appears here, searchable in an instant.' },
    links: { title: 'No links yet', hint: 'Copied URLs collect in this tab.' },
    files: { title: 'No files yet', hint: 'Files copied from Explorer appear here.' },
    references: { title: 'Nothing added yet', hint: 'Use + Add to keep a file here by reference - it is never copied and never expires.' },
    pinned: { title: 'Nothing pinned', hint: 'Pin an item to keep it safe from cleanup.' },
  }

  const ICONS: Record<TabId, string> = {
    all: '<rect x="4.5" y="4.5" width="15" height="15" rx="2"/><path d="M9 9.5h6M9 13h6M9 16.5h4"/>',
    images: '<rect x="3.5" y="5" width="17" height="14" rx="2"/><circle cx="9.5" cy="10.5" r="1.5"/><path d="M3.5 16.5l4.5-4 3.5 3 3.5-3.5 5 4.5"/>',
    text: '<path d="M5 6.5h14M5 10.5h14M5 14.5h9M5 18h6"/>',
    links: '<path d="M10 13.5a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/><path d="M14 10.5a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/>',
    files: '<path d="M13 3.5H7.5a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h9a2 2 0 0 0 2-2V9z"/><path d="M13 3.5v5.5h5.5"/>',
    references: '<path d="M13 3.5H7.5a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h9a2 2 0 0 0 2-2V9z"/><path d="M13 3.5v5.5h5.5"/><path d="M12 12v5M9.5 14.5h5"/>',
    pinned: '<path d="M12 3a5 5 0 0 0-5 5c0 4.2 5 9.5 5 9.5s5-5.3 5-9.5a5 5 0 0 0-5-5Z"/><circle cx="12" cy="8" r="1.7"/><path d="M12 17.5V21"/>',
  }

  const copy = $derived(COPY[tab])
</script>

<div class="empty">
  <div class="glow"></div>
  <svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    {@html ICONS[tab]}
  </svg>
  <p class="title">{copy.title}</p>
  <p class="hint">{copy.hint}</p>
</div>

<style>
  .empty {
    position: relative;
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 4px;
    padding: 32px;
    text-align: center;
    user-select: none;
    -webkit-user-select: none;
  }

  .glow {
    position: absolute;
    width: 280px;
    height: 280px;
    border-radius: 50%;
    background: radial-gradient(circle, var(--accent-soft), transparent 65%);
    opacity: 0.7;
    pointer-events: none;
  }

  .icon {
    position: relative;
    width: 40px;
    height: 40px;
    color: var(--text-3);
    margin-bottom: 10px;
  }

  .title {
    position: relative;
    margin: 0;
    color: var(--text-2);
    font-size: var(--fs-lg);
    font-weight: 600;
  }

  .hint {
    position: relative;
    margin: 0;
    max-width: 320px;
    color: var(--text-3);
    font-size: var(--fs-sm);
    line-height: 1.5;
  }
</style>