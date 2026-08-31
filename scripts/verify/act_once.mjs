// W35: dispatch a single interaction via CDP and exit. Action in argv[2]:
//   clickcopy  — plain click on a text card (expects copy+close)
//   enter      — select+focus a card, then Enter (expects copy+close)
//   esc        — Escape key (expects close)
//   click-select — plain click on a card while a selection exists (expects anchor)
// Prints one JSON line with the selected/focused evidence.
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

const action = process.argv[2] || 'esc'
const ev = {
  clickcopy: `(() => {
    const c = [...document.querySelectorAll('.card')].find((x) => (x.querySelector('.text-panel')?.textContent ?? '').trim().length > 8)
    if (!c) return { error: 'no text card' }
    const text = (c.querySelector('.text-panel')?.textContent ?? '').trim().slice(0, 80)
    c.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }))
    return { id: c.dataset.id, text }
  })()`,
  clickselect: `(() => {
    const cards = [...document.querySelectorAll('.card')]
    const c = cards[0]
    c.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }))
    return { id: c.dataset.id, sel: [...document.querySelectorAll('.card.selected')].map((x) => x.dataset.id) }
  })()`,
  enter: `(() => {
    const c = [...document.querySelectorAll('.card')][0]
    c.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, ctrlKey: true }))
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }))
    return { id: c.dataset.id, focus: document.querySelector('.card.focused')?.dataset.id ?? null }
  })()`,
  esc: `(() => {
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }))
    return { ok: true }
  })()`,
}
async function main() {
  await withPopup(async (c) => {
    const r = await evalIn(c, ev[action])
    await sleep(700)
    console.log(JSON.stringify(r))
  })
}
main().catch((e) => { console.error('FAIL:', e.message); process.exit(1) })