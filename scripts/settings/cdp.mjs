// CDP driver for the verification run. Node 22+ (global WebSocket/fetch).
// Usage:
//   node cdp.mjs targets
//   node cdp.mjs eval <targetId> <js-expr>
//   node cdp.mjs invoke <targetId> <command> <json-args>
//   node cdp.mjs shot <targetId> <outfile.png>
//   node cdp.mjs keys <targetId> <json-array-of-events>
//   node cdp.mjs html <targetId>
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

const [, , cmd, ...rest] = process.argv

async function main() {
  if (cmd === 'targets') {
    const ts = await getTargets()
    for (const t of ts) {
      console.log(JSON.stringify({ id: t.id, title: t.title, url: t.url, type: t.type }))
    }
    return
  }
  const id = rest[0]
  if (cmd === 'eval') {
    const expr = rest[1]
    const out = await withTarget(id, (c) =>
      c.send('Runtime.evaluate', {
        expression: expr,
        returnByValue: true,
        awaitPromise: true,
      }),
    )
    if (out.exceptionDetails) {
      console.error('EXCEPTION:', JSON.stringify(out.exceptionDetails, null, 2))
      process.exit(2)
    }
    console.log(typeof out.result.value === 'string' ? out.result.value : JSON.stringify(out.result.value))
    return
  }
  if (cmd === 'invoke') {
    const command = rest[1]
    const args = rest[2] ? JSON.parse(rest[2]) : {}
    const expr = `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)}, ${JSON.stringify(args)}).then(r => JSON.stringify(r), e => 'INVOKE_ERR:' + JSON.stringify(e))`
    const out = await withTarget(id, (c) =>
      c.send('Runtime.evaluate', { expression: expr, returnByValue: true, awaitPromise: true }),
    )
    if (out.exceptionDetails) {
      console.error('EXCEPTION:', JSON.stringify(out.exceptionDetails, null, 2))
      process.exit(2)
    }
    console.log(out.result.value)
    return
  }
  if (cmd === 'shot') {
    const file = rest[2]
    await withTarget(id, async (c) => {
      await c.send('Page.enable')
      await sleep(250)
      const out = await c.send('Page.captureScreenshot', { format: 'png' })
      if (!out.data) throw new Error('no screenshot data')
      const { writeFileSync } = await import('node:fs')
      writeFileSync(file, Buffer.from(out.data, 'base64'))
      console.log(`saved ${file} (${out.data.length} b64 chars)`)
    })
    return
  }
  if (cmd === 'keys') {
    const events = JSON.parse(rest[1])
    await withTarget(id, async (c) => {
      await c.send('Input.dispatchKeyEvent', { type: 'rawKeyDown' })
      for (const e of events) {
        await c.send('Input.dispatchKeyEvent', e)
      }
    })
    console.log(`sent ${events.length} key events`)
    return
  }
  if (cmd === 'html') {
    const out = await withTarget(id, (c) =>
      c.send('Runtime.evaluate', {
        expression: `document.documentElement.outerHTML`,
        returnByValue: true,
      }),
    )
    console.log(out.result.value)
    return
  }
  throw new Error(`unknown command ${cmd}`)
}

main().catch((e) => {
  console.error('FAIL:', e.message)
  process.exit(1)
})