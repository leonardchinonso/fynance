import type { ApiService } from "./service"
import { MockApiService } from "./mock_service"
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

const mockService = new MockApiService()
const realService = new RealApiService()

// Reactive API instance that delegates to the current mode's service.
// Components import `api` and call methods as before. The toggle
// switches which implementation handles the call.
let currentMode: ApiMode = getStoredMode()

// A write whose name starts with one of these mutates server state, so the
// request-keyed cache must be invalidated once it commits. Reads (`get*`/`list*`)
// and side-effect-free calls (`parseDocuments`, `exportData`) are left untouched.
// When adding an ApiService mutation whose name doesn't match, extend this list —
// a miss means stale data everywhere after the write (this bit `bulk*` once).
const MUTATION_PREFIX = /^(set|create|update|delete|patch|commit|upload|import|bulk)/
// Categories, currencies and profiles ripple into every money figure and label,
// so their writes invalidate static reference data too — not just volatile queries.
const RIPPLE_ALL = /categor|currenc|profile/i

function afterMutation(method: string): void {
  if (RIPPLE_ALL.test(method)) invalidateAll()
  else invalidateVolatile()
}

const handler: ProxyHandler<ApiService> = {
  get(_target, prop, receiver) {
    const service = currentMode === "live" ? realService : mockService
    const value = Reflect.get(service, prop, receiver)
    if (typeof value !== "function") return value

    const fn = value.bind(service)
    if (typeof prop !== "string" || !MUTATION_PREFIX.test(prop)) return fn

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
  },
}

export const api: ApiService = new Proxy(mockService, handler)

export function getApiMode(): ApiMode {
  return currentMode
}

export function setApiMode(mode: ApiMode) {
  currentMode = mode
  localStorage.setItem(STORAGE_KEY, mode)
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
