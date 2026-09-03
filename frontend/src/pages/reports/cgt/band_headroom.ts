import type { PutTaxInputsPayload } from "@/bindings/PutTaxInputsPayload"

/**
 * Headroom written to `allowable_income_remaining` when the user picks "basic
 * rate" in the filter bar.
 *
 * Deliberately a large sentinel rather than the real basic-rate band: the
 * control expresses "all my gains fall at the basic rate", and this is the
 * arithmetic that produces that, since the server charges gains at the basic
 * rate up to this figure. The precise unused band is a number only the taxpayer
 * knows, and it can be entered exactly on the tax-inputs screen — this is the
 * shortcut for the common case, not a claim about anyone's income.
 */
export const BASIC_BAND_HEADROOM = "99999999"

/**
 * Which CGT rate the user expects their gains to fall at.
 *
 * This is the request's band selector: it is converted into the taxpayer's
 * `allowable_income_remaining` before the report runs. "higher" means no unused
 * basic-rate income band, so every gain is charged at the higher rate; "basic"
 * means enough headroom to cover the whole gain. Those are the two ends of a
 * spectrum the user can set precisely on the tax-inputs screen — this control
 * only offers the ends, because the filter bar is not where a taxpayer works out
 * their unused income band to the pound.
 *
 * `null` means the user never touched the control on this visit, which is a
 * third state and not a synonym for either end — see {@link taxInputsPayloadForBand}.
 */
export type CgtBandSelection = "basic" | "higher"

/**
 * Build the `PUT /tax-inputs` body that generating a report should send, or
 * `null` when it should send no request at all.
 *
 * Every field here is the user's own, set on the tax-inputs screen, and the
 * backend reads an absent/null key as "leave the stored value alone". So the
 * only thing generating a report may write is a figure the user expressed *on
 * this screen*, by moving the band selector.
 *
 * `band === null` — the selector was never touched — therefore yields no
 * request, rather than a body carrying `"0"`. The selector has a default
 * position on screen, but a default is not an instruction: a user who entered a
 * precise headroom on the tax-inputs screen and then generated a report without
 * touching the selector used to have that figure silently replaced with `"0"`,
 * over-stating the tax due on a filing-grade report. The two ends of the
 * control are only meaningful once somebody chooses one.
 *
 * Returning `null` rather than a body of three nulls keeps the whole decision
 * in one tested place: the caller branches on this value alone, so it has no
 * separate condition of its own that could drift from this one.
 */
export function taxInputsPayloadForBand(
  band: CgtBandSelection | null,
): PutTaxInputsPayload | null {
  if (band === null) return null
  return {
    allowable_income_remaining: band === "basic" ? BASIC_BAND_HEADROOM : "0",
    // Explicit nulls: the backend reads an absent/null key as "leave the
    // stored value alone", which is exactly what generating a report should do
    // to figures the user set on the tax-inputs screen.
    brought_forward_losses: null,
    aea_claimed: null,
  }
}
