// W35 item 2: more than 200 items — loadMore must load past 200, not
// double-fire on a fast scroll, and stop cleanly at the end.
const PORT = process.env.RBF_CDP_PORT || '9333'
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
async function getTargets() { const res = await fetch(`http://127.0.0.1:${PORT}/json`); if (!res.ok) throw new Error('cdp'); return res.json() }
async function connect(wsUrl) {
  const ws = new WebSocket(wsUrl)
  await new Promise((res, rej) => { ws.addEventListener('open', res, { once: true }); ws.addEventListener('error', () => rej(new Error('ws'))) })
  let seq = 0; const pending = new Map()
  ws.addEventListener('message', (ev) => { const m = JSON.parse(ev.data); if (m.id && pending.has(m.id)) { const p = pending.get(m.id); pending.delete(m.id); m.error ? p.rej(new Error(JSON.stringify(m.error))) : p.res(m.result) } })
  return { send: (m, p = {}) => new Promise((res, rej) => { const id = ++seq; pending.set(id, { res, rej }); ws.send(JSON.stringify({ id, method: m, params: p })) }), close: () => ws.close() }
}
async function evalIn(c, e) { const o = await c.send('Runtime.evaluate', { expression: e, returnByValue: true, awaitPromise: true }); if (o.exceptionDetails) throw new Error(o.exceptionDetails.exception?.description ?? o.exceptionDetails.text); return o.result.value }
async function withPopup(fn) { const ts = await getTargets(); const t = ts.find((x) => x.type === 'page' && !/settings\.html/.test(x.url)); if (!t) throw new Error('no popup'); const c = await connect(t.webSocketDebuggerUrl); try { return await fn(c) } finally { c.close() } }
const GRID = `(() => { const g = [...document.querySelectorAll('.grid, .grid-viewport')].find((x) => x.scrollHeight > x.clientHeight + 10); return g })()`
async function reloadPage(c) {
  try { await c.send('Page.enable') } catch {}
  try { await c.send('Page.reload', { ignoreCache: true }) } catch {}
  await sleep(2000)
  for (let i = 0; i < 20; i++) {
    try { if (await evalIn(c, `(() => { const g = ${GRID}; return !!(g && document.querySelectorAll('.card').length > 0) })()`)) return } catch {}
    await sleep(500)
  }
}

// Slow full-range scroll sampling ids on disjoint viewports; then a second
// pass counts appends via MutationObserver.
const SLOW_COLLECT = `(async () => {
  const g = ${GRID}
  if (!g) return { error: 'no grid' }
  const seen = new Set()
  let guard = 0
  const vh = g.clientHeight
  while (guard < 4000) {
    const max = g.scrollHeight - g.clientHeight
    if (g.scrollTop >= max - 10) { guard++; if (guard >= 6) break; await new Promise((r) => setTimeout(r, 300)); continue }
    guard = 0
    g.scrollTop = Math.min(max, g.scrollTop + vh * 1.4)
    await new Promise((r) => requestAnimationFrame(() => setTimeout(r, 0)))
    for (const c of document.querySelectorAll('.card')) seen.add(c.dataset.id)
  }
  g.scrollTop = g.scrollHeight - g.clientHeight
  await new Promise((r) => setTimeout(r, 800))
  const idsBottom = [...document.querySelectorAll('.card')].map((c) => c.dataset.id)
  return { seenCount: seen.size, scrollHeight: Math.round(g.scrollHeight), scrollTop: Math.round(g.scrollTop), clientHeight: Math.round(g.clientHeight), cardsAtBottom: idsBottom.length, dupInBottomSnapshot: idsBottom.filter((id, i) => idsBottom.indexOf(id) !== i).length }
})()`

const FAST_BURST = `(async () => {
  const g = ${GRID}
  const vp = document.querySelector('.grid-viewport') ?? g
  let appends = 0
  const obs = new MutationObserver(() => appends++)
  obs.observe(vp, { childList: true, subtree: true })
  const startH = g.scrollHeight
  const max = g.scrollHeight - g.clientHeight
  for (let i = 0; i < 60; i++) {
    if (g.scrollTop >= max - 10) break
    g.scrollTop = Math.min(max, g.scrollTop + 2400)
    await new Promise((r) => requestAnimationFrame(() => setTimeout(r, 0)))
  }
  await new Promise((r) => setTimeout(r, 2000))
  obs.disconnect()
  return { appends, startScrollHeight: startH, endScrollHeight: g.scrollHeight, atBottom: g.scrollTop >= g.scrollHeight - g.clientHeight - 50 }
})()`

function expect(name, cond, detail) { console.log(JSON.stringify({ check: name, pass: !!cond, detail })); return !!cond }
async function main() {
  let allPass = true
  await withPopup(async (c) => {
    await reloadPage(c)
    const before = await evalIn(c, `(() => { const g = ${GRID}; return { scrollHeight: g.scrollHeight, cards: document.querySelectorAll('.card').length } })()`)
    // fast burst: does loadMore double-fire? watch mutation rate + whether it finishes
    const burst = await evalIn(c, FAST_BURST)
    allPass &= expect('fast-scroll-loads-more', burst.endScrollHeight > before.scrollHeight, { before: before.scrollHeight, after: burst.endScrollHeight })
    allPass &= expect('fast-scroll-no-append-storm', burst.appends <= 25, { appends: burst.appends })
    console.log(JSON.stringify({ check: 'burst', ...burst, initialScrollHeight: before.scrollHeight }))
    // full slow coverage + id scan
    await reloadPage(c)
    const coll = await evalIn(c, SLOW_COLLECT)
    allPass &= expect('full-list-loads', coll.scrollHeight > before.scrollHeight * 20, coll)
    allPass &= expect('no-dup-in-snapshot', coll.dupInBottomSnapshot === 0, { dup: coll.dupInBottomSnapshot })
    allPass &= expect('stops-cleanly-at-end', coll.scrollTop >= coll.scrollHeight - coll.clientHeight - 50, coll)
    console.log(JSON.stringify({ check: 'collect', ...coll, expectedTotal: Number(process.env.EXPECTED_TOTAL || 10017) }))
  })
  console.log(JSON.stringify({ allPass }))
}
main().catch((e) => { console.error('FAIL:', e.message); process.exit(1) })