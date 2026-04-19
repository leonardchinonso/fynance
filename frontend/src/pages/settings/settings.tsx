import { useState, useEffect } from "react"
import { api } from "@/api/client"
import type { Account, Profile } from "@/types"
import { cn } from "@/lib/utils"
import { ProfilesSection } from "./profiles_section"
import { AccountsSection } from "./accounts_section"
import { CategoriesSection } from "./categories_section"
import { IngestionSection } from "./ingestion_section"
import { AppearanceSection } from "./appearance_section"
import { DataSourceSection } from "./data_source_section"
import { User, Building2, Tag, Upload, Palette, Database } from "lucide-react"

const SECTIONS = [
  { id: "profiles", label: "Profiles", icon: User },
  { id: "accounts", label: "Accounts", icon: Building2 },
  { id: "categories", label: "Categories", icon: Tag },
  { id: "ingestion", label: "Data Ingestion", icon: Upload },
  { id: "appearance", label: "Appearance", icon: Palette },
  { id: "data-source", label: "Data Source", icon: Database },
] as const

export function SettingsPage() {
  const [profiles, setProfiles] = useState<Profile[]>([])
  const [accounts, setAccounts] = useState<Account[]>([])
  const [activeSection, setActiveSection] = useState("profiles")

  async function loadData() {
    const [p, a] = await Promise.all([
      api.getProfiles().catch(() => [] as Profile[]),
      api.getAccounts().catch(() => [] as Account[]),
    ])
    setProfiles(p)
    setAccounts(a)
  }

  useEffect(() => { loadData() }, [])

  function scrollTo(id: string) {
    setActiveSection(id)
    document.getElementById(id)?.scrollIntoView({ behavior: "smooth", block: "start" })
  }

  return (
    <div className="flex gap-6">
      {/* Sidebar nav (desktop) */}
      <nav className="hidden lg:block w-48 shrink-0 sticky top-20 self-start">
        <div className="space-y-0.5">
          {SECTIONS.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              onClick={() => scrollTo(id)}
              className={cn(
                "w-full flex items-center gap-2 rounded-md px-3 py-2 text-sm transition-colors text-left",
                activeSection === id
                  ? "bg-secondary text-secondary-foreground font-medium"
                  : "text-muted-foreground hover:bg-secondary/50 hover:text-foreground"
              )}
            >
              <Icon className="h-4 w-4 shrink-0" />
              {label}
            </button>
          ))}
        </div>
      </nav>

      {/* Mobile section tabs */}
      <div className="lg:hidden fixed bottom-0 left-0 right-0 z-40 border-t bg-background px-2 py-1.5 flex gap-0.5 overflow-x-auto">
        {SECTIONS.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            onClick={() => scrollTo(id)}
            className={cn(
              "flex flex-col items-center gap-0.5 rounded-md px-2 py-1 text-[10px] transition-colors whitespace-nowrap shrink-0",
              activeSection === id
                ? "bg-secondary text-secondary-foreground"
                : "text-muted-foreground"
            )}
          >
            <Icon className="h-3.5 w-3.5" />
            {label}
          </button>
        ))}
      </div>

      {/* Main content */}
      <div className="flex-1 min-w-0 space-y-6 pb-20 lg:pb-6">
        <div>
          <h1 className="text-2xl font-semibold">Settings</h1>
          <p className="text-sm text-muted-foreground mt-1">
            Manage your profiles, accounts, categories, and preferences.
          </p>
        </div>

        <ProfilesSection profiles={profiles} onRefresh={loadData} />
        <AccountsSection accounts={accounts} profiles={profiles} onRefresh={loadData} />
        <CategoriesSection />
        <IngestionSection />
        <AppearanceSection />
        <DataSourceSection />
      </div>
    </div>
  )
}
