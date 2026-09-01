<script lang="ts">
  import { THEME_LABELS, type Theme } from '../types'

  interface Props {
    theme: Theme
    selected: boolean
    onselect?: (t: Theme) => void
  }

  let { theme, selected, onselect }: Props = $props()
</script>

<!-- data-theme on the tile itself: the theme files match `[data-theme="x"]`
     as well as `:root[...]`, and custom properties inherit, so this subtree
     paints in ITS theme rather than the active one. That is the whole point of
     a preview — it has to show the theme you have not chosen yet. -->
<button
  type="button"
  class="tile"
  class:selected
  data-theme={theme}
  aria-pressed={selected}
  onclick={() => onselect?.(theme)}
>
  <span class="frame">
    <span class="titlebar">
      <span class="pill accent"></span>
      <span class="pill wide"></span>
      <span class="pill"></span>
    </span>
    <span class="tabs">
      <span class="tab on"></span>
      <span class="tab"></span>
      <span class="tab"></span>
    </span>
    <span class="grid">
      {#each Array.from({ length: 8 }, (_, i) => i) as i}
        <span class="card" class:live={i === 0}></span>
      {/each}
    </span>
    <span class="statusbar">
      <span class="pill sm"></span>
      <span class="dot"></span>
    </span>
  </span>
  <span class="name">{THEME_LABELS[theme]}</span>
</button>

<style>
  .tile {
    flex: 0 0 auto;
    display: grid;
    gap: 8px;
    width: 148px;
    padding: 10px;
    border: 1px solid var(--border-1);
    border-radius: var(--r-lg);
    /* The tile itself is painted in its own theme, so the whole swatch reads
       as that theme rather than as a thumbnail sitting on the current one. */
    background: var(--bg-0);
    cursor: pointer;
    transition:
      transform var(--dur-fast) var(--ease-out),
      border-color var(--dur-fast) var(--ease-out);
  }

  .tile:hover {
    transform: translateY(-2px);
  }

  .tile.selected {
    border-color: var(--accent);
    box-shadow: 0 0 0 1px var(--accent);
  }

  .frame {
    display: grid;
    gap: 4px;
    padding: 6px;
    border-radius: var(--r-sm);
    background: var(--bg-1);
    border: 1px solid var(--window-border);
  }

  .titlebar,
  .tabs,
  .statusbar {
    display: flex;
    align-items: center;
    gap: 3px;
  }

  .pill {
    height: 6px;
    flex: 0 0 14px;
    border-radius: var(--r-pill);
    background: var(--surface-2);
  }

  .pill.wide {
    flex: 1 1 auto;
    background: var(--surface-1);
  }

  .pill.sm {
    flex: 0 0 22px;
    height: 4px;
  }

  .pill.accent {
    background: var(--accent);
  }

  .tab {
    height: 5px;
    flex: 0 0 16px;
    border-radius: var(--r-pill);
    background: var(--surface-2);
  }

  .tab.on {
    background: var(--accent);
    flex-basis: 20px;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 3px;
  }

  .card {
    height: 17px;
    border-radius: 3px;
    background: var(--surface-1);
    border: 1px solid var(--border-1);
  }

  /* The live-clipboard ring, which is the accent's most visible job. */
  .card.live {
    border-color: var(--accent);
    box-shadow: 0 0 0 1px var(--accent);
  }

  .dot {
    margin-left: auto;
    width: 5px;
    height: 5px;
    border-radius: var(--r-pill);
    background: var(--accent);
  }

  .name {
    color: var(--text-1);
    font-size: var(--fs-sm);
    font-family: var(--font-ui);
    text-align: center;
  }
</style>
