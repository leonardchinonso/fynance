/**
 * Map each transaction or investment preview row to the index of its entry
 * in the commit payload, or null when the user may not edit or remove it.
 *
 * Backend invariant (`build_multi_preview` in `document_parser.rs`): the
 * payload holds every non-error row, duplicates included, in `rows[]` order.
 * So a row's payload index is the number of non-error rows before it.
 * Counting only "new" rows put every row after a duplicate one entry off,
 * and remove or edit then acted on its neighbour.
 *
 * Only "new" rows are actionable. A duplicate stays in the payload unchanged:
 * commit numbers rows that share a key across the whole payload, so dropping
 * one would shift the numbers of the rows after it.
 *
 * Holdings follow a different rule (new and modify rows only); see
 * `holdings_section.tsx`.
 */
export function editablePayloadIndices(
  rows: readonly { status: string }[]
): (number | null)[] {
  let next = 0
  return rows.map((row) => {
    if (row.status === "error") return null
    const index = next++
    return row.status === "new" ? index : null
  })
}
