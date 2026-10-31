import { describe, expect, it } from "vitest"
import type { CashSummaryResponse, HoldingsHistoryRow } from "@/types"
import { cashSummaryNetSavings, openingNetWorth } from "./overview_compat"

const row = (total_wealth: string): HoldingsHistoryRow =>
  ({ month: "2025-01", total_wealth }) as unknown as HoldingsHistoryRow

describe("openingNetWorth", () => {
  it("uses period_start_wealth when the backend sends it", () => {
    expect(openingNetWorth("150", [row("200")])).toBe("150")
  })

  it("falls back to the first bucket total for old backends without the field", () => {
    expect(openingNetWorth(undefined, [row("200"), row("300")])).toBe("200")
  })

  it("has no opening for an empty history", () => {
    expect(openingNetWorth(undefined, [])).toBeUndefined()
  })
})

describe("cashSummaryNetSavings", () => {
  it("uses net_savings when the backend sends it", () => {
    expect(cashSummaryNetSavings({ net_savings: "1750" } as CashSummaryResponse)).toBe("1750")
  })

  it("falls back to savings_growth for old backends without the rename", () => {
    const wire = { savings_growth: "42.50" } as unknown as CashSummaryResponse
    expect(cashSummaryNetSavings(wire)).toBe("42.50")
  })

  it("defaults to zero when the field is missing entirely", () => {
    expect(cashSummaryNetSavings({} as unknown as CashSummaryResponse)).toBe("0")
  })
})
