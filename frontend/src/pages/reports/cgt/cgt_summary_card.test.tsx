// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest"
import { afterEach, describe, it, expect } from "vitest"
import { cleanup, render, screen } from "@testing-library/react"
import type { CgtSummary } from "@/bindings/CgtSummary"
import type { TaxComputation } from "@/bindings/TaxComputation"
import type { TaxInputs } from "@/bindings/TaxInputs"
import { CgtSummaryCard } from "./cgt_summary_card"

/**
 * Covers the gap flagged by the reviewer of `70773f14`: the mock API fixture
 * cannot model income headroom, so it can never produce the four-band 2024-25
 * case — a taxpayer straddling both the basic/higher-rate boundary AND the 30
 * October 2024 rate change. That is the pathological maximum per
 * `TaxBandResult`'s doc comment, and the exact scenario the on-screen rate
 * rows exist to make visible, so it is the one case nobody had rendered.
 *
 * Per the item's route 2: the card is presentation-only (the computation is
 * server-side and separately tested), so a hand-built four-band
 * `TaxComputation` prop is a legitimate input to the thing under test, not a
 * fake. The fixture itself is deliberately left alone.
 *
 * Figures are invented round numbers, not real tax data (repo is public).
 * Rates (10%/20% pre-30 Oct 2024, 18%/24% post) are the actual statutory CGT
 * rates for 2024-25 — public tax law, not personal information.
 */

const summary: CgtSummary = {
  total_proceeds: "120000.00",
  total_allowable_costs: "60000.00",
  total_gains: "60000.00",
  total_losses: "0.00",
  net_gain_loss: "60000.00",
  base_currency: "GBP",
}

const taxInputs: TaxInputs = {
  profile_id: "test-profile",
  tax_year: "2024-25",
  brought_forward_losses: "0",
  allowable_income_remaining: "10000",
  aea_claimed: true,
  updated_at: null,
}

/**
 * The four-band 2024-25 case: two periods (pre/post 30 Oct 2024 rate change),
 * each split basic/higher by income headroom, per `TaxBandResult`'s doc
 * comment ("2024-25 produces up to four [bands], because the Autumn Budget
 * 2024 changed the rates mid-year").
 */
const fourBandTax: TaxComputation = {
  tax_year: "2024-25",
  bands: [
    {
      valid_from: "2024-04-06",
      valid_to: "2024-10-29",
      rate_kind: "basic",
      rate: "0.10",
      gains: "5000.00",
      deductions: "0.00",
      taxable: "5000.00",
      tax: "500.00",
    },
    {
      valid_from: "2024-04-06",
      valid_to: "2024-10-29",
      rate_kind: "higher",
      rate: "0.20",
      gains: "10000.00",
      deductions: "0.00",
      taxable: "10000.00",
      tax: "2000.00",
    },
    {
      valid_from: "2024-10-30",
      valid_to: "2025-04-05",
      rate_kind: "basic",
      rate: "0.18",
      gains: "8000.00",
      deductions: "0.00",
      taxable: "8000.00",
      tax: "1440.00",
    },
    {
      valid_from: "2024-10-30",
      valid_to: "2025-04-05",
      rate_kind: "higher",
      rate: "0.24",
      gains: "37000.00",
      deductions: "3000.00",
      taxable: "34000.00",
      tax: "8160.00",
    },
  ],
  total_gains: "60000.00",
  current_year_losses_applied: "0.00",
  brought_forward_losses_applied: "0.00",
  brought_forward_losses_remaining: "0.00",
  aea_applied: "3000.00",
  taxable_gain: "57000.00",
  tax_due: "12100.00",
  inputs: taxInputs,
}

describe("CgtSummaryCard, four-band 2024-25 case", () => {
  afterEach(() => cleanup())

  it("renders all four band rows with their rate and date-range label", () => {
    render(<CgtSummaryCard summary={summary} disposalCount={3} tax={fourBandTax} />)

    // Both periods need the date prefix (multipleBandPeriods is true), and
    // both basic/higher rate_kinds within each period need distinguishing —
    // otherwise two same-period rows read as a duplicate or a date error
    // rather than the basic/higher split they represent.
    expect(
      screen.getByText("Gains from 6 Apr 2024, basic rate @ 10%", { selector: "span" }),
    ).toBeInTheDocument()
    expect(
      screen.getByText("Gains from 6 Apr 2024, higher rate @ 20%", { selector: "span" }),
    ).toBeInTheDocument()
    expect(
      screen.getByText("Gains from 30 Oct 2024, basic rate @ 18%", { selector: "span" }),
    ).toBeInTheDocument()
    expect(
      screen.getByText("Gains from 30 Oct 2024, higher rate @ 24%", { selector: "span" }),
    ).toBeInTheDocument()
  })

  it("renders each band's tax figure, not just its label", () => {
    render(<CgtSummaryCard summary={summary} disposalCount={3} tax={fourBandTax} />)

    expect(screen.getByText("£500.00", { selector: "span" })).toBeInTheDocument()
    expect(screen.getByText("£2,000.00", { selector: "span" })).toBeInTheDocument()
    expect(screen.getByText("£1,440.00", { selector: "span" })).toBeInTheDocument()
    expect(screen.getByText("£8,160.00", { selector: "span" })).toBeInTheDocument()
  })

  it("does not clip a four-band label at phone width (~390px)", () => {
    // jsdom does not lay out text, so this cannot observe a visual wrap the
    // way a real viewport would. What it CAN pin: the label carries no
    // truncation styling (no `truncate`/`overflow-hidden`/`whitespace-nowrap`)
    // that would force a single-line clip regardless of width, and the row is
    // a flex container that allows its label span to wrap. That is the
    // structural precondition for "wraps instead of clipping" — the same
    // structure already verified by eye for the two-band case.
    render(<CgtSummaryCard summary={summary} disposalCount={3} tax={fourBandTax} />)

    const longestLabel = screen.getByText("Gains from 30 Oct 2024, higher rate @ 24%", {
      selector: "span",
    })
    expect(longestLabel.className).not.toMatch(/truncate|overflow-hidden|whitespace-nowrap/)

    const row = longestLabel.closest("div")
    expect(row?.className).toContain("flex")
    expect(row?.className).not.toMatch(/whitespace-nowrap/)
  })

  it("takes the card to thirteen rows total, one more than the two-band case", () => {
    // Nine rows shipped with no bands; two-band adds two row-equivalents
    // (verified in the merge review) taking it to eleven. Four-band bands
    // themselves are 4 rows, i.e. +2 versus the two-band case's 2 rows, so
    // the total is 13. Pinning the row count here is what would catch a
    // regression if density work later tries to collapse or hide rows.
    const { container } = render(
      <CgtSummaryCard summary={summary} disposalCount={3} tax={fourBandTax} />,
    )
    // Each Row renders one top-level flex row; Separator and the plain <p>
    // caption are not Rows. Count by the Row's characteristic classes
    // (items-baseline + justify-between) rather than a brittle DOM index.
    const rows = container.querySelectorAll(".items-baseline.justify-between")
    expect(rows.length).toBe(13)
  })
})
