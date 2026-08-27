/**
 * Which currency each money field on a `CgtRealizedEvent` is actually denominated
 * in — the one decision the disposal schedule kept getting wrong.
 *
 * `original_currency` is SOURCE METADATA, not a display label. The backend converts
 * `proceeds`, `cost_basis` and `gain_loss` into the preferred base currency (GBP)
 * via `fx.convert_as_of` before serialising them, so formatting any of those with
 * `original_currency` renders a GBP amount behind a "$" for a USD-sourced disposal.
 * `disposal_price` is the sole exception: it is `price_per_share` straight off the
 * trade and is genuinely native.
 *
 * Returning `undefined` means "let `formatCurrency` use its GBP default".
 *
 * This lives in its own module rather than beside the table component so it can be
 * unit-tested without a DOM harness (and so the component file keeps exporting only
 * components, per `react-refresh/only-export-components`). The same class of bug has
 * now been found three times across three components, which is why the rule is
 * stated in one place and pinned by tests.
 */
export type DisposalMoneyField = "disposal_price" | "proceeds" | "cost_basis" | "gain_loss"

export function currencyForField(
  field: DisposalMoneyField,
  originalCurrency: string,
): string | undefined {
  return field === "disposal_price" ? originalCurrency : undefined
}
