import type { ApiService } from "./service"
import { MockApiService } from "./mock_service"

// The single instance used by all components.
// When the Rust backend is ready, swap MockApiService for RealApiService:
//
//   import { RealApiService } from "./real_service"
//   export const api: ApiService = new RealApiService()
//
export const api: ApiService = new MockApiService()
