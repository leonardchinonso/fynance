import { cn } from "@/lib/utils"
import { Progress } from "@/components/ui/progress"
import { Ripple } from "./ripple"

interface Props {
  active: boolean
  /** Optional caption rendered beneath the ripple (ignored when `progress` is set). */
  text?: string
  /**
   * Optional determinate-ish progress shown beneath the ripple: a bar plus a live
   * label. When set, it replaces `text`. `value` is 0–100.
   */
  progress?: { value: number; label: string; state?: "running" | "error" | "done" }
  /**
   * When true, covers the entire viewport (z-100, blocks navbar clicks too).
   * Default behaviour overlays the nearest positioned ancestor (use this for
   * in-card reload spinners — the parent needs `position: relative`).
   */
  fullscreen?: boolean
}

/**
 * Semi-transparent overlay with a ripple animation, shown while a background
 * operation is in progress. Dims the content beneath and blocks pointer events.
 *
 * For in-place reloads: `<div className="relative"> ... <ReloadingOverlay /> </div>`.
 * For app-blocking work (e.g. file parse): pass `fullscreen` so the navbar is
 * covered too.
 */
export function ReloadingOverlay({ active, text, progress, fullscreen = false }: Props) {
  if (!active) return null
  return (
    <div
      role={fullscreen ? "dialog" : undefined}
      aria-modal={fullscreen ? true : undefined}
      aria-label={text ?? "Loading"}
      className={cn(
        "flex flex-col items-center justify-center gap-4 bg-background/80 text-foreground",
        fullscreen
          ? "fixed inset-0 z-[100] backdrop-blur-sm"
          : "absolute inset-0 z-10"
      )}
      // In fullscreen mode, swallow wheel events so the page beneath doesn't scroll.
      onWheel={fullscreen ? (e) => e.preventDefault() : undefined}
    >
      <Ripple size="md" />
      {progress ? (
        <div className="mt-4 w-64 max-w-[80vw]">
          <Progress
            value={progress.value}
            className={cn(
              "transition-all",
              progress.state === "error" &&
                "[&_[data-slot=progress-indicator]]:bg-destructive"
            )}
          />
          <p className="mt-3 max-w-xs px-4 text-center text-sm text-muted-foreground">
            {progress.label}
          </p>
        </div>
      ) : (
        text && (
          <p className="max-w-xs px-4 text-center text-sm text-muted-foreground">{text}</p>
        )
      )}
    </div>
  )
}
