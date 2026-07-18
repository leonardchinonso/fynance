import { describe, it, expect, beforeEach } from "vitest"
import { fetchQuery, getEntry, clearCache } from "./query_cache"

/** A promise whose resolution is controlled by the test, to force ordering. */
function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (err: unknown) => void
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej })
  return { promise, resolve, reject }
}

const OPTS = { staleTime: 60_000, isStatic: false }

function value(key: string): unknown {
  const status = getEntry(key)?.status
  return status?.kind === "success" ? status.value : undefined
}

beforeEach(() => clearCache())

describe("query cache — superseded requests", () => {
  it("discards a lagging older request so it cannot clobber a newer result", async () => {
    const key = "accounts"
    const older = deferred<string>()
    const newer = deferred<string>()

    // An in-flight request, then a force-refresh (e.g. after a token change)
    // that supersedes it.
    const pOlder = fetchQuery(key, () => older.promise, { ...OPTS, force: true })
    const pNewer = fetchQuery(key, () => newer.promise, { ...OPTS, force: true })

    // The newer request resolves first and commits.
    newer.resolve("new-token-data")
    await pNewer
    expect(value(key)).toBe("new-token-data")

    // The older request resolves afterwards; it must be discarded, not committed.
    older.resolve("old-token-data")
    await pOlder
    expect(value(key)).toBe("new-token-data")
  })

  it("does not let a lagging older rejection overwrite a newer success", async () => {
    const key = "profiles"
    const older = deferred<string>()
    const newer = deferred<string>()

    const pOlder = fetchQuery(key, () => older.promise, { ...OPTS, force: true })
    const pNewer = fetchQuery(key, () => newer.promise, { ...OPTS, force: true })

    newer.resolve("fresh")
    await pNewer

    // The stale request fails (e.g. an old 401) after the new one already succeeded.
    older.reject(new Error("401 Unauthorized"))
    await pOlder.catch(() => {})

    expect(getEntry(key)?.status.kind).toBe("success")
    expect(value(key)).toBe("fresh")
  })
})

describe("query cache — basics", () => {
  it("commits a successful fetch", async () => {
    const key = "currencies"
    await fetchQuery(key, () => Promise.resolve("gbp"), OPTS)
    expect(value(key)).toBe("gbp")
  })

  it("dedupes concurrent non-force callers into a single fetch", async () => {
    const key = "categories"
    let calls = 0
    const d = deferred<string>()
    const fetcher = () => { calls++; return d.promise }

    const p1 = fetchQuery(key, fetcher, OPTS)
    const p2 = fetchQuery(key, fetcher, OPTS)

    expect(p1).toBe(p2)
    expect(calls).toBe(1)

    d.resolve("ok")
    await p1
  })

  it("serves a fresh cached value without refetching", async () => {
    const key = "sections"
    let calls = 0
    await fetchQuery(key, () => { calls++; return Promise.resolve("a") }, OPTS)
    await fetchQuery(key, () => { calls++; return Promise.resolve("b") }, OPTS)

    expect(calls).toBe(1)
    expect(value(key)).toBe("a")
  })

  it("force refetches even when a fresh value is cached", async () => {
    const key = "holdings"
    await fetchQuery(key, () => Promise.resolve("stale"), OPTS)
    await fetchQuery(key, () => Promise.resolve("fresh"), { ...OPTS, force: true })
    expect(value(key)).toBe("fresh")
  })
})
