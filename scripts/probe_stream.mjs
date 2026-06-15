// Detailed SSE timeline probe: fires a real /api/parse and prints every progress
// event with a timestamp (ms from POST start) so we can see what streams back and
// where it stalls. Usage:
//   BASE=http://127.0.0.1:7433 ACCOUNT_ID=tomi-premium-bonds COPIES=4 \
//   node scripts/probe_stream.mjs <file>
import { readFileSync, readdirSync } from "node:fs"
import { randomUUID } from "node:crypto"

const BASE = process.env.BASE ?? "http://127.0.0.1:7433"
const ACCOUNT_ID = process.env.ACCOUNT_ID ?? "tomi-premium-bonds"
const DIR = process.env.FILES_DIR // upload every pdf/csv/xlsx in this folder
const COPIES = Number(process.env.COPIES ?? 4)
const file = process.argv[2] ?? ".playwright-mcp/Download-18278731-1781449966305.pdf"
const hints = JSON.parse(
  process.env.HINTS ??
    '{"return_type":{"transactions":true,"holdings":{"enabled":true,"period":"quarterly"},"investments":false},"experimental":null,"hint":null}',
)
const parseId = randomUUID()
const t0 = Date.now()
const ac = new AbortController()
const ts = () => String(Date.now() - t0).padStart(6) + "ms"

async function readSse() {
  const res = await fetch(`${BASE}/api/parse/progress/${parseId}`, {
    headers: { Accept: "text/event-stream" },
    signal: ac.signal,
  })
  console.log(`${ts()}  SSE connected (${res.status})`)
  const reader = res.body.getReader()
  const dec = new TextDecoder()
  let buf = ""
  try {
    while (true) {
      const { value, done } = await reader.read()
      if (done) break
      buf += dec.decode(value, { stream: true })
      let i
      while ((i = buf.indexOf("\n\n")) !== -1) {
        const frame = buf.slice(0, i)
        buf = buf.slice(i + 2)
        let ev = null, data = null
        for (const line of frame.split("\n")) {
          if (line.startsWith("event:")) ev = line.slice(6).trim()
          else if (line.startsWith("data:")) data = line.slice(5).trim()
        }
        if (!ev) continue
        const d = data ? JSON.parse(data) : {}
        let extra = ""
        if (ev === "phase") extra = `phase=${d.phase} "${d.message}"`
        else if (ev === "llm_start") extra = `model=${d.model} input=${d.input_tokens}`
        else if (ev === "llm_progress") extra = `section=${d.section ?? "—"} items=${d.items} out_tokens=${d.output_tokens}`
        else if (ev === "done") extra = `total_ms=${d.total_ms}`
        else if (ev === "error") extra = `${d.code}: ${d.message}`
        console.log(`${ts()}  ${ev.padEnd(13)} ${extra}`)
        if (ev === "done" || ev === "error") return
      }
    }
  } catch {}
}

const form = new FormData()
let paths
if (DIR) {
  paths = readdirSync(DIR).filter((f) => /\.(pdf|csv|xlsx)$/i.test(f)).map((f) => `${DIR}/${f}`)
} else {
  paths = Array.from({ length: COPIES }, () => file)
}
for (const p of paths) {
  const name = p.split(/[/\\]/).pop()
  form.append("files[]", new Blob([readFileSync(p)]), name)
}
form.append("account_id", ACCOUNT_ID)
form.append("hints", JSON.stringify(hints))
form.append("parse_id", parseId)

console.log(`POST /api/parse  account=${ACCOUNT_ID} files=${paths.length} hints=${JSON.stringify(hints.return_type)}`)
const sse = readSse()
const res = await fetch(`${BASE}/api/parse`, { method: "POST", body: form })
console.log(`${ts()}  POST returned ${res.status}`)
if (res.ok) {
  const preview = await res.json()
  console.log(`${ts()}  preview: tx=${preview.transactions?.count} holdings=${preview.holdings?.count} inv=${preview.investments?.count}`)
} else {
  console.log(`${ts()}  POST body: ${(await res.text()).slice(0, 300)}`)
}
await Promise.race([sse, new Promise((r) => setTimeout(r, 4000))])
ac.abort()
