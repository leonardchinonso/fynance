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
  category_source: "rule" | "agent" | "manual" | null
  confidence: number | null
  account: string
  is_recurring: boolean
  amountMin: number
  amountMax: number
  isIncome: boolean
  frequency: "monthly" | "weekly" | "biweekly" | "occasional"
  dayOfMonth?: number
}

// ── Alex's recurring transactions ──
const ALEX_RECURRING: TxTemplate[] = [
  { description: "COMPANY LTD SALARY", normalized: "Salary", category: "Income: Salary", category_source: "rule", confidence: null, account: "monzo-current", is_recurring: true, amountMin: 4200, amountMax: 4200, isIncome: true, frequency: "monthly", dayOfMonth: 28 },
  { description: "MORTGAGE DIRECT DEBIT", normalized: "Mortgage Payment", category: "Housing: Rent / Mortgage", category_source: "rule", confidence: null, account: "monzo-current", is_recurring: true, amountMin: 1416.10, amountMax: 1416.10, isIncome: false, frequency: "monthly", dayOfMonth: 1 },
  { description: "BRITISH GAS", normalized: "British Gas", category: "Housing: Utilities", category_source: "rule", confidence: null, account: "monzo-current", is_recurring: true, amountMin: 85, amountMax: 140, isIncome: false, frequency: "monthly", dayOfMonth: 5 },
  { description: "VIRGIN MEDIA", normalized: "Virgin Media", category: "Housing: Internet & Phone", category_source: "rule", confidence: null, account: "monzo-current", is_recurring: true, amountMin: 42, amountMax: 42, isIncome: false, frequency: "monthly", dayOfMonth: 12 },
  { description: "NETFLIX.COM", normalized: "Netflix", category: "Entertainment: Streaming Services", category_source: "rule", confidence: null, account: "revolut-current", is_recurring: true, amountMin: 15.99, amountMax: 15.99, isIncome: false, frequency: "monthly", dayOfMonth: 15 },
  { description: "SPOTIFY UK", normalized: "Spotify", category: "Entertainment: Streaming Services", category_source: "rule", confidence: null, account: "revolut-current", is_recurring: true, amountMin: 10.99, amountMax: 10.99, isIncome: false, frequency: "monthly", dayOfMonth: 18 },
  { description: "PUREGYM LTD", normalized: "PureGym", category: "Health: Gym & Fitness", category_source: "rule", confidence: null, account: "monzo-current", is_recurring: true, amountMin: 45, amountMax: 45, isIncome: false, frequency: "monthly", dayOfMonth: 1 },
  { description: "AVIVA INSURANCE", normalized: "Home Insurance", category: "Finance: Insurance", category_source: "rule", confidence: null, account: "monzo-current", is_recurring: true, amountMin: 32.50, amountMax: 32.50, isIncome: false, frequency: "monthly", dayOfMonth: 8 },
  { description: "PENSION CONTRIBUTION", normalized: "Pension Contribution", category: "Finance: Investment Transfer", category_source: "rule", confidence: null, account: "monzo-current", is_recurring: true, amountMin: 500, amountMax: 500, isIncome: false, frequency: "monthly", dayOfMonth: 28 },
  { description: "SAVINGS TRANSFER", normalized: "Savings Transfer", category: "Finance: Savings Transfer", category_source: "rule", confidence: null, account: "monzo-current", is_recurring: true, amountMin: 400, amountMax: 400, isIncome: false, frequency: "monthly", dayOfMonth: 29 },
  { description: "TRADING 212 DEPOSIT", normalized: "Investment Deposit", category: "Finance: Investment Transfer", category_source: "rule", confidence: null, account: "monzo-current", is_recurring: true, amountMin: 500, amountMax: 500, isIncome: false, frequency: "monthly", dayOfMonth: 29 },
  { description: "COUNCIL TAX", normalized: "Council Tax", category: "Housing: Utilities", category_source: "rule", confidence: null, account: "monzo-current", is_recurring: true, amountMin: 165, amountMax: 165, isIncome: false, frequency: "monthly", dayOfMonth: 3 },
  { description: "WATER BILL DD", normalized: "Thames Water", category: "Housing: Utilities", category_source: "rule", confidence: null, account: "monzo-current", is_recurring: true, amountMin: 38, amountMax: 38, isIncome: false, frequency: "monthly", dayOfMonth: 7 },
  { description: "RENTAL INCOME TENANT", normalized: "Rental Income", category: "Income: Other Income", category_source: "rule", confidence: null, account: "monzo-current", is_recurring: true, amountMin: 300, amountMax: 300, isIncome: true, frequency: "monthly", dayOfMonth: 5 },
]

// ── Sam's recurring transactions ──
const SAM_RECURRING: TxTemplate[] = [
  { description: "EMPLOYER LTD SALARY", normalized: "Salary", category: "Income: Salary", category_source: "rule", confidence: null, account: "monzo-sam", is_recurring: true, amountMin: 1800, amountMax: 1800, isIncome: true, frequency: "monthly", dayOfMonth: 25 },
  { description: "DISNEY+ SUBSCRIPTION", normalized: "Disney+", category: "Entertainment: Streaming Services", category_source: "rule", confidence: null, account: "monzo-sam", is_recurring: true, amountMin: 10.99, amountMax: 10.99, isIncome: false, frequency: "monthly", dayOfMonth: 20 },
  { description: "YOUTUBE PREMIUM", normalized: "YouTube Premium", category: "Entertainment: Streaming Services", category_source: "rule", confidence: null, account: "monzo-sam", is_recurring: true, amountMin: 12.99, amountMax: 12.99, isIncome: false, frequency: "monthly", dayOfMonth: 22 },
  { description: "DAVID LLOYD GYM", normalized: "David Lloyd", category: "Health: Gym & Fitness", category_source: "rule", confidence: null, account: "monzo-sam", is_recurring: true, amountMin: 89, amountMax: 89, isIncome: false, frequency: "monthly", dayOfMonth: 1 },
  { description: "T212 INVEST", normalized: "Investment Deposit", category: "Finance: Investment Transfer", category_source: "rule", confidence: null, account: "monzo-sam", is_recurring: true, amountMin: 400, amountMax: 400, isIncome: false, frequency: "monthly", dayOfMonth: 26 },
  { description: "PENSION SALARY DEDUCT", normalized: "Pension Contribution", category: "Finance: Investment Transfer", category_source: "rule", confidence: null, account: "monzo-sam", is_recurring: true, amountMin: 350, amountMax: 350, isIncome: false, frequency: "monthly", dayOfMonth: 25 },
]

// ── Joint account recurring ──
const JOINT_RECURRING: TxTemplate[] = [
  { description: "JOINT GROCERIES TESCO", normalized: "Tesco (Joint)", category: "Food: Groceries", category_source: "rule", confidence: null, account: "joint-current", is_recurring: false, amountMin: 60, amountMax: 150, isIncome: false, frequency: "weekly" },
  { description: "JOINT ALEX CONTRIB", normalized: "Alex Contribution", category: "Income: Other Income", category_source: "rule", confidence: null, account: "joint-current", is_recurring: true, amountMin: 800, amountMax: 800, isIncome: true, frequency: "monthly", dayOfMonth: 1 },
  { description: "JOINT SAM CONTRIB", normalized: "Sam Contribution", category: "Income: Other Income", category_source: "rule", confidence: null, account: "joint-current", is_recurring: true, amountMin: 800, amountMax: 800, isIncome: true, frequency: "monthly", dayOfMonth: 1 },
  { description: "JOINT SAVINGS XFER", normalized: "Joint Savings", category: "Finance: Savings Transfer", category_source: "rule", confidence: null, account: "joint-current", is_recurring: true, amountMin: 500, amountMax: 500, isIncome: false, frequency: "monthly", dayOfMonth: 2 },
]

// ── Alex's variable spending ──
const ALEX_VARIABLE: TxTemplate[] = [
  { description: "LIDL GB LONDON", normalized: "Lidl", category: "Food: Groceries", category_source: "rule", confidence: null, account: "monzo-current", is_recurring: false, amountMin: 15, amountMax: 75, isIncome: false, frequency: "weekly" },
  { description: "SAINSBURYS", normalized: "Sainsbury's", category: "Food: Groceries", category_source: "agent", confidence: 0.92, account: "revolut-current", is_recurring: false, amountMin: 10, amountMax: 60, isIncome: false, frequency: "biweekly" },
  { description: "NANDOS", normalized: "Nando's", category: "Food: Dining & Bars", category_source: "agent", confidence: 0.95, account: "revolut-current", is_recurring: false, amountMin: 15, amountMax: 40, isIncome: false, frequency: "biweekly" },
  { description: "WAGAMAMA", normalized: "Wagamama", category: "Food: Dining & Bars", category_source: "agent", confidence: 0.91, account: "revolut-current", is_recurring: false, amountMin: 18, amountMax: 35, isIncome: false, frequency: "occasional" },
  { description: "PIZZA EXPRESS", normalized: "Pizza Express", category: "Food: Dining & Bars", category_source: "agent", confidence: 0.88, account: "monzo-current", is_recurring: false, amountMin: 25, amountMax: 55, isIncome: false, frequency: "occasional" },
  { description: "PRET A MANGER", normalized: "Pret A Manger", category: "Food: Coffee & Cafes", category_source: "rule", confidence: null, account: "monzo-current", is_recurring: false, amountMin: 3.50, amountMax: 7.50, isIncome: false, frequency: "weekly" },
  { description: "COSTA COFFEE", normalized: "Costa Coffee", category: "Food: Coffee & Cafes", category_source: "rule", confidence: null, account: "revolut-current", is_recurring: false, amountMin: 3, amountMax: 6, isIncome: false, frequency: "weekly" },
  { description: "TFL TRAVEL CHARGE", normalized: "TfL", category: "Transport: Public Transit", category_source: "rule", confidence: null, account: "monzo-current", is_recurring: false, amountMin: 2.50, amountMax: 8.50, isIncome: false, frequency: "weekly" },
  { description: "UBER *TRIP", normalized: "Uber", category: "Transport: Taxi & Rideshare", category_source: "agent", confidence: 0.97, account: "revolut-current", is_recurring: false, amountMin: 8, amountMax: 25, isIncome: false, frequency: "occasional" },
  { description: "AMAZON UK", normalized: "Amazon", category: "Shopping: General", category_source: "agent", confidence: 0.72, account: "monzo-current", is_recurring: false, amountMin: 8, amountMax: 120, isIncome: false, frequency: "occasional" },
  { description: "UNIQLO UK", normalized: "Uniqlo", category: "Shopping: Clothing", category_source: "agent", confidence: 0.85, account: "revolut-current", is_recurring: false, amountMin: 25, amountMax: 100, isIncome: false, frequency: "occasional" },
  { description: "THE BARBER SHOP", normalized: "Barber", category: "Personal Care: Haircut & Beauty", category_source: "manual", confidence: null, account: "monzo-current", is_recurring: false, amountMin: 25, amountMax: 35, isIncome: false, frequency: "occasional" },
  { description: "BOOTS PHARMACY", normalized: "Boots", category: "Health: Pharmacy", category_source: "agent", confidence: 0.90, account: "monzo-current", is_recurring: false, amountMin: 5, amountMax: 30, isIncome: false, frequency: "occasional" },
  { description: "WATERSTONES", normalized: "Waterstones", category: "Education: Courses & Books", category_source: "agent", confidence: 0.82, account: "revolut-current", is_recurring: false, amountMin: 8, amountMax: 25, isIncome: false, frequency: "occasional" },
  { description: "ODEON CINEMA", normalized: "Odeon Cinema", category: "Entertainment: Events & Concerts", category_source: "agent", confidence: 0.93, account: "monzo-current", is_recurring: false, amountMin: 10, amountMax: 20, isIncome: false, frequency: "occasional" },
  { description: "PAYPAL *UNKNOWN", normalized: "PayPal Payment", category: null, category_source: null, confidence: null, account: "monzo-current", is_recurring: false, amountMin: 5, amountMax: 50, isIncome: false, frequency: "occasional" },
  { description: "APPLE.COM/BILL", normalized: "Apple", category: "Shopping: Electronics", category_source: "agent", confidence: 0.78, account: "revolut-current", is_recurring: false, amountMin: 0.99, amountMax: 12.99, isIncome: false, frequency: "occasional" },
]

// ── Sam's variable spending ──
const SAM_VARIABLE: TxTemplate[] = [
  { description: "TESCO STORES", normalized: "Tesco", category: "Food: Groceries", category_source: "rule", confidence: null, account: "monzo-sam", is_recurring: false, amountMin: 20, amountMax: 90, isIncome: false, frequency: "weekly" },
  { description: "ALDI STORES UK", normalized: "Aldi", category: "Food: Groceries", category_source: "agent", confidence: 0.94, account: "monzo-sam", is_recurring: false, amountMin: 15, amountMax: 65, isIncome: false, frequency: "biweekly" },
  { description: "FIVE GUYS", normalized: "Five Guys", category: "Food: Dining & Bars", category_source: "agent", confidence: 0.96, account: "monzo-sam", is_recurring: false, amountMin: 12, amountMax: 28, isIncome: false, frequency: "biweekly" },
  { description: "STARBUCKS", normalized: "Starbucks", category: "Food: Coffee & Cafes", category_source: "rule", confidence: null, account: "monzo-sam", is_recurring: false, amountMin: 4, amountMax: 8, isIncome: false, frequency: "weekly" },
  { description: "TFL CONTACTLESS", normalized: "TfL", category: "Transport: Public Transit", category_source: "rule", confidence: null, account: "monzo-sam", is_recurring: false, amountMin: 3, amountMax: 9, isIncome: false, frequency: "weekly" },
  { description: "BOLT RIDE", normalized: "Bolt", category: "Transport: Taxi & Rideshare", category_source: "agent", confidence: 0.94, account: "monzo-sam", is_recurring: false, amountMin: 6, amountMax: 20, isIncome: false, frequency: "occasional" },
  { description: "ZARA UK", normalized: "Zara", category: "Shopping: Clothing", category_source: "agent", confidence: 0.87, account: "monzo-sam", is_recurring: false, amountMin: 20, amountMax: 80, isIncome: false, frequency: "occasional" },
  { description: "H&M ONLINE", normalized: "H&M", category: "Shopping: Clothing", category_source: "agent", confidence: 0.89, account: "monzo-sam", is_recurring: false, amountMin: 15, amountMax: 60, isIncome: false, frequency: "occasional" },
  { description: "JOHN LEWIS", normalized: "John Lewis", category: "Shopping: General", category_source: "agent", confidence: 0.80, account: "monzo-sam", is_recurring: false, amountMin: 15, amountMax: 150, isIncome: false, frequency: "occasional" },
  { description: "SUPERDRUG", normalized: "Superdrug", category: "Health: Pharmacy", category_source: "agent", confidence: 0.88, account: "monzo-sam", is_recurring: false, amountMin: 4, amountMax: 25, isIncome: false, frequency: "occasional" },
  { description: "SALON HAIRCUT", normalized: "Hair Salon", category: "Personal Care: Haircut & Beauty", category_source: "manual", confidence: null, account: "monzo-sam", is_recurring: false, amountMin: 40, amountMax: 80, isIncome: false, frequency: "occasional" },
  { description: "UDEMY COURSE", normalized: "Udemy", category: "Education: Courses & Books", category_source: "agent", confidence: 0.91, account: "monzo-sam", is_recurring: false, amountMin: 10, amountMax: 50, isIncome: false, frequency: "occasional" },
  { description: "TICKETMASTER UK", normalized: "Ticketmaster", category: "Entertainment: Events & Concerts", category_source: "agent", confidence: 0.95, account: "monzo-sam", is_recurring: false, amountMin: 30, amountMax: 120, isIncome: false, frequency: "occasional" },
  { description: "SQ *MARKET STALL", normalized: "Square Payment", category: null, category_source: null, confidence: null, account: "monzo-sam", is_recurring: false, amountMin: 3, amountMax: 20, isIncome: false, frequency: "occasional" },
  { description: "GIFT CARD PURCHASE", normalized: "Gift", category: "Gifts & Donations: Gifts", category_source: "manual", confidence: null, account: "monzo-sam", is_recurring: false, amountMin: 20, amountMax: 50, isIncome: false, frequency: "occasional" },
]

// ── Occasional one-off templates (shared) ──
const OCCASIONAL_ALEX: TxTemplate[] = [
  { description: "EASYJET FLIGHTS", normalized: "EasyJet", category: "Travel: Flights", category_source: "agent", confidence: 0.98, account: "revolut-current", is_recurring: false, amountMin: 60, amountMax: 250, isIncome: false, frequency: "occasional" },
  { description: "BOOKING.COM", normalized: "Booking.com", category: "Travel: Accommodation", category_source: "agent", confidence: 0.96, account: "revolut-current", is_recurring: false, amountMin: 80, amountMax: 300, isIncome: false, frequency: "occasional" },
  { description: "CURRYS PC WORLD", normalized: "Currys", category: "Shopping: Electronics", category_source: "agent", confidence: 0.90, account: "monzo-current", is_recurring: false, amountMin: 30, amountMax: 400, isIncome: false, frequency: "occasional" },
  { description: "FREELANCE PAYMENT", normalized: "Freelance Income", category: "Income: Freelance", category_source: "manual", confidence: null, account: "revolut-current", is_recurring: false, amountMin: 200, amountMax: 800, isIncome: true, frequency: "occasional" },
]

const OCCASIONAL_SAM: TxTemplate[] = [
  { description: "RYANAIR", normalized: "Ryanair", category: "Travel: Flights", category_source: "agent", confidence: 0.97, account: "monzo-sam", is_recurring: false, amountMin: 40, amountMax: 200, isIncome: false, frequency: "occasional" },
  { description: "AIRBNB", normalized: "Airbnb", category: "Travel: Accommodation", category_source: "agent", confidence: 0.95, account: "monzo-sam", is_recurring: false, amountMin: 60, amountMax: 250, isIncome: false, frequency: "occasional" },
  { description: "HOLIDAY SPENDING", normalized: "Holiday Spending", category: "Travel: Holiday Spending", category_source: "manual", confidence: null, account: "monzo-sam", is_recurring: false, amountMin: 20, amountMax: 100, isIncome: false, frequency: "occasional" },
]

const MONTHS: string[] = []
for (let y = 2023; y <= 2026; y++) {
  const maxMonth = y === 2026 ? 3 : 12
  for (let m = 1; m <= maxMonth; m++) {
    MONTHS.push(`${y}-${m.toString().padStart(2, "0")}`)
  }
}

function daysInMonth(ym: string): number {
  const [y, m] = ym.split("-").map(Number)
  return new Date(y, m, 0).getDate()
}

function generateFromTemplates(
  templates: TxTemplate[],
  months: string[],
  rand: () => number,
  counterStart: number
): { transactions: Transaction[]; counter: number } {
  const transactions: Transaction[] = []
  let counter = counterStart

  for (const month of months) {
    const dim = daysInMonth(month)

    for (const t of templates) {
      let occurrences: number
      if (t.frequency === "monthly") {
        occurrences = 1
      } else if (t.frequency === "weekly") {
        occurrences = 4
      } else if (t.frequency === "biweekly") {
        occurrences = 2
      } else {
        // occasional: ~40% chance per month
        occurrences = rand() > 0.6 ? 1 : 0
      }

      for (let j = 0; j < occurrences; j++) {
        const day = t.dayOfMonth
          ? Math.min(t.dayOfMonth, dim)
          : Math.max(1, Math.min(dim, Math.floor(rand() * dim) + 1))
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

  return { transactions, counter }
}

function generateTransactions(): Transaction[] {
  const rand = seededRandom(42)
  let counter = 1
  const all: Transaction[] = []

  // Alex recurring + variable + occasional
  const alex1 = generateFromTemplates(ALEX_RECURRING, MONTHS, rand, counter)
  counter = alex1.counter
  all.push(...alex1.transactions)

  const alex2 = generateFromTemplates(ALEX_VARIABLE, MONTHS, rand, counter)
  counter = alex2.counter
  all.push(...alex2.transactions)

  const alex3 = generateFromTemplates(OCCASIONAL_ALEX, MONTHS, rand, counter)
  counter = alex3.counter
  all.push(...alex3.transactions)

  // Sam recurring + variable + occasional
  const sam1 = generateFromTemplates(SAM_RECURRING, MONTHS, rand, counter)
  counter = sam1.counter
  all.push(...sam1.transactions)

  const sam2 = generateFromTemplates(SAM_VARIABLE, MONTHS, rand, counter)
  counter = sam2.counter
  all.push(...sam2.transactions)

  const sam3 = generateFromTemplates(OCCASIONAL_SAM, MONTHS, rand, counter)
  counter = sam3.counter
  all.push(...sam3.transactions)

  // Joint account
  const joint = generateFromTemplates(JOINT_RECURRING, MONTHS, rand, counter)
  all.push(...joint.transactions)

  // Sort by date descending
  all.sort((a, b) => b.date.localeCompare(a.date))

  return all
}

export const MOCK_TRANSACTIONS: Transaction[] = generateTransactions()
