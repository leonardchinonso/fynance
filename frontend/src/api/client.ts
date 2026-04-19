import type { ApiService } from "./service"
import { MockApiService } from "./mock_service"
import { RealApiService } from "./real_service"

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

const handler: ProxyHandler<ApiService> = {
  get(_target, prop, receiver) {
    const service = currentMode === "live" ? realService : mockService
    const value = Reflect.get(service, prop, receiver)
    if (typeof value === "function") {
      return value.bind(service)
    }
    return value
  },
}

export const api: ApiService = new Proxy(mockService, handler)

export function getApiMode(): ApiMode {
  return currentMode
}

export function setApiMode(mode: ApiMode) {
  currentMode = mode
  localStorage.setItem(STORAGE_KEY, mode)
}
