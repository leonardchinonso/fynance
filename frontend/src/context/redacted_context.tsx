import { createContext, useContext, useState, useCallback, Fragment } from "react"
import { isRedactedInitial, setRedacted as setRedactedModule } from "@/lib/utils"

interface RedactedContextValue {
  redacted: boolean
  toggleRedacted: () => void
}

const RedactedContext = createContext<RedactedContextValue | null>(null)

/**
 * Owns the privacy ("redacted amounts") toggle.
 *
 * formatCurrency reads a module-scope flag, so most components that render
 * money never consume this context. To make a toggle take effect everywhere
 * instantly we remount the subtree via a changing `key`. This provider sits
 * below the data-fetching contexts (currencies, profiles, category colours),
 * so toggling re-runs formatting and page renders but does not refetch
 * reference data. Toggling is a deliberate, infrequent action, so the brief
 * page-data re-render is an acceptable trade for guaranteed correctness.
 */
export function RedactedProvider({ children }: { children: React.ReactNode }) {
  const [redacted, setRedactedState] = useState(isRedactedInitial())

  const toggleRedacted = useCallback(() => {
    setRedactedState((prev) => {
      const next = !prev
      setRedactedModule(next)
      return next
    })
  }, [])

  return (
    <RedactedContext.Provider value={{ redacted, toggleRedacted }}>
      <Fragment key={redacted ? "redacted" : "clear"}>{children}</Fragment>
    </RedactedContext.Provider>
  )
}

export function useRedacted(): RedactedContextValue {
  const ctx = useContext(RedactedContext)
  if (!ctx) throw new Error("useRedacted must be used inside RedactedProvider")
  return ctx
}
