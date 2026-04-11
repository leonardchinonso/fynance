// Category taxonomy matching docs/design/03_data_model.md

export const CATEGORY_TAXONOMY: Record<string, string[]> = {
  Income: ["Salary", "Freelance", "Investments", "Other Income"],
  Housing: ["Rent / Mortgage", "Utilities", "Internet & Phone", "Home Maintenance"],
  Food: ["Groceries", "Dining & Bars", "Coffee & Cafes"],
  Transport: ["Public Transit", "Taxi & Rideshare", "Fuel", "Car Maintenance"],
  Health: ["Gym & Fitness", "Medical & Dental", "Pharmacy"],
  Shopping: ["Clothing", "Electronics", "General"],
  Entertainment: ["Streaming Services", "Events & Concerts", "Hobbies"],
  Travel: ["Flights", "Accommodation", "Holiday Spending"],
  Finance: ["Savings Transfer", "Investment Transfer", "Fees & Charges", "Insurance"],
  "Personal Care": ["Haircut & Beauty"],
  "Gifts & Donations": ["Gifts"],
  Education: ["Courses & Books"],
  Other: ["Uncategorized"],
}

export const PARENT_CATEGORIES = Object.keys(CATEGORY_TAXONOMY)

export const ALL_CATEGORIES: string[] = Object.entries(CATEGORY_TAXONOMY).flatMap(
  ([parent, children]) => children.map((child) => `${parent}: ${child}`)
)
