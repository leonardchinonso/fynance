import { KeyRound } from "lucide-react"
import { useNavigate, useLocation } from "react-router-dom"
import { NonIdealState } from "./non_ideal_state"

const AUTH_ERROR_PREFIXES = [
  "Authorization required",
  "Your token may be expired",
]

interface AuthAwareErrorProps {
  error: string
  onRetry?: () => void
}

export function AuthAwareError({ error, onRetry }: AuthAwareErrorProps) {
  const navigate = useNavigate()
  const location = useLocation()
  const isAuthError = AUTH_ERROR_PREFIXES.some((p) => error.startsWith(p))

  function goToAuth() {
    if (location.pathname === "/settings") {
      document.getElementById("auth")?.scrollIntoView({ behavior: "smooth", block: "start" })
      history.replaceState(null, "", "#auth")
    } else {
      navigate("/settings#auth")
    }
  }

  if (isAuthError) {
    return (
      <NonIdealState
        icon={<KeyRound className="h-10 w-10" />}
        title={error}
        action={{
          label: "Go to Settings › Auth",
          onClick: goToAuth,
        }}
      />
    )
  }

  return (
    <NonIdealState
      title="Could not load data"
      description={error}
      action={onRetry ? { label: "Try again", onClick: onRetry } : undefined}
    />
  )
}
