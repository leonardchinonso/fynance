import { describe, it, expect } from "vitest"
import { editablePayloadIndices } from "./payload_index"

interface Row {
  description: string
  status: "new" | "duplicate" | "error"
}

/**
 * The commit payload the backend builds for `rows`: every non-error row,
 * duplicates included, in order.
 */
function backendPayload(rows: Row[]): string[] {
  return rows.filter((r) => r.status !== "error").map((r) => r.description)
}

/** What `buildCommitPayloads` sends after the user removes `removed` rows. */
function committedAfterRemoving(rows: Row[], removed: string[]): string[] {
  const indices = editablePayloadIndices(rows)
  const marked = new Set<number>()
  rows.forEach((row, i) => {
    const index = indices[i]
    if (removed.includes(row.description) && index !== null) marked.add(index)
  })
  return backendPayload(rows).filter((_, i) => !marked.has(i))
}

describe("editablePayloadIndices", () => {
  it("counts duplicate rows, which the payload holds, but does not offer them", () => {
    const rows: Row[] = [
      { description: "Rent", status: "duplicate" },
      { description: "Coffee", status: "new" },
      { description: "Taxi", status: "new" },
    ]
    expect(editablePayloadIndices(rows)).toEqual([null, 1, 2])
  })

  it("skips error rows, which the payload leaves out", () => {
    const rows: Row[] = [
      { description: "Unreadable", status: "error" },
      { description: "Coffee", status: "new" },
      { description: "Rent", status: "duplicate" },
      { description: "Taxi", status: "new" },
    ]
    expect(editablePayloadIndices(rows)).toEqual([null, 0, null, 2])
  })

  it("points every editable row at its own payload entry", () => {
    const rows: Row[] = [
      { description: "Rent", status: "duplicate" },
      { description: "Unreadable", status: "error" },
      { description: "Coffee", status: "new" },
      { description: "TfL", status: "duplicate" },
      { description: "Taxi", status: "new" },
    ]
    const payload = backendPayload(rows)
    editablePayloadIndices(rows).forEach((index, i) => {
      if (index !== null) expect(payload[index]).toBe(rows[i]?.description)
    })
  })

  it("removing a row after a duplicate drops that row, not its neighbour", () => {
    const rows: Row[] = [
      { description: "Rent", status: "duplicate" },
      { description: "Coffee", status: "new" },
      { description: "Taxi", status: "new" },
    ]
    expect(committedAfterRemoving(rows, ["Taxi"])).toEqual(["Rent", "Coffee"])
  })
})
