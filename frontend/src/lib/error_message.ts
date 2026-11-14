/**
 * Pull a readable string out of a fetch error. The api client throws
 * `Error("400 Bad Request: {\"code\":..., \"error\":...}")`; pluck the
 * `error` field out of the JSON body when present.
 */
export function extractErrorMessage(err: unknown): string {
  const raw = err instanceof Error ? err.message : String(err)
  const jsonStart = raw.indexOf("{")
  if (jsonStart !== -1) {
    try {
      const body = JSON.parse(raw.slice(jsonStart))
      if (body && typeof body.error === "string") return body.error
    } catch { /* fall through */ }
  }
  return raw
}
