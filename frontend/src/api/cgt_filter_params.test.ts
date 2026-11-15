import { describe, expect, it } from "vitest"
import { ukTaxYearsBetween } from "./cgt_filter_params"

describe("ukTaxYearsBetween", () => {
  it("lists every year from the earliest date to the current one, newest first", () => {
    expect(ukTaxYearsBetween("2022-05-01", new Date("2025-01-10T12:00:00"))).toEqual([
      "2024-25", "2023-24", "2022-23",
    ])
  })

  it("places 5 April in the earlier year and 6 April in the later one", () => {
    expect(ukTaxYearsBetween("2023-04-05", new Date("2023-06-01T12:00:00"))).toEqual(["2023-24", "2022-23"])
    expect(ukTaxYearsBetween("2023-04-06", new Date("2023-06-01T12:00:00"))).toEqual(["2023-24"])
  })

  it("returns just the current year when the earliest date is in the future", () => {
    expect(ukTaxYearsBetween("2030-01-01", new Date("2025-01-10T12:00:00"))).toEqual(["2024-25"])
  })
})
