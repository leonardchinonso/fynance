/**
 * Stable, order-independent serialization of query inputs into a string key.
 *
 * The same request shape must always produce the same string regardless of:
 *  - object key insertion order (`{a,b}` === `{b,a}`)
 *  - element order of a primitive array (filter sets like selected account ids,
 *    excluded category ids — order carries no meaning, so `["a","b"]` === `["b","a"]`)
 *
 * `undefined` collapses to `null` so an omitted optional and an explicit `null`
 * key the same entry (e.g. `profileId: undefined` vs a missing field).
 *
 * Arrays whose elements are objects keep their order (order may be meaningful
 * there); only all-primitive arrays are sorted.
 */
export function stableKey(value: unknown): string {
  return JSON.stringify(normalize(value))
}

function normalize(value: unknown): unknown {
  if (value === undefined || value === null) return null
  if (typeof value !== "object") return value

  if (Array.isArray(value)) {
    const items = value.map(normalize)
    const allPrimitive = items.every((i) => i === null || typeof i !== "object")
    if (allPrimitive) {
      return [...items].sort((a, b) => String(a).localeCompare(String(b)))
    }
    return items
  }

  const obj = value as Record<string, unknown>
  const out: Record<string, unknown> = {}
  for (const key of Object.keys(obj).sort()) {
    out[key] = normalize(obj[key])
  }
  return out
}
