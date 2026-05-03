import { KeyRound } from "lucide-react"
import { useNavigate } from "react-router-dom"
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
  const isAuthError = AUTH_ERROR_PREFIXES.some((p) => error.startsWith(p))

  if (isAuthError) {
    return (
      <NonIdealState
        icon={<KeyRound className="h-10 w-10" />}
        title={error}
        action={{
          label: "Go to Settings › Auth",
          onClick: () => navigate("/settings#auth"),
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
