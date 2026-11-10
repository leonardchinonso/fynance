import { api } from "@/api/client"
import type { EmploymentPay, TaxYearSummary } from "@/types/tax_v1"
import type { RemoteData } from "@/lib/remote_data"
import { useQuery } from "@/hooks/use_query"

/**
 * All employment-pay rows, unfiltered: the Tax page derives the tax-year
 * picker from the years present in this list and filters client-side.
 * Returns `[data, reload]`.
 */
export function useEmploymentPay(): [RemoteData<EmploymentPay[]>, () => void] {
  return useQuery(() => api.listEmploymentPay(), { tag: "employment-pay", hard: [], soft: [] })
}

/** The tax-year summary (readiness checklist, interest, gift aid).
 *  `taxYear` is a hard input: switching years shows a fresh loading state. */
export function useTaxSummary(taxYear: string): [RemoteData<TaxYearSummary>, () => void] {
  return useQuery(() => api.getTaxSummary(taxYear), {
    tag: "tax-summary",
    hard: [taxYear],
    soft: [],
  })
}
