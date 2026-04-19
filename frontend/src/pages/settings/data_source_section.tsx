import { useState } from "react"
import { getApiMode, setApiMode, MOCK_ONLY, type ApiMode } from "@/api/client"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { Database, TestTube2, Info } from "lucide-react"

export function DataSourceSection() {
  const [mode, setModeState] = useState<ApiMode>(getApiMode)

  function handleChange(value: string) {
    if (!value) return
    const next = value as ApiMode
    setApiMode(next)
    setModeState(next)
    window.location.reload()
  }

  return (
    <Card id="data-source">
      <CardHeader>
        <CardTitle className="text-lg">Data Source</CardTitle>
        <p className="text-sm text-muted-foreground">
          Switch between live backend data and mock data for development.
        </p>
      </CardHeader>
      <CardContent>
        {MOCK_ONLY ? (
          <div className="flex items-start gap-2 rounded-lg border bg-muted/50 p-3">
            <Info className="h-4 w-4 text-muted-foreground mt-0.5 shrink-0" />
            <div>
              <p className="text-sm font-medium">Mock mode is enforced</p>
              <p className="text-xs text-muted-foreground">
                The VITE_MOCK_ONLY environment variable is set. The toggle is disabled.
              </p>
            </div>
          </div>
        ) : (
          <div className="space-y-2">
            <label className="text-sm font-medium">API Mode</label>
            <ToggleGroup type="single" value={mode} onValueChange={handleChange} className="justify-start">
              <ToggleGroupItem value="live" className="gap-1.5 px-3">
                <Database className="h-4 w-4" /> Live
              </ToggleGroupItem>
              <ToggleGroupItem value="mock" className="gap-1.5 px-3">
                <TestTube2 className="h-4 w-4" /> Mock
              </ToggleGroupItem>
            </ToggleGroup>
            <p className="text-xs text-muted-foreground">
              Changing the data source will reload the page.
            </p>
          </div>
        )}
      </CardContent>
    </Card>
  )
}
