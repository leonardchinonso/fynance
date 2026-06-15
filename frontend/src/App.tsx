import { lazy, Suspense } from "react"
import { BrowserRouter, Routes, Route, Navigate } from "react-router-dom"
import { ProfileProvider } from "@/context/profile_context"
import { PreferredCurrencyProvider } from "@/context/preferred_currency_context"
import { CategoryColorsProvider } from "@/context/category_colors_context"
import { CategoryNamesProvider } from "@/context/category_names_context"
import { ProfileColorsProvider } from "@/context/profile_colors_context"
import { RedactedProvider } from "@/context/redacted_context"
import { TooltipProvider } from "@/components/ui/tooltip"
import { Navbar } from "@/components/navbar"
import { TransactionsPage } from "@/pages/transactions"
import { BudgetPage } from "@/pages/budget"
import { PortfolioPage } from "@/pages/portfolio"
import { ReportsLanding } from "@/pages/reports/reports_landing"
import { DocumentsPage } from "@/pages/reports/documents/documents_page"
import {
  SettingsPage,
  SettingsGeneralPage,
  SettingsAccountsPage,
  SettingsCategoriesPage,
  SettingsAuthPage,
} from "@/pages/settings"
import { ImportPage } from "@/pages/import"

// CGT report page pulls in @react-pdf/renderer (~250 KB gzipped); code-split
// so users who never open Reports don't pay for it.
const CgtReportPage = lazy(() =>
  import("@/pages/reports/cgt/cgt_report_page").then((m) => ({ default: m.CgtReportPage })),
)

function getHomepage(): string {
  try {
    return localStorage.getItem("fynance-homepage") || "/portfolio"
  } catch {
    return "/portfolio"
  }
}

function Layout() {
  const homepage = getHomepage()

  return (
    <div className="flex flex-col h-screen bg-background overflow-x-hidden">
      <Navbar />
      <main className="flex-1 overflow-y-auto">
      <div className="mx-auto max-w-[1600px] px-3 sm:px-6 py-4 sm:py-6 w-full">
        <Routes>
          <Route path="/" element={<Navigate to={homepage} replace />} />
          <Route path="/portfolio" element={<PortfolioPage />} />
          <Route path="/budget" element={<BudgetPage />} />
          <Route path="/transactions" element={<TransactionsPage />} />
          <Route path="/reports" element={<ReportsLanding />} />
          <Route path="/reports/documents" element={<DocumentsPage />} />
          <Route
            path="/reports/cgt"
            element={
              <Suspense fallback={<div className="p-6 text-sm text-muted-foreground">Loading report…</div>}>
                <CgtReportPage />
              </Suspense>
            }
          />
          <Route
            path="/reports/cgt/:reportId"
            element={
              <Suspense fallback={<div className="p-6 text-sm text-muted-foreground">Loading report…</div>}>
                <CgtReportPage />
              </Suspense>
            }
          />
          <Route path="/settings" element={<SettingsPage />}>
            <Route index element={<Navigate to="general" replace />} />
            <Route path="general" element={<SettingsGeneralPage />} />
            <Route path="accounts" element={<SettingsAccountsPage />} />
            <Route path="categories" element={<SettingsCategoriesPage />} />
            <Route path="auth" element={<SettingsAuthPage />} />
          </Route>
          <Route path="/import" element={<ImportPage />} />
          <Route path="/import/wizard" element={<ImportPage />} />
          <Route path="/import/single" element={<ImportPage />} />
          <Route path="*" element={<Navigate to={homepage} replace />} />
        </Routes>
      </div>
      </main>
    </div>
  )
}

export default function App() {
  return (
    <BrowserRouter>
      <ProfileProvider>
        <PreferredCurrencyProvider>
          <CategoryColorsProvider>
            <CategoryNamesProvider>
              <ProfileColorsProvider>
                <RedactedProvider>
                  <TooltipProvider>
                    <Layout />
                  </TooltipProvider>
                </RedactedProvider>
              </ProfileColorsProvider>
            </CategoryNamesProvider>
          </CategoryColorsProvider>
        </PreferredCurrencyProvider>
      </ProfileProvider>
    </BrowserRouter>
  )
}
