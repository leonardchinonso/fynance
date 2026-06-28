import { useCallback, useEffect, useState } from "react"
import type { IngestionPreview } from "@/bindings/IngestionPreview"
import type { ImportPayload } from "@/bindings/ImportPayload"
import type { HoldingsImportPayload } from "@/bindings/HoldingsImportPayload"
import type { InvestmentsImportPayload } from "@/bindings/InvestmentsImportPayload"

const STORAGE_KEY = "fynance-recent-imports"
const MAX_ENTRIES = 50

export interface PreviewEdits {
  txPayload: ImportPayload | null
  holdingsPayload: HoldingsImportPayload | null
  invPayload: InvestmentsImportPayload | null
  /** Indices into the corresponding payload array (Sets aren't JSON-serializable). */
  txDeleted: number[]
  holdingsDeleted: number[]
  invDeleted: number[]
}

export interface RecentImportEntry {
  id: string
  /** ms since epoch */
  timestamp: number
  accountId: string
  fileNames: string[]
  preview: IngestionPreview
  edits: PreviewEdits
}

// ts-rs maps Rust u64 to bigint; over HTTP this lands as a number, but the
// mock returns BigInt directly. JSON.stringify can't serialize BigInts, so
// stringify them as numbers (safe range) for storage.
function bigintReplacer(_key: string, value: unknown): unknown {
  return typeof value === "bigint" ? Number(value) : value
}

function readStorage(): RecentImportEntry[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return []
    const parsed = JSON.parse(raw)
    return Array.isArray(parsed) ? (parsed as RecentImportEntry[]) : []
  } catch {
    return []
  }
}

function writeStorage(entries: RecentImportEntry[]) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(entries, bigintReplacer))
  } catch {
    // Quota exceeded or storage disabled — silently drop. The cache is a
    // convenience; losing it should not block the user.
  }
}

function defaultEditsFor(preview: IngestionPreview): PreviewEdits {
  return {
    txPayload: preview.transactions.payload,
    holdingsPayload: preview.holdings.payload,
    invPayload: preview.investments.payload,
    txDeleted: [],
    holdingsDeleted: [],
    invDeleted: [],
  }
}

/**
 * LRU cache of in-progress parse responses backed by `localStorage`.
 *
 * Newest entries are at index 0. Capacity is `MAX_ENTRIES`; older entries are
 * dropped from the tail on insert. Entries persist across reloads so an
 * accidental refresh during review doesn't lose the user's edits.
 *
 * Cross-tab sync via the `storage` event keeps multiple open tabs consistent.
 */
export function useRecentImports() {
  const [entries, setEntries] = useState<RecentImportEntry[]>(readStorage)

  useEffect(() => {
    const handler = (e: StorageEvent) => {
      if (e.key !== STORAGE_KEY) return
      setEntries(readStorage())
    }
    window.addEventListener("storage", handler)
    return () => window.removeEventListener("storage", handler)
  }, [])

  const persist = useCallback((next: RecentImportEntry[]) => {
    setEntries(next)
    writeStorage(next)
  }, [])

  const add = useCallback(
    (input: { accountId: string; fileNames: string[]; preview: IngestionPreview }): string => {
      const id = `imp_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`
      const entry: RecentImportEntry = {
        id,
        timestamp: Date.now(),
        accountId: input.accountId,
        fileNames: input.fileNames,
        preview: input.preview,
        edits: defaultEditsFor(input.preview),
      }
      const next = [entry, ...readStorage()].slice(0, MAX_ENTRIES)
      persist(next)
      return id
    },
    [persist]
  )

  const updateEdits = useCallback(
    (id: string, edits: PreviewEdits) => {
      const current = readStorage()
      const idx = current.findIndex((e) => e.id === id)
      if (idx === -1) return
      const next = current.slice()
      next[idx] = { ...current[idx], edits }
      persist(next)
    },
    [persist]
  )

  const remove = useCallback(
    (id: string) => {
      persist(readStorage().filter((e) => e.id !== id))
    },
    [persist]
  )

  const getById = useCallback((id: string): RecentImportEntry | null => {
    return readStorage().find((e) => e.id === id) ?? null
  }, [])

  return { entries, add, updateEdits, remove, getById }
}
