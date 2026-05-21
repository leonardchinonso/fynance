import { NavLink, Outlet } from "react-router-dom"
import { cn } from "@/lib/utils"
import { Settings as SettingsIcon, Building2, Tag, KeyRound } from "lucide-react"

const SECTIONS = [
  { to: "/settings/general",    label: "General",    icon: SettingsIcon },
  { to: "/settings/accounts",   label: "Accounts",   icon: Building2 },
  { to: "/settings/categories", label: "Categories", icon: Tag },
  { to: "/settings/auth",       label: "Auth",       icon: KeyRound },
] as const

/**
 * Settings shell: sticky sidebar nav + an <Outlet /> for the active section.
 * Each section is its own routed page (e.g. `/settings/general`,
 * `/settings/accounts`) so the sidebar feels like navigation rather than
 * an anchor-scroller through one giant column.
 */
export function SettingsPage() {
  return (
    <div className="flex gap-6">
      <nav className="hidden lg:block w-48 shrink-0 sticky top-6 self-start">
        <div className="space-y-0.5">
          {SECTIONS.map(({ to, label, icon: Icon }) => (
            <NavLink
              key={to}
              to={to}
              className={({ isActive }) => cn(
                "w-full flex items-center gap-2 rounded-md px-3 py-2 text-sm transition-colors",
                isActive
                  ? "bg-secondary text-secondary-foreground font-medium"
                  : "text-muted-foreground hover:bg-secondary/50 hover:text-foreground"
              )}
            >
              <Icon className="h-4 w-4 shrink-0" />
              {label}
            </NavLink>
          ))}
        </div>
      </nav>

      <div className="lg:hidden fixed bottom-0 left-0 right-0 z-40 border-t bg-background px-2 py-1.5 flex gap-0.5 overflow-x-auto">
        {SECTIONS.map(({ to, label, icon: Icon }) => (
          <NavLink
            key={to}
            to={to}
            className={({ isActive }) => cn(
              "flex flex-col items-center gap-0.5 rounded-md px-2 py-1 text-[10px] transition-colors whitespace-nowrap shrink-0",
              isActive ? "bg-secondary text-secondary-foreground" : "text-muted-foreground"
            )}
          >
            <Icon className="h-3.5 w-3.5" />
            {label}
          </NavLink>
        ))}
      </div>

      <div className="flex-1 min-w-0 space-y-6 pb-20 lg:pb-6">
        <Outlet />
      </div>
    </div>
  )
}
