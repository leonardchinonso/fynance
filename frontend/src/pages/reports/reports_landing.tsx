import { useNavigate } from "react-router-dom"
import { Receipt, FileText, ArrowRight } from "lucide-react"
import { cn } from "@/lib/utils"

interface CardOption {
  to: string | null
  icon: React.ReactNode
  title: string
  description: string
  disabled?: boolean
}

const OPTIONS: CardOption[] = [
  {
    to: "/reports/cgt",
    icon: <Receipt className="h-6 w-6" />,
    title: "Capital Gains Tax report",
    description:
      "Generate a UK HMRC-style report of your disposals, gains, and S104 pool workings for any tax year.",
  },
  {
    to: null,
    icon: <FileText className="h-6 w-6" />,
    title: "More reports coming soon",
    description:
      "Monthly summaries, AI-generated analysis, and export tools will appear here.",
    disabled: true,
  },
]

export function ReportsLanding() {
  const navigate = useNavigate()
  return (
    <div className="max-w-3xl mx-auto py-4">
      <div className="mb-6">
        <h1 className="text-xl font-semibold">Reports</h1>
        <p className="text-sm text-muted-foreground">Pick a report to generate.</p>
      </div>

      <div className="grid gap-3 sm:grid-cols-2">
        {OPTIONS.map((opt) => (
          <button
            key={opt.title}
            onClick={() => opt.to && navigate(opt.to)}
            disabled={opt.disabled}
            aria-disabled={opt.disabled}
            className={cn(
              "group text-left rounded-xl border bg-card p-5 transition-all",
              opt.disabled
                ? "opacity-60 cursor-not-allowed"
                : "hover:border-foreground/30 hover:shadow-sm focus-visible:outline-none focus-visible:ring-3 focus-visible:ring-ring/50",
            )}
          >
            <div className="flex items-start gap-3">
              <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-secondary text-secondary-foreground">
                {opt.icon}
              </div>
              <div className="flex-1 min-w-0">
                <div className="flex items-center justify-between gap-2">
                  <h2 className="text-base font-semibold">{opt.title}</h2>
                  {!opt.disabled && (
                    <ArrowRight className="h-4 w-4 shrink-0 text-muted-foreground transition-transform group-hover:translate-x-0.5" />
                  )}
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
