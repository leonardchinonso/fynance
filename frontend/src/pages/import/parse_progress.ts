// Drives the import progress bar during a /api/parse call. The bar value eases
// toward calibrated segment ceilings (snapped forward by the real `llm_start` /
// `post_processing` / `done` events). The label is cosmetic and purely time-driven:
// the backend can't confirm sub-steps within the single opaque LLM call, so we walk
// a fixed list of plausible phrases, each owning an uneven slice of the *expected*
// total duration so the cadence feels organic. It never loops — once the parse
// overruns the estimate, the label simply holds on "Almost there".
//
// Pace is sized from production parse logs, not the tiny test fixture: real PDF
// statements run ~0.65s/KB and routinely take 2-3 minutes; CSV/Excel are far faster.
//
// Aliased: the generated binding's `ProgressEvent` shadows the DOM global.
import type { ProgressEvent as ParseProgressEvent } from "@/bindings/ProgressEvent"

export interface ParseProgressUi {
  /** 0–100 */
  value: number
  label: string
  state: "running" | "error" | "done"
}

// Segment ceilings on the 0–100 bar. post-processing (~1ms) is folded into the
// final jump to 100, so it gets no segment of its own. The LLM segment is wide and
// approached asymptotically — for a long parse the bar realistically sits in the
// 50-80% range and keeps inching up, rather than parking at the ceiling.
const SEG = { pre: 15, llm: 90 } as const

// Cosmetic, time-driven labels for the parse wait. Each owns a deliberately uneven
// slice (`weight`, a fraction of the expected total duration) so the cadence feels
// organic rather than uniform. The last label holds until the parse finishes.
const WAIT_LABELS = [
  { text: "Reading your statements", weight: 0.1 },
  { text: "Scanning each page", weight: 0.06 },
  { text: "Pulling out transactions", weight: 0.16 },
  { text: "Reading dates and amounts", weight: 0.18 },
  { text: "Identifying merchants", weight: 0.08 },
  { text: "Matching categories", weight: 0.13 },
  { text: "Checking holdings and balances", weight: 0.07 },
  { text: "Cross-referencing accounts", weight: 0.1 },
  { text: "Tidying up the numbers", weight: 0.05 },
  { text: "Almost there", weight: 0.07 },
] as const

// Cumulative start fraction for each label (label i begins once elapsed reaches the
// sum of all earlier weights). The final entry's end is open — it holds to the end.
const LABEL_STARTS: number[] = WAIT_LABELS.reduce<number[]>((acc, _l, i) => {
  acc.push(i === 0 ? 0 : acc[i - 1] + WAIT_LABELS[i - 1].weight)
  return acc
}, [])

// Expected segment durations. The LLM time scales with statement size and is the
// dominant, highly-variable phase; these are deliberate slight over-estimates so the
// bar paces steadily rather than racing to the ceiling and freezing.
function estimate(files: File[]): { preMs: number; llmMs: number } {
  const hasPdf = files.some((f) => /\.pdf$/i.test(f.name))
  const kb = files.reduce((sum, f) => sum + f.size, 0) / 1024
  const preMs = hasPdf ? 1800 : 800
  const llmMs = hasPdf
    ? Math.min(210_000, Math.max(20_000, kb * 650)) // PDF: ~0.65s/KB, 20s floor, 3.5min cap
    : Math.max(2_000, kb * 60) //                       CSV/Excel: ~60ms/KB, 2s floor
  return { preMs, llmMs }
}

type Phase = "pre" | "llm" | "post" | "done" | "error"

export class ParseProgressController {
  private phase: Phase = "pre"
  private value = 0
  private postLabel: string | null = null
  private errorLabel: string | null = null
  private readonly est: { preMs: number; llmMs: number }
  private readonly startedAt: number
  private readonly expectedTotalMs: number
  private lastTick: number

  constructor(files: File[], now: number = performance.now()) {
    this.est = estimate(files)
    this.startedAt = now
    this.expectedTotalMs = this.est.preMs + this.est.llmMs
    this.lastTick = now
  }

  /** Apply one SSE progress event. */
  onEvent(event: ParseProgressEvent): void {
    switch (event.event) {
      case "llm_start":
        this.phase = "llm"
        this.value = Math.max(this.value, SEG.pre)
        break
      case "llm_progress":
        if (this.phase === "pre") this.value = Math.max(this.value, SEG.pre)
        this.phase = "llm"
        break
      case "phase":
        if (event.phase === "post_processing") {
          this.phase = "post"
          this.value = Math.max(this.value, SEG.llm)
          this.postLabel = event.message
        }
        break
      case "done":
        this.phase = "done"
        this.value = 100
        break
      case "error":
        // not_found is transient (the channel was not ready yet); ignore it.
        if (event.code !== "not_found") {
          this.phase = "error"
          this.errorLabel = event.message
        }
        break
    }
  }

  /** Force completion — the parse POST resolved, even if the SSE `done` was missed. */
  complete(): void {
    if (this.phase !== "error") {
      this.phase = "done"
      this.value = 100
    }
  }

  /** Mark a terminal failure — the parse POST rejected. */
  fail(message: string): void {
    this.phase = "error"
    this.errorLabel = message
  }

  /** Advance the eased value and return the current UI snapshot. */
  sample(now: number = performance.now()): ParseProgressUi {
    if (this.phase === "pre" || this.phase === "llm") {
      const ceil = this.phase === "pre" ? SEG.pre : SEG.llm
      // tau ≈ expected segment duration: the bar reaches ~63% of the segment at the
      // expected time and keeps creeping toward (never reaching) the ceiling, so it is
      // always perceptibly moving even when a parse runs far longer than expected.
      const tau = this.phase === "pre" ? this.est.preMs : this.est.llmMs
      const dt = Math.max(0, now - this.lastTick)
      this.value = Math.min(ceil, this.value + (ceil - this.value) * (1 - Math.exp(-dt / tau)))
    }
    this.lastTick = now
    return { value: this.value, label: this.label(now), state: this.uiState() }
  }

  private uiState(): ParseProgressUi["state"] {
    if (this.phase === "error") return "error"
    if (this.phase === "done") return "done"
    return "running"
  }

  private label(now: number): string {
    if (this.phase === "error") return this.errorLabel ?? "Import failed"
    if (this.phase === "done") return "Done"
    if (this.phase === "post") return this.postLabel ?? "Checking for duplicates"
    // pre + llm: walk the cosmetic label list by fraction of the expected total,
    // holding on the last one once we overrun (never looping back).
    const f = this.expectedTotalMs > 0 ? (now - this.startedAt) / this.expectedTotalMs : 1
    let idx = 0
    while (idx + 1 < LABEL_STARTS.length && f >= LABEL_STARTS[idx + 1]) idx++
    return WAIT_LABELS[idx].text
  }
}
