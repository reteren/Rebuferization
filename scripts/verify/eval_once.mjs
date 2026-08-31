// Minimal CDP eval helper: node eval_once.mjs "<js expression>"
const PORT = process.env.RBF_CDP_PORT || '9333'
async function main() {
  const res = await fetch(`http://127.0.0.1:${PORT}/json`)
  const ts = await res.json()
  const t = ts.find((x) => x.type === 'page' && !/settings\.html/.test(x.url))
  if (!t) throw new Error('no popup target')
  const ws = new WebSocket(t.webSocketDebuggerUrl)
  await new Promise((res, rej) => { ws.addEventListener('open', res, { once: true }); ws.addEventListener('error', () => rej(new Error('ws'))) })
  let seq = 0; const pending = new Map()
  ws.addEventListener('message', (ev) => { const m = JSON.parse(ev.data); if (m.id && pending.has(m.id)) { const p = pending.get(m.id); pending.delete(m.id); m.error ? p.rej(new Error(JSON.stringify(m.error))) : p.res(m.result) } })
  const send = (method, params = {}) => new Promise((res, rej) => { const id = ++seq; pending.set(id, { res, rej }); ws.send(JSON.stringify({ id, method, params })) })
  const o = await send('Runtime.evaluate', { expression: process.argv[2], returnByValue: true, awaitPromise: true })
  if (o.exceptionDetails) { console.error('EXC: ' + (o.exceptionDetails.exception?.description ?? o.exceptionDetails.text)); process.exit(1) }
  console.log(typeof o.result.value === 'string' ? o.result.value : JSON.stringify(o.result.value))
  ws.close()
  process.exit(0)
}
main().catch((e) => { console.error('FAIL:', e.message); process.exit(1) })