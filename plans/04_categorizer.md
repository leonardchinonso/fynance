# Categorization System

> **Deferred.** Categorization is not part of the current implementation. Once CSV import and SQLite storage are working, categorization will be added iteratively, starting with a regex rule engine before considering any AI-based approach.

## Planned Pipeline (future)

```
description (normalized)
        |
        v
[rules.rs: regex match]
        |
   confidence >= 0.85?
   /            \
 Yes             No
  |               |
  v               v
DONE         [review_queue in db]
```

The rule engine reads patterns from `config/rules.yaml` and matches against normalized transaction descriptions. Transactions that do not match any rule go into a review queue for manual categorization.

AI-based categorization (Claude Haiku via the Anthropic API) may be added after the rule engine is in place, as a fallback for low-confidence matches.

## Category Taxonomy (`config/categories.yaml`)

```yaml
categories:
  income:
    - "Income: Salary"
    - "Income: Refund"
    - "Income: Transfer In"

  fixed:
    - "Housing: Rent/Mortgage"
    - "Housing: Utilities"
    - "Housing: Insurance"
    - "Finance: Loan Payment"

  variable_needs:
    - "Food: Groceries"
    - "Transport: Gas"
    - "Transport: Rideshare & Transit"
    - "Transport: Parking & Tolls"
    - "Health: Medical & Dental"
    - "Health: Pharmacy"
    - "Health: Fitness"

  discretionary:
    - "Food: Dining & Bars"
    - "Food: Coffee"
    - "Digital: Subscriptions"
    - "Digital: Apps & Software"
    - "Shopping: Clothing"
    - "Shopping: Electronics"
    - "Shopping: Amazon & Online"
    - "Life: Entertainment"
    - "Life: Travel"
    - "Life: Personal Care"

  financial:
    - "Finance: Internal Transfer"
    - "Finance: Fees & Interest"
    - "Finance: Investment"

  other:
    - "Other"
```
