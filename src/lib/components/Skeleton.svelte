<script lang="ts">
  let { count = 24 }: { count?: number } = $props()

  const tiles = $derived(Array.from({ length: Math.max(0, count) }, (_, i) => i))
</script>

<div class="skeleton" role="status" aria-label="Loading">
  {#each tiles as i (i)}
    <div class="tile" style="--skel-delay:{i % 8 * 0.12}s"></div>
  {/each}
</div>

<style>
  .skeleton {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(104px, 156px));
    gap: var(--grid-gap);
    padding: var(--grid-pad);
    align-content: start;
    height: 100%;
    overflow: hidden;
  }

  .tile {
    position: relative;
    aspect-ratio: 3 / 4;
    border-radius: var(--card-radius);
    background: var(--surface-1);
    border: 1px solid var(--border-1);
    overflow: hidden;
  }

  .tile::after {
    content: '';
    position: absolute;
    inset: 0;
    background: var(--surface-3);
    opacity: 0;
    animation: skel-pulse 1.5s var(--ease-in-out) infinite;
    animation-delay: var(--skel-delay, 0s);
  }

  @keyframes skel-pulse {
    0%,
    100% {
      opacity: 0;
    }
    50% {
      opacity: 0.4;
    }
  }
</style>