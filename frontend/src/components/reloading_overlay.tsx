import { Ripple } from "./ripple"

/**
 * Semi-transparent overlay with a ripple animation, shown while a background
 * reload is in progress. Dims the content beneath and blocks pointer events
 * without changing the cursor.
 *
 * The parent element must have `position: relative` (add `className="relative"`).
 *
 * Usage:
 * ```tsx
 * <div className="relative">
 *   <div className={data.status === "reloading" ? "pointer-events-none" : ""}>
 *     <Content />
 *   </div>
 *   <ReloadingOverlay active={data.status === "reloading"} />
 * </div>
 * ```
 */
export function ReloadingOverlay({ active }: { active: boolean }) {
  if (!active) return null
  return (
    <div className="absolute inset-0 z-10 flex items-center justify-center bg-background/70 text-foreground">
      <Ripple size="md" />
    </div>
  )
}
