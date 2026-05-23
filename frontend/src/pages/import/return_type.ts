import type { AccountType } from "@/bindings/AccountType"
import type { ParseHints } from "@/bindings/ParseHints"

const INVESTMENT_TYPES: ReadonlySet<AccountType> = new Set<AccountType>([
  "investment",
  "investment_isa",
  "pension",
])

export function defaultHintsForAccount(accountType: AccountType): ParseHints {
  const isInvestment = INVESTMENT_TYPES.has(accountType)
  return {
    return_type: {
      transactions: !isInvestment,
      holdings: { enabled: true, period: null },
      investments: isInvestment,
    },
    experimental: null,
    hint: null,
  }
}
