// CDP measurement driver for the W26 UI perf run (popup grid at 10,000 items).
// Protocol plumbing follows scripts/settings/cdp.mjs (Node 22+, global
// WebSocket/fetch). Usage:
//   node measure.mjs targets
//   node measure.mjs state              — dump the popup's current UI state
//   node measure.mjs frames <mode>      — scroll the grid, sample frame times
//        mode: slow-down | fast-down | fast-up | slow-up
//   node measure.mjs headers            — snapshot group headers + card rects
//   node measure.mjs zoom <nTicks>      — ctrl+wheel ticks, measure relayout
//   node measure.mjs search <query>     — type a query, measure render latency
//   node measure.mjs tabs               — click each tab, measure rebuild
//   node measure.mjs mem                — JS heap + DOM counters
//   node measure.mjs shot <file.png>    — screenshot
// Every measure prints one JSON line. Outputs are stable JSON so the
// orchestrator can tee them into out/results.jsonl.
//
// Frame timing method: a requestAnimationFrame loop advances scrollTop and
// records timestamps; a frame's time is the delta between consecutive rAF
// callbacks, which brackets script + layout + paint of the previous frame.
// This measures the achieved main-thread frame rate under continuous
// scrolling (not the compositor's vsync). Longtask entries (>50 ms stalls)
// are collected alongside. The popup must be VISIBLE (rAF is throttled for
// hidden WebViews) — the orchestrator shows it before measuring.

const PORT = process.env.RBF_CDP_PORT || '9333'
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

async function getTargets() {
  const res = await fetch(`http://127.0.0.1:${PORT}/json`)
  if (!res.ok) throw new Error(`CDP /json failed: ${res.status}`)
  return res.json()
}

async function connect(wsUrl) {
  const ws = new WebSocket(wsUrl)
  await new Promise((resolve, reject) => {
    ws.addEventListener('open', resolve, { once: true })
    ws.addEventListener('error', (e) => reject(new Error('ws error')), { once: true })
  })
  let seq = 0
  const pending = new Map()
  ws.addEventListener('message', (ev) => {
    const msg = JSON.parse(ev.data)
    if (msg.id && pending.has(msg.id)) {
      const { resolve, reject } = pending.get(msg.id)
      pending.delete(msg.id)
      if (msg.error) reject(new Error(JSON.stringify(msg.error)))
      else resolve(msg.result)
    }
  })
  return {
    send(method, params = {}) {
      return new Promise((resolve, reject) => {
        const id = ++seq
        pending.set(id, { resolve, reject })
        ws.send(JSON.stringify({ id, method, params }))
      })
    },
    close() { ws.close() },
  }
}

async function withTarget(id, fn) {
  const targets = await getTargets()
  const t = targets.find((x) => String(x.id).startsWith(id) || x.id === id)
  if (!t) {
    const all = targets.map((x) => `${x.id}:${x.title}:${x.url}`).join('\n')
    throw new Error(`target ${id} not found. Targets:\n${all}`)
  }
  const c = await connect(t.webSocketDebuggerUrl)
  try {
    return await fn(c)
  } finally {
    c.close()
  }
}

async function evalIn(c, expression) {
  const out = await c.send('Runtime.evaluate', {
    expression,
    returnByValue: true,
    awaitPromise: true,
  })
  if (out.exceptionDetails) {
    throw new Error(
      `eval exception: ${out.exceptionDetails.text} ${out.exceptionDetails.exception?.description ?? ''}`,
    )
  }
  return out.result.value
}

// Find the popup target: the page that is NOT the settings page. The dev
// server serves the popup at http://localhost:1420/ (no index.html suffix),
// so title/url matching on index.html is unreliable here.
async function findPopup() {
  const targets = await getTargets()
  const t = targets.find(
    (x) => x.type === 'page' && !/settings\.html/.test(x.url) && x.title !== 'Rebuffer — Settings',
  )
  if (!t) {
    const all = targets.map((x) => `${x.type}:${x.title}:${x.url}`).join('\n')
    throw new Error(`popup target not found:\n${all}`)
  }
  return t.id
}

const [, , cmd, ...rest] = process.argv

const SCROLL_GRID = `(() => {
  const cands = [...document.querySelectorAll('.grid, .grid-viewport')]
  const grid = cands.find((g) => g.scrollHeight > g.clientHeight + 10) ?? cands[0]
  return grid
})()`

const GRID_STATE = `(() => {
  const cands = [...document.querySelectorAll('.grid, .grid-viewport')]
  const grid = cands.find((g) => g.scrollHeight > g.clientHeight + 10) ?? cands[0]
  const status = document.querySelector('.status .stats')?.textContent ?? null
  const tabCounts = [...document.querySelectorAll('.tab')].map((b) => ({
    label: b.querySelector('.label')?.textContent ?? '',
    count: b.querySelector('.count')?.textContent ?? '',
    active: b.getAttribute('aria-selected') === 'true',
  }))
  const headers = [...document.querySelectorAll('.group-header')].map((h) => h.textContent.trim())
  const img = document.querySelector('.card .thumb')
  return {
    url: location.href,
    visibility: document.visibilityState,
    status,
    tabCounts,
    scroll: grid
      ? { scrollTop: grid.scrollTop, clientHeight: grid.clientHeight, scrollHeight: grid.scrollHeight }
      : null,
    cards: document.querySelectorAll('.card').length,
    headers,
    thumbNaturalWidth: img ? (img.naturalWidth || 0) : -1,
    brokenThumbs: [...document.querySelectorAll('.thumb')].filter((i) => i.complete && i.naturalWidth === 0).length,
    zoomStep: document.querySelector('.grid') ? parseInt(getComputedStyle(document.querySelector('.grid')).getPropertyValue('--grid-gap') || '10', 10) : null,
    firstCardIds: [...document.querySelectorAll('.card')].slice(0, 8).map((c) => c.dataset.id),
  }
})()`

const FRAME_DRIVER = (mode, step, dir, cycles) => `(async () => {
  const cands = [...document.querySelectorAll('.grid, .grid-viewport')]
  const grid = cands.find((g) => g.scrollHeight > g.clientHeight + 10) ?? cands[0]
  if (!grid) return { error: 'no scroll container' }
  const startScrollHeight = grid.scrollHeight
  const startScrollTop = grid.scrollTop
  const times = []
  const longtasks = []
  const ltObs = new PerformanceObserver((l) => {
    for (const e of l.getEntries()) longtasks.push({ d: Math.round(e.duration * 10) / 10, t: Math.round(e.startTime) })
  })
  ltObs.observe({ type: 'longtask', buffered: true })
  const cardsBefore = document.querySelectorAll('.card').length
  const t0 = performance.now()
  // One "cycle" is a full sweep in the requested direction and back, so the
  // recycling path is exercised in both directions; the grid is only ~200
  // items tall (see the paging finding), so several cycles are needed for a
  // useful frame sample.
  for (let c = 0; c < ${cycles}; c++) {
    const maxScroll = grid.scrollHeight - grid.clientHeight
    await new Promise((resolve) => {
      let started = false
      const tick = () => {
        const now = performance.now()
        if (started) times.push(now - t0)
        started = true
        const cur = grid.scrollTop
        const next = cur + ${dir} * ${step}
        const target = ${dir} > 0 ? maxScroll : 0
        const atEnd = ${dir} > 0 ? next >= target : next <= 0
        if (atEnd) {
          grid.scrollTop = target
          times.push(performance.now() - t0)
          resolve()
          return
        }
        grid.scrollTop = next
        requestAnimationFrame(tick)
      }
      requestAnimationFrame(tick)
    })
  }
  await new Promise((r) => setTimeout(r, 250))
  ltObs.disconnect()
  const deltas = []
  for (let i = 1; i < times.length; i++) deltas.push(times[i] - times[i - 1])
  const sorted = [...deltas].sort((a, b) => a - b)
  const q = (p) => (sorted.length ? sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))] : 0)
  const over = (t) => deltas.filter((d) => d > t).length
  return {
    mode: '${mode}',
    cycles: ${cycles},
    frames: deltas.length,
    median: Math.round(q(0.5) * 100) / 100,
    p95: Math.round(q(0.95) * 100) / 100,
    p99: Math.round(q(0.99) * 100) / 100,
    worst: sorted.length ? Math.round(sorted[sorted.length - 1] * 100) / 100 : null,
    over16p7: over(16.7),
    over33p3: over(33.3),
    over50: over(50),
    scrollPx: Math.round(grid.scrollTop - startScrollTop),
    startScrollHeight: Math.round(startScrollHeight),
    endScrollHeight: Math.round(grid.scrollHeight),
    startScrollTop: Math.round(startScrollTop),
    endScrollTop: Math.round(grid.scrollTop),
    cardsBefore,
    cardsAfter: document.querySelectorAll('.card').length,
    longtasks,
    elapsedMs: Math.round((times[times.length - 1] ?? 0) * 10) / 10,
  }
})()`

const HEADERS_SNAPSHOT = `(() => {
  const cands = [...document.querySelectorAll('.grid, .grid-viewport')]
  const grid = cands.find((g) => g.scrollHeight > g.clientHeight + 10) ?? cands[0]
  const headers = [...document.querySelectorAll('.group-header')].map((h) => {
    const r = h.getBoundingClientRect()
    const gr = h.parentElement.getBoundingClientRect()
    return {
      text: h.textContent.trim(),
      top: Math.round(r.top),
      bottom: Math.round(r.bottom),
      groupTop: Math.round(gr.top),
      groupBottom: Math.round(gr.bottom),
    }
  })
  const cards = [...document.querySelectorAll('.card')].map((c) => {
    const r = c.getBoundingClientRect()
    return { id: c.dataset.id, top: Math.round(r.top), left: Math.round(r.left) }
  })
  return {
    scrollTop: Math.round(grid ? grid.scrollTop : -1),
    clientHeight: grid ? Math.round(grid.clientHeight) : -1,
    scrollHeight: grid ? Math.round(grid.scrollHeight) : -1,
    headers,
    cardCount: cards.length,
    cards,
  }
})()`

const ZOOM_DRIVER = (ticks) => `(async () => {
  const vp = document.querySelector('.grid-viewport') ?? document.querySelector('.grid')
  if (!vp) return { error: 'no viewport' }
  const card = () => document.querySelector('.card')
  const widthBefore = card() ? card().getBoundingClientRect().width : null
  const times = []
  let rafRunning = true
  const t0 = performance.now()
  const tick = () => {
    times.push(performance.now() - t0)
    if (rafRunning) requestAnimationFrame(tick)
  }
  requestAnimationFrame(tick)
  const longtasks = []
  const ltObs = new PerformanceObserver((l) => {
    for (const e of l.getEntries()) longtasks.push(Math.round(e.duration * 10) / 10)
  })
  ltObs.observe({ type: 'longtask', buffered: true })
  for (let i = 0; i < ${ticks}; i++) {
    vp.dispatchEvent(new WheelEvent('wheel', { ctrlKey: true, deltaY: -200, bubbles: true, cancelable: true }))
    await new Promise((r) => setTimeout(r, 90))
  }
  // wait until the tile width stabilises (two equal reads 50 ms apart)
  let settledAt = -1
  let finalWidth = null
  const settleDeadline = performance.now() + 4000
  let prev = null
  let stableSince = -1
  while (performance.now() < settleDeadline) {
    const c = card()
    const w = c ? c.getBoundingClientRect().width : null
    if (w !== null && w === prev) {
      if (stableSince < 0) stableSince = performance.now()
      else if (performance.now() - stableSince > 50) {
        settledAt = performance.now() - t0
        finalWidth = w
        break
      }
    } else {
      stableSince = -1
      prev = w
    }
    await new Promise((r) => setTimeout(r, 25))
  }
  rafRunning = false
  ltObs.disconnect()
  const deltas = []
  for (let i = 1; i < times.length; i++) deltas.push(times[i] - times[i - 1])
  const sorted = [...deltas].sort((a, b) => a - b)
  const q = (p) => (sorted.length ? sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))] : 0)
  return {
    ticks: ${ticks},
    widthBefore,
    finalWidth,
    settledMs: Math.round(settledAt * 10) / 10,
    frames: deltas.length,
    median: Math.round(q(0.5) * 100) / 100,
    p95: Math.round(q(0.95) * 100) / 100,
    worst: sorted.length ? Math.round(sorted[sorted.length - 1] * 100) / 100 : null,
    over16p7: deltas.filter((d) => d > 16.7).length,
    longtasks,
    headers: [...document.querySelectorAll('.group-header')].map((h) => h.textContent.trim()),
  }
})()`

const SEARCH_DRIVER = (query) => `(async () => {
  const input = document.querySelector('#rebuffer-toolbar input')
  const vp = document.querySelector('.grid-viewport') ?? document.querySelector('.grid')
  if (!input) return { error: 'no search input' }
  input.focus()
  const mutations = []
  const obs = new MutationObserver(() => mutations.push(performance.now()))
  obs.observe(vp, { childList: true, subtree: true })
  const times = []
  let rafRunning = true
  const t0 = performance.now()
  const tick = () => {
    times.push(performance.now() - t0)
    if (rafRunning) requestAnimationFrame(tick)
  }
  requestAnimationFrame(tick)
  const keystrokes = []
  for (const ch of ${JSON.stringify(query)}) {
    const k0 = performance.now()
    input.value += ch
    input.dispatchEvent(new InputEvent('input', { bubbles: true, data: ch, inputType: 'insertText' }))
    keystrokes.push(Math.round((performance.now() - k0) * 1000) / 1000)
    await new Promise((r) => setTimeout(r, 70))
  }
  const tLastChar = performance.now()
  const deadline = tLastChar + 4000
  while (performance.now() < deadline && mutations.length === 0) {
    await new Promise((r) => setTimeout(r, 10))
  }
  const tFirstMutation = mutations.length ? Math.round((mutations[0] - tLastChar) * 10) / 10 : -1
  // wait for the grid to settle after results render
  await new Promise((r) => setTimeout(r, 400))
  rafRunning = false
  obs.disconnect()
  const deltas = []
  for (let i = 1; i < times.length; i++) deltas.push(times[i] - times[i - 1])
  const sorted = [...deltas].sort((a, b) => a - b)
  const q = (p) => (sorted.length ? sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))] : 0)
  return {
    query: ${JSON.stringify(query)},
    firstMutationAfterLastCharMs: tFirstMutation,
    mutations: mutations.length,
    keystrokeTimesMs: keystrokes,
    frames: deltas.length,
    median: Math.round(q(0.5) * 100) / 100,
    p95: Math.round(q(0.95) * 100) / 100,
    worst: sorted.length ? Math.round(sorted[sorted.length - 1] * 100) / 100 : null,
    over16p7: deltas.filter((d) => d > 16.7).length,
    cardsAfter: document.querySelectorAll('.card').length,
    emptyState: !!document.querySelector('.empty'),
    inputValue: input.value,
  }
})()`

const SEARCH_CLEAR = `(async () => {
  const input = document.querySelector('#rebuffer-toolbar input')
  const vp = document.querySelector('.grid-viewport') ?? document.querySelector('.grid')
  if (!input) return { error: 'no search input' }
  const mutations = []
  const obs = new MutationObserver(() => mutations.push(performance.now()))
  obs.observe(vp, { childList: true, subtree: true })
  const t0 = performance.now()
  input.value = ''
  input.dispatchEvent(new InputEvent('input', { bubbles: true, data: null, inputType: 'deleteContentBackward' }))
  const deadline = performance.now() + 4000
  while (performance.now() < deadline && mutations.length === 0) {
    await new Promise((r) => setTimeout(r, 10))
  }
  const tFirst = mutations.length ? Math.round((mutations[0] - t0) * 10) / 10 : -1
  await new Promise((r) => setTimeout(r, 400))
  obs.disconnect()
  return { firstMutationMs: tFirst, mutations: mutations.length, cardsAfter: document.querySelectorAll('.card').length, inputValue: input.value }
})()`

const TABS_DRIVER = `(async () => {
  const vp = document.querySelector('.grid-viewport') ?? document.querySelector('.grid')
  const labels = ['Images', 'Text', 'Links', 'Files', 'Pinned', 'All']
  const out = []
  for (const label of labels) {
    const btn = [...document.querySelectorAll('.tab')].find((b) => b.querySelector('.label')?.textContent === label)
    if (!btn) { out.push({ label, error: 'no tab button' }); continue }
    const mutations = []
    const obs = new MutationObserver(() => mutations.push(performance.now()))
    obs.observe(vp, { childList: true, subtree: true })
    const t0 = performance.now()
    btn.click()
    const deadline = performance.now() + 4000
    while (performance.now() < deadline && mutations.length === 0) {
      await new Promise((r) => setTimeout(r, 5))
    }
    const tFirst = mutations.length ? Math.round((mutations[0] - t0) * 10) / 10 : -1
    await new Promise((r) => setTimeout(r, 350))
    obs.disconnect()
    out.push({
      label,
      firstMutationMs: tFirst,
      mutations: mutations.length,
      cards: document.querySelectorAll('.card').length,
      headers: [...document.querySelectorAll('.group-header')].map((h) => h.textContent.trim()),
      activeTab: document.querySelector('.tab[aria-selected="true"] .label')?.textContent ?? null,
    })
  }
  return out
})()`

const MEM_SNAPSHOT = `(() => {
  const pm = performance.memory
    ? { jsHeap: performance.memory.usedJSHeapSize, totalJSHeap: performance.memory.totalJSHeapSize, limit: performance.memory.jsHeapSizeLimit }
    : null
  return {
    perfMemory: pm,
    domCards: document.querySelectorAll('.card').length,
    domNodes: document.querySelectorAll('*').length,
    domHeaders: document.querySelectorAll('.group-header').length,
    domSections: document.querySelectorAll('.group').length,
  }
})()`

async function cdpMetrics(c) {
  const out = {}
  try {
    const m = await c.send('Performance.getMetrics')
    out.metrics = Object.fromEntries(m.metrics.map((x) => [x.name, x.value]))
  } catch {
    out.metrics = null
  }
  try {
    const d = await c.send('Memory.getDOMCounters')
    out.domCounters = d
  } catch {
    out.domCounters = null
  }
  return out
}

async function main() {
  if (cmd === 'targets') {
    const ts = await getTargets()
    for (const t of ts) console.log(JSON.stringify({ id: t.id, title: t.title, url: t.url, type: t.type }))
    return
  }

  const popup = await findPopup()

  if (cmd === 'state') {
    await withTarget(popup, async (c) => {
      console.log(JSON.stringify(await evalIn(c, GRID_STATE)))
    })
    return
  }

  if (cmd === 'frames') {
    const mode = rest[0]
    const cycles = parseInt(rest[1] || '3', 10)
    const DRIVES = {
      'slow-down': [150, 1],
      'fast-down': [1200, 1],
      'fast-up': [1200, -1],
      'slow-up': [150, -1],
    }
    if (!DRIVES[mode]) throw new Error(`unknown frames mode ${mode}`)
    const [step, dir] = DRIVES[mode]
    await withTarget(popup, async (c) => {
      console.log(JSON.stringify(await evalIn(c, FRAME_DRIVER(mode, step, dir, cycles))))
    })
    return
  }

  if (cmd === 'headers') {
    await withTarget(popup, async (c) => {
      console.log(JSON.stringify(await evalIn(c, HEADERS_SNAPSHOT)))
    })
    return
  }

  if (cmd === 'zoom') {
    const ticks = parseInt(rest[0] || '5', 10)
    await withTarget(popup, async (c) => {
      console.log(JSON.stringify(await evalIn(c, ZOOM_DRIVER(ticks))))
    })
    return
  }

  if (cmd === 'search') {
    const query = rest[0] || 'clipboard'
    await withTarget(popup, async (c) => {
      console.log(JSON.stringify(await evalIn(c, SEARCH_DRIVER(query))))
    })
    return
  }

  if (cmd === 'search-clear') {
    await withTarget(popup, async (c) => {
      console.log(JSON.stringify(await evalIn(c, SEARCH_CLEAR)))
    })
    return
  }

  if (cmd === 'tabs') {
    await withTarget(popup, async (c) => {
      console.log(JSON.stringify(await evalIn(c, TABS_DRIVER)))
    })
    return
  }

  if (cmd === 'mem') {
    await withTarget(popup, async (c) => {
      console.log(JSON.stringify({ ...(await evalIn(c, MEM_SNAPSHOT)), ...(await cdpMetrics(c)) }))
    })
    return
  }

  if (cmd === 'shot') {
    const file = rest[0]
    await withTarget(popup, async (c) => {
      await c.send('Page.enable')
      await sleep(250)
      const out = await c.send('Page.captureScreenshot', { format: 'png' })
      if (!out.data) throw new Error('no screenshot data')
      const { writeFileSync } = await import('node:fs')
      writeFileSync(file, Buffer.from(out.data, 'base64'))
      console.log(`saved ${file}`)
    })
    return
  }

  if (cmd === 'scrolldown') {
    // raw helper: scroll the grid to an absolute fraction, e.g. scrolldown 0.5
    const frac = parseFloat(rest[0] || '1')
    await withTarget(popup, async (c) => {
      const r = await evalIn(c, `(() => {
        const cands = [...document.querySelectorAll('.grid, .grid-viewport')]
        const grid = cands.find((g) => g.scrollHeight > g.clientHeight + 10) ?? cands[0]
        grid.scrollTop = (grid.scrollHeight - grid.clientHeight) * ${frac}
        return { scrollTop: grid.scrollTop, scrollHeight: grid.scrollHeight, clientHeight: grid.clientHeight }
      })()`)
      console.log(JSON.stringify(r))
    })
    return
  }

  if (cmd === 'eval') {
    await withTarget(popup, async (c) => {
      const v = await evalIn(c, rest[0])
      console.log(typeof v === 'string' ? v : JSON.stringify(v))
    })
    return
  }

  throw new Error(`unknown command ${cmd}`)
}

main().catch((e) => {
  console.error('FAIL:', e.message)
  process.exit(1)
})