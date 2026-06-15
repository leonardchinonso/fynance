#!/usr/bin/env node
// Empirical phase-timing calibration for the /api/parse SSE progress stream.
//
// For each input config it opens the SSE progress stream, fires the multipart
// POST /api/parse, and timestamps every progress event relative to parse start.
// Runs each config N times, drops the warm-up run, and prints + writes a summary
// of per-phase durations and their fraction of total time. The output
// (scripts/parse_timings.json) calibrates the frontend progress bar.
//
// Prereqs: a backend serving locally with a valid LLM API key, and an existing
// account id. Usage:
//   FYNANCE_DB_PATH=... cargo run -- account add --id cal --name Cal --institution test --type current
//   FYNANCE_DB_PATH=... cargo run -- serve --no-open
//   ACCOUNT_ID=cal node scripts/calibrate_parse.mjs
//
// Env: BASE_URL (default http://127.0.0.1:7433), ACCOUNT_ID (required),
//      RUNS (default 6), TOKEN (optional bearer), CONFIGS (csv of ids to limit).

import { readFileSync, writeFileSync, mkdtempSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { randomUUID } from "node:crypto"

const BASE = process.env.BASE_URL ?? "http://127.0.0.1:7433"
const ACCOUNT_ID = process.env.ACCOUNT_ID
const RUNS = Number(process.env.RUNS ?? 6)
const TOKEN = process.env.TOKEN
const FIX = "backend/tests/fixtures"

if (!ACCOUNT_ID) {
  console.error("ACCOUNT_ID env var is required (an existing account on the serving DB).")
  process.exit(1)
}

const rt = (transactions, holdings, investments) => ({
  return_type: { transactions, holdings: { enabled: holdings, period: null }, investments },
  experimental: null,
  hint: null,
})

// A tiny synthetic investments CSV so we can exercise the investments path.
const investCsv = join(mkdtempSync(join(tmpdir(), "cal-")), "invest.csv")
writeFileSync(
  investCsv,
  "date,type,symbol,quantity,price,fee,currency\n" +
    "2026-01-15,buy,VWRL,10,95.00,1.00,GBP\n" +
    "2026-02-10,sell,VWRL,4,120.00,1.00,GBP\n" +
    "2026-03-01,buy,VUSA,5,80.00,1.00,GBP\n",
)

const CONFIGS = [
  { id: "txn-small", files: [`${FIX}/monzo.csv`], hints: rt(true, false, false) },
  { id: "txn-mid", files: [`${FIX}/lloyds.csv`], hints: rt(true, false, false) },
  { id: "txn-multi", files: [`${FIX}/monzo.csv`, `${FIX}/lloyds.csv`, `${FIX}/revolut.csv`], hints: rt(true, false, false) },
  { id: "pdf", files: [`${FIX}/sample_statement.pdf`], hints: rt(true, false, false) },
  { id: "holdings", files: [`${FIX}/sample_holdings.xlsx`], hints: rt(false, true, false) },
  { id: "invest", files: [investCsv], hints: rt(false, false, true) },
]

const only = process.env.CONFIGS?.split(",").map((s) => s.trim())
const configs = only ? CONFIGS.filter((c) => only.includes(c.id)) : CONFIGS

const authHeaders = TOKEN ? { Authorization: `Bearer ${TOKEN}` } : {}

// Read the SSE stream, recording {event, data, t} for each frame until `done`,
// `error`, or the abort signal fires. `t0` is the parse-start reference.
async function readSse(parseId, t0, signal, events) {
  const res = await fetch(`${BASE}/api/parse/progress/${parseId}`, {
    headers: { Accept: "text/event-stream", ...authHeaders },
    signal,
  })
  const reader = res.body.getReader()
  const decoder = new TextDecoder()
  let buf = ""
  try {
    while (true) {
      const { value, done } = await reader.read()
      if (done) break
      buf += decoder.decode(value, { stream: true })
      let idx
      while ((idx = buf.indexOf("\n\n")) !== -1) {
        const frame = buf.slice(0, idx)
        buf = buf.slice(idx + 2)
        let ev = null
        let data = null
        for (const line of frame.split("\n")) {
          if (line.startsWith("event:")) ev = line.slice(6).trim()
          else if (line.startsWith("data:")) data = line.slice(5).trim()
        }
        if (!ev) continue
        events.push({ event: ev, data: data ? JSON.parse(data) : null, t: Date.now() - t0 })
        if (ev === "done" || ev === "error") return
      }
    }
  } catch {
    // aborted or stream closed
  }
}

function buildForm(cfg, parseId) {
  const form = new FormData()
  for (const p of cfg.files) {
    const buf = readFileSync(p)
    form.append("files[]", new Blob([buf]), p.split(/[/\\]/).pop())
  }
  form.append("account_id", ACCOUNT_ID)
  form.append("hints", JSON.stringify(cfg.hints))
  form.append("parse_id", parseId)
  return form
}

async function runOnce(cfg) {
  const parseId = randomUUID()
  const events = []
  const ac = new AbortController()
  const t0 = Date.now()
  const sse = readSse(parseId, t0, ac.signal, events)
  let ok = true
  let errMsg = null
  try {
    const res = await fetch(`${BASE}/api/parse`, {
      method: "POST",
      headers: authHeaders,
      body: buildForm(cfg, parseId),
    })
    if (!res.ok) {
      ok = false
      errMsg = `${res.status} ${await res.text()}`
    } else {
      await res.json()
    }
  } catch (e) {
    ok = false
    errMsg = String(e)
  }
  // Give the terminal SSE frame a moment, then stop reading.
  await Promise.race([sse, new Promise((r) => setTimeout(r, 1500))])
  ac.abort()
  return { ok, errMsg, events }
}

// Phase boundaries from the recorded event timeline. Robust to both the split
// path (preprocessing -> sending_to_llm) and the unified path (building_context):
// anchor on llm_start / post_processing / done, which always appear.
function phaseDurations(events) {
  const at = (pred) => events.find(pred)?.t
  const tLlmStart = at((e) => e.event === "llm_start")
  const tPost = at((e) => e.event === "phase" && e.data?.phase === "post_processing")
  const tDone = at((e) => e.event === "done")
  const phases = events.filter((e) => e.event === "phase").map((e) => e.data?.phase)
  if (tLlmStart == null || tPost == null || tDone == null) return null
  return {
    D_pre: tLlmStart, // start -> LLM call begins (file read + context build)
    D_llm: tPost - tLlmStart, // the LLM call (dominant)
    D_post: tDone - tPost, // dedup / assembly
    total: tDone,
    phases,
  }
}

const stats = (xs) => {
  const n = xs.length
  const mean = xs.reduce((a, b) => a + b, 0) / n
  const sd = Math.sqrt(xs.reduce((a, b) => a + (b - mean) ** 2, 0) / Math.max(1, n - 1))
  return { mean: Math.round(mean), sd: Math.round(sd), n }
}

const results = {}
for (const cfg of configs) {
  process.stdout.write(`\n[${cfg.id}] `)
  const runs = []
  for (let i = 0; i < RUNS; i++) {
    const { ok, errMsg, events } = await runOnce(cfg)
    if (!ok) {
      process.stdout.write(`x(${errMsg?.slice(0, 60)}) `)
      continue
    }
    const d = phaseDurations(events)
    if (!d) {
      process.stdout.write("?(incomplete) ")
      continue
    }
    process.stdout.write(`${d.total}ms `)
    runs.push(d)
  }
  // Drop the first successful run (warm-up).
  const used = runs.slice(1)
  if (used.length === 0) {
    results[cfg.id] = { error: "no complete runs" }
    continue
  }
  const keys = ["D_pre", "D_llm", "D_post", "total"]
  const agg = {}
  for (const k of keys) agg[k] = stats(used.map((r) => r[k]))
  const totMean = agg.total.mean || 1
  agg.fractions = {
    pre: +(agg.D_pre.mean / totMean).toFixed(3),
    llm: +(agg.D_llm.mean / totMean).toFixed(3),
    post: +(agg.D_post.mean / totMean).toFixed(3),
  }
  agg.phases = used.at(-1).phases
  results[cfg.id] = { runs: used.length, ...agg }
}

console.log("\n\n===== Per-config summary (ms, mean±sd; fractions of total) =====")
for (const [id, r] of Object.entries(results)) {
  if (r.error) {
    console.log(`${id.padEnd(10)} ${r.error}`)
    continue
  }
  const f = r.fractions
  console.log(
    `${id.padEnd(10)} total=${r.total.mean}±${r.total.sd}  ` +
      `pre=${r.D_pre.mean}(${f.pre}) llm=${r.D_llm.mean}(${f.llm}) post=${r.D_post.mean}(${f.post})  ` +
      `n=${r.runs}  phases=[${r.phases.join(" ")}]`,
  )
}

writeFileSync("scripts/parse_timings.json", JSON.stringify(results, null, 2) + "\n")
console.log("\nWrote scripts/parse_timings.json")
