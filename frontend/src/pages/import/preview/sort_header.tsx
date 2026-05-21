import { TableHead } from "@/components/ui/table"
import { ArrowDown, ArrowUp, ArrowUpDown } from "lucide-react"
import { cn } from "@/lib/utils"

interface Props {
  /** Display label for the column. */
  label: string
  /** Stable id of this column; matches the value stored in URL state. */
  columnId: string
  /** Currently active sort column (or "" / null when nothing is sorted). */
  activeColumn: string
  /** Direction of the active sort. Ignored when `activeColumn !== columnId`. */
  direction: "asc" | "desc"
  /** Click handler — cycle is none → asc → desc → none, owned by the parent. */
  onClick: () => void
  className?: string
  align?: "left" | "right"
}

/** A clickable TableHead with a sort indicator. Use inside `<TableHeader>`. */
export function SortHeader({ label, columnId, activeColumn, direction, onClick, className, align }: Props) {
  const isActive = activeColumn === columnId
  const Icon = !isActive ? ArrowUpDown : direction === "asc" ? ArrowUp : ArrowDown
  return (
    <TableHead className={cn(className, align === "right" && "text-right")}>
      <button
        type="button"
        onClick={onClick}
        className={cn(
          "inline-flex items-center gap-1 cursor-pointer transition-colors hover:text-foreground",
          isActive ? "text-foreground" : "text-muted-foreground",
          align === "right" && "ml-auto"
        )}
      >
        {label}
        <Icon className={cn("h-3 w-3", !isActive && "opacity-50")} />
      </button>
    </TableHead>
  )
}
