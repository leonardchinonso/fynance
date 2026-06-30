// AUTO-GENERATED from backend CategoryType (cargo test). Do not edit.
import type { CategoryType } from "./CategoryType"

export const ALL_CATEGORY_TYPES: readonly CategoryType[] = ["spending", "income_taxable", "income_non_taxable", "interest_taxable", "interest_non_taxable", "internal_transfer", "donation_taxable", "donation_non_taxable"]
export const INCOME_TYPES: readonly CategoryType[] = ["income_taxable", "income_non_taxable"]
export const SPENDING_TYPES: readonly CategoryType[] = ["spending", "donation_taxable", "donation_non_taxable"]
export const CHART_EXCLUDED_TYPES: readonly CategoryType[] = ["internal_transfer"]

export const CATEGORY_TYPE_LABELS: Record<CategoryType, string> = {
  spending: "Spending",
  income_taxable: "Income (taxable)",
  income_non_taxable: "Income (non-taxable)",
  interest_taxable: "Interest (taxable)",
  interest_non_taxable: "Interest (non-taxable)",
  internal_transfer: "Internal transfer",
  donation_taxable: "Donation (taxable)",
  donation_non_taxable: "Donation (non-taxable)",
}
