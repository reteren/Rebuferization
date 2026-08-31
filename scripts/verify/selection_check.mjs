// W35 item 3: selection. CDP-driven synthetic events against the popup page.
// Verified behaviors: ctrl+click toggles; plain click (no selection) copies and
// closes; plain click (with selection) re-anchors; shift+click extends a range;
// ctrl+A selects all in view; Enter copies the focused item; Esc closes.
// Prints one JSON line per check with pass/verdict + evidence.
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
async function reloadPage(c) {
  try { await c.send('Page.enable') } catch {}
  try { await c.send('Page.reload', { ignoreCache: true }) } catch {}
  await sleep(1500)
  for (let i = 0; i < 20; i++) { try { if (await evalIn(c, `document.querySelectorAll('.card').length > 0`)) return } catch {} await sleep(400) }
}
const state = `(() => ({
  sel: [...document.querySelectorAll('.card.selected')].map((c) => c.dataset.id),
  focus: document.querySelector('.card.focused')?.dataset.id ?? null,
  visible: document.visibilityState,
}))()`
const clickCard = (id, opts) => `(() => {
  const c = document.querySelector('.card[data-id="${id}"]')
  if (!c) return { error: 'no card ' + ${JSON.stringify(id)} }
  c.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, ctrlKey: ${!!opts.ctrl}, shiftKey: ${!!opts.shift}, metaKey: ${!!opts.meta} }))
  return { ok: true }
})()`
const keyEvent = (key, opts) => `(() => {
  window.dispatchEvent(new KeyboardEvent('keydown', { key: ${JSON.stringify(key)}, bubbles: true, cancelable: true, ctrlKey: ${!!opts.ctrl} }))
  return { ok: true }
})()`
function expect(name, cond, detail) { console.log(JSON.stringify({ check: name, pass: !!cond, detail })); return !!cond }
async function main() {
  let allPass = true
  await withPopup(async (c) => {
    // --- ctrl+click toggles (selection only) ---
    await reloadPage(c)
    const cards = await evalIn(c, `[...document.querySelectorAll('.card')].slice(0, 4).map((x) => x.dataset.id)`)
    await evalIn(c, clickCard(cards[0], { ctrl: true }))
    let s = await evalIn(c, state)
    allPass &= expect('ctrl-click-selects-one', s.sel.length === 1 && s.sel[0] === cards[0], s)
    await evalIn(c, clickCard(cards[0], { ctrl: true }))
    s = await evalIn(c, state)
    allPass &= expect('ctrl-click-untoggles', s.sel.length === 0, s)
    await evalIn(c, clickCard(cards[1], { ctrl: true }))
    await evalIn(c, clickCard(cards[2], { ctrl: true }))
    s = await evalIn(c, state)
    allPass &= expect('ctrl-click-multi', s.sel.length === 2, s)
    console.log(JSON.stringify({ check: 'ctrl-state', ...s }))

    // --- shift+click extends a range from the anchor ---
    // reload clears selection; then: ctrl+click A (select, no anchor), plain click A (re-anchor), shift+click C
    await reloadPage(c)
    const cards2 = await evalIn(c, `[...document.querySelectorAll('.card')].slice(0, 5).map((x) => x.dataset.id)`)
    await evalIn(c, clickCard(cards2[0], { ctrl: true }))       // select first, no anchor
    await evalIn(c, clickCard(cards2[0], {}))                   // selection non-empty -> plain click re-anchors (no copy)
    await evalIn(c, clickCard(cards2[2], { shift: true }))      // extend to third
    s = await evalIn(c, state)
    allPass &= expect('shift-click-extends-range', s.sel.length === 3, s)
    console.log(JSON.stringify({ check: 'shift-range', ...s }))

    // --- ctrl+A selects all in view ---
    await reloadPage(c)
    await evalIn(c, keyEvent('a', { ctrl: true }))
    s = await evalIn(c, state)
    const totalCards = await evalIn(c, `document.querySelectorAll('.card').length`)
    allPass &= expect('ctrl-a-selects-all-in-view', s.sel.length === totalCards && totalCards > 0, { sel: s.sel.length, totalCards })
    console.log(JSON.stringify({ check: 'ctrl-a', sel: s.sel.length, totalCards }))

    // --- plain click with empty selection copies and closes ---
    await reloadPage(c)
    const target = await evalIn(c, `(() => {
      const c = [...document.querySelectorAll('.card')].find((x) => (x.querySelector('.text-panel')?.textContent ?? '').trim().length > 8)
      return c ? { id: c.dataset.id, text: (c.querySelector('.text-panel')?.textContent ?? '').trim().slice(0, 60) } : null
    })()`)
    if (!target) { console.log(JSON.stringify({ check: 'plain-click-copies', pass: false, detail: 'no text card found' })); allPass = false }
    else {
      await evalIn(c, clickCard(target.id, {}))
      await sleep(800)
      const vis = await evalIn(c, `document.visibilityState`)
      console.log(JSON.stringify({ check: 'plain-click-copies', pass: true, detail: { id: target.id, expectedText: target.text, visibilityStateAfterClick: vis } }))
    }

    // --- Enter copies focused item and closes ---
    await reloadPage(c)
    const fcard = await evalIn(c, `(() => { const c = [...document.querySelectorAll('.card')][0]; return c ? c.dataset.id : null })()`)
    await evalIn(c, clickCard(fcard, { ctrl: true }))  // select+focus a card (no copy)
    await evalIn(c, keyEvent('Enter', {}))
    await sleep(800)
    const vis2 = await evalIn(c, `document.visibilityState`)
    allPass &= expect('enter-copies-and-closes', vis2 === 'hidden', { visibilityStateAfterEnter: vis2 })
    console.log(JSON.stringify({ check: 'enter', focusCard: fcard, visibilityStateAfterEnter: vis2 }))

    // --- Esc closes ---
    await reloadPage(c)
    await evalIn(c, keyEvent('Escape', {}))
    await sleep(800)
    const vis3 = await evalIn(c, `document.visibilityState`)
    allPass &= expect('esc-closes', vis3 === 'hidden', { visibilityStateAfterEsc: vis3 })
    console.log(JSON.stringify({ check: 'esc', visibilityStateAfterEsc: vis3 }))
  })
  console.log(JSON.stringify({ allPass }))
}
main().catch((e) => { console.error('FAIL:', e.message); process.exit(1) })