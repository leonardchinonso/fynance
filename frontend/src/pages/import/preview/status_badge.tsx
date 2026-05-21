import { Badge } from "@/components/ui/badge"
import { cn } from "@/lib/utils"

/** Visual indicator of what will happen to a row on commit. */
export function StatusBadge({ status }: { status: string }) {
  if (status === "new") {
    return <Badge className={cn("bg-emerald-100 text-emerald-700 hover:bg-emerald-100 dark:bg-emerald-900/30 dark:text-emerald-400")}>Create</Badge>
  }
  if (status === "modify") {
    return <Badge className={cn("bg-amber-100 text-amber-700 hover:bg-amber-100 dark:bg-amber-900/30 dark:text-amber-400")}>Update</Badge>
  }
  if (status === "duplicate") {
    return <Badge variant="outline" className="text-muted-foreground">Skip (duplicate)</Badge>
  }
  if (status === "error") {
    return <Badge variant="outline" className="border-destructive/50 text-destructive">Skip (error)</Badge>
  }
  return <Badge variant="outline">{status}</Badge>
}
