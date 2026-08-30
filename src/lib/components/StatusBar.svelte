<script lang="ts">
  let { itemCount, totalBytes }: { itemCount: number; totalBytes: number } = $props()

  function formatBytes(n: number): string {
    if (!Number.isFinite(n) || n < 0) return '0 B'
    if (n < 1024) return `${n} B`
    const units = ['KB', 'MB', 'GB', 'TB'] as const
    let v = n
    let u = -1
    do {
      v /= 1024
      u++
    } while (v >= 1024 && u < units.length - 1)
    const unit = units[u] ?? 'GB'
    return `${v >= 100 ? Math.round(v) : v.toFixed(1)} ${unit}`
  }

  const countText = $derived(itemCount.toLocaleString('en-US'))
  const sizeText = $derived(formatBytes(totalBytes))
</script>

<div class="status">
  <span class="stats">{countText} items - {sizeText}</span>
</div>

<style>
  .status {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px var(--grid-pad);
    border-top: 1px solid var(--border-1);
    background: color-mix(in srgb, var(--surface-1) 45%, transparent);
    color: var(--text-3);
    font-size: var(--fs-xs);
    user-select: none;
    -webkit-user-select: none;
  }
</style>