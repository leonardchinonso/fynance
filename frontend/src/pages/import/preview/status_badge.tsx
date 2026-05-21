import { Badge } from "@/components/ui/badge"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { cn } from "@/lib/utils"

/**
 * Visual indicator of what will happen to a row on commit.
 *
 * - `new`        → green "Create"
 * - `modify`     → amber "Update"
 * - `duplicate`  → yellow "Skip" (backend-detected; tooltip explains)
 * - `removed`    → red    "Skip" (frontend manual delete; tooltip explains)
 * - `error`      → red    "Skip" (backend extraction error; tooltip explains)
 */
export function StatusBadge({ status, errorMessage }: { status: string; errorMessage?: string | null }) {
  if (status === "new") {
    return (
      <Badge className={cn("bg-emerald-100 text-emerald-700 hover:bg-emerald-100 dark:bg-emerald-900/30 dark:text-emerald-400")}>
        Create
      </Badge>
    )
  }
  if (status === "modify") {
    return (
      <Badge className={cn("bg-amber-100 text-amber-700 hover:bg-amber-100 dark:bg-amber-900/30 dark:text-amber-400")}>
        Update
      </Badge>
    )
  }
  if (status === "duplicate") {
    return (
      <Tooltip>
        <TooltipTrigger
          render={
            <Badge
              variant="outline"
              className={cn(
                "border-yellow-500/40 bg-yellow-500/10 text-yellow-700 dark:text-yellow-400 hover:bg-yellow-500/10 cursor-help"
              )}
            >
              Skip
            </Badge>
          }
        />
        <TooltipContent>This row is already in your database. Skipping on commit.</TooltipContent>
      </Tooltip>
    )
  }
  if (status === "removed") {
    return (
      <Tooltip>
        <TooltipTrigger
          render={
            <Badge
              variant="outline"
              className={cn(
                "border-rose-500/40 bg-rose-500/10 text-rose-600 dark:text-rose-400 hover:bg-rose-500/10 cursor-help"
              )}
            >
              Skip
            </Badge>
          }
        />
        <TooltipContent>Manually removed. Click the undo icon to restore.</TooltipContent>
      </Tooltip>
    )
  }
  if (status === "error") {
    return (
      <Tooltip>
        <TooltipTrigger
          render={
            <Badge
              variant="outline"
              className="border-destructive/50 bg-destructive/10 text-destructive cursor-help"
            >
              Skip
            </Badge>
          }
        />
        <TooltipContent>{errorMessage ?? "Could not be parsed. Skipping on commit."}</TooltipContent>
      </Tooltip>
    )
  }
  return <Badge variant="outline">{status}</Badge>
}
