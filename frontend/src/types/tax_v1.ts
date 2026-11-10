// Hand-written mirrors of the tax reporting V1 response types
// (docs/plans/28_tax_reporting_v1.md). DELETE THIS FILE once ts-rs generated
// bindings/ include these types; the plan's "ts-rs: derive TS for all new
// response types" step replaces them.
//
// Money renders as decimal strings, same convention as every other endpoint.

/** One employment-pay row (table `employment_pay`). NULLs arrive as null/omitted. */
export interface EmploymentPay {
  id: string
  /** "YYYY-YY" (e.g. "2025-26"), the UK 6 Apr .. 5 Apr window. */
  tax_year: string
  period_start: string | null
  period_end: string | null
  employer: string
  /** Annual summary row (P60) rather than a payslip period. */
  is_p60: boolean
  gross_pay: string
  tax_deducted: string
  /** null = unknown, deliberately distinct from 0. */
  employees_ni: string | null
  student_loan: string | null
  net_pay: string | null
  currency: string
  transaction_id: string | null
  document_ids: string[]
  notes: string | null
  created_at: string
  updated_at: string
}

/** Body of POST /api/employment-pay and PATCH /api/employment-pay/:id. */
export interface EmploymentPayPayload {
  tax_year: string
  period_start: string | null
  period_end: string | null
  employer: string
  is_p60: boolean
  gross_pay: string
  tax_deducted: string
  employees_ni: string | null
  student_loan: string | null
  net_pay: string | null
  currency: string
  transaction_id: string | null
  document_ids: string[]
  notes: string | null
}

/**
 * Sums of the employment-pay rows in the tax year. NULL legs sum as 0;
 * `has_unknown` is set when any summed leg had unknown (NULL) values.
 */
export interface EmploymentSummary {
  gross_pay: string
  tax_deducted: string
  employees_ni: string
  student_loan: string
  net_pay: string
  payslip_count: number
  has_p60: boolean
  has_unknown?: boolean
}

/** Interest totals for one account with interest in the tax year. */
export interface InterestAccountTotal {
  account_id: string
  account_name: string
  taxable: string
  non_taxable: string
}

/** Interest legs converted to the preferred currency. Only accounts with
 *  interest in the range appear in `by_account`. */
export interface InterestSummary {
  taxable_total: string
  non_taxable_total: string
  by_account: InterestAccountTotal[]
}

/** `donation_taxable` / `donation_non_taxable` outflows in the tax year. */
export interface DonationSummary {
  gift_aid_total: string
  non_gift_aid_total: string
}

/** Response of GET /api/tax/summary/:tax_year. */
export interface TaxYearSummary {
  tax_year: string
  start_date: string
  end_date: string
  employment: EmploymentSummary
  interest: InterestSummary
  donations: DonationSummary
}
