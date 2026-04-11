import type { Transaction } from "@/types"

// Simple seeded pseudo-random for deterministic data
function seededRandom(seed: number): () => number {
  let s = seed
  return () => {
    s = (s * 1664525 + 1013904223) & 0xffffffff
    return (s >>> 0) / 0xffffffff
  }
}

function randomAmount(rand: () => number, min: number, max: number): string {
  const val = min + rand() * (max - min)
  return val.toFixed(2)
}

function makeFingerprint(i: number): string {
  const hex = i.toString(16).padStart(8, "0")
  return `${hex}${hex}${hex}${hex}${hex}${hex}${hex}${hex}`
}

function makeId(i: number): string {
  const hex = i.toString(16).padStart(8, "0")
  return `${hex.slice(0, 8)}-${hex.slice(0, 4)}-4${hex.slice(1, 4)}-a${hex.slice(1, 4)}-${hex}${hex.slice(0, 4)}`
}

interface TxTemplate {
  description: string
  normalized: string
  category: string | null
  category_source: "rule" | "claude" | "manual" | null
  confidence: number | null
  account: string
  is_recurring: boolean
  amountMin: number
  amountMax: number
  isIncome: boolean
  frequency: "monthly" | "weekly" | "biweekly" | "occasional"
  dayOfMonth?: number
}

const RECURRING_TEMPLATES: TxTemplate[] = [
  // Opemipo income
  {
    description: "COMPANY LTD SALARY",
    normalized: "Salary",
    category: "Income: Salary",
    category_source: "rule",
    confidence: null,
    account: "monzo-current",
    is_recurring: true,
    amountMin: 5200,
    amountMax: 5200,
    isIncome: true,
    frequency: "monthly",
    dayOfMonth: 28,
  },
  // Mortgage
  {
    description: "MORTGAGE DIRECT DEBIT",
    normalized: "Mortgage Payment",
    category: "Housing: Rent / Mortgage",
    category_source: "rule",
    confidence: null,
    account: "monzo-current",
    is_recurring: true,
    amountMin: 1416.10,
    amountMax: 1416.10,
    isIncome: false,
    frequency: "monthly",
    dayOfMonth: 1,
  },
  // Utilities
  {
    description: "BRITISH GAS",
    normalized: "British Gas",
    category: "Housing: Utilities",
    category_source: "rule",
    confidence: null,
    account: "monzo-current",
    is_recurring: true,
    amountMin: 95,
    amountMax: 130,
    isIncome: false,
    frequency: "monthly",
    dayOfMonth: 5,
  },
  // Internet
  {
    description: "VIRGIN MEDIA",
    normalized: "Virgin Media",
    category: "Housing: Internet & Phone",
    category_source: "rule",
    confidence: null,
    account: "monzo-current",
    is_recurring: true,
    amountMin: 42,
    amountMax: 42,
    isIncome: false,
    frequency: "monthly",
    dayOfMonth: 12,
  },
  // Netflix
  {
    description: "NETFLIX.COM",
    normalized: "Netflix",
    category: "Entertainment: Streaming Services",
    category_source: "rule",
    confidence: null,
    account: "revolut-current",
    is_recurring: true,
    amountMin: 15.99,
    amountMax: 15.99,
    isIncome: false,
    frequency: "monthly",
    dayOfMonth: 15,
  },
  // Spotify
  {
    description: "SPOTIFY UK",
    normalized: "Spotify",
    category: "Entertainment: Streaming Services",
    category_source: "rule",
    confidence: null,
    account: "revolut-current",
    is_recurring: true,
    amountMin: 10.99,
    amountMax: 10.99,
    isIncome: false,
    frequency: "monthly",
    dayOfMonth: 18,
  },
  // Gym
  {
    description: "PUREGYM LTD",
    normalized: "PureGym",
    category: "Health: Gym & Fitness",
    category_source: "rule",
    confidence: null,
    account: "monzo-current",
    is_recurring: true,
    amountMin: 45,
    amountMax: 45,
    isIncome: false,
    frequency: "monthly",
    dayOfMonth: 1,
  },
  // Insurance
  {
    description: "AVIVA INSURANCE",
    normalized: "Home Insurance",
    category: "Finance: Insurance",
    category_source: "rule",
    confidence: null,
    account: "monzo-current",
    is_recurring: true,
    amountMin: 32.50,
    amountMax: 32.50,
    isIncome: false,
    frequency: "monthly",
    dayOfMonth: 8,
  },
  // Pension contribution
  {
    description: "PENSION CONTRIBUTION",
    normalized: "Pension Contribution",
    category: "Finance: Investment Transfer",
    category_source: "rule",
    confidence: null,
    account: "monzo-current",
    is_recurring: true,
    amountMin: 500,
    amountMax: 500,
    isIncome: false,
    frequency: "monthly",
    dayOfMonth: 28,
  },
  // Savings transfer
  {
    description: "SAVINGS TRANSFER",
    normalized: "Savings Transfer",
    category: "Finance: Savings Transfer",
    category_source: "rule",
    confidence: null,
    account: "monzo-current",
    is_recurring: true,
    amountMin: 500,
    amountMax: 500,
    isIncome: false,
    frequency: "monthly",
    dayOfMonth: 29,
  },
  // Investment
  {
    description: "TRADING 212 DEPOSIT",
    normalized: "Investment Deposit",
    category: "Finance: Investment Transfer",
    category_source: "rule",
    confidence: null,
    account: "monzo-current",
    is_recurring: true,
    amountMin: 500,
    amountMax: 500,
    isIncome: false,
    frequency: "monthly",
    dayOfMonth: 29,
  },
]

const VARIABLE_TEMPLATES: TxTemplate[] = [
  // Groceries
  {
    description: "LIDL GB LONDON",
    normalized: "Lidl",
    category: "Food: Groceries",
    category_source: "rule",
    confidence: null,
    account: "monzo-current",
    is_recurring: false,
    amountMin: 15,
    amountMax: 75,
    isIncome: false,
    frequency: "weekly",
  },
  {
    description: "TESCO STORES",
    normalized: "Tesco",
    category: "Food: Groceries",
    category_source: "rule",
    confidence: null,
    account: "monzo-current",
    is_recurring: false,
    amountMin: 20,
    amountMax: 90,
    isIncome: false,
    frequency: "biweekly",
  },
  {
    description: "SAINSBURYS",
    normalized: "Sainsbury's",
    category: "Food: Groceries",
    category_source: "claude",
    confidence: 0.92,
    account: "revolut-current",
    is_recurring: false,
    amountMin: 10,
    amountMax: 60,
    isIncome: false,
    frequency: "biweekly",
  },
  // Dining
  {
    description: "NANDOS",
    normalized: "Nando's",
    category: "Food: Dining & Bars",
    category_source: "claude",
    confidence: 0.95,
    account: "revolut-current",
    is_recurring: false,
    amountMin: 15,
    amountMax: 40,
    isIncome: false,
    frequency: "biweekly",
  },
  {
    description: "PIZZA EXPRESS",
    normalized: "Pizza Express",
    category: "Food: Dining & Bars",
    category_source: "claude",
    confidence: 0.88,
    account: "monzo-current",
    is_recurring: false,
    amountMin: 25,
    amountMax: 55,
    isIncome: false,
    frequency: "occasional",
  },
  {
    description: "WAGAMAMA",
    normalized: "Wagamama",
    category: "Food: Dining & Bars",
    category_source: "claude",
    confidence: 0.91,
    account: "revolut-current",
    is_recurring: false,
    amountMin: 18,
    amountMax: 35,
    isIncome: false,
    frequency: "occasional",
  },
  // Coffee
  {
    description: "PRET A MANGER",
    normalized: "Pret A Manger",
    category: "Food: Coffee & Cafes",
    category_source: "rule",
    confidence: null,
    account: "monzo-current",
    is_recurring: false,
    amountMin: 3.50,
    amountMax: 7.50,
    isIncome: false,
    frequency: "weekly",
  },
  {
    description: "COSTA COFFEE",
    normalized: "Costa Coffee",
    category: "Food: Coffee & Cafes",
    category_source: "rule",
    confidence: null,
    account: "revolut-current",
    is_recurring: false,
    amountMin: 3,
    amountMax: 6,
    isIncome: false,
    frequency: "weekly",
  },
  // Transport
  {
    description: "TFL TRAVEL CHARGE",
    normalized: "TfL",
    category: "Transport: Public Transit",
    category_source: "rule",
    confidence: null,
    account: "monzo-current",
    is_recurring: false,
    amountMin: 2.50,
    amountMax: 8.50,
    isIncome: false,
    frequency: "weekly",
  },
  {
    description: "UBER *TRIP",
    normalized: "Uber",
    category: "Transport: Taxi & Rideshare",
    category_source: "claude",
    confidence: 0.97,
    account: "revolut-current",
    is_recurring: false,
    amountMin: 8,
    amountMax: 25,
    isIncome: false,
    frequency: "occasional",
  },
  // Shopping
  {
    description: "AMAZON UK",
    normalized: "Amazon",
    category: "Shopping: General",
    category_source: "claude",
    confidence: 0.72,
    account: "monzo-current",
    is_recurring: false,
    amountMin: 8,
    amountMax: 120,
    isIncome: false,
    frequency: "occasional",
  },
  {
    description: "UNIQLO UK",
    normalized: "Uniqlo",
    category: "Shopping: Clothing",
    category_source: "claude",
    confidence: 0.85,
    account: "revolut-current",
    is_recurring: false,
    amountMin: 25,
    amountMax: 100,
    isIncome: false,
    frequency: "occasional",
  },
  // Uncategorized
  {
    description: "PAYPAL *UNKNOWN",
    normalized: "PayPal Payment",
    category: null,
    category_source: null,
    confidence: null,
    account: "monzo-current",
    is_recurring: false,
    amountMin: 5,
    amountMax: 50,
    isIncome: false,
    frequency: "occasional",
  },
  {
    description: "SQ *MARKET STALL",
    normalized: "Square Payment",
    category: null,
    category_source: null,
    confidence: null,
    account: "revolut-current",
    is_recurring: false,
    amountMin: 3,
    amountMax: 20,
    isIncome: false,
    frequency: "occasional",
  },
  // Haircut
  {
    description: "THE BARBER SHOP",
    normalized: "Barber",
    category: "Personal Care: Haircut & Beauty",
    category_source: "manual",
    confidence: null,
    account: "monzo-current",
    is_recurring: false,
    amountMin: 25,
    amountMax: 35,
    isIncome: false,
    frequency: "occasional",
  },
]

const MONTHS = [
  "2025-10",
  "2025-11",
  "2025-12",
  "2026-01",
  "2026-02",
  "2026-03",
]

function daysInMonth(ym: string): number {
  const [y, m] = ym.split("-").map(Number)
  return new Date(y, m, 0).getDate()
}

function generateTransactions(): Transaction[] {
  const transactions: Transaction[] = []
  const rand = seededRandom(42)
  let counter = 1

  for (const month of MONTHS) {
    const dim = daysInMonth(month)

    // Recurring transactions
    for (const t of RECURRING_TEMPLATES) {
      const day = Math.min(t.dayOfMonth ?? 1, dim)
      const date = `${month}-${day.toString().padStart(2, "0")}`
      const amount = randomAmount(rand, t.amountMin, t.amountMax)

      transactions.push({
        id: makeId(counter),
        date,
        description: t.description,
        normalized: t.normalized,
        amount: t.isIncome ? amount : `-${amount}`,
        currency: "GBP",
        account_id: t.account,
        category: t.category,
        category_source: t.category_source,
        confidence: t.confidence,
        notes: null,
        is_recurring: t.is_recurring,
        fingerprint: makeFingerprint(counter),
        fitid: null,
      })
      counter++
    }

    // Variable transactions
    for (const t of VARIABLE_TEMPLATES) {
      let occurrences: number
      switch (t.frequency) {
        case "weekly":
          occurrences = 4
          break
        case "biweekly":
          occurrences = 2
          break
        case "occasional":
          occurrences = rand() > 0.5 ? 1 : 0
          break
        default:
          occurrences = 1
      }

      for (let j = 0; j < occurrences; j++) {
        const day = Math.max(1, Math.min(dim, Math.floor(rand() * dim) + 1))
        const date = `${month}-${day.toString().padStart(2, "0")}`
        const amount = randomAmount(rand, t.amountMin, t.amountMax)

        transactions.push({
          id: makeId(counter),
          date,
          description: t.description,
          normalized: t.normalized,
          amount: t.isIncome ? amount : `-${amount}`,
          currency: "GBP",
          account_id: t.account,
          category: t.category,
          category_source: t.category_source,
          confidence: t.confidence,
          notes: null,
          is_recurring: t.is_recurring,
          fingerprint: makeFingerprint(counter),
          fitid: null,
        })
        counter++
      }
    }
  }

  // Sort by date descending
  transactions.sort((a, b) => b.date.localeCompare(a.date))

  return transactions
}

export const MOCK_TRANSACTIONS: Transaction[] = generateTransactions()
