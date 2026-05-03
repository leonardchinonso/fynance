import { useState } from "react"
import { Eye, EyeOff, KeyRound, Terminal } from "lucide-react"
import { getAuthToken, setAuthToken, MOCK_ONLY } from "@/api/client"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Info } from "lucide-react"

export function AuthSection() {
  const [inputValue, setInputValue] = useState(() => getAuthToken() ?? "")
  const [showToken, setShowToken] = useState(false)
  const [saved, setSaved] = useState(false)

  const storedToken = getAuthToken() ?? ""
  const isDirty = inputValue.trim() !== storedToken

  function handleSave() {
    setAuthToken(inputValue.trim() || null)
    setSaved(true)
    setTimeout(() => setSaved(false), 2000)
  }

  function handleClear() {
    setAuthToken(null)
    setInputValue("")
  }

  return (
    <Card id="auth">
      <CardHeader>
        <CardTitle className="text-lg">Auth</CardTitle>
        <p className="text-sm text-muted-foreground">
          Bearer token for API authentication.
        </p>
      </CardHeader>
      <CardContent className="space-y-4">
        {MOCK_ONLY ? (
          <div className="flex items-start gap-2 rounded-lg border bg-muted/50 p-3">
            <Info className="h-4 w-4 text-muted-foreground mt-0.5 shrink-0" />
            <p className="text-sm text-muted-foreground">The demo webpage does not support API tokens.</p>
          </div>
        ) : (
          <>
            <div className="flex items-start gap-2 rounded-lg border bg-muted/50 p-3">
              <KeyRound className="h-4 w-4 text-muted-foreground mt-0.5 shrink-0" />
              <div className="space-y-1">
                <p className="text-sm font-medium">When do you need a token?</p>
                <p className="text-xs text-muted-foreground">
                  API tokens are required when running fynance outside of your local device (such as via
                  Docker or on an external network). If running locally you can leave this blank,
                  otherwise generate a token via the CLI and paste it below.
                </p>
              </div>
            </div>

            <div className="flex items-start gap-2 rounded-lg border bg-muted/50 p-3">
              <Terminal className="h-4 w-4 text-muted-foreground mt-0.5 shrink-0" />
              <div className="space-y-1">
                <p className="text-sm font-medium">Generate a token</p>
                <code className="text-xs bg-background border rounded px-2 py-1 block">
                  fynance token create --name browser
                </code>
                <p className="text-xs text-muted-foreground">
                  Run this inside the container: <code className="text-xs">docker exec &lt;container_name&gt; fynance token create --name browser</code>
                </p>
              </div>
            </div>

            <div className="space-y-2">
              <label className="text-sm font-medium">API Token</label>
              <div className="relative">
                <Input
                  type={showToken ? "text" : "password"}
                  value={inputValue}
                  onChange={(e) => setInputValue(e.target.value)}
                  placeholder="fyn_..."
                  className="pr-10"
                />
                <button
                  type="button"
                  onClick={() => setShowToken((v) => !v)}
                  className="absolute right-3 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground transition-colors"
                >
                  {showToken ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                </button>
              </div>
              <p className="text-xs text-muted-foreground">Tokens start with fyn_...</p>
            </div>

            <div className="flex items-center gap-2">
              <Button onClick={handleSave} disabled={!isDirty} size="sm">
                Save
              </Button>
              {storedToken && (
                <Button onClick={handleClear} variant="destructive" size="sm">
                  Clear
                </Button>
              )}
              {saved && (
                <span className="text-xs text-muted-foreground">Saved!</span>
              )}
            </div>
          </>
        )}
      </CardContent>
    </Card>
  )
}
