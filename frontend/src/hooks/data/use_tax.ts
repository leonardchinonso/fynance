import { api } from "@/api/client"
import type { IncomeCheck } from "@/bindings/IncomeCheck"
import type { IncomeRecord } from "@/bindings/IncomeRecord"
import type { IncomeSource } from "@/bindings/IncomeSource"
import type { LineTypeInfo } from "@/bindings/LineTypeInfo"
import type { TaxYearSummary } from "@/bindings/TaxYearSummary"
import type { VestCandidate } from "@/bindings/VestCandidate"
import type { RemoteData } from "@/lib/remote_data"
import { useQuery } from "@/hooks/use_query"

/** The line-type catalogue the tables build their columns from. Session-stable. */
export function useIncomeLineTypes(): [RemoteData<LineTypeInfo[]>, () => void] {
  return useQuery(() => api.listIncomeLineTypes(), {
    tag: "income-line-types", hard: [], soft: [], static: true,
  })
}

export function useIncomeSources(profileId: string): [RemoteData<IncomeSource[]>, () => void] {
  return useQuery(() => api.listIncomeSources(profileId), {
    tag: "income-sources", hard: [profileId], soft: [],
  })
}

/**
 * Income records. With no `profileId` this lists every profile's records:
 * the navbar's tab-visibility check uses that, gated by `enabled`.
 * The Tax page filters by year client-side to derive its year picker.
 */
export function useIncomeRecords(
  { profileId, enabled = true }: { profileId?: string; enabled?: boolean } = {},
): [RemoteData<IncomeRecord[]>, () => void] {
  return useQuery(() => api.listIncomeRecords({ profileId }), {
    tag: "income-records", hard: [profileId], soft: [], enabled,
  })
}

export function useIncomeChecks(
  profileId: string, taxYear: string,
): [RemoteData<IncomeCheck[]>, () => void] {
  return useQuery(() => api.listIncomeChecks({ profileId, taxYear }), {
    tag: "income-checks", hard: [profileId, taxYear], soft: [],
  })
}

/** One profile's tax-year summary. Both inputs are hard: switching shows a fresh loading state. */
export function useTaxSummary(
  taxYear: string, profileId: string,
): [RemoteData<TaxYearSummary>, () => void] {
  return useQuery(() => api.getTaxSummary(taxYear, profileId), {
    tag: "tax-summary", hard: [taxYear, profileId], soft: [],
  })
}

/** `vest` events near a record's RSU line. */
export function useVestCandidates(recordId: string): [RemoteData<VestCandidate[]>, () => void] {
  return useQuery(() => api.listVestCandidates(recordId), {
    tag: "vest-candidates", hard: [recordId], soft: [],
  })
}
