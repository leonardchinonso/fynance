# Transaction Categorization

## Category Taxonomy

Start with these 25 categories (inspired by YNAB + personal finance standards). Expand later.

```rust
pub const CATEGORIES: &[&str] = &[
    // Income
    "Income: Salary",
    "Income: Transfer",
    "Income: Refund",
    // Fixed Expenses
    "Housing: Rent/Mortgage",
    "Housing: Utilities",
    "Housing: Insurance",
    // Variable Necessities
    "Food: Groceries",
    "Food: Dining & Bars",
    "Food: Coffee",
    // Transportation
    "Transport: Gas",
    "Transport: Parking & Tolls",
    "Transport: Rideshare & Transit",
    // Health
    "Health: Medical & Dental",
    "Health: Pharmacy",
    "Health: Fitness",
    // Digital
    "Digital: Subscriptions",
    "Digital: Apps & Software",
    // Shopping
    "Shopping: Clothing",
    "Shopping: Electronics",
    "Shopping: Amazon & Online",
    // Life
    "Life: Entertainment",
    "Life: Travel",
    "Life: Personal Care",
    // Finance
    "Finance: Fees & Interest",
    "Finance: Internal Transfer",
];
```

## Layer 1: Rule-Based (Regex)

Free, instant, handles ~65% of transactions. Rules are maintained in `config/rules.yaml` for easy editing without recompilation.

```yaml
# config/rules.yaml
rules:
  - pattern: '(?i)\b(WHOLE FOODS|TRADER JOE|KROGER|SAFEWAY|COSTCO|SPROUTS|ALDI|PUBLIX)\b'
    category: "Food: Groceries"
    confidence: 0.95
  - pattern: '(?i)\b(STARBUCKS|DUNKIN|BLUE BOTTLE|PHILZ|PEETS)\b'
    category: "Food: Coffee"
    confidence: 0.95
  - pattern: '(?i)\b(DOORDASH|UBER EATS|GRUBHUB|SEAMLESS|CHIPOTLE|MCDONALD|SUBWAY)\b'
    category: "Food: Dining & Bars"
    confidence: 0.90
  - pattern: '(?i)\b(SHELL|BP|CHEVRON|MOBIL|ARCO|EXXON|SUNOCO)\b'
    category: "Transport: Gas"
    confidence: 0.95
  - pattern: '(?i)\bUBER\*(?!EATS)|\bLYFT\b'
    category: "Transport: Rideshare & Transit"
    confidence: 0.95
  - pattern: '(?i)\b(SPOTIFY|NETFLIX|HULU|DISNEY\+|APPLE\.COM/BILL|AMAZON PRIME|HBO)\b'
    category: "Digital: Subscriptions"
    confidence: 0.95
  - pattern: '(?i)\bAMZN\*|AMAZON\.(COM|MKTPLACE)\b'
    category: "Shopping: Amazon & Online"
    confidence: 0.85
  - pattern: '(?i)\b(CVS|WALGREENS|RITE AID|WALMART PHARMACY)\b'
    category: "Health: Pharmacy"
    confidence: 0.90
  - pattern: '(?i)\b(ZELLE|VENMO|PAYPAL TRANSFER|ACH TRANSFER|WIRE TRANSFER)\b'
    category: "Finance: Internal Transfer"
    confidence: 0.90
  - pattern: '(?i)\b(MONTHLY FEE|OVERDRAFT|LATE FEE|ANNUAL FEE|ATM FEE)\b'
    category: "Finance: Fees & Interest"
    confidence: 0.95
```

```rust
use regex::Regex;
use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct RuleConfig {
    pub rules: Vec<RuleDef>,
}

#[derive(Debug, Deserialize)]
pub struct RuleDef {
    pub pattern: String,
    pub category: String,
    pub confidence: f64,
}

pub struct CompiledRule {
    pub regex: Regex,
    pub category: String,
    pub confidence: f64,
}

pub fn load_rules(path: &Path) -> anyhow::Result<Vec<CompiledRule>> {
    let text = fs::read_to_string(path)?;
    let config: RuleConfig = serde_yaml::from_str(&text)?;
    let mut compiled = Vec::with_capacity(config.rules.len());
    for rule in config.rules {
        compiled.push(CompiledRule {
            regex: Regex::new(&rule.pattern)?,
            category: rule.category,
            confidence: rule.confidence,
        });
    }
    Ok(compiled)
}

pub fn rule_categorize<'a>(
    description: &str,
    rules: &'a [CompiledRule],
) -> Option<(&'a str, f64)> {
    for rule in rules {
        if rule.regex.is_match(description) {
            return Some((&rule.category, rule.confidence));
        }
    }
    None
}
```

## Layer 2: Claude API (Few-Shot)

For transactions where rule confidence is below 0.85. Use the Batch API for historical imports (50% cost reduction).

### System Prompt and Few-Shot

```rust
pub const SYSTEM_PROMPT: &str = r#"You are a personal finance transaction categorizer. You will be given a bank transaction description and must assign it to exactly one category.

Categories (use exactly as written):
- Food: Groceries
- Food: Dining & Bars
- Food: Coffee
- Transport: Gas
- Transport: Rideshare & Transit
- Transport: Parking & Tolls
- Health: Medical & Dental
- Health: Pharmacy
- Health: Fitness
- Housing: Rent/Mortgage
- Housing: Utilities
- Housing: Insurance
- Digital: Subscriptions
- Digital: Apps & Software
- Shopping: Clothing
- Shopping: Electronics
- Shopping: Amazon & Online
- Life: Entertainment
- Life: Travel
- Life: Personal Care
- Finance: Internal Transfer
- Finance: Fees & Interest
- Income: Salary
- Income: Refund
- Income: Transfer

Respond ONLY with valid JSON: {"category": "...", "confidence": 0.0}"#;

pub const FEW_SHOT: &str = r#"Examples:
"TRADER JOES #123 MENLO PARK CA" -> {"category": "Food: Groceries", "confidence": 0.98}
"STARBUCKS STORE 12345" -> {"category": "Food: Coffee", "confidence": 0.97}
"DOORDASH*CHIPOTLE" -> {"category": "Food: Dining & Bars", "confidence": 0.95}
"CHEVRON 00123456" -> {"category": "Transport: Gas", "confidence": 0.96}
"LYFT *RIDE SUN 3PM" -> {"category": "Transport: Rideshare & Transit", "confidence": 0.97}
"NETFLIX.COM" -> {"category": "Digital: Subscriptions", "confidence": 0.99}
"AMZN MKTP US*1A2B3C" -> {"category": "Shopping: Amazon & Online", "confidence": 0.90}
"CVS/PHARMACY #4567" -> {"category": "Health: Pharmacy", "confidence": 0.93}
"KAISER PERMANENTE" -> {"category": "Health: Medical & Dental", "confidence": 0.96}
"ZELLE PAYMENT TO JOHN S" -> {"category": "Finance: Internal Transfer", "confidence": 0.90}
"PAYROLL DIRECT DEPOSIT" -> {"category": "Income: Salary", "confidence": 0.99}"#;
```

See `research/07_claude_api.md` for full HTTP client code.

## Hybrid Pipeline

```
Transaction Description
        |
        v
[Rule-Based Regex]
        |
   confidence >= 0.85?
   /           \
 Yes            No
  |              |
  v              v
DONE        [Claude API]
                 |
            confidence >= 0.80?
            /           \
          Yes             No
           |               |
           v               v
         DONE        [Flag for manual review]
```

```rust
pub enum Outcome {
    Categorized { category: String, confidence: f64, source: &'static str },
    NeedsReview { suggested: String, confidence: f64 },
}

pub async fn run_pipeline(
    description: &str,
    rules: &[CompiledRule],
    http: &reqwest::Client,
    api_key: &str,
) -> anyhow::Result<Outcome> {
    // Layer 1
    if let Some((cat, conf)) = rule_categorize(description, rules) {
        if conf >= 0.85 {
            return Ok(Outcome::Categorized {
                category: cat.to_string(),
                confidence: conf,
                source: "rules",
            });
        }
    }

    // Layer 2
    let result = crate::categorizer::claude::categorize_one(
        http, api_key, description, SYSTEM_PROMPT, FEW_SHOT
    ).await?;

    if result.confidence >= 0.75 {
        Ok(Outcome::Categorized {
            category: result.category,
            confidence: result.confidence,
            source: "claude",
        })
    } else {
        Ok(Outcome::NeedsReview {
            suggested: result.category,
            confidence: result.confidence,
        })
    }
}
```

## Manual Review Queue

Track uncategorized or low-confidence transactions for user correction. Stored in the `review_queue` SQLite table. Interactive review command reads from this table and writes back the user's corrections.

## Cost Estimates

Assuming 3 years of statements, ~100 transactions/month per account, 2 accounts = ~7,200 transactions.

| Layer | Transactions | Cost |
|---|---|---|
| Rules (free) | ~4,700 (65%) | $0.00 |
| Claude Haiku Batch | ~2,500 (35%) | ~$0.25 |
| Total | 7,200 | ~$0.25 |

Ongoing monthly cost: ~$0.01-0.05/month.
