import { describe, it, expect } from "vitest"
import type { PutTaxInputsPayload } from "@/bindings/PutTaxInputsPayload"
import {
  BASIC_BAND_HEADROOM,
  taxInputsPayloadForBand,
  type CgtBandSelection,
} from "./band_headroom"

/**
 * Stand-in for the stored `tax_inputs` row, applying the same rules as
 * generating a report does: a `null` payload means no request was sent at all,
 * and within a payload a `null` field leaves the stored value alone, matching
 * the read-modify-write in `PUT /api/tax-inputs/:profile/:year`.
 *
 * Modelled rather than mocked so the assertions are about the figure the user
 * would find on the tax-inputs screen afterwards, not about the shape of a
 * request body. A payload-shape assertion alone would still pass if the caller
 * sent a correct body at the wrong time.
 */
function storedAfterGenerate(
  stored: { allowable_income_remaining: string; brought_forward_losses: string },
  payload: PutTaxInputsPayload | null,
) {
  if (payload === null) return stored
  return {
    allowable_income_remaining:
      payload.allowable_income_remaining ?? stored.allowable_income_remaining,
    brought_forward_losses:
      payload.brought_forward_losses ?? stored.brought_forward_losses,
  }
}

describe("taxInputsPayloadForBand", () => {
  it("sends no request at all when the selector was never touched", () => {
    expect(taxInputsPayloadForBand(null)).toBeNull()
  })

  it("writes the sentinel headroom when the user picks the basic rate", () => {
    expect(taxInputsPayloadForBand("basic")?.allowable_income_remaining).toBe(
      BASIC_BAND_HEADROOM,
    )
  })

  it("writes zero headroom when the user picks the higher rate", () => {
    expect(taxInputsPayloadForBand("higher")?.allowable_income_remaining).toBe("0")
  })

  it.each(["basic", "higher"] as const)(
    "never disturbs losses or the AEA choice, band=%s",
    (band: CgtBandSelection) => {
      const payload = taxInputsPayloadForBand(band)
      expect(payload?.brought_forward_losses).toBeNull()
      expect(payload?.aea_claimed).toBeNull()
    },
  )
})

/**
 * The regression this file exists for.
 *
 * The band selector draws a default position on every page load. Generating a
 * report used to send that default as a real figure, so a taxpayer who had
 * entered a precise unused income band on the tax-inputs screen and then
 * generated a report — without ever touching the selector — had it silently
 * replaced with "0". That over-states the tax due on a filing-grade report, and
 * nothing on screen says a stored figure was overwritten.
 */
describe("generating a report does not overwrite a headroom set elsewhere", () => {
  // A precise figure of the kind only the taxpayer knows: neither "0" nor the
  // basic-rate sentinel, so it cannot survive by coinciding with either branch.
  const stored = {
    allowable_income_remaining: "17432.18",
    brought_forward_losses: "2500",
  }

  it("leaves the user's precise figure intact when the selector is untouched", () => {
    const after = storedAfterGenerate(stored, taxInputsPayloadForBand(null))
    expect(after.allowable_income_remaining).toBe("17432.18")
    expect(after.brought_forward_losses).toBe("2500")
  })

  it("still lets an explicit higher-rate choice zero the headroom", () => {
    // The fix must not go so far that the control stops working: choosing a
    // band deliberately is exactly when writing the figure is correct.
    const after = storedAfterGenerate(stored, taxInputsPayloadForBand("higher"))
    expect(after.allowable_income_remaining).toBe("0")
  })

  it("still lets an explicit basic-rate choice raise the headroom", () => {
    const after = storedAfterGenerate(stored, taxInputsPayloadForBand("basic"))
    expect(after.allowable_income_remaining).toBe(BASIC_BAND_HEADROOM)
  })

  it("leaves brought-forward losses alone even on an explicit band choice", () => {
    const after = storedAfterGenerate(stored, taxInputsPayloadForBand("higher"))
    expect(after.brought_forward_losses).toBe("2500")
  })
})
