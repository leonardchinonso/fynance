import { BrowserRouter, Routes, Route, Navigate } from "react-router-dom"
import { ProfileProvider } from "@/context/profile_context"
import { TooltipProvider } from "@/components/ui/tooltip"
import { Navbar } from "@/components/navbar"
import { TransactionsPage } from "@/pages/transactions"
import { BudgetPage } from "@/pages/budget"
import { PortfolioPage } from "@/pages/portfolio"
import { ReportsPage } from "@/pages/reports"

function Layout() {
  return (
    <div className="min-h-screen bg-background">
      <Navbar />
      <main className="mx-auto max-w-7xl px-4 py-6">
        <Routes>
          <Route path="/" element={<Navigate to="/transactions" replace />} />
          <Route path="/transactions" element={<TransactionsPage />} />
          <Route path="/budget" element={<BudgetPage />} />
          <Route path="/portfolio" element={<PortfolioPage />} />
          <Route path="/reports" element={<ReportsPage />} />
          <Route path="*" element={<Navigate to="/transactions" replace />} />
        </Routes>
      </main>
    </div>
  )
}

export default function App() {
  return (
    <BrowserRouter>
      <ProfileProvider>
        <TooltipProvider>
          <Layout />
        </TooltipProvider>
      </ProfileProvider>
    </BrowserRouter>
  )
}
