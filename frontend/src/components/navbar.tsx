import { useState, useRef } from "react"
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
import { Sun, Moon, Monitor, Star, Pin, X, Bookmark, Menu, Database, TestTube2 } from "lucide-react"
import { getApiMode, setApiMode, type ApiMode } from "@/api/client"
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
  const [dragUrl, setDragUrl] = useState<string | null>(null)
  const { homepage, setHomepage, isHomepage } = useHomepage()
  const location = useLocation()
  const navigate = useNavigate()
  const [showPinDialog, setShowPinDialog] = useState(false)
  const [pinLabel, setPinLabel] = useState("")
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false)
  const [apiMode, setApiModeState] = useState<ApiMode>(getApiMode)

  function toggleApiMode() {
    const next: ApiMode = apiMode === "mock" ? "live" : "mock"
    setApiMode(next)
    setApiModeState(next)
    window.location.reload()
  }

  const nameExists = pinnedViews.some((v) => v.label.toLowerCase() === pinLabel.trim().toLowerCase())

  function handlePinSave() {
    if (pinLabel.trim() && !nameExists) {
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
              dragUrl={dragUrl}
              setDragUrl={setDragUrl}
              reorderPinnedViews={reorderPinnedViews}
              renamePinnedView={renamePinnedView}
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
            <Button variant={apiMode === "live" ? "default" : "ghost"} size="sm" className="h-8 gap-1.5 px-2 text-xs"
              onClick={toggleApiMode}
              title={`API: ${apiMode} (click to toggle)`}>
              {apiMode === "live" ? <Database className="h-3.5 w-3.5" /> : <TestTube2 className="h-3.5 w-3.5" />}
              <span className="hidden lg:inline">{apiMode === "live" ? "Live" : "Mock"}</span>
            </Button>
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
            {/* API mode */}
            <div className="flex items-center justify-between">
              <span className="text-sm">Data source</span>
              <Button variant={apiMode === "live" ? "default" : "outline"} size="sm" className="h-7 gap-1.5 px-2 text-xs"
                onClick={toggleApiMode}>
                {apiMode === "live" ? <Database className="h-3.5 w-3.5" /> : <TestTube2 className="h-3.5 w-3.5" />}
                {apiMode === "live" ? "Live" : "Mock"}
              </Button>
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
                  <div key={view.id} className="flex items-center gap-2">
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
              onKeyDown={(e) => { if (e.key === "Enter" && !nameExists) handlePinSave() }} autoFocus />
            {nameExists && (
              <p className="text-xs text-destructive">A view with this name already exists</p>
            )}
            <div className="flex justify-end gap-2">
              <Button variant="outline" size="sm" onClick={() => setShowPinDialog(false)}>Cancel</Button>
              <Button size="sm" onClick={handlePinSave} disabled={!pinLabel.trim() || nameExists}>Save</Button>
            </div>
          </div>
        </DialogContent>
      </Dialog>
    </>
  )
}

function SavedViewsPopover({
  pinnedViews, dragUrl, setDragUrl, reorderPinnedViews, renamePinnedView,
  isHomepage, setHomepage, unpinView, navigate, location, onSaveNew,
}: {
  pinnedViews: { id: string; label: string; url: string; createdAt: string }[]
  renamePinnedView: (url: string, newLabel: string) => void
  dragUrl: string | null
  setDragUrl: (url: string | null) => void
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
  const [editingId, setEditingId] = useState<string | null>(null)
  const [editValue, setEditValue] = useState("")
  const listRef = useRef<HTMLDivElement>(null)
  const rectsRef = useRef<Map<string, DOMRect>>(new Map())

  // FLIP animation: capture positions before render
  function captureRects() {
    if (!listRef.current) return
    const items = listRef.current.querySelectorAll("[data-drag-item]")
    const map = new Map<string, DOMRect>()
    items.forEach((el) => {
      const url = el.getAttribute("data-pin-id")
      if (url) map.set(url, el.getBoundingClientRect())
    })
    rectsRef.current = map
  }

  // FLIP animation: animate from old positions to new
  function animateRects() {
    if (!listRef.current) return
    const items = listRef.current.querySelectorAll("[data-drag-item]")
    items.forEach((el) => {
      const url = el.getAttribute("data-pin-id")
      if (!url) return
      const oldRect = rectsRef.current.get(url)
      if (!oldRect) return
      const newRect = el.getBoundingClientRect()
      const deltaY = oldRect.top - newRect.top
      if (Math.abs(deltaY) < 1) return
      const htmlEl = el as HTMLElement
      htmlEl.style.transition = "none"
      htmlEl.style.transform = `translateY(${deltaY}px)`
      requestAnimationFrame(() => {
        htmlEl.style.transition = "transform 200ms ease"
        htmlEl.style.transform = ""
      })
    })
  }

  return (
    <Popover>
      <PopoverTrigger className="rounded-md px-2.5 py-1.5 text-sm font-medium text-muted-foreground hover:bg-secondary/50 hover:text-foreground transition-colors flex items-center gap-1.5 whitespace-nowrap">
        <Bookmark className="h-3.5 w-3.5" />
        Saved
        {pinnedViews.length > 0 && (
          <Badge variant="secondary" className="h-4 min-w-4 px-1 text-[10px] rounded-full">{pinnedViews.length}</Badge>
        )}
      </PopoverTrigger>
      <PopoverContent className="w-[230px] !p-1.5 !gap-1" align="start">
        {pinnedViews.length > 0 && (
          <>
            <p className="text-xs font-medium text-muted-foreground mb-1 px-1">Saved Views</p>
            <div className="mb-1" ref={listRef}>
              {pinnedViews.map((view, idx) => (
                <div
                  key={view.id}
                  data-drag-item
                  data-pin-id={view.id}
                  className={cn(
                    "flex items-center gap-0.5 group rounded-md",
                    dragUrl === view.id && "invisible"
                  )}
                  style={{
                    opacity: dragUrl !== null && dragUrl !== view.url ? 0.6 : 1,
                    cursor: dragUrl === view.id ? "grabbing" : undefined,
                  }}
                  onPointerDown={(e) => {
                    const target = e.target as HTMLElement
                    if (!target.closest("[data-drag-handle]")) return
                    e.preventDefault()
                    const draggedId = view.id
                    setDragUrl(draggedId)
                    let settling = false

                    // Create floating drag image
                    const sourceEl = (e.currentTarget as HTMLElement)
                    const sourceRect = sourceEl.getBoundingClientRect()
                    const ghost = sourceEl.cloneNode(true) as HTMLElement
                    ghost.style.cssText = `
                      position: fixed;
                      left: ${sourceRect.left}px;
                      top: ${sourceRect.top}px;
                      width: ${sourceRect.width}px;
                      pointer-events: none;
                      z-index: 9999;
                      opacity: 0.9;
                      background: hsl(var(--muted));
                      border: 1px solid hsl(var(--border));
                      border-radius: 6px;
                      box-shadow: 0 4px 12px rgba(0,0,0,0.3);
                      transition: none;
                    `
                    document.body.appendChild(ghost)
                    const offsetY = e.clientY - sourceRect.top

                    const onMove = (ev: PointerEvent) => {
                      // Move ghost to follow cursor Y, clamped within the list bounds
                      if (listRef.current) {
                        const listRect = listRef.current.getBoundingClientRect()
                        const ghostHeight = sourceRect.height
                        const minY = listRect.top
                        const maxY = listRect.bottom - ghostHeight
                        const targetY = Math.max(minY, Math.min(maxY, ev.clientY - offsetY))
                        ghost.style.top = `${targetY}px`
                      } else {
                        ghost.style.top = `${ev.clientY - offsetY}px`
                      }

                      if (!listRef.current || settling) return

                      const items = Array.from(listRef.current.querySelectorAll("[data-drag-item]"))
                      const fromIdx = items.findIndex((el) => el.getAttribute("data-pin-id") === draggedId)
                      if (fromIdx === -1) return

                      // Step 1: Find which item the cursor is within (full bounding box)
                      let toIdx = -1
                      let targetRect: DOMRect | null = null
                      for (let i = 0; i < items.length; i++) {
                        if (i === fromIdx) continue
                        const rect = items[i].getBoundingClientRect()
                        if (ev.clientY >= rect.top && ev.clientY <= rect.bottom) {
                          toIdx = i
                          targetRect = rect
                          break
                        }
                      }

                      // Step 2: Check if cursor is in the forgiving zone based on direction
                      if (toIdx !== -1 && targetRect) {
                        const threshold = targetRect.height * 0.25
                        const isAbove = toIdx < fromIdx
                        const inActiveZone = isAbove
                          ? ev.clientY <= targetRect.bottom - threshold
                          : ev.clientY >= targetRect.top + threshold
                        if (!inActiveZone) toIdx = -1
                      }

                      if (toIdx === -1 || toIdx === fromIdx) return

                      captureRects()
                      reorderPinnedViews(fromIdx, toIdx)

                      // Wait for React to commit new DOM, then FLIP animate
                      settling = true
                      requestAnimationFrame(() => requestAnimationFrame(() => {
                        animateRects()
                        // Allow next move after animation
                        setTimeout(() => { settling = false }, 200)
                      }))
                    }
                    const onUp = () => {
                      setDragUrl(null)
                      ghost.remove()
                      document.removeEventListener("pointermove", onMove)
                      document.removeEventListener("pointerup", onUp)
                      document.body.style.cursor = ""
                    }
                    document.body.style.cursor = "grabbing"
                    document.addEventListener("pointermove", onMove)
                    document.addEventListener("pointerup", onUp)
                  }}
                >
                  <span data-drag-handle className="cursor-grab active:cursor-grabbing p-1 text-muted-foreground opacity-30 group-hover:opacity-70 shrink-0 touch-none" title="Drag to reorder">
                    <svg width="10" height="10" viewBox="0 0 10 10" fill="currentColor"><circle cx="3" cy="2" r="1"/><circle cx="7" cy="2" r="1"/><circle cx="3" cy="5" r="1"/><circle cx="7" cy="5" r="1"/><circle cx="3" cy="8" r="1"/><circle cx="7" cy="8" r="1"/></svg>
                  </span>
                  <button onClick={() => setHomepage(view.url)}
                    className="p-1 rounded hover:bg-muted transition-colors shrink-0"
                    title={isHomepage(view.url) ? "This is your homepage" : "Set as homepage"}>
                    <Star className={cn("h-3 w-3", isHomepage(view.url) ? "text-yellow-500" : "text-muted-foreground opacity-40 group-hover:opacity-100")} fill={isHomepage(view.url) ? "currentColor" : "none"} />
                  </button>
                  {editingId === view.id ? (
                    <input
                      className="flex-1 min-w-0 rounded-md border bg-background px-2 py-1 text-sm outline-none focus:ring-1 focus:ring-ring"
                      value={editValue}
                      onChange={(e) => setEditValue(e.target.value)}
                      onBlur={() => {
                        if (editValue.trim() && editValue.trim() !== view.label) renamePinnedView(view.url, editValue.trim())
                        setEditingId(null)
                      }}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") { if (editValue.trim() && editValue.trim() !== view.label) renamePinnedView(view.url, editValue.trim()); setEditingId(null) }
                        if (e.key === "Escape") setEditingId(null)
                      }}
                      autoFocus
                      onClick={(e) => e.stopPropagation()}
                    />
                  ) : (
                    <button onClick={() => navigate(view.url)}
                      onDoubleClick={(e) => { e.preventDefault(); setEditingId(view.id); setEditValue(view.label) }}
                      className={cn("flex-1 text-left rounded-md px-2 py-1.5 text-sm transition-colors truncate",
                        view.url === currentUrl ? "bg-secondary text-secondary-foreground" : "text-foreground hover:bg-muted")}
                      title="Double-click to rename">
                      {view.label}
                    </button>
                  )}
                  <button onClick={() => { unpinView(view.url); if (isHomepage(view.url)) setHomepage("/portfolio") }}
                    className="p-1 rounded opacity-0 group-hover:opacity-100 hover:bg-destructive/20 transition-opacity shrink-0">
                    <X className="h-3 w-3 text-muted-foreground" />
                  </button>
                </div>
              ))}
            </div>
            <div className="border-t border-border my-0.5" />
          </>
        )}
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
          Save current view
        </button>
      </PopoverContent>
    </Popover>
  )
}

