import { NavLink } from "react-router-dom"
import { cn } from "@/lib/utils"
import { useProfiles } from "@/context/profile_context"
import { useUrlFilters } from "@/hooks/use_url_filters"
import { useTheme } from "@/hooks/use_theme"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Button } from "@/components/ui/button"
import { Sun, Moon, Monitor } from "lucide-react"

const NAV_ITEMS = [
  { to: "/transactions", label: "Transactions" },
  { to: "/budget", label: "Budget" },
  { to: "/portfolio", label: "Portfolio" },
  { to: "/reports", label: "Reports" },
]

export function Navbar() {
  const { profiles } = useProfiles()
  const { profileId, setProfileId } = useUrlFilters()
  const { theme, setTheme } = useTheme()

  return (
    <nav className="sticky top-0 z-50 border-b bg-background/95 backdrop-blur supports-[backdrop-filter]:bg-background/60">
      <div className="mx-auto flex h-14 max-w-7xl items-center gap-6 px-4">
        {/* Logo + title */}
        <NavLink to="/" className="flex items-center gap-2">
          <img
            src="/favicon.png"
            alt="fynance logo"
            className="h-7 w-7 rounded"
          />
          <span className="text-lg font-semibold">fynance</span>
        </NavLink>

        {/* Nav tabs */}
        <div className="flex items-center gap-1">
          {NAV_ITEMS.map((item) => (
            <NavLink
              key={item.to}
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
          ))}
        </div>

        <div className="flex-1" />

        {/* Theme toggle */}
        <div className="flex items-center gap-1">
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
        </div>

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
  )
}
