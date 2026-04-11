# Categorization System

## Pipeline Flow

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
DONE         [claude.rs: Haiku + few-shot]
                  |
             confidence >= 0.75?
             /            \
           Yes              No
            |                |
            v                v
          DONE         [review_queue in db]
```

## Rule Engine (`src/categorizer/rules.rs`)

```rust
use anyhow::Result;
use once_cell::sync::OnceCell;
use regex::Regex;
use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct RulesFile {
    rules: Vec<RuleDef>,
}

#[derive(Debug, Deserialize)]
struct RuleDef {
    pattern: String,
    category: String,
    confidence: f64,
}

pub struct CompiledRule {
    pub regex: Regex,
    pub category: String,
    pub confidence: f64,
}

pub fn load(path: &Path) -> Result<Vec<CompiledRule>> {
    let text = fs::read_to_string(path)?;
    let file: RulesFile = serde_yaml::from_str(&text)?;
    file.rules.into_iter().map(|r| {
        Ok(CompiledRule {
            regex: Regex::new(&r.pattern)?,
            category: r.category,
            confidence: r.confidence,
        })
    }).collect()
}

pub fn match_rules<'a>(
    description: &str,
    rules: &'a [CompiledRule],
) -> Option<(&'a str, f64)> {
    rules.iter()
        .find(|r| r.regex.is_match(description))
        .map(|r| (r.category.as_str(), r.confidence))
}
```

## Claude Categorizer (`src/categorizer/claude.rs`)

```rust
use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

const MODEL_HAIKU: &str = "claude-haiku-4-5-20251001";

const SYSTEM_PROMPT: &str = r#"You are a personal finance transaction categorizer.

Available categories (use exactly as written):
Food: Groceries | Food: Dining & Bars | Food: Coffee
Transport: Gas | Transport: Rideshare & Transit | Transport: Parking & Tolls
Health: Medical & Dental | Health: Pharmacy | Health: Fitness
Housing: Rent/Mortgage | Housing: Utilities | Housing: Insurance
Digital: Subscriptions | Digital: Apps & Software
Shopping: Clothing | Shopping: Electronics | Shopping: Amazon & Online
Life: Entertainment | Life: Travel | Life: Personal Care
Finance: Internal Transfer | Finance: Fees & Interest
Income: Salary | Income: Refund | Income: Transfer In
Other

Respond ONLY with valid JSON: {"category": "...", "confidence": 0.0}"#;

const FEW_SHOT: &str = r#"Examples:
"WHOLE FOODS MARKET" -> {"category": "Food: Groceries", "confidence": 0.98}
"STARBUCKS STORE 12345" -> {"category": "Food: Coffee", "confidence": 0.97}
"DOORDASH*CHIPOTLE" -> {"category": "Food: Dining & Bars", "confidence": 0.95}
"CHEVRON 00123456" -> {"category": "Transport: Gas", "confidence": 0.96}
"LYFT *RIDE SUN 3PM" -> {"category": "Transport: Rideshare & Transit", "confidence": 0.97}
"NETFLIX.COM" -> {"category": "Digital: Subscriptions", "confidence": 0.99}
"AMZN MKTP US*1A2B3C" -> {"category": "Shopping: Amazon & Online", "confidence": 0.90}
"CVS/PHARMACY #4567" -> {"category": "Health: Pharmacy", "confidence": 0.93}
"KAISER PERMANENTE" -> {"category": "Health: Medical & Dental", "confidence": 0.96}
"ZELLE PAYMENT SENT" -> {"category": "Finance: Internal Transfer", "confidence": 0.90}
"PAYROLL DIRECT DEPOSIT" -> {"category": "Income: Salary", "confidence": 0.99}"#;

#[derive(Debug, Deserialize)]
pub struct CatResult {
    pub category: String,
    pub confidence: f64,
}

fn user_msg(description: &str) -> String {
    format!("Transaction: \"{}\"\n\nReturn JSON only.", description)
}

/// Single on-demand categorization request with prompt caching.
pub async fn categorize_one(client: &Client, api_key: &str, description: &str) -> Result<CatResult> {
    let body = json!({
        "model": MODEL_HAIKU,
        "max_tokens": 128,
        "system": [
            { "type": "text", "text": SYSTEM_PROMPT, "cache_control": { "type": "ephemeral" } },
            { "type": "text", "text": FEW_SHOT, "cache_control": { "type": "ephemeral" } }
        ],
        "messages": [{ "role": "user", "content": user_msg(description) }]
    });

    let resp: serde_json::Value = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-beta", "prompt-caching-2024-07-31")
        .json(&body)
        .send().await?
        .error_for_status()?
        .json().await?;

    let text = resp["content"][0]["text"].as_str()
        .context("no text in Claude response")?;
    serde_json::from_str(text).context("Claude returned invalid JSON")
}

/// Submit a batch of transactions to the Batch API (50% cheaper, async).
/// Returns the batch ID to poll later.
pub async fn submit_batch(
    client: &Client,
    api_key: &str,
    items: &[(&str, &str)],  // (custom_id, description)
) -> Result<String> {
    let requests: Vec<_> = items.iter().map(|(id, desc)| json!({
        "custom_id": id,
        "params": {
            "model": MODEL_HAIKU,
            "max_tokens": 128,
            "system": format!("{}\n\n{}", SYSTEM_PROMPT, FEW_SHOT),
            "messages": [{ "role": "user", "content": user_msg(desc) }]
        }
    })).collect();

    let resp: serde_json::Value = client
        .post("https://api.anthropic.com/v1/messages/batches")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&json!({ "requests": requests }))
        .send().await?
        .error_for_status()?
        .json().await?;

    Ok(resp["id"].as_str().context("no batch id")?.to_string())
}

/// Collect results for a completed batch.
pub async fn fetch_batch_results(
    client: &Client,
    api_key: &str,
    batch_id: &str,
) -> Result<Vec<(String, CatResult)>> {
    let url = format!("https://api.anthropic.com/v1/messages/batches/{}/results", batch_id);
    let body = client
        .get(&url)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .send().await?
        .error_for_status()?
        .text().await?;

    // JSONL: one JSON object per line
    let mut results = Vec::new();
    for line in body.lines() {
        let obj: serde_json::Value = serde_json::from_str(line)?;
        if obj["result"]["type"] != "succeeded" { continue; }
        let custom_id = obj["custom_id"].as_str().unwrap_or("").to_string();
        let text = obj["result"]["message"]["content"][0]["text"].as_str().unwrap_or("{}");
        if let Ok(cat) = serde_json::from_str::<CatResult>(text) {
            results.push((custom_id, cat));
        }
    }
    Ok(results)
}
```

## Pipeline Orchestrator (`src/categorizer/pipeline.rs`)

```rust
use super::{claude, rules::{self, CompiledRule}};
use crate::storage::db::Db;
use anyhow::Result;
use std::path::Path;

const HIGH_CONF: f64 = 0.85;
const REVIEW_THRESHOLD: f64 = 0.75;

pub enum Outcome {
    Categorized { category: String, confidence: f64, source: &'static str },
    NeedsReview { suggested: String, confidence: f64 },
}

/// Full hybrid pipeline for a single transaction.
pub async fn categorize(
    id: &str,
    description: &str,
    compiled_rules: &[CompiledRule],
    http: &reqwest::Client,
    api_key: &str,
) -> Result<Outcome> {
    if let Some((cat, conf)) = rules::match_rules(description, compiled_rules) {
        if conf >= HIGH_CONF {
            return Ok(Outcome::Categorized {
                category: cat.to_string(), confidence: conf, source: "rules"
            });
        }
    }

    let result = claude::categorize_one(http, api_key, description).await?;

    if result.confidence >= REVIEW_THRESHOLD {
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

/// Process all uncategorized transactions in the database.
pub async fn run_all(
    db: &Db,
    compiled_rules: &[CompiledRule],
    http: &reqwest::Client,
    api_key: &str,
    use_batch: bool,
) -> Result<()> {
    let uncategorized = db.get_uncategorized()?;
    println!("Found {} uncategorized transactions", uncategorized.len());

    let mut to_claude: Vec<(String, String)> = Vec::new();

    // Pass 1: rules (free, instant)
    for txn in &uncategorized {
        if let Some((cat, conf)) = rules::match_rules(&txn.description, compiled_rules) {
            if conf >= HIGH_CONF {
                db.set_category(&txn.id, cat, conf, "rules")?;
                continue;
            }
        }
        to_claude.push((txn.id.clone(), txn.description.clone()));
    }

    println!("Rules handled {}; sending {} to Claude", uncategorized.len() - to_claude.len(), to_claude.len());

    if to_claude.is_empty() { return Ok(()); }

    if use_batch && to_claude.len() > 10 {
        // Batch API: submit and return ID for later polling
        let items: Vec<_> = to_claude.iter().map(|(id, desc)| (id.as_str(), desc.as_str())).collect();
        let batch_id = claude::submit_batch(http, api_key, &items).await?;
        println!("Batch submitted: {}. Run `fynance categorize --check {}` to collect results.", batch_id, batch_id);
    } else {
        // On-demand, one at a time
        for (id, desc) in &to_claude {
            let result = claude::categorize_one(http, api_key, desc).await?;
            if result.confidence >= REVIEW_THRESHOLD {
                db.set_category(id, &result.category, result.confidence, "claude")?;
            } else {
                db.add_review_queue(id, &result.category, result.confidence)?;
            }
        }
    }

    let review_count = db.review_queue_count()?;
    if review_count > 0 {
        println!("{} transactions need manual review. Run `fynance review`.", review_count);
    }
    Ok(())
}
```

## Interactive Review (`src/commands/review.rs`)

```rust
use crate::storage::db::Db;
use std::io::{self, BufRead, Write};

pub fn run(db: &Db, categories: &[&str]) -> anyhow::Result<()> {
    let queue = db.get_review_queue(20)?;
    if queue.is_empty() {
        println!("No transactions need review.");
        return Ok(());
    }

    let stdin = io::stdin();
    let stdout = io::stdout();

    for item in queue {
        println!("\nDate:        {}", item.date);
        println!("Description: {}", item.description);
        println!("Amount:      ${:.2}", item.amount);
        println!("Suggested:   {} ({:.0}%)", item.suggested, item.confidence * 100.0);
        print!("Category [Enter=accept / 'l'=list / 's'=skip]: ");
        stdout.lock().flush()?;

        let mut input = String::new();
        stdin.lock().read_line(&mut input)?;
        let input = input.trim();

        match input {
            "" => { db.confirm_review(&item.transaction_id, &item.suggested)?; }
            "s" => { continue; }
            "l" => {
                for (i, cat) in categories.iter().enumerate() {
                    println!("  {:2}. {}", i + 1, cat);
                }
                print!("Number or name: ");
                stdout.lock().flush()?;
                let mut choice = String::new();
                stdin.lock().read_line(&mut choice)?;
                let choice = choice.trim();
                let cat = if let Ok(n) = choice.parse::<usize>() {
                    categories.get(n - 1).copied().unwrap_or("Other")
                } else {
                    choice
                };
                db.confirm_review(&item.transaction_id, cat)?;
            }
            other => { db.confirm_review(&item.transaction_id, other)?; }
        }
    }

    let remaining = db.review_queue_count()?;
    println!("\nDone. {} items remaining.", remaining);
    Ok(())
}
```
