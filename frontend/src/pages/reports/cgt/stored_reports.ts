import type { CapitalGainsResponse } from "@/bindings/CapitalGainsResponse"
import type { CgtFilters } from "@/api/service"

/**
 * A generated CGT report kept in localStorage so the user can revisit it via a
 * dedicated URL without re-running the engine. Capped at MAX_REPORTS entries,
 * FIFO eviction.
 */
export interface StoredCgtReport {
  id: string
  generatedAt: string
  filters: CgtFilters
  /**
   * Higher/additional vs basic CGT rate band chosen at generation. Frontend-only:
   * the tax estimate is computed client-side, so this is not part of the backend
   * contract (`CgtFilters`). Optional for reports stored before this field
   * existed — treat `undefined` as `true`.
   */
  higherRate?: boolean
  response: CapitalGainsResponse
}

const STORAGE_KEY = "fynance-cgt-reports"
const MAX_REPORTS = 20

function readAll(): StoredCgtReport[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return []
    const parsed = JSON.parse(raw)
    return Array.isArray(parsed) ? (parsed as StoredCgtReport[]) : []
  } catch {
    return []
  }
}

function writeAll(reports: StoredCgtReport[]): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(reports.slice(0, MAX_REPORTS)))
  } catch {
    // Quota errors and private mode failures are non-fatal — the report is
    // still visible in-memory for this session.
  }
}

export function listStoredReports(): StoredCgtReport[] {
  return readAll()
}

export function getStoredReport(id: string): StoredCgtReport | undefined {
  return readAll().find((r) => r.id === id)
}

export function saveStoredReport(report: StoredCgtReport): void {
  const all = readAll().filter((r) => r.id !== report.id)
  writeAll([report, ...all])
}

export function deleteStoredReport(id: string): void {
  writeAll(readAll().filter((r) => r.id !== id))
}

export function newReportId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID()
  }
  return `cgt_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 10)}`
}
