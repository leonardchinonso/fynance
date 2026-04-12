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
import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet"
import { Sun, Moon, Monitor, Star, Pin, X, Bookmark, Menu } from "lucide-react"
import { Badge } from "@/components/ui/badge"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"

const NAV_ITEMS = [
  { to: "/portfolio", label: "Portfolio", shortLabel: "Portfolio" },
  { to: "/budget", label: "Budget", shortLabel: "Budget" },
  { to: "/transactions", label: "Transactions", shortLabel: "Txns" },
  { to: "/reports", label: "Reports", shortLabel: "Reports" },
]

export function Navbar() {
  const { profiles } = useProfiles()
  const { profileId, setProfileId } = useUrlFilters()
  const { theme, setTheme } = useTheme()
  const { pinnedViews, pinCurrentView, unpinView, renamePinnedView, reorderPinnedViews } = usePinnedViews()
  const [dragIndex, setDragIndex] = useState<number | null>(null)
  const { homepage, setHomepage, isHomepage } = useHomepage()
  const location = useLocation()
  const navigate = useNavigate()
  const [showPinDialog, setShowPinDialog] = useState(false)
  const [pinLabel, setPinLabel] = useState("")
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false)

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
        <div className="mx-auto flex h-14 max-w-[1600px] items-center gap-2 sm:gap-4 px-3 sm:px-6 overflow-hidden">
          {/* Logo */}
          <NavLink to={homepage} className="flex items-center gap-2 shrink-0">
            <img src="/favicon.png" alt="fynance logo" className="h-7 w-7 rounded" />
            <span className="text-lg font-semibold hidden sm:inline">fynance</span>
          </NavLink>

          {/* Desktop nav tabs */}
          <div className="hidden md:flex items-center gap-0.5 min-w-0 overflow-hidden">
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
                <button
                  className={cn(
                    "absolute -right-1 -top-1 rounded-full p-0.5 transition-opacity",
                    "opacity-0 group-hover:opacity-60 hover:!opacity-100",
                    isHomepage(item.to) ? "text-yellow-500" : "text-muted-foreground"
                  )}
                  onClick={(e) => { e.preventDefault(); setHomepage(item.to) }}
                  title={isHomepage(item.to) ? "This is your homepage" : `Set ${item.label} as homepage`}
                >
                  <Star className="h-3 w-3" fill={isHomepage(item.to) ? "currentColor" : "none"} />
                </button>
              </div>
            ))}
            {/* Saved views dropdown - always visible */}
            <SavedViewsPopover
              pinnedViews={pinnedViews}
              dragIndex={dragIndex}
              setDragIndex={setDragIndex}
              reorderPinnedViews={reorderPinnedViews}
              isHomepage={isHomepage}
              setHomepage={setHomepage}
              unpinView={unpinView}
              navigate={navigate}
              location={location}
              onSaveNew={() => { setPinLabel(""); setShowPinDialog(true) }}
            />
          </div>

          {/* Mobile nav tabs - compact, no overflow */}
          <div className="flex md:hidden items-center gap-0.5 flex-1 min-w-0">
            {NAV_ITEMS.map((item) => (
              <NavLink
                key={item.to}
                to={item.to}
                className={({ isActive }) =>
                  cn(
                    "rounded-md px-2 py-1 text-xs font-medium transition-colors whitespace-nowrap shrink-0",
                    isActive
                      ? "bg-secondary text-secondary-foreground"
                      : "text-muted-foreground"
                  )
                }
              >
                {item.shortLabel}
              </NavLink>
            ))}
          </div>

          <div className="flex-1 hidden md:block" />

          {/* Desktop actions */}
          <div className="hidden md:flex items-center gap-1">
            <Button variant="ghost" size="icon" className="h-8 w-8"
              onClick={() => { const next = theme === "light" ? "dark" : theme === "dark" ? "system" : "light"; setTheme(next) }}
              title={`Theme: ${theme}`}>
              {theme === "light" ? <Sun className="h-4 w-4" /> : theme === "dark" ? <Moon className="h-4 w-4" /> : <Monitor className="h-4 w-4" />}
            </Button>
          </div>

          {/* Profile selector - hidden on mobile (in hamburger menu) */}
          <div className="hidden md:block">
            <Select value={profileId || "all"} onValueChange={(v) => setProfileId(v === "all" ? undefined : v)}>
              <SelectTrigger className="w-[140px]">
                <span className="truncate">
                  {profileId ? profiles.find((p) => p.id === profileId)?.name ?? profileId : "All profiles"}
                </span>
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">All profiles</SelectItem>
                {profiles.map((p) => (<SelectItem key={p.id} value={p.id}>{p.name}</SelectItem>))}
              </SelectContent>
            </Select>
          </div>

          {/* Mobile hamburger */}
          <Button variant="ghost" size="icon" className="h-8 w-8 md:hidden"
            onClick={() => setMobileMenuOpen(true)}>
            <Menu className="h-5 w-5" />
          </Button>
        </div>
      </nav>

      {/* Mobile menu sheet */}
      <Sheet open={mobileMenuOpen} onOpenChange={setMobileMenuOpen}>
        <SheetContent className="w-[280px] px-4">
          <SheetHeader>
            <SheetTitle>Menu</SheetTitle>
          </SheetHeader>
          <div className="mt-4 space-y-4">
            {/* Profile selector (mobile) */}
            <div className="flex items-center justify-between">
              <span className="text-sm">Profile</span>
              <Select value={profileId || "all"} onValueChange={(v) => setProfileId(v === "all" ? undefined : v)}>
                <SelectTrigger className="w-[130px]">
                  <span className="truncate">
                    {profileId ? profiles.find((p) => p.id === profileId)?.name ?? profileId : "All profiles"}
                  </span>
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="all">All profiles</SelectItem>
                  {profiles.map((p) => (<SelectItem key={p.id} value={p.id}>{p.name}</SelectItem>))}
                </SelectContent>
              </Select>
            </div>
            {/* Theme */}
            <div className="flex items-center justify-between">
              <span className="text-sm">Theme</span>
              <div className="flex gap-1">
                {(["light", "dark", "system"] as const).map((t) => (
                  <Button key={t} variant={theme === t ? "secondary" : "ghost"} size="sm" className="h-7 px-2 text-xs capitalize"
                    onClick={() => setTheme(t)}>{t}</Button>
                ))}
              </div>
            </div>
            {/* Save view */}
            <Button variant="outline" size="sm" className="w-full" onClick={() => { setMobileMenuOpen(false); setPinLabel(""); setShowPinDialog(true) }}>
              <Bookmark className="h-4 w-4 mr-2" /> Save current view
            </Button>
            {/* Pinned views */}
            {pinnedViews.length > 0 && (
              <div className="space-y-1">
                <p className="text-xs font-medium text-muted-foreground uppercase tracking-wider">Saved Views</p>
                {pinnedViews.map((view) => (
                  <div key={view.url} className="flex items-center gap-2">
                    <button
                      onClick={() => { navigate(view.url); setMobileMenuOpen(false) }}
                      className={cn(
                        "flex-1 text-left rounded-md px-2 py-1.5 text-sm transition-colors",
                        location.pathname + location.search === view.url
                          ? "bg-secondary text-secondary-foreground"
                          : "text-muted-foreground hover:bg-secondary/50"
                      )}
                    >
                      <Pin className="h-3 w-3 inline mr-1.5 opacity-50" />{view.label}
                    </button>
                    <button onClick={() => { unpinView(view.url); if (isHomepage(view.url)) setHomepage("/portfolio") }}
                      className="p-1 rounded hover:bg-destructive/20"><X className="h-3 w-3 text-muted-foreground" /></button>
                  </div>
                ))}
              </div>
            )}
          </div>
        </SheetContent>
      </Sheet>

      {/* Pin view dialog */}
      <Dialog open={showPinDialog} onOpenChange={setShowPinDialog}>
        <DialogContent className="sm:max-w-sm">
          <DialogHeader><DialogTitle>Save current view</DialogTitle></DialogHeader>
          <div className="space-y-3 pt-2">
            <Input placeholder="View name (e.g. Monthly budget)" value={pinLabel}
              onChange={(e) => setPinLabel(e.target.value)}
              onKeyDown={(e) => { if (e.key === "Enter") handlePinSave() }} autoFocus />
            <div className="flex justify-end gap-2">
              <Button variant="outline" size="sm" onClick={() => setShowPinDialog(false)}>Cancel</Button>
              <Button size="sm" onClick={handlePinSave} disabled={!pinLabel.trim()}>Save</Button>
            </div>
          </div>
        </DialogContent>
      </Dialog>
    </>
  )
}

function SavedViewsPopover({
  pinnedViews, dragIndex, setDragIndex, reorderPinnedViews,
  isHomepage, setHomepage, unpinView, navigate, location, onSaveNew,
}: {
  pinnedViews: { label: string; url: string; createdAt: string }[]
  dragIndex: number | null
  setDragIndex: (i: number | null) => void
  reorderPinnedViews: (from: number, to: number) => void
  isHomepage: (path: string) => boolean
  setHomepage: (path: string) => void
  unpinView: (url: string) => void
  navigate: (url: string) => void
  location: { pathname: string; search: string }
  onSaveNew: () => void
}) {
  const currentUrl = location.pathname + location.search
  const existingMatch = pinnedViews.find((v) => v.url === currentUrl)

  return (
    <Popover>
      <PopoverTrigger className="rounded-md px-2.5 py-1.5 text-sm font-medium text-muted-foreground hover:bg-secondary/50 hover:text-foreground transition-colors flex items-center gap-1.5 whitespace-nowrap">
        <Bookmark className="h-3.5 w-3.5" />
        Saved
        {pinnedViews.length > 0 && (
          <Badge variant="secondary" className="h-4 min-w-4 px-1 text-[10px] rounded-full">{pinnedViews.length}</Badge>
        )}
      </PopoverTrigger>
      <PopoverContent className="w-[240px] p-2" align="start">
        {pinnedViews.length > 0 && (
          <>
            <p className="text-xs font-medium text-muted-foreground mb-1.5 px-1">Saved Views</p>
            <div className="space-y-0.5 mb-2">
              {pinnedViews.map((view, idx) => (
                <div
                  key={view.url}
                  className={cn(
                    "flex items-center gap-0.5 group rounded-md transition-all duration-150",
                    dragIndex === idx && "bg-muted border border-border shadow-md scale-[1.02] z-10 relative",
                    dragIndex !== null && dragIndex !== idx && "opacity-70"
                  )}
                  draggable
                  onDragStart={(e) => {
                    setDragIndex(idx)
                    const ghost = document.createElement("div")
                    ghost.style.cssText = "position:fixed;top:-1000px;opacity:0"
                    document.body.appendChild(ghost)
                    e.dataTransfer.setDragImage(ghost, 0, 0)
                    requestAnimationFrame(() => document.body.removeChild(ghost))
                  }}
                  onDragOver={(e) => { e.preventDefault(); if (dragIndex !== null && dragIndex !== idx) { reorderPinnedViews(dragIndex, idx); setDragIndex(idx) } }}
                  onDragEnd={() => setDragIndex(null)}
                >
                  <span className="cursor-grab active:cursor-grabbing p-1 text-muted-foreground opacity-30 group-hover:opacity-70 shrink-0" title="Drag to reorder">
                    <svg width="10" height="10" viewBox="0 0 10 10" fill="currentColor"><circle cx="3" cy="2" r="1"/><circle cx="7" cy="2" r="1"/><circle cx="3" cy="5" r="1"/><circle cx="7" cy="5" r="1"/><circle cx="3" cy="8" r="1"/><circle cx="7" cy="8" r="1"/></svg>
                  </span>
                  <button onClick={() => setHomepage(view.url)}
                    className="p-1 rounded hover:bg-muted transition-colors shrink-0"
                    title={isHomepage(view.url) ? "This is your homepage" : "Set as homepage"}>
                    <Star className={cn("h-3 w-3", isHomepage(view.url) ? "text-yellow-500" : "text-muted-foreground opacity-40 group-hover:opacity-100")} fill={isHomepage(view.url) ? "currentColor" : "none"} />
                  </button>
                  <button onClick={() => navigate(view.url)}
                    className={cn("flex-1 text-left rounded-md px-2 py-1.5 text-sm transition-colors truncate",
                      view.url === currentUrl ? "bg-secondary text-secondary-foreground" : "text-foreground hover:bg-muted")}>
                    {view.label}
                  </button>
                  <button onClick={() => { unpinView(view.url); if (isHomepage(view.url)) setHomepage("/portfolio") }}
                    className="p-1 rounded opacity-0 group-hover:opacity-100 hover:bg-destructive/20 transition-opacity shrink-0">
                    <X className="h-3 w-3 text-muted-foreground" />
                  </button>
                </div>
              ))}
            </div>
            <div className="border-t border-border my-1" />
          </>
        )}
        {/* Save current view button */}
        <button
          onClick={existingMatch ? undefined : onSaveNew}
          disabled={!!existingMatch}
          className={cn(
            "w-full flex items-center gap-2 rounded-md px-2 py-1.5 text-sm transition-colors",
            existingMatch
              ? "text-muted-foreground/50 cursor-not-allowed"
              : "text-muted-foreground hover:bg-muted hover:text-foreground"
          )}
          title={existingMatch ? `Current view is already saved as "${existingMatch.label}"` : "Save the current page and filters as a view"}
        >
          <Bookmark className="h-3.5 w-3.5 shrink-0" />
          {existingMatch ? `Already saved as "${existingMatch.label}"` : "Save current view"}
        </button>
      </PopoverContent>
    </Popover>
  )
}

