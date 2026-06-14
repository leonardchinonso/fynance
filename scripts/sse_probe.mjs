// Quick probe: does GET /api/parse/progress/:id actually deliver llm_progress
// events to a subscriber? Fires a real parse and counts the SSE events received.
// Usage: BASE=http://127.0.0.1:7433 ACCOUNT_ID=ope-amex node scripts/sse_probe.mjs <file>
import { readFileSync } from "node:fs"
import { randomUUID } from "node:crypto"

const BASE = process.env.BASE ?? "http://127.0.0.1:7433"
const ACCOUNT_ID = process.env.ACCOUNT_ID ?? "ope-amex"
const file = process.argv[2] ?? ".playwright-mcp/Download-18278731-1781449966305.pdf"
const parseId = randomUUID()
const counts = {}
const ac = new AbortController()

async function readSse() {
  const res = await fetch(`${BASE}/api/parse/progress/${parseId}`, {
    headers: { Accept: "text/event-stream" },
    signal: ac.signal,
  })
  console.log("SSE response status:", res.status, res.headers.get("content-type"))
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
        counts[ev] = (counts[ev] ?? 0) + 1
        if (ev === "llm_progress" || ev === "llm_start") {
          const d = JSON.parse(data)
          console.log(`  <- ${ev}`, d.output_tokens ?? d.input_tokens ?? "", "tokens")
        }
        if (ev === "done" || ev === "error") return
      }
    }
  } catch {}
}

const sse = readSse()
const form = new FormData()
form.append("files[]", new Blob([readFileSync(file)]), file.split(/[/\\]/).pop())
form.append("account_id", ACCOUNT_ID)
form.append("hints", JSON.stringify({ return_type: { transactions: true, holdings: { enabled: false, period: null }, investments: false }, experimental: null, hint: null }))
form.append("parse_id", parseId)

const t0 = Date.now()
const res = await fetch(`${BASE}/api/parse`, { method: "POST", body: form })
console.log(`POST /api/parse -> ${res.status} in ${Date.now() - t0}ms`)
await Promise.race([sse, new Promise((r) => setTimeout(r, 3000))])
ac.abort()
console.log("\nSSE event counts:", counts)
console.log(counts.llm_progress > 0 ? "✅ llm_progress IS delivered" : "❌ NO llm_progress received")
