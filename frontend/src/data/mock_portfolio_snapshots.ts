import type { PortfolioSnapshot } from "@/types"

// Generate monthly snapshots from Jan 2024 to Mar 2026 for all accounts.
// Each account has a base value and a growth pattern with some variation.

interface AccountSeed {
  account_id: string
  startBalance: number // Jan 2024 value
  monthlyGrowth: number // average monthly growth rate (0.01 = 1%)
  volatility: number // random variation factor
}

const ACCOUNT_SEEDS: AccountSeed[] = [
  { account_id: "monzo-current", startBalance: 2200, monthlyGrowth: 0.005, volatility: 0.15 },
  { account_id: "revolut-current", startBalance: 900, monthlyGrowth: 0.008, volatility: 0.2 },
  { account_id: "lloyds-savings", startBalance: 5000, monthlyGrowth: 0.03, volatility: 0.02 },
  { account_id: "t212-isa-alex", startBalance: 18000, monthlyGrowth: 0.025, volatility: 0.06 },
  { account_id: "premium-bonds", startBalance: 3000, monthlyGrowth: 0.02, volatility: 0.01 },
  { account_id: "pension-alex", startBalance: 38000, monthlyGrowth: 0.012, volatility: 0.03 },
  { account_id: "monzo-sam", startBalance: 1500, monthlyGrowth: 0.006, volatility: 0.18 },
  { account_id: "t212-isa-sam", startBalance: 12000, monthlyGrowth: 0.022, volatility: 0.05 },
  { account_id: "pension-sam", startBalance: 24000, monthlyGrowth: 0.013, volatility: 0.03 },
  { account_id: "home-value", startBalance: 320000, monthlyGrowth: 0.003, volatility: 0.005 },
  { account_id: "mortgage-alex", startBalance: 195000, monthlyGrowth: -0.005, volatility: 0.001 },
  { account_id: "joint-savings", startBalance: 2000, monthlyGrowth: 0.04, volatility: 0.03 },
  { account_id: "joint-current", startBalance: 1200, monthlyGrowth: 0.005, volatility: 0.12 },
]

const MONTHS: string[] = []
for (let y = 2024; y <= 2026; y++) {
  const maxMonth = y === 2026 ? 3 : 12
  for (let m = 1; m <= maxMonth; m++) {
    MONTHS.push(`${y}-${m.toString().padStart(2, "0")}`)
  }
}

// Deterministic pseudo-random
function seededRandom(seed: number): () => number {
  let s = seed
  return () => {
    s = (s * 1664525 + 1013904223) & 0xffffffff
    return (s >>> 0) / 0xffffffff
  }
}

function generateSnapshots(): PortfolioSnapshot[] {
  const snapshots: PortfolioSnapshot[] = []
  const rand = seededRandom(123)

  for (const seed of ACCOUNT_SEEDS) {
    let balance = seed.startBalance
    for (const month of MONTHS) {
      snapshots.push({
        snapshot_date: `${month}-01`,
        account_id: seed.account_id,
        balance: balance.toFixed(2),
        currency: "GBP",
      })
      // Grow with some randomness
      const variation = 1 + (rand() - 0.5) * 2 * seed.volatility
      balance = balance * (1 + seed.monthlyGrowth) * variation
      balance = Math.max(balance, 100) // floor
    }
  }

  return snapshots
}

export const MOCK_PORTFOLIO_SNAPSHOTS: PortfolioSnapshot[] = generateSnapshots()
