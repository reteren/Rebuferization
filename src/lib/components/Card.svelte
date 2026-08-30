<script lang="ts">
  import type { ItemDto, Kind } from '../types'

  interface Props {
    item: ItemDto
    selected: boolean
    focused: boolean
    zoom: number
    showAge: boolean
    formatLabelSize: 'off' | 'small' | 'medium' | 'large'
    style?: string
    onactivate?: (item: ItemDto) => void
    oncontextmenu?: (item: ItemDto, x: number, y: number) => void
    ontoggle?: (item: ItemDto, mode: 'single' | 'ctrl' | 'shift') => void
  }

  let {
    item,
    selected,
    focused,
    zoom,
    showAge,
    formatLabelSize,
    style,
    onactivate,
    oncontextmenu,
    ontoggle,
  }: Props = $props()

  const KIND_FALLBACK: Record<Kind, string> = {
    text: 'TXT',
    image: 'IMG',
    video: 'VID',
    file: 'FILE',
    other: 'BIN',
  }

  const formatLabel = $derived(
    item.ext ??
      (item.subKind === 'link' ? 'URL' : item.subKind === 'color' ? 'HEX' : KIND_FALLBACK[item.kind]),
  )

  const ageLabel = $derived.by(() => {
    const ms = Math.max(0, Date.now() - item.createdAt)
    if (ms < 60_000) return 'now'
    const m = Math.floor(ms / 60_000)
    if (m < 60) return `${m}m`
    const h = Math.floor(m / 60)
    if (h < 24) return `${h}h`
    return `${Math.floor(h / 24)}d`
  })

  const textFont = $derived(`${(7.5 + (zoom - 1) * 1.6).toFixed(1)}px`)
  const labelFont = $derived(
    formatLabelSize === 'large' ? 'var(--fs-lg)' : formatLabelSize === 'medium' ? 'var(--fs-sm)' : 'var(--fs-2xs)',
  )

  const domain = $derived.by(() => {
    if (item.subKind !== 'link' || !item.previewText) return null
    try {
      return new URL(item.previewText).hostname.replace(/^www\./, '')
    } catch {
      return item.previewText
    }
  })

  const domainHue = $derived.by(() => {
    const s = domain ?? 'rebuffer'
    let h = 0
    for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) >>> 0
    return h % 360
  })

  const fileName = $derived(item.fileNames[0] ?? item.title ?? item.refPath ?? 'file')

  const extColor = $derived.by(() => {
    const palette = ['var(--ext-1)', 'var(--ext-2)', 'var(--ext-3)', 'var(--ext-4)', 'var(--ext-5)', 'var(--ext-6)', 'var(--ext-7)']
    let h = 0
    for (let i = 0; i < formatLabel.length; i++) h = (h * 31 + formatLabel.charCodeAt(i)) >>> 0
    return palette[h % palette.length] ?? 'var(--ext-1)'
  })

  function handleClick(e: MouseEvent): void {
    if (e.ctrlKey || e.metaKey) {
      ontoggle?.(item, 'ctrl')
      return
    }
    if (e.shiftKey) {
      ontoggle?.(item, 'shift')
      return
    }
    onactivate?.(item)
  }

  function handleKeydown(e: KeyboardEvent): void {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault()
      onactivate?.(item)
    }
  }

  function handleContext(e: MouseEvent): void {
    e.preventDefault()
    oncontextmenu?.(item, e.clientX, e.clientY)
  }
</script>

<div
  class="card"
  class:selected
  class:focused
  class:missing={item.missing}
  style={style}
  role="gridcell"
  aria-selected={selected}
  tabindex="-1"
  onclick={handleClick}
  onkeydown={handleKeydown}
  oncontextmenu={handleContext}
>
  <div class="preview">
    {#if item.kind === 'image'}
      {#if item.thumbUrl}
        <img class="thumb" src={item.thumbUrl} alt="" draggable="false" decoding="async" />
      {:else}
        <div class="glyph-fallback"></div>
      {/if}
    {:else if item.kind === 'video'}
      {#if item.thumbUrl}
        <img class="thumb" src={item.thumbUrl} alt="" draggable="false" decoding="async" />
      {/if}
      <span class="play" aria-hidden="true">
        <svg viewBox="0 0 24 24" width="13" height="13"><path d="M8.2 5.6v12.8L19 12z" fill="currentColor" /></svg>
      </span>
    {:else if item.kind === 'text'}
      {#if item.subKind === 'link'}
        <div class="link-preview">
          <span class="favicon" style="--fav-hue:{domainHue}">{domain ? (domain[0]?.toUpperCase() ?? '?') : '&bull;'}</span>
          <span class="domain">{domain ?? item.previewText}</span>
          {#if item.title}<span class="link-title">{item.title}</span>{/if}
        </div>
      {:else if item.subKind === 'color'}
        <div class="color-preview" style="background:{item.previewText ?? 'var(--swatch-empty)'}">
          <span class="color-chip">{item.previewText}</span>
        </div>
      {:else}
        <div class="text-panel" class:code={item.subKind === 'code'} style="--fs-text:{textFont}">
          {item.previewText ?? ''}
        </div>
      {/if}
    {:else if item.kind === 'file'}
      <div class="file-preview">
        <span class="doc" style="color:{extColor}">
          <svg viewBox="0 0 24 24" width="32" height="32" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round">
            <path d="M6 3.5h8l4 4v13H6z" />
            <path d="M14 3.5v4h4" />
          </svg>
          {#if item.fileNames.length > 1}
            <svg class="doc-back" viewBox="0 0 24 24" width="32" height="32" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round">
              <path d="M6 3.5h8l4 4v13H6z" />
              <path d="M14 3.5v4h4" />
            </svg>
          {/if}
        </span>
        <span class="fname">{item.fileNames.length > 1 ? `${item.fileNames.length} files` : fileName}</span>
      </div>
    {:else}
      <div class="glyph-fallback"></div>
    {/if}
  </div>

  {#if showAge}
    <span class="badge age">{ageLabel}</span>
  {/if}

  <div class="badges">
    {#if item.pinned}
      <span class="badge pin-b" title="Pinned">
        <svg viewBox="0 0 24 24" width="10" height="10" fill="currentColor">
          <path d="M12 2a6 6 0 0 0-6 6c0 4.6 6 11 6 11s6-6.4 6-11a6 6 0 0 0-6-6Z" />
          <circle cx="12" cy="8" r="2.2" fill="var(--surface-2)" />
        </svg>
      </span>
    {/if}
    {#if item.missing}
      <span class="badge miss">Missing</span>
    {/if}
  </div>

  {#if formatLabelSize !== 'off'}
    <span class="badge fmt" style="--label-fs:{labelFont}">{formatLabel}</span>
  {/if}
</div>

<style>
  .card {
    position: relative;
    background: var(--surface-1);
    border: 1px solid var(--border-1);
    border-radius: var(--card-radius);
    overflow: hidden;
    cursor: default;
    transform: scale(1);
    transition:
      transform var(--dur-fast) var(--ease-out),
      opacity var(--dur-med) var(--ease-out);
    will-change: transform;
    user-select: none;
    -webkit-user-select: none;
    -webkit-tap-highlight-color: transparent;
  }

  .card:hover {
    transform: scale(0.97);
  }

  .card::before {
    content: '';
    position: absolute;
    inset: 0;
    z-index: 5;
    border-radius: inherit;
    border: 1px solid var(--border-3);
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease-out);
    pointer-events: none;
  }

  .card:hover::before {
    opacity: 1;
  }

  .card.selected::before {
    opacity: 1;
    border-color: var(--accent);
  }

  .card.focused::before {
    opacity: 1;
    border-color: var(--accent-strong);
  }

  .card::after {
    content: '';
    position: absolute;
    inset: 0;
    z-index: 4;
    background: var(--overlay-1);
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease-out);
    pointer-events: none;
  }

  .card:hover::after {
    opacity: 1;
  }

  .card.selected::after,
  .card.focused::after {
    background: var(--accent-soft);
    opacity: 1;
  }

  .card.missing {
    opacity: 0.5;
  }

  .card.missing .thumb,
  .card.missing .preview {
    filter: grayscale(0.55);
  }

  .preview {
    position: absolute;
    inset: 0;
    z-index: 1;
  }

  .thumb {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .glyph-fallback {
    position: absolute;
    inset: 0;
    background: linear-gradient(150deg, var(--surface-2), var(--bg-0));
  }

  /* ---- video ---------------------------------------------------------- */

  .play {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    color: var(--text-bright);
  }

  .play::before {
    content: '';
    width: 34px;
    height: 34px;
    border-radius: 50%;
    background: var(--scrim-1);
    border: 1px solid var(--border-media-2);
    backdrop-filter: blur(4px);
  }

  .play svg {
    position: absolute;
    margin-left: 1.5px;
  }

  /* ---- text ----------------------------------------------------------- */

  .text-panel {
    position: absolute;
    inset: 8px;
    border-radius: calc(var(--card-radius) - 2px);
    background: var(--bg-0);
    border: 1px solid var(--border-1);
    padding: 8px 9px;
    color: var(--text-2);
    font-size: var(--fs-text);
    line-height: 1.42;
    font-family: var(--font-ui);
    overflow: hidden;
    display: -webkit-box;
    line-clamp: 7;
    -webkit-line-clamp: 7;
    -webkit-box-orient: vertical;
    white-space: pre-wrap;
    overflow-wrap: break-word;
    word-break: normal;
  }

  .text-panel.code {
    font-family: var(--font-mono);
    color: var(--text-1);
  }

  /* ---- link ----------------------------------------------------------- */

  .link-preview {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 10px;
    text-align: center;
  }

  .favicon {
    width: 30px;
    height: 30px;
    border-radius: 8px;
    display: grid;
    place-items: center;
    font-size: 14px;
    font-weight: 700;
    color: var(--text-bright);
    background: linear-gradient(135deg, hsl(var(--fav-hue) 62% 52%), hsl(calc(var(--fav-hue) + 40) 60% 38%));
    box-shadow: var(--shadow-1);
  }

  .domain {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-xs);
    font-weight: 600;
    color: var(--text-1);
  }

  .link-title {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-2xs);
    color: var(--text-3);
  }

  /* ---- color ---------------------------------------------------------- */

  .color-preview {
    position: absolute;
    inset: 8px;
    border-radius: calc(var(--card-radius) - 2px);
    display: grid;
    place-items: center;
  }

  .color-chip {
    padding: 3px 8px;
    border-radius: var(--r-sm);
    background: var(--scrim-1);
    border: 1px solid var(--border-media-1);
    color: var(--text-bright);
    font-family: var(--font-mono);
    font-size: var(--fs-2xs);
  }

  /* ---- file ----------------------------------------------------------- */

  .file-preview {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 10px;
  }

  .doc {
    position: relative;
    width: 32px;
    height: 32px;
    display: block;
    opacity: 0.92;
  }

  .doc-back {
    position: absolute;
    inset: 0;
    transform: translate(-4px, 4px);
    opacity: 0.35;
  }

  .fname {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-2xs);
    color: var(--text-2);
    line-height: 1.35;
  }

  /* ---- badges --------------------------------------------------------- */

  .badge {
    position: absolute;
    z-index: 3;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 6px;
    border-radius: var(--r-xs);
    background: var(--scrim-2);
    border: 1px solid var(--border-1);
    color: var(--text-2);
    font-size: var(--fs-2xs);
    line-height: 1.4;
    white-space: nowrap;
    pointer-events: none;
    backdrop-filter: blur(6px);
  }

  .age {
    top: 6px;
    left: 6px;
  }

  .badges {
    position: absolute;
    top: 6px;
    right: 6px;
    z-index: 3;
    display: flex;
    gap: 4px;
  }

  .pin-b {
    color: var(--accent);
    padding: 2px 5px;
  }

  .miss {
    color: var(--danger);
  }

  .fmt {
    bottom: 6px;
    left: 6px;
    font-size: var(--label-fs);
    font-weight: 650;
    letter-spacing: 0.05em;
    color: var(--text-1);
  }
</style>