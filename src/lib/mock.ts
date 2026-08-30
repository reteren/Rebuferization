// OWNER: worker W5 — development fixture; nothing outside W5 imports this.
import type { ItemDto, Kind } from './types'

function mulberry32(seed: number): () => number {
  let a = seed >>> 0
  return () => {
    a = (a + 0x6d2b79f5) | 0
    let t = Math.imul(a ^ (a >>> 15), 1 | a)
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

function pick<T>(rnd: () => number, arr: readonly T[]): T {
  return arr[Math.floor(rnd() * arr.length)] ?? arr[0]!
}

// ---------------------------------------------------------------------------
// hand-rolled animated GIF so the mock animates offline with no assets
// ---------------------------------------------------------------------------

const GIF_W = 48
const GIF_H = 48
const GIF_PALETTE: ReadonlyArray<readonly [number, number, number]> = [
  [12, 15, 21],
  [122, 162, 255],
  [187, 154, 247],
  [233, 236, 244],
]

function gifFrame(t: number, total: number): Uint8Array {
  const out = new Uint8Array(GIF_W * GIF_H)
  const cx = GIF_W / 2
  const cy = GIF_H / 2
  const r = 15
  const ang = (t / total) * Math.PI * 2
  const px = cx + Math.cos(ang) * r
  const py = cy + Math.sin(ang) * r
  const trail: Array<[number, number]> = []
  for (let k = 1; k <= 6; k++) {
    const a = ang - k * 0.3
    trail.push([cx + Math.cos(a) * r, cy + Math.sin(a) * r])
  }
  for (let y = 0; y < GIF_H; y++) {
    for (let x = 0; x < GIF_W; x++) {
      const dx = x - px
      const dy = y - py
      if (dx * dx + dy * dy < 16) {
        const hx = x - (px - 1.6)
        const hy = y - (py - 1.6)
        out[y * GIF_W + x] = hx * hx + hy * hy < 5 ? 3 : 1
        continue
      }
      let c = 0
      for (const [tx, ty] of trail) {
        const tdx = x - tx
        const tdy = y - ty
        if (tdx * tdx + tdy * tdy < 9) {
          c = 2
          break
        }
      }
      out[y * GIF_W + x] = c
    }
  }
  return out
}

function lzwEncode(minCodeSize: number, data: Uint8Array): Uint8Array {
  const clear = 1 << minCodeSize
  const eoi = clear + 1
  let codeSize = minCodeSize + 1
  let next = eoi + 1
  const dict = new Map<number, number>()
  const out: number[] = []
  let buf = 0
  let nbits = 0
  const emit = (code: number): void => {
    buf |= code << nbits
    nbits += codeSize
    while (nbits >= 8) {
      out.push(buf & 0xff)
      buf >>>= 8
      nbits -= 8
    }
  }
  emit(clear)
  let prefix = data[0] ?? 0
  for (let i = 1; i < data.length; i++) {
    const k = data[i] ?? 0
    const key = prefix * 256 + k
    const hit = dict.get(key)
    if (hit !== undefined) {
      prefix = hit
    } else {
      emit(prefix)
      if (next >= 4096) {
        emit(clear)
        dict.clear()
        codeSize = minCodeSize + 1
        next = eoi + 1
      } else {
        dict.set(key, next)
        next++
        if (next === 1 << codeSize && codeSize < 12) codeSize++
      }
      prefix = k
    }
  }
  emit(prefix)
  emit(eoi)
  if (nbits > 0) out.push(buf & 0xff)
  return new Uint8Array(out)
}

function buildGif(frames: Uint8Array[]): string {
  const bytes: number[] = []
  const push = (...b: number[]): void => {
    bytes.push(...b)
  }
  push(0x47, 0x49, 0x46, 0x38, 0x39, 0x61)
  push(GIF_W & 0xff, (GIF_W >> 8) & 0xff)
  push(GIF_H & 0xff, (GIF_H >> 8) & 0xff)
  const sizeField = Math.ceil(Math.log2(GIF_PALETTE.length)) - 1
  push(0x80 | (7 << 4) | sizeField)
  push(0, 0)
  for (const [r, g, b] of GIF_PALETTE) push(r, g, b)
  const minCode = Math.max(2, sizeField + 1)
  for (const frame of frames) {
    push(0x21, 0xf9, 0x04, 0x04, 9, 0, 0, 0)
    push(0x2c, 0, 0, GIF_W & 0xff, (GIF_W >> 8) & 0xff, GIF_H & 0xff, (GIF_H >> 8) & 0xff, 0)
    push(minCode)
    const lzw = lzwEncode(minCode, frame)
    for (let i = 0; i < lzw.length; i += 255) {
      const chunk = lzw.subarray(i, i + 255)
      push(chunk.length)
      for (const b of chunk) push(b)
    }
    push(0)
  }
  push(0x3b)
  let bin = ''
  for (let i = 0; i < bytes.length; i += 0x8000) {
    bin += String.fromCharCode(...bytes.slice(i, i + 0x8000))
  }
  return 'data:image/gif;base64,' + btoa(bin)
}

const GIF_URI = buildGif(Array.from({ length: 6 }, (_, i) => gifFrame(i, 6)))

// ---------------------------------------------------------------------------
// procedural thumbnail art (SVG data URIs) — deterministic per id
// ---------------------------------------------------------------------------

function svgUri(svg: string): string {
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`
}

function makeArt(id: number): { w: number; h: number; url: string } {
  const rnd = mulberry32(id * 2654435761)
  const w = pick(rnd, [1280, 1920, 800, 512, 1440, 1024])
  const h = pick(rnd, [720, 1080, 600, 512, 900, 768])
  const hue = Math.floor(rnd() * 360)
  const h2 = (hue + 30 + Math.floor(rnd() * 70)) % 360
  const sat = 45 + Math.floor(rnd() * 30)
  const style = Math.floor(rnd() * 6)
  let inner = ''
  switch (style) {
    case 0: {
      inner = `
        <defs>
          <linearGradient id="g" x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" stop-color="hsl(${hue},${sat}%,46%)"/>
            <stop offset="0.55" stop-color="hsl(${h2},${sat + 5}%,26%)"/>
            <stop offset="1" stop-color="hsl(${(h2 + 25) % 360},${sat}%,12%)"/>
          </linearGradient>
          <filter id="b"><feGaussianBlur stdDeviation="40"/></filter>
        </defs>
        <rect width="100%" height="100%" fill="url(#g)"/>
        <ellipse cx="${25 + Math.floor(rnd() * 50)}%" cy="${20 + Math.floor(rnd() * 50)}%" rx="42%" ry="30%" fill="hsl(${hue},90%,68%)" opacity="0.5" filter="url(#b)"/>
        <ellipse cx="${60 + Math.floor(rnd() * 30)}%" cy="80%" rx="35%" ry="25%" fill="hsl(${h2},85%,60%)" opacity="0.35" filter="url(#b)"/>`
      break
    }
    case 1: {
      inner = `
        <defs><filter id="b"><feGaussianBlur stdDeviation="70"/></filter></defs>
        <rect width="100%" height="100%" fill="#0d0f14"/>
        <ellipse cx="30%" cy="35%" rx="45%" ry="35%" fill="hsl(${hue},80%,60%)" opacity="0.55" filter="url(#b)"/>
        <ellipse cx="70%" cy="60%" rx="40%" ry="30%" fill="hsl(${h2},75%,50%)" opacity="0.42" filter="url(#b)"/>
        <ellipse cx="50%" cy="95%" rx="50%" ry="28%" fill="hsl(${(hue + 120) % 360},60%,45%)" opacity="0.3" filter="url(#b)"/>`
      break
    }
    case 2: {
      const r0 = Math.round(Math.min(w, h) * 0.1)
      const cx = Math.round(w / 2)
      const cy = Math.round(h / 2)
      const rings = [r0, r0 * 2, r0 * 3, r0 * 4, r0 * 5]
        .map(
          (r, i) =>
            `<circle cx="${cx}" cy="${cy}" r="${r}" fill="none" stroke="hsl(${(hue + i * 14) % 360},70%,62%)" stroke-opacity="${0.85 - i * 0.14}" stroke-width="${3 - i * 0.4}"/>`,
        )
        .join('')
      inner = `
        <rect width="100%" height="100%" fill="#0d0f14"/>
        ${rings}
        <circle cx="${cx}" cy="${cy}" r="${Math.round(r0 * 0.28)}" fill="hsl(${hue},85%,72%)"/>`
      break
    }
    case 3: {
      const dots: string[] = []
      for (let x = 8; x < w; x += 14) {
        for (let y = 8; y < h; y += 14) {
          const r = 1.5 + rnd() * 2.8
          dots.push(
            `<circle cx="${x}" cy="${y}" r="${r.toFixed(1)}" fill="hsl(${(hue + Math.floor(rnd() * 24)) % 360},70%,${58 + Math.floor(rnd() * 14)}%)"/>`,
          )
        }
      }
      inner = `
        <rect width="100%" height="100%" fill="#0d0f14"/>
        ${dots.join('')}`
      break
    }
    case 4: {
      inner = `
        <defs>
          <linearGradient id="g" x1="0" y1="1" x2="1" y2="0">
            <stop offset="0" stop-color="hsl(${hue},60%,16%)"/>
            <stop offset="1" stop-color="hsl(${h2},65%,30%)"/>
          </linearGradient>
        </defs>
        <rect width="100%" height="100%" fill="url(#g)"/>
        <rect width="100%" height="100%" fill="repeating-linear-gradient(115deg, hsl(${hue},85%,64%,0.32) 0 36, transparent 36 96)"/>
        <rect width="100%" height="100%" fill="repeating-linear-gradient(-65deg, hsl(${h2},80%,58%,0.18) 0 24, transparent 24 140)"/>`
      break
    }
    default: {
      const cells: string[] = []
      for (let i = 0; i < 8; i++) {
        for (let j = 0; j < 6; j++) {
          const ch = (hue + i * 9 + j * 17) % 360
          const l = 26 + Math.floor(rnd() * 22)
          cells.push(
            `<rect x="${i * 12.5}%" y="${j * 16.6}%" width="12.5%" height="16.6%" rx="7" fill="hsl(${ch},46%,${l}%)"/>`,
          )
        }
      }
      inner = `
        <rect width="100%" height="100%" fill="#0d0f14"/>
        ${cells.join('')}`
    }
  }
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${w} ${h}" preserveAspectRatio="xMidYMid slice">${inner}</svg>`
  return { w, h, url: svgUri(svg) }
}

function makeVideoArt(id: number): string {
  const rnd = mulberry32(id * 2654435761 + 7)
  const hue = Math.floor(rnd() * 360)
  const h2 = (hue + 40) % 360
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1920 1080" preserveAspectRatio="xMidYMid slice">
    <defs>
      <filter id="b"><feGaussianBlur stdDeviation="120"/></filter>
      <radialGradient id="v" cx="50%" cy="50%" r="75%">
        <stop offset="58%" stop-color="#000" stop-opacity="0"/>
        <stop offset="100%" stop-color="#000" stop-opacity="0.55"/>
      </radialGradient>
    </defs>
    <rect width="100%" height="100%" fill="#0d0f14"/>
    <ellipse cx="30%" cy="40%" rx="40%" ry="35%" fill="hsl(${hue},75%,52%)" opacity="0.5" filter="url(#b)"/>
    <ellipse cx="70%" cy="55%" rx="35%" ry="30%" fill="hsl(${h2},70%,42%)" opacity="0.42" filter="url(#b)"/>
    <rect width="100%" height="100%" fill="url(#v)"/>
    <rect y="0" width="100%" height="72" fill="#000" opacity="0.55"/>
    <rect y="1008" width="100%" height="72" fill="#000" opacity="0.55"/>
  </svg>`
  return svgUri(svg)
}

// ---------------------------------------------------------------------------
// content pools
// ---------------------------------------------------------------------------

const WORDS = [
  'clipboard', 'history', 'paste', 'buffer', 'capture', 'snippet', 'window',
  'glass', 'dark', 'liquid', 'search', 'zoom', 'tile', 'grid', 'scroll',
  'focus', 'pin', 'copy', 'format', 'text', 'image', 'file', 'link', 'color',
  'code', 'shell', 'blob', 'store', 'query', 'sort', 'filter', 'facet',
  'frame', 'preview', 'thumbnail', 'render', 'cache', 'index', 'token',
  'motion', 'easing', 'accent', 'radius', 'shadow',
]

function textBlock(rnd: () => number, maxLines: number): string {
  const lines: string[] = []
  const n = 1 + Math.floor(rnd() * maxLines)
  for (let i = 0; i < n; i++) {
    const words = 2 + Math.floor(rnd() * 8)
    const parts: string[] = []
    for (let j = 0; j < words; j++) {
      parts.push(pick(rnd, WORDS))
    }
    lines.push(parts.join(' '))
  }
  return lines.join('\n')
}

function firstLine(text: string): string {
  return text.split('\n')[0] ?? text
}

const SEVEN_LINES = [
  'The quick brown fox',
  'jumps over the lazy',
  'dog, carrying a',
  'small tin can and',
  'a pocket watch. It',
  'was a very good',
  'morning for it.',
].join('\n')

const CODE_POOL = [
  `function debounce(fn: () => void, ms = 120) {
  let timer: number | undefined
  return () => {
    clearTimeout(timer)
    timer = setTimeout(fn, ms)
  }
}`,
  `{
  "name": "rebuffer",
  "version": "0.1.0",
  "scripts": { "build": "vite build" }
}`,
  `body {
  background: rgba(12, 14, 19, 0.92);
  color: #e9ecf4;
  font-family: "Segoe UI";
}`,
  `npx tsc --noEmit
npx svelte-check --tsconfig ./tsconfig.json
npx vite build`,
]

const LINKS: Array<{ url: string; title: string }> = [
  { url: 'https://github.com/rust-lang/rust', title: 'rust-lang/rust: Empowering everyone to build reliable software' },
  { url: 'https://developer.mozilla.org/en-US/docs/Web/API/Clipboard', title: 'Clipboard API - Web APIs' },
  { url: 'https://news.ycombinator.com/', title: 'Hacker News' },
  { url: 'https://opencode.ai/docs', title: 'opencode - terminal-first AI coding agent' },
  { url: 'https://tailwindcss.com/docs/display', title: 'Display - Tailwind CSS' },
  { url: 'https://www.figma.com/blog/design-tokens/', title: 'Design tokens - Figma Blog' },
]

const COLORS = [
  '#7aa2ff', '#4ec9b0', '#bb9af7', '#e0af68', '#f7768e',
  '#2ac3de', '#9ece6a', '#c0caf5', '#ff9e64', 'rgb(74, 58, 255)', 'hsl(210, 80%, 60%)',
]

const FILES: Array<{ names: string[] }> = [
  { names: ['quarterly_report.xlsx'] },
  { names: ['photo_2026_08_29.jpg'] },
  { names: ['presentation_final.pptx'] },
  { names: ['budget_v2.xlsx', 'budget_v3.xlsx'] },
  { names: ['design_assets.zip', 'icon_set.zip'] },
  { names: ['src/main.rs', 'src/lib.rs', 'Cargo.toml', 'README.md'] },
  { names: ['rebuffer_setup.exe'] },
  { names: ['notes_offboarding.md', 'README.md', 'LICENSE.txt'] },
]

const SOURCE_APPS = [
  'chrome.exe', 'msedge.exe', 'explorer.exe', 'vscode.exe', 'winword.exe',
  'powerpnt.exe', 'slack.exe', 'discord.exe', 'notepad.exe', 'obsidian.exe',
]

// ---------------------------------------------------------------------------
// item builders
// ---------------------------------------------------------------------------

function base(rnd: () => number, id: number, createdAt: number, kind: Kind, extra: Partial<ItemDto> = {}): ItemDto {
  return {
    id,
    kind,
    subKind: null,
    animatedUrl: null,
    title: null,
    previewText: null,
    thumbUrl: null,
    ext: null,
    byteSize: 0,
    width: null,
    height: null,
    durationMs: null,
    createdAt,
    pinned: false,
    isReference: false,
    refPath: null,
    sourceApp: pick(rnd, SOURCE_APPS),
    copyCount: 1 + Math.floor(rnd() * 18),
    missing: false,
    fileNames: [],
    ...extra,
  }
}

function imageItem(rnd: () => number, id: number, createdAt: number, opts: { pinned?: boolean } = {}): ItemDto {
  const art = makeArt(id)
  const animated = rnd() < 0.06
  const ext = animated ? 'GIF' : pick(rnd, ['PNG', 'PNG', 'JPG', 'WEBP'])
  const date = new Date(createdAt).toISOString().slice(0, 10)
  return base(rnd, id, createdAt, 'image', {
    subKind: animated ? 'animated' : null,
    ext,
    title: `Screenshot ${date} at 0${id % 9}.${ext.toLowerCase()}`,
    thumbUrl: animated ? GIF_URI : art.url,
    // The original blob: the only source that actually animates. The static
    // webp thumbnail (GIF_URI here stands in for the decoded first frame) is
    // what renderers fall back to when the flag is off.
    animatedUrl: animated ? GIF_URI : null,
    width: art.w,
    height: art.h,
    byteSize: Math.round((200_000 + rnd() * 4_800_000) / 1024) * 1024,
    pinned: opts.pinned ?? false,
    copyCount: 1 + Math.floor(rnd() * 24),
  })
}

function textItem(rnd: () => number, id: number, createdAt: number): ItemDto {
  const r = rnd()
  if (r < 0.28) {
    const code = pick(rnd, CODE_POOL)
    return base(rnd, id, createdAt, 'text', {
      subKind: 'code',
      ext: pick(rnd, ['TS', 'JSON', 'CSS', 'SH']),
      previewText: code,
      title: firstLine(code),
      byteSize: code.length * 3 + 40,
    })
  }
  if (r < 0.4) {
    const link = pick(rnd, LINKS)
    return base(rnd, id, createdAt, 'text', {
      subKind: 'link',
      ext: null,
      previewText: link.url,
      title: link.title,
      byteSize: link.url.length + 80,
    })
  }
  if (r < 0.48) {
    const color = pick(rnd, COLORS)
    return base(rnd, id, createdAt, 'text', {
      subKind: 'color',
      ext: null,
      previewText: color,
      byteSize: color.length,
    })
  }
  if (r < 0.55) {
    const t = textBlock(rnd, 3)
    return base(rnd, id, createdAt, 'text', {
      subKind: 'rich',
      ext: 'RTF',
      previewText: t,
      title: firstLine(t),
      byteSize: t.length * 4 + 60,
    })
  }
  const t = textBlock(rnd, 7)
  return base(rnd, id, createdAt, 'text', {
    subKind: 'plain',
    ext: 'TXT',
    previewText: t,
    title: firstLine(t),
    byteSize: t.length * 2 + 20,
  })
}

function fileItem(rnd: () => number, id: number, createdAt: number, opts: { missing?: boolean } = {}): ItemDto {
  const f = pick(rnd, FILES)
  const first = f.names[0] ?? 'file.bin'
  const ext = first.includes('.') ? first.slice(first.lastIndexOf('.') + 1).toUpperCase() : 'FILE'
  return base(rnd, id, createdAt, 'file', {
    ext,
    fileNames: f.names,
    title: first,
    isReference: true,
    refPath: `C:\\Users\\alex\\${f.names.length > 1 ? 'projects\\rebuffer' : 'Desktop'}\\${first}`,
    byteSize: Math.round((20_000 + rnd() * 40_000_000) / 1024) * 1024,
    missing: opts.missing ?? false,
  })
}

function videoItem(rnd: () => number, id: number, createdAt: number): ItemDto {
  const dur = 8_000 + Math.floor(rnd() * 900_000)
  return base(rnd, id, createdAt, 'video', {
    ext: pick(rnd, ['MP4', 'MP4', 'WEBM', 'MOV']),
    thumbUrl: makeVideoArt(id),
    width: 1920,
    height: 1080,
    durationMs: dur,
    title: `screen recording ${Math.max(1, Math.floor(dur / 60_000))}m.mp4`,
    byteSize: Math.round((2_000_000 + rnd() * 60_000_000) / 1024) * 1024,
  })
}

function otherItem(rnd: () => number, id: number, createdAt: number): ItemDto {
  return base(rnd, id, createdAt, 'other', {
    ext: pick(rnd, ['BIN', 'DAT', 'RAW']),
    previewText: `0x${(id * 7919).toString(16)}`,
    byteSize: Math.round((1_000 + rnd() * 50_000) / 64) * 64,
  })
}

function randomAge(rnd: () => number): number {
  const r = rnd()
  const h = 3.6e6
  const d = 8.64e7
  if (r < 0.38) return rnd() * 2 * h
  if (r < 0.55) return (2 + rnd() * 22) * h
  if (r < 0.7) return (1 + rnd() * 6) * d
  if (r < 0.85) return (7 + rnd() * 23) * d
  if (r < 0.95) return (30 + rnd() * 60) * d
  return (90 + rnd() * 240) * d
}

// ---------------------------------------------------------------------------
// required coverage: every kind and sub-kind, pinned, missing, 7-line text
// ---------------------------------------------------------------------------

function requiredItems(now: number): ItemDto[] {
  const m = 60_000
  const d = 86_400_000
  const r1 = mulberry32(1)
  const r2 = mulberry32(2)
  const r3 = mulberry32(3)
  const r4 = mulberry32(4)
  const r5 = mulberry32(5)
  const r6 = mulberry32(6)
  const r7 = mulberry32(7)
  const r8 = mulberry32(8)
  const r9 = mulberry32(9)
  const r10 = mulberry32(10)
  const r11 = mulberry32(11)
  const art1 = makeArt(1)
  const art9 = makeArt(9)
  return [
    {
      ...base(r1, 1, now - 2 * m, 'image'),
      subKind: null,
      ext: 'PNG',
      title: 'Screenshot 2026-08-30 at 09.12.41.png',
      thumbUrl: art1.url,
      width: art1.w,
      height: art1.h,
      byteSize: 2_412_348,
      copyCount: 3,
      sourceApp: 'explorer.exe',
    },
    {
      ...base(r2, 2, now - 6 * m, 'text'),
      subKind: 'plain',
      ext: 'TXT',
      previewText: SEVEN_LINES,
      title: 'The quick brown fox',
      byteSize: 342,
      copyCount: 1,
      sourceApp: 'notepad.exe',
    },
    {
      ...base(r3, 3, now - 12 * m, 'image'),
      subKind: 'animated',
      ext: 'GIF',
      title: 'screen_capture_looping.gif',
      thumbUrl: GIF_URI,
      animatedUrl: GIF_URI,
      width: 48,
      height: 48,
      byteSize: 18_204,
      copyCount: 5,
      sourceApp: 'chrome.exe',
    },
    {
      ...base(r4, 4, now - 19 * m, 'video'),
      subKind: null,
      ext: 'MP4',
      title: 'screen recording 2m.mp4',
      thumbUrl: makeVideoArt(4),
      width: 1920,
      height: 1080,
      durationMs: 132_000,
      byteSize: 18_403_328,
      copyCount: 2,
      sourceApp: 'obsidian.exe',
    },
    {
      ...base(r5, 5, now - 26 * m, 'text'),
      subKind: 'code',
      ext: 'TS',
      previewText: CODE_POOL[0] ?? '',
      title: 'function debounce(fn: () => void, ms = 120) {',
      byteSize: 412,
      copyCount: 7,
      sourceApp: 'vscode.exe',
    },
    {
      ...base(r6, 6, now - 33 * m, 'text'),
      subKind: 'link',
      ext: null,
      previewText: 'https://github.com/rust-lang/rust',
      title: 'rust-lang/rust: Empowering everyone to build reliable software',
      byteSize: 190,
      copyCount: 4,
      sourceApp: 'chrome.exe',
    },
    {
      ...base(r7, 7, now - 41 * m, 'text'),
      subKind: 'color',
      ext: null,
      previewText: '#7aa2ff',
      byteSize: 24,
      copyCount: 2,
      sourceApp: 'figma.exe',
    },
    {
      ...base(r8, 8, now - 52 * m, 'file'),
      subKind: null,
      ext: 'RS',
      fileNames: ['src/main.rs', 'src/lib.rs', 'Cargo.toml', 'README.md'],
      title: 'src/main.rs',
      isReference: true,
      refPath: 'C:\\Users\\alex\\projects\\rebuffer',
      byteSize: 48_211,
      copyCount: 3,
      sourceApp: 'explorer.exe',
    },
    {
      ...base(r9, 9, now - 3 * d, 'image'),
      subKind: null,
      ext: 'PNG',
      title: 'palette_ref_2026.png',
      thumbUrl: art9.url,
      width: art9.w,
      height: art9.h,
      byteSize: 1_204_736,
      pinned: true,
      copyCount: 12,
      sourceApp: 'explorer.exe',
    },
    {
      ...base(r10, 10, now - 4 * d, 'file'),
      subKind: null,
      ext: 'PDF',
      fileNames: ['quarterly_report_final.pdf'],
      title: 'quarterly_report_final.pdf',
      isReference: true,
      refPath: 'C:\\Users\\alex\\Desktop\\quarterly_report_final.pdf',
      byteSize: 2_418_176,
      missing: true,
      copyCount: 1,
      sourceApp: 'explorer.exe',
    },
    {
      ...base(r11, 11, now - 71 * m, 'text'),
      subKind: 'plain',
      ext: 'TXT',
      previewText: 'Meeting at 4 - bring the demo build.',
      title: 'Meeting at 4 - bring the demo build.',
      byteSize: 96,
      copyCount: 6,
      sourceApp: 'slack.exe',
    },
  ]
}

// ---------------------------------------------------------------------------

export function mockItems(n: number): ItemDto[] {
  const now = Date.now()
  const out = requiredItems(now)
  const rnd = mulberry32(0x5eed)
  for (let i = out.length; i < n; i++) {
    const id = 1000 + i
    const createdAt = now - randomAge(rnd)
    const roll = rnd()
    let item: ItemDto
    if (roll < 0.3) item = imageItem(rnd, id, createdAt)
    else if (roll < 0.62) item = textItem(rnd, id, createdAt)
    else if (roll < 0.84) item = fileItem(rnd, id, createdAt)
    else if (roll < 0.92) item = videoItem(rnd, id, createdAt)
    else item = otherItem(rnd, id, createdAt)
    out.push(item)
  }
  out.sort((a, b) => b.createdAt - a.createdAt)
  return out
}