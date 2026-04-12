import type { AccountSnapshot } from "@/types"

// Generate monthly snapshots from Jan 2023 to Mar 2026 for all accounts.
// Models a realistic UK couple's wealth journey: gradual salary increases,
// regular contributions, market volatility, a house purchase, travel periods,
// and pension growth from employer + employee contributions.

interface AccountSeed {
  account_id: string
  // Starting balance in Jan 2023
  startBalance: number
  // Monthly contribution (positive = deposit, negative = withdrawal tendency)
  monthlyContribution: number
  // Annual contribution increase rate (e.g. salary raises mean higher savings)
  contributionGrowthRate: number
  // Monthly market return rate (for investment/pension accounts)
  monthlyReturn: number
  // Volatility as std dev of monthly returns
  volatility: number
  // One-off events: month -> adjustment
  events?: Record<string, { type: "set" | "add" | "multiply"; value: number }>
  // Floor value (balance can't go below this, default 0)
  floor?: number
}

const ACCOUNT_SEEDS: AccountSeed[] = [
  // ── Alex's accounts ──

  // Monzo Current: salary in, bills out. Balance fluctuates around 1500-3500.
  // Net monthly contribution is small (salary minus expenses leaves ~200-400 buffer growth)
  {
    account_id: "monzo-current",
    startBalance: 1450,
    monthlyContribution: 50,
    contributionGrowthRate: 0.03,
    monthlyReturn: 0,
    volatility: 0.12,
    floor: 400,
    events: {
      // Christmas spending dips
      "2023-12": { type: "add", value: -600 },
      "2024-12": { type: "add", value: -800 },
      "2025-12": { type: "add", value: -700 },
      // House deposit withdrawal
      "2024-05": { type: "add", value: -8000 },
      // Tax rebate
      "2024-09": { type: "add", value: 1200 },
      // Summer holiday spending
      "2023-07": { type: "add", value: -500 },
      "2024-07": { type: "add", value: -400 },
      "2025-08": { type: "add", value: -600 },
    },
  },

  // Revolut Current: spending card, stays relatively low
  {
    account_id: "revolut-current",
    startBalance: 820,
    monthlyContribution: 30,
    contributionGrowthRate: 0.02,
    monthlyReturn: 0,
    volatility: 0.18,
    floor: 100,
    events: {
      "2023-07": { type: "add", value: -300 }, // Holiday spending
      "2024-07": { type: "add", value: -250 },
      "2025-08": { type: "add", value: -400 },
    },
  },

  // Lloyds Savings: regular deposits, slow interest. Building emergency fund.
  {
    account_id: "lloyds-savings",
    startBalance: 3500,
    monthlyContribution: 400,
    contributionGrowthRate: 0.05,
    monthlyReturn: 0.004, // ~5% annual savings rate
    volatility: 0.005,
    events: {
      // Large withdrawal for house deposit
      "2024-05": { type: "add", value: -5000 },
      // Slowly rebuilding after
      "2024-06": { type: "multiply", value: 1.0 },
    },
  },

  // Alex's ISA: monthly contributions + market returns with real volatility
  {
    account_id: "t212-isa-alex",
    startBalance: 8500,
    monthlyContribution: 500,
    contributionGrowthRate: 0.04,
    monthlyReturn: 0.006, // ~7.5% annual equity return
    volatility: 0.035,
    events: {
      "2023-03": { type: "multiply", value: 1.04 },   // Q1 rally
      "2023-10": { type: "multiply", value: 0.94 },   // October selloff
      "2023-11": { type: "multiply", value: 1.03 },   // Recovery
      "2024-03": { type: "multiply", value: 1.05 },   // Spring rally
      "2024-08": { type: "multiply", value: 0.91 },   // Summer correction
      "2024-09": { type: "multiply", value: 0.97 },   // Continued weakness
      "2024-10": { type: "multiply", value: 1.04 },   // Recovery begins
      "2024-11": { type: "multiply", value: 1.03 },   // Continued recovery
      "2025-01": { type: "multiply", value: 1.04 },   // New year optimism
      "2025-04": { type: "multiply", value: 0.96 },   // Tariff concerns
      "2025-07": { type: "multiply", value: 1.03 },   // Relief rally
      "2025-10": { type: "multiply", value: 0.97 },   // Q4 wobble
      "2025-12": { type: "multiply", value: 1.04 },   // Year-end rally
      // Reduced contributions around house purchase
      "2024-05": { type: "add", value: -300 },
      "2024-06": { type: "add", value: -300 },
      "2024-07": { type: "add", value: -300 },
    },
  },

  // Premium Bonds: lump deposits at key points
  {
    account_id: "premium-bonds",
    startBalance: 0,
    monthlyContribution: 0,
    contributionGrowthRate: 0,
    monthlyReturn: 0,
    volatility: 0,
    events: {
      "2023-06": { type: "set", value: 1000 },
      "2023-12": { type: "set", value: 2000 },
      "2024-01": { type: "set", value: 3000 },
      "2024-06": { type: "set", value: 3000 }, // No more after house purchase
      "2025-03": { type: "set", value: 5000 },
      "2025-06": { type: "set", value: 6000 },
      "2025-09": { type: "set", value: 7000 },
      "2025-12": { type: "set", value: 8000 },
      "2026-03": { type: "set", value: 9500 },
    },
  },

  // Alex's Pension: employer + employee contributions (~£850/month total),
  // growing with salary raises, plus market-like returns
  {
    account_id: "pension-alex",
    startBalance: 18000,
    monthlyContribution: 850,
    contributionGrowthRate: 0.04,
    monthlyReturn: 0.005, // ~6% annual pension fund return
    volatility: 0.025,
    events: {
      "2023-10": { type: "multiply", value: 0.97 },   // Market dip
      "2024-03": { type: "multiply", value: 1.03 },   // Recovery
      "2024-08": { type: "multiply", value: 0.95 },   // Summer correction
      "2024-10": { type: "multiply", value: 1.02 },   // Bounce back
      "2025-01": { type: "multiply", value: 1.03 },   // New year
      "2025-04": { type: "multiply", value: 0.97 },   // Volatility
      "2025-10": { type: "multiply", value: 0.98 },   // Minor dip
      "2026-01": { type: "multiply", value: 1.02 },   // Recovery
    },
  },

  // Home value: purchased mid-2024, gentle appreciation
  {
    account_id: "home-value",
    startBalance: 0,
    monthlyContribution: 0,
    contributionGrowthRate: 0,
    monthlyReturn: 0.002, // ~2.5% annual home appreciation
    volatility: 0.003,
    events: {
      "2024-06": { type: "set", value: 310000 }, // House purchase
    },
  },

  // Mortgage: starts when house is bought, principal slowly decreases
  // (monthly payments reduce it, but interest mostly offsets early on)
  {
    account_id: "mortgage-alex",
    startBalance: 0,
    monthlyContribution: -600, // Net principal reduction per month
    contributionGrowthRate: 0,
    monthlyReturn: 0,
    volatility: 0,
    events: {
      "2024-06": { type: "set", value: 248000 }, // Mortgage starts
    },
  },

  // ── Sam's accounts ──

  // Sam's Monzo: lower salary, similar pattern
  {
    account_id: "monzo-sam",
    startBalance: 980,
    monthlyContribution: 40,
    contributionGrowthRate: 0.03,
    monthlyReturn: 0,
    volatility: 0.14,
    floor: 200,
    events: {
      "2023-12": { type: "add", value: -400 },
      "2024-12": { type: "add", value: -500 },
      "2025-12": { type: "add", value: -450 },
      "2023-07": { type: "add", value: -300 },
      "2025-08": { type: "add", value: -350 },
    },
  },

  // Sam's ISA: smaller monthly contributions but still growing
  {
    account_id: "t212-isa-sam",
    startBalance: 5200,
    monthlyContribution: 400,
    contributionGrowthRate: 0.035,
    monthlyReturn: 0.006,
    volatility: 0.035,
    events: {
      "2023-10": { type: "multiply", value: 0.95 },
      "2023-11": { type: "multiply", value: 1.02 },
      "2024-03": { type: "multiply", value: 1.04 },
      "2024-08": { type: "multiply", value: 0.90 },
      "2024-09": { type: "multiply", value: 0.98 },
      "2024-10": { type: "multiply", value: 1.03 },
      "2024-11": { type: "multiply", value: 1.03 },
      "2025-01": { type: "multiply", value: 1.04 },
      "2025-04": { type: "multiply", value: 0.97 },
      "2025-07": { type: "multiply", value: 1.02 },
      "2025-10": { type: "multiply", value: 0.96 },
      "2025-12": { type: "multiply", value: 1.03 },
    },
  },

  // Sam's Pension: employer + employee contributions (~£600/month),
  // growing over time with salary raises
  {
    account_id: "pension-sam",
    startBalance: 12000,
    monthlyContribution: 600,
    contributionGrowthRate: 0.035,
    monthlyReturn: 0.005,
    volatility: 0.025,
    events: {
      "2023-10": { type: "multiply", value: 0.97 },
      "2024-03": { type: "multiply", value: 1.02 },
      "2024-08": { type: "multiply", value: 0.94 },
      "2024-10": { type: "multiply", value: 1.02 },
      "2025-01": { type: "multiply", value: 1.03 },
      "2025-04": { type: "multiply", value: 0.97 },
      "2025-10": { type: "multiply", value: 0.98 },
      "2026-01": { type: "multiply", value: 1.02 },
    },
  },

  // ── Joint accounts ──

  // Joint savings: building up for house, depleted for deposit, rebuilding after
  {
    account_id: "joint-savings",
    startBalance: 2500,
    monthlyContribution: 500,
    contributionGrowthRate: 0.03,
    monthlyReturn: 0.003,
    volatility: 0.01,
    events: {
      // Big withdrawal for house deposit
      "2024-05": { type: "add", value: -12000 },
      "2024-06": { type: "set", value: 1500 }, // Reset after house purchase
      // Christmas gifts
      "2023-12": { type: "add", value: -500 },
      "2024-12": { type: "add", value: -600 },
      "2025-12": { type: "add", value: -400 },
    },
  },

  // Joint current: household bills, fluctuates with spending
  {
    account_id: "joint-current",
    startBalance: 650,
    monthlyContribution: 20,
    contributionGrowthRate: 0.02,
    monthlyReturn: 0,
    volatility: 0.10,
    floor: 200,
    events: {
      "2024-06": { type: "add", value: 500 },  // Extra buffer after moving
      "2024-07": { type: "add", value: -400 }, // Moving costs
    },
  },
]

// Generate months from Jan 2023 to Mar 2026
const MONTHS: string[] = []
for (let y = 2023; y <= 2026; y++) {
  const maxMonth = y === 2026 ? 3 : 12
  for (let m = 1; m <= maxMonth; m++) {
    MONTHS.push(`${y}-${m.toString().padStart(2, "0")}`)
  }
}

function seededRandom(seed: number): () => number {
  let s = seed
  return () => {
    s = (s * 1664525 + 1013904223) & 0xffffffff
    return (s >>> 0) / 0xffffffff
  }
}

// Box-Muller transform for normally distributed random numbers
function gaussianRandom(rand: () => number): number {
  const u1 = rand()
  const u2 = rand()
  return Math.sqrt(-2 * Math.log(u1 + 0.0001)) * Math.cos(2 * Math.PI * u2)
}

function generateBalances(): AccountSnapshot[] {
  const snapshots: AccountSnapshot[] = []
  const rand = seededRandom(777)

  for (const seed of ACCOUNT_SEEDS) {
    let balance = seed.startBalance
    let contribution = seed.monthlyContribution
    const floor = seed.floor ?? 0

    for (let i = 0; i < MONTHS.length; i++) {
      const month = MONTHS[i]

      // Apply events first
      const event = seed.events?.[month]
      if (event) {
        if (event.type === "set") balance = event.value
        else if (event.type === "add") balance += event.value
        else if (event.type === "multiply") balance *= event.value
      }

      // Add monthly contribution
      balance += contribution

      // Apply market return with volatility (Gaussian noise)
      if (seed.monthlyReturn !== 0 || seed.volatility > 0) {
        const noise = gaussianRandom(rand) * seed.volatility
        const monthReturn = seed.monthlyReturn + noise
        balance *= (1 + monthReturn)
      } else if (seed.volatility > 0) {
        // For cash accounts: just add noise proportional to balance
        const noise = gaussianRandom(rand) * seed.volatility
        balance *= (1 + noise)
      }

      // Apply floor
      if (balance < floor) balance = floor

      // Only record if account has been "opened" (has had a non-zero balance)
      if (balance > 0 || seed.startBalance > 0) {
        snapshots.push({
          as_of: `${month}-01T00:00:00`,
          account_id: seed.account_id,
          balance: Math.max(0, balance).toFixed(2),
          currency: "GBP",
        })
      }

      // Grow contributions annually (apply 1/12 of annual rate each month)
      if (seed.contributionGrowthRate > 0) {
        contribution *= (1 + seed.contributionGrowthRate / 12)
      }
    }
  }

  return snapshots
}

export const MOCK_ACCOUNT_BALANCES: AccountSnapshot[] = generateBalances()
