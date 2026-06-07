# Categorization System

> **Updated after Prompt 1.1.** No longer deferred. Categorization is Phase 5 of the v2 plan and ships with both rules and Claude fallback.

## Pipeline Overview

```
transactions without category
            │
            ▼
  ┌─────────────────────┐
  │  rules.rs           │  YAML patterns matched against normalized description
  │  (fast, free)       │
  └──────────┬──────────┘
             │
        match found?
         /        \
       yes         no
        │           │
        ▼           ▼
    category    ┌───────────────────────┐
    (rule)      │  claude.rs            │  Claude Haiku Batch API
                │  (prompt-cached)      │  (50% cheaper than on-demand)
                └──────────┬────────────┘
                           │
                  confidence >= 0.75?
                    /            \
                  yes             no
                   │               │
                   ▼               ▼
              category         review queue
              (claude)         (surfaced in UI)
```

## Rule Engine (`src/categorizer/rules.rs`)

```rust
use anyhow::Result;
use regex::Regex;
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize)]
pub struct RuleFile {
    pub rules: Vec<RuleEntry>,
}

#[derive(Deserialize)]
pub struct RuleEntry {
    pub pattern: String,
    pub category: String,
    #[serde(default)]
    pub priority: i32,
}

pub struct CompiledRule {
    pub regex: Regex,
    pub category: String,
    pub priority: i32,
}

pub fn load_rules(path: &Path) -> Result<Vec<CompiledRule>> {
    let yaml = std::fs::read_to_string(path)?;
    let file: RuleFile = serde_yaml::from_str(&yaml)?;
    let mut rules: Vec<CompiledRule> = file.rules.into_iter()
        .map(|r| Ok(CompiledRule {
            regex: Regex::new(&format!("(?i){}", r.pattern))?,
            category: r.category,
            priority: r.priority,
        }))
        .collect::<Result<_>>()?;
    rules.sort_by_key(|r| -r.priority);
    Ok(rules)
}

pub fn match_rule<'a>(description: &str, rules: &'a [CompiledRule]) -> Option<&'a str> {
    rules.iter()
        .find(|r| r.regex.is_match(description))
        .map(|r| r.category.as_str())
}
```

### Example `config/rules.yaml`

```yaml
rules:
  # Groceries (UK)
  - pattern: "(?:LIDL|ALDI|TESCO|SAINSBURYS|WAITROSE|M&S FOOD|MARKS.?SPENCER|ASDA|MORRISONS|OCADO)"
    category: "Food: Groceries"
    priority: 10

  # Dining
  - pattern: "(?:PRET|NANDOS|WAGAMAMA|DISHOOM|FRANCO MANCA|DELIVEROO|UBER EATS|JUST EAT|HONEST BURGERS)"
    category: "Food: Dining & Bars"
    priority: 10

  # Coffee
  - pattern: "(?:STARBUCKS|PRET.*COFFEE|COSTA|CAFFE NERO|BLUE BOTTLE|WATCH HOUSE|GRIND)"
    category: "Food: Coffee & Cafes"
    priority: 10

  # Transport
  - pattern: "(?:TFL|TRANSPORT FOR LONDON|UBER|BOLT|TAXI|TRAINLINE|LNER|GWR)"
    category: "Transport: Public Transit"
    priority: 10

  # Utilities & Telecoms
  - pattern: "(?:OCTOPUS ENERGY|BRITISH GAS|EDF|EON|THAMES WATER|VIRGIN MEDIA|BT GROUP|VODAFONE|EE|GIFFGAFF|O2)"
    category: "Housing: Utilities"
    priority: 10

  # Subscriptions
  - pattern: "(?:NETFLIX|SPOTIFY|DISNEY PLUS|APPLE.COM/BILL|AMAZON PRIME|YOUTUBE PREMIUM|PATREON)"
    category: "Entertainment: Streaming Services"
    priority: 10

  # Rent / Mortgage (catch-all; user configures)
  - pattern: "RENT|MORTGAGE"
    category: "Housing: Rent / Mortgage"
    priority: 20

  # Internal transfers between own accounts
  - pattern: "(?:MONZO.*POT|SAVINGS TRANSFER|VAULT|TRANSFER TO)"
    category: "Finance: Savings Transfer"
    priority: 20
```

Rules live in `config/rules.yaml` and are version-controlled. The DB table `category_rules` is a read cache so the UI can display and edit rules later without parsing YAML.

## Category Taxonomy (`config/categories.yaml`)

```yaml
categories:
  Income:
    - "Income: Salary"
    - "Income: Freelance"
    - "Income: Investments"
    - "Income: Other"

  Housing:
    - "Housing: Rent / Mortgage"
    - "Housing: Utilities"
    - "Housing: Internet & Phone"
    - "Housing: Home Maintenance"

  Food:
    - "Food: Groceries"
    - "Food: Dining & Bars"
    - "Food: Coffee & Cafes"

  Transport:
    - "Transport: Public Transit"
    - "Transport: Taxi & Rideshare"
    - "Transport: Fuel"
    - "Transport: Car Maintenance"

  Health:
    - "Health: Gym & Fitness"
    - "Health: Medical & Dental"
    - "Health: Pharmacy"

  Shopping:
    - "Shopping: Clothing"
    - "Shopping: Electronics"
    - "Shopping: General"

  Entertainment:
    - "Entertainment: Streaming Services"
    - "Entertainment: Events & Concerts"
    - "Entertainment: Hobbies"

  Travel:
    - "Travel: Flights"
    - "Travel: Accommodation"
    - "Travel: Holiday Spending"

  Finance:
    - "Finance: Savings Transfer"
    - "Finance: Investment Transfer"
    - "Finance: Fees & Charges"
    - "Finance: Insurance"

  Personal:
    - "Personal Care: Haircut & Beauty"
    - "Gifts & Donations: Gifts"
    - "Education: Courses & Books"

  Other:
    - "Other: Uncategorized"
```

The `Finance: Savings Transfer` and `Finance: Investment Transfer` categories are excluded from budget totals since they move money between your own accounts.

## Claude Integration (`src/categorizer/claude.rs`)

```rust
use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const MODEL: &str = "claude-haiku-4-5-20251001";

#[derive(Serialize)]
struct MessagesRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    system: Vec<SystemBlock<'a>>,
    messages: Vec<Message<'a>>,
}

#[derive(Serialize)]
struct SystemBlock<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<CacheControl<'a>>,
}

#[derive(Serialize)]
struct CacheControl<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
}

#[derive(Serialize)]
struct Message<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct MessagesResponse {
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
struct ContentBlock {
    text: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CatResult {
    pub category: String,
    pub confidence: f64,
}

pub struct ClaudeCategorizer {
    client: Client,
    api_key: String,
    categories_block: String,     // the system prompt listing all valid categories
}

impl ClaudeCategorizer {
    pub async fn categorize_one(&self, normalized_description: &str) -> Result<CatResult> {
        // Prompt caching: the first system block (categories list) is marked cache_control.
        // Subsequent requests within 5 minutes skip re-processing the category list.
        let req = MessagesRequest {
            model: MODEL,
            max_tokens: 64,
            system: vec![SystemBlock {
                kind: "text",
                text: &self.categories_block,
                cache_control: Some(CacheControl { kind: "ephemeral" }),
            }],
            messages: vec![Message {
                role: "user",
                content: normalized_description,
            }],
        };

        let resp: MessagesResponse = self.client
            .post(API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&req)
            .send()
            .await?
            .json()
            .await?;

        let raw = resp.content.into_iter()
            .filter_map(|c| c.text)
            .collect::<Vec<_>>()
            .join("");
        parse_response(&raw)
    }
}

/// Expected response: "Food: Groceries|0.92"
fn parse_response(s: &str) -> Result<CatResult> {
    let trimmed = s.trim();
    let (category, confidence) = trimmed.split_once('|').unwrap_or((trimmed, "0.8"));
    Ok(CatResult {
        category: category.trim().to_string(),
        confidence: confidence.trim().parse::<f64>().unwrap_or(0.8),
    })
}
```

The `categories_block` system prompt includes the full taxonomy from `config/categories.yaml` and example prompts. Because it is marked for prompt caching, a full batch of categorization requests reuses the cached block across every call.

## Claude Data Minimization

Only the **normalized description** is sent. Dates, amounts, account IDs, and raw (unnormalized) descriptions are never transmitted. See `../design/05_security_isolation.md`.

## Pipeline (`src/categorizer/pipeline.rs`)

```rust
use crate::categorizer::{rules, claude};
use crate::storage::db::Db;
use anyhow::Result;

pub async fn run_all(
    db: &Db,
    rules: &[rules::CompiledRule],
    claude: &claude::ClaudeCategorizer,
) -> Result<PipelineStats> {
    let uncategorized = db.get_uncategorized_transactions()?;
    let mut stats = PipelineStats::default();

    for txn in uncategorized {
        if let Some(category) = rules::match_rule(&txn.description, rules) {
            db.set_category(&txn.id, category, "rule", None)?;
            stats.rule_matches += 1;
            continue;
        }

        match claude.categorize_one(&txn.description).await {
            Ok(result) => {
                if result.confidence >= 0.75 {
                    db.set_category(&txn.id, &result.category, "claude", Some(result.confidence))?;
                    stats.claude_matches += 1;
                } else {
                    db.set_category(&txn.id, &result.category, "claude", Some(result.confidence))?;
                    stats.review_queue += 1;
                }
            }
            Err(e) => {
                tracing::warn!("claude categorize failed: {}", e);
                stats.errors += 1;
            }
        }
    }

    Ok(stats)
}

#[derive(Default, Debug)]
pub struct PipelineStats {
    pub rule_matches: usize,
    pub claude_matches: usize,
    pub review_queue: usize,
    pub errors: usize,
}
```

## CLI + API

```bash
fynance categorize            # synchronous, one-by-one
fynance categorize --batch    # use Claude Batch API (cheaper, 5-30 min)
fynance categorize --check <batch_id>  # collect batch results
```

The UI triggers categorization via `POST /api/categorize`. The server responds with streaming progress (SSE or polling) and the Transactions view reflects updates in place.
