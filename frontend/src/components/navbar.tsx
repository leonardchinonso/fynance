import { useState } from "react"
import { NavLink, useLocation, useNavigate } from "react-router-dom"
import { cn } from "@/lib/utils"
import { useProfiles } from "@/context/profile_context"
import { useUrlFilters } from "@/hooks/use_url_filters"
import { useTheme } from "@/hooks/use_theme"
import { usePinnedViews } from "@/hooks/use_pinned_views"
import { useHomepage } from "@/hooks/use_homepage"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
} from "@/components/ui/select"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Sun, Moon, Monitor, Star, Pin, X, Bookmark } from "lucide-react"

const NAV_ITEMS = [
  { to: "/portfolio", label: "Portfolio" },
  { to: "/budget", label: "Budget" },
  { to: "/transactions", label: "Transactions" },
  { to: "/reports", label: "Reports" },
]

export function Navbar() {
  const { profiles } = useProfiles()
  const { profileId, setProfileId } = useUrlFilters()
  const { theme, setTheme } = useTheme()
  const { pinnedViews, pinCurrentView, unpinView } = usePinnedViews()
  const { homepage, setHomepage, isHomepage } = useHomepage()
  const location = useLocation()
  const navigate = useNavigate()
  const [showPinDialog, setShowPinDialog] = useState(false)
  const [pinLabel, setPinLabel] = useState("")

  function handlePinSave() {
    if (pinLabel.trim()) {
      pinCurrentView(pinLabel.trim())
      setPinLabel("")
      setShowPinDialog(false)
    }
  }

  return (
    <>
      <nav className="sticky top-0 z-50 border-b bg-background/95 backdrop-blur supports-[backdrop-filter]:bg-background/60">
        <div className="mx-auto flex h-14 max-w-7xl items-center gap-4 px-4">
          {/* Logo + title */}
          <NavLink to={homepage} className="flex items-center gap-2 shrink-0">
            <img
              src="/favicon.png"
              alt="fynance logo"
              className="h-7 w-7 rounded"
            />
            <span className="text-lg font-semibold">fynance</span>
          </NavLink>

          {/* Nav tabs */}
          <div className="flex items-center gap-0.5">
            {NAV_ITEMS.map((item) => (
              <div key={item.to} className="group relative flex items-center">
                <NavLink
                  to={item.to}
                  className={({ isActive }) =>
                    cn(
                      "rounded-md px-3 py-1.5 text-sm font-medium transition-colors",
                      isActive
                        ? "bg-secondary text-secondary-foreground"
                        : "text-muted-foreground hover:bg-secondary/50 hover:text-foreground"
                    )
                  }
                >
                  {item.label}
                </NavLink>
                {/* Star icon for homepage - visible on hover */}
                <button
                  className={cn(
                    "absolute -right-1 -top-1 rounded-full p-0.5 transition-opacity",
                    isHomepage(item.to)
                      ? "opacity-100 text-yellow-500"
                      : "opacity-0 group-hover:opacity-60 hover:!opacity-100 text-muted-foreground"
                  )}
                  onClick={(e) => {
                    e.preventDefault()
                    setHomepage(item.to)
                  }}
                  title={
                    isHomepage(item.to)
                      ? "This is your homepage"
                      : `Set ${item.label} as homepage`
                  }
                >
                  <Star
                    className="h-3 w-3"
                    fill={isHomepage(item.to) ? "currentColor" : "none"}
                  />
                </button>
              </div>
            ))}

            {/* Pinned views separator */}
            {pinnedViews.length > 0 && (
              <div className="mx-1.5 h-5 w-px bg-border" />
            )}

            {/* Pinned view tabs */}
            {pinnedViews.map((view) => (
              <div key={view.url} className="group relative flex items-center">
                <button
                  onClick={() => navigate(view.url)}
                  className={cn(
                    "rounded-md px-2.5 py-1.5 text-sm font-medium transition-colors flex items-center gap-1.5",
                    location.pathname + location.search === view.url
                      ? "bg-secondary text-secondary-foreground"
                      : "text-muted-foreground hover:bg-secondary/50 hover:text-foreground"
                  )}
                >
                  <Pin className="h-3 w-3 opacity-50" />
                  {view.label}
                </button>
                {/* Delete button on hover */}
                <button
                  className="absolute -right-1 -top-1 rounded-full bg-destructive/80 p-0.5 opacity-0 group-hover:opacity-100 transition-opacity hover:bg-destructive"
                  onClick={() => unpinView(view.url)}
                  title="Remove pinned view"
                >
                  <X className="h-2.5 w-2.5 text-destructive-foreground" />
                </button>
              </div>
            ))}
          </div>

          <div className="flex-1" />

          {/* Save/pin current view button */}
          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8"
            onClick={() => {
              setPinLabel("")
              setShowPinDialog(true)
            }}
            title="Save current view"
          >
            <Bookmark className="h-4 w-4" />
          </Button>

          {/* Theme toggle */}
          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8"
            onClick={() => {
              const next =
                theme === "light"
                  ? "dark"
                  : theme === "dark"
                    ? "system"
                    : "light"
              setTheme(next)
            }}
            title={`Theme: ${theme}`}
          >
            {theme === "light" ? (
              <Sun className="h-4 w-4" />
            ) : theme === "dark" ? (
              <Moon className="h-4 w-4" />
            ) : (
              <Monitor className="h-4 w-4" />
            )}
          </Button>

          {/* Profile selector */}
          <Select
            value={profileId || "all"}
            onValueChange={(v) => setProfileId(v === "all" ? undefined : v)}
          >
            <SelectTrigger className="w-[140px]">
              <span>
                {profileId
                  ? profiles.find((p) => p.id === profileId)?.name ?? profileId
                  : "All profiles"}
              </span>
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">All profiles</SelectItem>
              {profiles.map((p) => (
                <SelectItem key={p.id} value={p.id}>
                  {p.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </nav>

      {/* Pin view dialog */}
      <Dialog open={showPinDialog} onOpenChange={setShowPinDialog}>
        <DialogContent className="sm:max-w-sm">
          <DialogHeader>
            <DialogTitle>Save current view</DialogTitle>
          </DialogHeader>
          <div className="space-y-3 pt-2">
            <Input
              placeholder="View name (e.g. Monthly budget)"
              value={pinLabel}
              onChange={(e) => setPinLabel(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") handlePinSave()
              }}
              autoFocus
            />
            <div className="flex justify-end gap-2">
              <Button
                variant="outline"
                size="sm"
                onClick={() => setShowPinDialog(false)}
              >
                Cancel
              </Button>
              <Button size="sm" onClick={handlePinSave} disabled={!pinLabel.trim()}>
                Save
              </Button>
            </div>
          </div>
        </DialogContent>
      </Dialog>
    </>
  )
}
