import { createContext, useContext, useCallback } from "react"
import { getRedacted, setRedacted } from "@/lib/utils"
import { useRedactedFlag } from "@/hooks/use_redacted_flag"

interface RedactedContextValue {
  redacted: boolean
  toggleRedacted: () => void
}

const RedactedContext = createContext<RedactedContextValue | null>(null)

/**
 * Owns the privacy ("redacted amounts") toggle.
 *
 * The flag lives at module scope in lib/utils (formatCurrency reads it at call
 * time) with a subscriber list. Components that format money during render
 * subscribe via useRedactedFlag(), so toggling re-renders exactly those
 * components in place. There is no remount, so open dialogs, selections, and
 * unsaved edits survive the toggle.
 */
export function RedactedProvider({ children }: { children: React.ReactNode }) {
  const redacted = useRedactedFlag()

  const toggleRedacted = useCallback(() => {
    setRedacted(!getRedacted())
  }, [])

  return (
    <RedactedContext.Provider value={{ redacted, toggleRedacted }}>
      {children}
    </RedactedContext.Provider>
  )
}

export function useRedacted(): RedactedContextValue {
  const ctx = useContext(RedactedContext)
  if (!ctx) throw new Error("useRedacted must be used inside RedactedProvider")
  return ctx
}
