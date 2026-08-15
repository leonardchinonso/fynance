import type { Profile } from "@/types"

// UTRs here are obviously-fake placeholder digits. Never put a real HMRC
// reference in fixtures — this repository is public.
export const MOCK_PROFILES: Profile[] = [
  { id: "alex", name: "Alex", utr: "1234567890" },
  { id: "sam", name: "Sam", utr: null },
]
