import { Component, type ErrorInfo, type ReactNode } from "react"

interface Props {
  children: ReactNode
}

interface State {
  error: Error | null
}

/**
 * Catches render errors below it so one broken page or widget doesn't
 * white-screen the whole app. Mount with a `key` (e.g. the route path) to
 * auto-reset when the user navigates away.
 */
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null }

  static getDerivedStateFromError(error: Error): State {
    return { error }
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("Render error:", error, info.componentStack)
  }

  render() {
    if (this.state.error) {
      return (
        <div className="flex flex-col items-center justify-center gap-3 rounded-lg border border-destructive/30 bg-destructive/5 p-8 my-8 text-center">
          <p className="text-sm font-medium">Something went wrong rendering this page.</p>
          <p className="max-w-md break-words text-xs text-muted-foreground">
            {this.state.error.message}
          </p>
          <button
            type="button"
            className="rounded-md border px-3 py-1.5 text-sm font-medium hover:bg-muted transition-colors"
            onClick={() => this.setState({ error: null })}
          >
            Try again
          </button>
        </div>
      )
    }
    return this.props.children
  }
}
