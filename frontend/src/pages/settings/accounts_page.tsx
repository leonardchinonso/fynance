import { useProfiles } from "@/context/profile_context"
import { useAccounts } from "@/hooks/data"
import { AccountsSection } from "./accounts_section"

export function SettingsAccountsPage() {
  const { profilesData, refreshProfiles } = useProfiles()
  const [accountsData, refreshAccounts] = useAccounts()

  function refresh() {
    refreshProfiles()
    refreshAccounts()
  }

  return (
    <>
      <div>
        <h1 className="text-2xl font-semibold">Accounts</h1>
        <p className="text-sm text-muted-foreground mt-1">
          Bank accounts, investment accounts, credit cards, and other financial accounts.
        </p>
      </div>

      <AccountsSection data={accountsData} profilesData={profilesData} onRefresh={refresh} />
    </>
  )
}
