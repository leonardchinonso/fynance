// Drives the import progress bar from the /api/parse SSE stream plus a wall-clock
// tick. Real events (`llm_start`, `post_processing`, `done`) snap the bar forward
// between calibrated segments; between events the bar eases toward the current
// segment ceiling, slowing as it approaches so it always moves but never completes
// a segment until the real event lands. Tuning is calibrated from real parses —
// see scripts/parse_timings.md / scripts/parse_timings.json.
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
// final jump to 100, so it gets no segment of its own.
const SEG = { pre: 15, llm: 90 } as const

// Cosmetic during the opaque LLM stage: the backend can't confirm sub-steps, so
// these stay generic. The model name + live token count below are the real signal.
const ROTATION = [
  "Reading your statements",
  "Extracting transactions",
  "Matching categories",
  "Almost there",
]
const ROTATION_MS = 3000

// Expected per-segment durations, from scripts/parse_timings.json: pre ~800ms for
// CSV/XLSX and ~1800ms for PDF; the LLM call ~1.4s for a small CSV, ~+1.5s per extra
// file, and ~9s for a PDF.
function estimate(files: File[]): { preMs: number; llmMs: number } {
  const hasPdf = files.some((f) => /\.pdf$/i.test(f.name))
  const n = Math.max(1, files.length)
  return {
    preMs: hasPdf ? 1800 : 800,
    llmMs: hasPdf ? 9000 : 1400 + (n - 1) * 1500,
  }
}

type Phase = "pre" | "llm" | "post" | "done" | "error"

export class ParseProgressController {
  private phase: Phase = "pre"
  private value = 0
  private tokens = 0
  private postLabel: string | null = null
  private errorLabel: string | null = null
  private readonly est: { preMs: number; llmMs: number }
  private readonly startedAt: number
  private lastTick: number

  constructor(files: File[], now: number = performance.now()) {
    this.est = estimate(files)
    this.startedAt = now
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
        this.tokens = event.output_tokens
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
      const tau = (this.phase === "pre" ? this.est.preMs : this.est.llmMs) * 0.7
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
    const base = ROTATION[Math.floor((now - this.startedAt) / ROTATION_MS) % ROTATION.length]
    if (this.phase === "llm" && this.tokens > 0) {
      return `${base} · ${this.tokens.toLocaleString()} tokens`
    }
    return base
  }
}
