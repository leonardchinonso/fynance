import type { AccountType } from "@/bindings/AccountType"
import type { AssetClass } from "@/bindings/AssetClass"

export function accountTypeToAssetClass(t: AccountType): AssetClass {
  switch (t) {
    case "investment":
    case "investment_isa": return "Investments"
    case "pension":        return "Pension"
    case "property":       return "Property"
    case "checking":
    case "savings":
    case "cash":
    case "credit":         return "Cash"
  }
}
