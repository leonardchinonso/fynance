// Drives the import progress bar from the /api/parse SSE stream plus a wall-clock
// tick. Real events (`llm_start`, `post_processing`, `done`) snap the bar forward
// between segments; between events the bar eases toward the current segment ceiling
// with a time-constant sized to the *expected* duration, so it keeps creeping the
// whole time (never saturating early) and the label carries the honest live signals
// (elapsed clock + streamed token count).
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
  private items = 0
  private section: string | null = null
  private postLabel: string | null = null
  private errorLabel: string | null = null
  private readonly est: { preMs: number; llmMs: number }
  private lastTick: number

  constructor(files: File[], now: number = performance.now()) {
    this.est = estimate(files)
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
        this.items = event.items
        if (event.section) this.section = event.section
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
    return { value: this.value, label: this.label(), state: this.uiState() }
  }

  private uiState(): ParseProgressUi["state"] {
    if (this.phase === "error") return "error"
    if (this.phase === "done") return "done"
    return "running"
  }

  private label(): string {
    if (this.phase === "error") return this.errorLabel ?? "Import failed"
    if (this.phase === "done") return "Done"
    if (this.phase === "post") return this.postLabel ?? "Checking for duplicates"
    if (this.phase === "pre") return "Reading your statement…"
    // LLM phase: real progress derived from the streamed tool JSON — which section
    // the model is on and how many rows it has produced so far.
    const what = this.section ? `Extracting ${this.section}` : "Reading your statement"
    const found = this.items > 0 ? ` · ${this.items.toLocaleString()} found` : ""
    return `${what}${found}`
  }
}
