import type { CategoryType } from "@/bindings/CategoryType"

/**
 * User-facing grouping of the backend `CategoryType` enum. The taxable /
 * non-taxable split is kept on the backend (for tax reporting) but is an
 * implementation detail in the UI: filters, charts and the settings dropdown
 * present one option per group, and a group fetches/represents all its
 * underlying types.
 */
export interface TypeGroup {
  key: string
  label: string
  /** Underlying backend types this group covers. */
  types: CategoryType[]
  /** Type written when assigning this group to a category. */
  canonical: CategoryType
  /** Shared color for this group: settings tags and type-grouped charts. */
  color: string
}

const NEUTRAL = "#78716c"

export const CATEGORY_TYPE_GROUPS: TypeGroup[] = [
  { key: "spending", label: "Spending", types: ["spending"], canonical: "spending", color: "#3b82f6" },
  { key: "income", label: "Income", types: ["income_taxable", "income_non_taxable"], canonical: "income_taxable", color: "#22c55e" },
  { key: "interest", label: "Interest", types: ["interest_taxable", "interest_non_taxable"], canonical: "interest_taxable", color: "#14b8a6" },
  { key: "internal_transfer", label: "Internal transfer", types: ["internal_transfer"], canonical: "internal_transfer", color: NEUTRAL },
  { key: "donation", label: "Donation", types: ["donation_taxable", "donation_non_taxable"], canonical: "donation_taxable", color: "#a855f7" },
]

const GROUP_BY_TYPE: Record<CategoryType, TypeGroup> = Object.fromEntries(
  CATEGORY_TYPE_GROUPS.flatMap((g) => g.types.map((t) => [t, g])),
) as Record<CategoryType, TypeGroup>

/** Group a raw type belongs to (e.g. `income_non_taxable` -> the "income" group). */
export function groupForType(t: CategoryType): TypeGroup | undefined {
  return GROUP_BY_TYPE[t]
}

/** Grouped display label for a raw type (e.g. `income_taxable` -> "Income"). */
export function groupLabelForType(t: CategoryType | string): string {
  return GROUP_BY_TYPE[t as CategoryType]?.label ?? String(t)
}

/** Expand selected group keys to the underlying backend types (for API params). */
export function expandGroups(groupKeys: string[]): CategoryType[] {
  return CATEGORY_TYPE_GROUPS.filter((g) => groupKeys.includes(g.key)).flatMap((g) => g.types)
}

/** Which group keys are represented by a set of raw types (for the filter UI). */
export function groupsForTypes(types: string[]): string[] {
  return CATEGORY_TYPE_GROUPS.filter((g) => g.types.some((t) => types.includes(t))).map((g) => g.key)
}

/** Group color for a raw type (e.g. settings tag for `income_non_taxable`). */
export function colorForType(t: CategoryType | string): string {
  return GROUP_BY_TYPE[t as CategoryType]?.color ?? NEUTRAL
}

/** Group color by the grouped label charts render (e.g. "Income"). */
export function colorForGroupLabel(label: string): string {
  return CATEGORY_TYPE_GROUPS.find((g) => g.label === label)?.color ?? NEUTRAL
}
