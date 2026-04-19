import { useTheme } from "@/hooks/use_theme"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { Sun, Moon, Monitor } from "lucide-react"

export function AppearanceSection() {
  const { theme, setTheme } = useTheme()

  return (
    <Card id="appearance">
      <CardHeader>
        <CardTitle className="text-lg">Appearance</CardTitle>
        <p className="text-sm text-muted-foreground">
          Choose how fynance looks on your device.
        </p>
      </CardHeader>
      <CardContent>
        <div className="space-y-2">
          <label className="text-sm font-medium">Theme</label>
          <ToggleGroup value={[theme]} onValueChange={(v) => { if (v.length) setTheme(v[0] as "light" | "dark" | "system") }}
            className="justify-start">
            <ToggleGroupItem value="light" className="gap-1.5 px-3">
              <Sun className="h-4 w-4" /> Light
            </ToggleGroupItem>
            <ToggleGroupItem value="dark" className="gap-1.5 px-3">
              <Moon className="h-4 w-4" /> Dark
            </ToggleGroupItem>
            <ToggleGroupItem value="system" className="gap-1.5 px-3">
              <Monitor className="h-4 w-4" /> System
            </ToggleGroupItem>
          </ToggleGroup>
        </div>
      </CardContent>
    </Card>
  )
}
