import { useSyncExternalStore } from "react"
import { getRedacted, subscribeRedacted } from "@/lib/utils"

/**
 * Subscribes the component to the redacted (privacy) flag.
 *
 * `formatCurrency` reads the flag at call time from module scope, so any
 * component that formats money during render must call this hook (the return
 * value can be ignored) to re-render when the toggle flips. Without it the
 * component keeps showing the previously formatted string until something else
 * re-renders it.
 */
export function useRedactedFlag(): boolean {
  return useSyncExternalStore(subscribeRedacted, getRedacted)
}
