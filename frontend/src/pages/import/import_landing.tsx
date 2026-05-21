import { useNavigate } from "react-router-dom"
import { FileUp, ListChecks, ArrowRight } from "lucide-react"
import { cn } from "@/lib/utils"

interface CardOption {
  to: string
  icon: React.ReactNode
  title: string
  description: string
}

const OPTIONS: CardOption[] = [
  {
    to: "/import/single",
    icon: <FileUp className="h-6 w-6" />,
    title: "Import to specific account",
    description:
      "Pick one account, drop in statements (CSV, PDF, XLSX), review the parsed rows, and commit.",
  },
  {
    to: "/import/wizard",
    icon: <ListChecks className="h-6 w-6" />,
    title: "Monthly ingestion wizard",
    description:
      "Walk through your selected accounts in order. Upload, review, commit, then move to the next.",
  },
]

export function ImportLanding() {
  const navigate = useNavigate()
  return (
    <div className="max-w-3xl mx-auto py-4">
      <div className="mb-6">
        <h1 className="text-xl font-semibold">Import</h1>
        <p className="text-sm text-muted-foreground">Choose how you want to bring in data.</p>
      </div>

      <div className="grid gap-3 sm:grid-cols-2">
        {OPTIONS.map((opt) => (
          <button
            key={opt.to}
            onClick={() => navigate(opt.to)}
            className={cn(
              "group text-left rounded-xl border bg-card p-5",
              "transition-all hover:border-foreground/30 hover:shadow-sm",
              "focus-visible:outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
            )}
          >
            <div className="flex items-start gap-3">
              <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-secondary text-secondary-foreground">
                {opt.icon}
              </div>
              <div className="flex-1 min-w-0">
                <div className="flex items-center justify-between gap-2">
                  <h2 className="text-base font-semibold">{opt.title}</h2>
                  <ArrowRight className="h-4 w-4 shrink-0 text-muted-foreground transition-transform group-hover:translate-x-0.5" />
                </div>
                <p className="mt-1 text-xs text-muted-foreground">{opt.description}</p>
              </div>
            </div>
          </button>
        ))}
      </div>
    </div>
  )
}
