import type { Budget } from "@/types"

// Standing budget targets applied to each month
const BUDGET_TARGETS: { category: string; amount: string }[] = [
  { category: "Housing: Rent / Mortgage", amount: "1416.10" },
  { category: "Housing: Utilities", amount: "120.00" },
  { category: "Housing: Internet & Phone", amount: "45.00" },
  { category: "Food: Groceries", amount: "300.00" },
  { category: "Food: Dining & Bars", amount: "150.00" },
  { category: "Food: Coffee & Cafes", amount: "40.00" },
  { category: "Transport: Public Transit", amount: "80.00" },
  { category: "Transport: Taxi & Rideshare", amount: "50.00" },
  { category: "Health: Gym & Fitness", amount: "50.00" },
  { category: "Entertainment: Streaming Services", amount: "30.00" },
  { category: "Shopping: General", amount: "100.00" },
  { category: "Shopping: Clothing", amount: "80.00" },
  { category: "Finance: Insurance", amount: "35.00" },
  { category: "Personal Care: Haircut & Beauty", amount: "30.00" },
  { category: "Travel: Accommodation", amount: "200.00" },
]

const MONTHS = [
  "2025-10",
  "2025-11",
  "2025-12",
  "2026-01",
  "2026-02",
  "2026-03",
]

export const MOCK_BUDGETS: Budget[] = MONTHS.flatMap((month) =>
  BUDGET_TARGETS.map((t) => ({
    month,
    category: t.category,
    amount: t.amount,
  }))
)
