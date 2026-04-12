import { useState } from "react"
import { Download } from "lucide-react"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"

export function ExportButton() {
  const [toast, setToast] = useState<string | null>(null)

  function showToast(format: string) {
    setToast(`Export to ${format} coming soon`)
    setTimeout(() => setToast(null), 2000)
  }

  return (
    <div className="relative">
      <DropdownMenu>
        <DropdownMenuTrigger className="inline-flex shrink-0 items-center justify-center gap-2 rounded-md border bg-background px-3 py-1.5 text-sm font-medium shadow-xs hover:bg-accent hover:text-accent-foreground">
          <Download className="h-4 w-4" />
          Export
        </DropdownMenuTrigger>
        <DropdownMenuContent>
          <DropdownMenuItem onClick={() => showToast("CSV")}>
            Export CSV
          </DropdownMenuItem>
          <DropdownMenuItem onClick={() => showToast("Image")}>
            Export Image
          </DropdownMenuItem>
          <DropdownMenuItem onClick={() => showToast("Markdown")}>
            Export Markdown
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      {toast && (
        <div className="absolute right-0 top-full mt-2 z-50 rounded-md bg-popover border px-3 py-2 text-sm shadow-lg whitespace-nowrap text-muted-foreground">
          {toast}
        </div>
      )}
    </div>
  )
}
