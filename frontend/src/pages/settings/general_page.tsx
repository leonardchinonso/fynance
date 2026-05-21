import { useProfiles } from "@/context/profile_context"
import { useCurrencies } from "@/hooks/data"
import { useRefreshPreferredCurrency } from "@/context/preferred_currency_context"
import { ProfilesSection } from "./profiles_section"
import { CurrenciesSection } from "./currencies_section"
import { AppearanceSection } from "./appearance_section"
import { DataSourceSection } from "./data_source_section"

export function SettingsGeneralPage() {
  const { profilesData, refreshProfiles } = useProfiles()
  const [currenciesData, refreshCurrencies] = useCurrencies()
  const refreshContextCurrencies = useRefreshPreferredCurrency()

  function refresh() {
    refreshProfiles()
    refreshCurrencies()
    refreshContextCurrencies()
  }

  return (
    <>
      <div>
        <h1 className="text-2xl font-semibold">General</h1>
        <p className="text-sm text-muted-foreground mt-1">
          Profiles, currencies, appearance, and data source.
        </p>
      </div>

      <ProfilesSection data={profilesData} onRefresh={refresh} />
      <CurrenciesSection data={currenciesData} onRefresh={refresh} />
      <AppearanceSection />
      <DataSourceSection />
    </>
  )
}
