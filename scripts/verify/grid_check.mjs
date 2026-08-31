// W35 item 1: grid at depth — deep scroll on the ~10,000-item store, confirm
// cards render at every depth, the pinned date header is correct and pinned to
// the viewport top, and stays correct after a zoom change.
// Prints one JSON line per phase with PASS/FAIL verdicts.
const PORT = process.env.RBF_CDP_PORT || '9333'
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

async function getTargets() {
  const res = await fetch(`http://127.0.0.1:${PORT}/json`)
  if (!res.ok) throw new Error(`CDP /json failed: ${res.status}`)
  return res.json()
}
async function connect(wsUrl) {
  const ws = new WebSocket(wsUrl)
  await new Promise((res, rej) => {
    ws.addEventListener('open', res, { once: true })
    ws.addEventListener('error', (e) => rej(new Error('ws error')), { once: true })
  })
  let seq = 0
  const pending = new Map()
  ws.addEventListener('message', (ev) => {
    const msg = JSON.parse(ev.data)
    if (msg.id && pending.has(msg.id)) {
      const { res, rej } = pending.get(msg.id)
      pending.delete(msg.id)
      if (msg.error) rej(new Error(JSON.stringify(msg.error)))
      else res(msg.result)
    }
  })
  return {
    send(m, p = {}) { return new Promise((res, rej) => { const id = ++seq; pending.set(id, { res, rej }); ws.send(JSON.stringify({ id, method: m, params: p })) }) },
    close() { ws.close() },
  }
}
async function evalIn(c, expression) {
  const out = await c.send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })
  if (out.exceptionDetails) throw new Error(`eval: ${out.exceptionDetails.text} ${out.exceptionDetails.exception?.description ?? ''}`)
  return out.result.value
}
async function withPopup(fn) {
  const targets = await getTargets()
  const t = targets.find((x) => x.type === 'page' && !/settings\.html/.test(x.url))
  if (!t) throw new Error('no popup target')
  const c = await connect(t.webSocketDebuggerUrl)
  try { return await fn(c) } finally { c.close() }
}

const SNAPSHOT = `(() => {
  const cands = [...document.querySelectorAll('.grid, .grid-viewport')]
  const grid = cands.find((g) => g.scrollHeight > g.clientHeight + 10) ?? cands[0]
  const gridRectTop = Math.round(grid.getBoundingClientRect().top)
  const headers = [...document.querySelectorAll('.group-header')].map((h) => {
    const r = h.getBoundingClientRect()
    const naturalTop = parseInt(h.style.top, 10) || 0
    const pinned = h.classList.contains('pinned')
    return { text: h.textContent.trim(), rectTop: Math.round(r.top), rectBottom: Math.round(r.bottom), naturalTop, pinned }
  })
  const cards = [...document.querySelectorAll('.card')].map((c) => { const r = c.getBoundingClientRect(); return { id: c.dataset.id, top: Math.round(r.top), bottom: Math.round(r.bottom) } })
  const vh = grid ? grid.clientHeight : 0
  const cardsInView = cards.filter((c) => c.bottom > 0 && c.top < vh).length
  const pinnedAtTop = headers.filter((h) => h.pinned && Math.abs(h.rectTop - gridRectTop) <= 2)
  return {
    scrollTop: Math.round(grid ? grid.scrollTop : -1),
    clientHeight: grid ? Math.round(grid.clientHeight) : -1,
    scrollHeight: grid ? Math.round(grid.scrollHeight) : -1,
    gridRectTop,
    cards: cards.length,
    cardsInView,
    headers,
    pinnedAtTop: pinnedAtTop.map((h) => ({ text: h.text, naturalTop: h.naturalTop })),
    zoom: document.querySelector('.zoom-dial')?.textContent ?? '',
  }
})()`

const DEEP_SCROLL = `(async () => {
  const cands = [...document.querySelectorAll('.grid, .grid-viewport')]
  const grid = cands.find((g) => g.scrollHeight > g.clientHeight + 10) ?? cands[0]
  if (!grid) return { error: 'no grid' }
  let passes = 0
  const step = 600
  while (passes < 120) {
    const max = grid.scrollHeight - grid.clientHeight
    if (grid.scrollTop >= max - step) {
      passes++
      if (passes >= 4) break
      await new Promise((r) => setTimeout(r, 250))
      continue
    }
    passes = 0
    grid.scrollTop = Math.min(max, grid.scrollTop + step)
    await new Promise((r) => requestAnimationFrame(() => setTimeout(r, 0)))
  }
  grid.scrollTop = grid.scrollHeight - grid.clientHeight
  await new Promise((r) => setTimeout(r, 600))
  return { done: true }
})()`

const SCROLL_TO = (frac) => `(async () => {
  const cands = [...document.querySelectorAll('.grid, .grid-viewport')]
  const grid = cands.find((g) => g.scrollHeight > g.clientHeight + 10) ?? cands[0]
  if (!grid) return { error: 'no grid' }
  grid.scrollTop = (grid.scrollHeight - grid.clientHeight) * ${frac}
  await new Promise((r) => setTimeout(r, 500))
  return { done: true }
})()`

const ZOOM = `(async () => {
  const vp = document.querySelector('.grid-viewport') ?? document.querySelector('.grid')
  if (!vp) return { error: 'no viewport' }
  const before = document.querySelector('.card')?.getBoundingClientRect().width ?? -1
  for (let i = 0; i < 2; i++) {
    vp.dispatchEvent(new WheelEvent('wheel', { ctrlKey: true, deltaY: -200, bubbles: true, cancelable: true }))
    await new Promise((r) => setTimeout(r, 120))
  }
  await new Promise((r) => setTimeout(r, 800))
  const after = document.querySelector('.card')?.getBoundingClientRect().width ?? -1
  return { tileWBefore: before, tileWAfter: after }
})()`

const ZOOM_OUT = `(async () => {
  const vp = document.querySelector('.grid-viewport') ?? document.querySelector('.grid')
  if (!vp) return { error: 'no viewport' }
  for (let i = 0; i < 2; i++) {
    vp.dispatchEvent(new WheelEvent('wheel', { ctrlKey: true, deltaY: 200, bubbles: true, cancelable: true }))
    await new Promise((r) => setTimeout(r, 120))
  }
  await new Promise((r) => setTimeout(r, 800))
  return { done: true }
})()`

const expect = (name, cond, detail) => {
  console.log(JSON.stringify({ check: name, pass: !!cond, detail }))
  return !!cond
}

async function main() {
  let allPass = true
  await withPopup(async (c) => {
    await evalIn(c, DEEP_SCROLL)
    const bottom = await evalIn(c, SNAPSHOT)
    const bOk = bottom.cardsInView > 0 && bottom.pinnedAtTop.length === 1
    allPass &= expect('bottom-cards-render', bottom.cardsInView > 0, { cardsInView: bottom.cardsInView, scrollTop: bottom.scrollTop, scrollHeight: bottom.scrollHeight })
    allPass &= expect('bottom-single-pinned-header', bottom.pinnedAtTop.length === 1, bottom.pinnedAtTop)
    console.log(JSON.stringify({ phase: 'bottom', ...bottom }))

    for (const f of [0.25, 0.5, 0.75]) {
      await evalIn(c, SCROLL_TO(f))
      const s = await evalIn(c, SNAPSHOT)
      const ok = s.cardsInView > 0 && s.pinnedAtTop.length === 1
      allPass &= expect(`depth-${f}-ok`, ok, { cardsInView: s.cardsInView, pinned: s.pinnedAtTop })
      console.log(JSON.stringify({ phase: `depth-${f}`, ...s }))
    }

    // zoom in (two ticks), re-check at the same depth, then back out
    const z = await evalIn(c, ZOOM)
    await evalIn(c, SCROLL_TO(0.5))
    const afterZoom = await evalIn(c, SNAPSHOT)
    allPass &= expect('zoom-resizes-tiles', z.tileWAfter > z.tileWBefore, z)
    allPass &= expect('depth-after-zoom-ok', afterZoom.cardsInView > 0 && afterZoom.pinnedAtTop.length === 1, { cardsInView: afterZoom.cardsInView, pinned: afterZoom.pinnedAtTop, tileW: z.tileWAfter })
    console.log(JSON.stringify({ phase: 'after-zoom', ...afterZoom, zoomTiles: z }))
    await evalIn(c, ZOOM_OUT)
  })
  console.log(JSON.stringify({ allPass }))
}
main().catch((e) => { console.error('FAIL:', e.message); process.exit(1) })