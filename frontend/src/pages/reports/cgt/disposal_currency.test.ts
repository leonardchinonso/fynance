import { describe, it, expect } from "vitest"
import { currencyForField } from "./disposal_currency"
import { formatCurrency } from "@/lib/utils"

/**
 * `original_currency` on a `CgtRealizedEvent` is SOURCE metadata, not a display
 * label. The backend converts `proceeds`, `cost_basis` and `gain_loss` into the
 * preferred base currency (GBP) before serialising them; only `disposal_price`
 * is native to the trade.
 *
 * Rendering a base-currency amount with the source currency put GBP figures
 * behind a "$" on a filing-grade report. The same mistake has been found three
 * times across three components, so it is pinned here.
 */
describe("currencyForField", () => {
  it("labels disposal_price with the trade's native currency", () => {
    expect(currencyForField("disposal_price", "USD")).toBe("USD")
  })

  it.each(["proceeds", "cost_basis", "gain_loss"] as const)(
    "leaves %s to the base-currency default, never the source currency",
    (field) => {
      expect(currencyForField(field, "USD")).toBeUndefined()
    },
  )

  it("does not depend on the source currency happening to equal the base", () => {
    // A GBP-sourced holding cannot distinguish the two behaviours, which is why
    // the bug survived: every fixture was GBP. Use a currency that differs.
    expect(currencyForField("proceeds", "EUR")).toBeUndefined()
    expect(currencyForField("disposal_price", "EUR")).toBe("EUR")
  })
})

describe("rendered output for a USD-sourced disposal", () => {
  const originalCurrency = "USD"

  it("renders converted base-currency money with the GBP symbol, not '$'", () => {
    // These three are already GBP on the wire. A "$" prefix here is the defect.
    for (const field of ["proceeds", "cost_basis", "gain_loss"] as const) {
      const rendered = formatCurrency("3000.00", currencyForField(field, originalCurrency))
      expect(rendered).toBe("£3,000.00")
      expect(rendered).not.toContain("$")
    }
  })

  it("still renders the native disposal price in its own currency", () => {
    const rendered = formatCurrency(
      "30.00",
      currencyForField("disposal_price", originalCurrency),
    )
    expect(rendered).toBe("$30.00")
  })
})
