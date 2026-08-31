<script lang="ts">
  interface Props {
    value: number
    onchange?: (n: number) => void
  }

  let { value, onchange }: Props = $props()

  const MIN = 1
  const MAX = 5
  const STEP_PX = 14

  // svelte-ignore state_referenced_locally
  let level = $state(value)
  let dragging = $state(false)
  let dragStartY = 0
  let dragBase = 0
  let wheelAcc = 0
  let lastSent = 0

  $effect(() => {
    if (!dragging) level = value
  })

  function send(n: number): void {
    const next = Math.min(MAX, Math.max(MIN, Math.round(n)))
    if (next === lastSent) return
    lastSent = next
    level = next
    onchange?.(next)
  }

  function onPointerDown(e: PointerEvent): void {
    dragging = true
    dragStartY = e.clientY
    dragBase = level
    lastSent = 0
    ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
  }

  function onPointerMove(e: PointerEvent): void {
    if (!dragging) return
    send(dragBase + (dragStartY - e.clientY) / STEP_PX)
  }

  function endDrag(): void {
    dragging = false
    lastSent = 0
  }

  function onWheel(e: WheelEvent): void {
    e.preventDefault()
    wheelAcc += e.deltaY
    while (wheelAcc <= -STEP_PX) {
      wheelAcc += STEP_PX
      send(level - 1)
    }
    while (wheelAcc >= STEP_PX) {
      wheelAcc -= STEP_PX
      send(level + 1)
    }
  }

  function onKeydown(e: KeyboardEvent): void {
    if (e.key === 'ArrowUp' || e.key === 'ArrowRight') {
      e.preventDefault()
      send(level + 1)
    } else if (e.key === 'ArrowDown' || e.key === 'ArrowLeft') {
      e.preventDefault()
      send(level - 1)
    }
  }
</script>

<div
  class="dial"
  class:dragging
  role="slider"
  aria-label="Zoom"
  aria-valuemin={MIN}
  aria-valuemax={MAX}
  aria-valuenow={level}
  tabindex="0"
  title="Drag or scroll to zoom"
  onkeydown={onKeydown}
  onpointerdown={onPointerDown}
  onpointermove={onPointerMove}
  onpointerup={endDrag}
  onpointercancel={endDrag}
  onwheel={onWheel}
>
  <div class="track">
    {#each Array.from({ length: MAX }, (_, i) => i + 1) as step}
      <!-- The fill rises from the bottom like a level meter: a bigger zoom
           lights the lower dots first, so increasing zoom fills upward. -->
      <span class="dot" class:active={step > MAX - level}></span>
    {/each}
  </div>
</div>

<style>
  .dial {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 3px;
    padding: 6px 5px;
    border-radius: var(--r-pill);
    background: var(--surface-2);
    border: 1px solid var(--border-1);
    cursor: ns-resize;
    user-select: none;
    -webkit-user-select: none;
    touch-action: none;
    transition: transform var(--dur-fast) var(--ease-out), opacity var(--dur-fast) var(--ease-out);
  }

  .dial:hover {
    background: var(--surface-4);
  }

  .dial.dragging {
    transform: scale(1.06);
  }

  .track {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 5px;
    padding: 2px 0;
  }

  .dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--accent);
    opacity: 0.22;
    transition: opacity var(--dur-fast) var(--ease-out), transform var(--dur-fast) var(--ease-out);
  }

  .dot.active {
    opacity: 1;
  }
</style>