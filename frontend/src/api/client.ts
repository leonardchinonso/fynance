import type { ApiService } from "./service"
import { RealApiService } from "./real_service"
import { invalidateAll, invalidateVolatile, clearCache } from "@/lib/query_cache"

const STORAGE_KEY = "fynance-api-mode"

export type ApiMode = "mock" | "live"

/** When true, the app is locked to mock mode (set via VITE_MOCK_ONLY env var). */
export const MOCK_ONLY = !!import.meta.env.VITE_MOCK_ONLY

function getStoredMode(): ApiMode {
  if (MOCK_ONLY) return "mock"
  const stored = localStorage.getItem(STORAGE_KEY)
  return stored === "mock" ? "mock" : "live"
}

const realService = new RealApiService()

// The mock service (and the src/data fixtures it pulls in) is loaded lazily so
// live-mode users never download it. The promise doubles as the singleton
// guard; a failed chunk load resets it so the next call retries.
let mockServicePromise: Promise<ApiService> | null = null

function loadMock(): Promise<ApiService> {
  if (!mockServicePromise) {
    mockServicePromise = import("./mock_service").then((m) => new m.MockApiService())
    mockServicePromise.catch(() => { mockServicePromise = null })
  }
  return mockServicePromise
}

// Reactive API instance that delegates to the current mode's service.
// Components import `api` and call methods as before. The toggle
// switches which implementation handles the call.
let currentMode: ApiMode = getStoredMode()

// Warm the mock chunk when the session starts in mock mode so the first api
// call doesn't pay the dynamic-import latency.
if (currentMode === "mock") void loadMock()

// A write whose name starts with one of these mutates server state, so the
// request-keyed cache must be invalidated once it commits. Reads (`get*`/`list*`)
// and side-effect-free calls (`exportData`) are left untouched. `parse` is
// included because POST /api/parse stores uploaded source documents even though
// nothing is committed yet, and the Documents page must see them.
// When adding an ApiService mutation whose name doesn't match, extend this list:
// a miss means stale data everywhere after the write (this bit `bulk*` once).
const MUTATION_PREFIX = /^(set|create|update|delete|patch|commit|upload|import|bulk|parse)/
// Categories, currencies and profiles ripple into every money figure and label,
// so their writes invalidate static reference data too — not just volatile queries.
const RIPPLE_ALL = /categor|currenc|profile/i

function afterMutation(method: string): void {
  if (RIPPLE_ALL.test(method)) invalidateAll()
  else invalidateVolatile()
}

function wrapMutation(prop: string, fn: (...args: unknown[]) => unknown) {
  if (!MUTATION_PREFIX.test(prop)) return fn
  return (...args: unknown[]) => {
    const result = fn(...args)
    if (result && typeof (result as Promise<unknown>).then === "function") {
      return (result as Promise<unknown>).then((v) => {
        afterMutation(prop)
        return v
      })
    }
    return result
  }
}

const handler: ProxyHandler<ApiService> = {
  get(_target, prop, receiver) {
    if (currentMode === "live") {
      const value = Reflect.get(realService, prop, receiver)
      if (typeof value !== "function") return value
      const fn = value.bind(realService)
      if (typeof prop !== "string") return fn
      return wrapMutation(prop, fn)
    }

    // Not thenable: every string prop below yields a function, and a `then`
    // that isn't a real method would make `await api` misbehave.
    if (typeof prop !== "string" || prop === "then") return undefined
    // The one synchronous ApiService method; inlined (matching
    // MockApiService.documentDownloadUrl) so callers don't need the mock
    // chunk loaded to build an href.
    if (prop === "documentDownloadUrl") {
      return (id: string) => `/api/documents/${encodeURIComponent(id)}/download`
    }
    const fn = (...args: unknown[]) =>
      loadMock().then((svc) =>
        (Reflect.get(svc, prop) as (...a: unknown[]) => unknown).apply(svc, args),
      )
    return wrapMutation(prop, fn)
  },
}

export const api: ApiService = new Proxy({} as ApiService, handler)

export function getApiMode(): ApiMode {
  return currentMode
}

export function setApiMode(mode: ApiMode) {
  currentMode = mode
  localStorage.setItem(STORAGE_KEY, mode)
  if (mode === "mock") void loadMock()
  // Mock and live return different data for the same request shape; drop
  // everything so cached entries don't bleed across the switch.
  clearCache()
}

export const AUTH_TOKEN_KEY = "fynance-auth-token"

export function getAuthToken(): string | null {
  try { return localStorage.getItem(AUTH_TOKEN_KEY) || null } catch { return null }
}

export function setAuthToken(token: string | null) {
  try {
    if (token) localStorage.setItem(AUTH_TOKEN_KEY, token)
    else localStorage.removeItem(AUTH_TOKEN_KEY)
  } catch {}
}
