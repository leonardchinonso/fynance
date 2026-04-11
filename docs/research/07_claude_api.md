# Claude API Integration (Rust)

## Models to Use

| Use Case | Model | Why |
|---|---|---|
| Bulk historical categorization | `claude-haiku-4-5-20251001` via Batch API | Cheapest, fast enough, 50% batch discount |
| On-demand single categorization | `claude-haiku-4-5-20251001` | Fast response, low cost |
| PDF extraction fallback | `claude-sonnet-4-6` | Better vision/reasoning |
| Budget analysis and monthly insights | `claude-sonnet-4-6` | Longer context, nuanced reasoning |

## HTTP Client Setup

The Anthropic Rust ecosystem has a few community SDKs (e.g. `misanthropic`, `async-anthropic`), but calling the REST API directly with `reqwest` is equally simple and avoids churn risk from alpha SDKs.

```rust
use reqwest::Client;
use std::time::Duration;

pub fn build_client() -> anyhow::Result<Client> {
    let client = Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent("fynance/0.1")
        .build()?;
    Ok(client)
}

pub fn get_api_key() -> anyhow::Result<String> {
    std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| anyhow::anyhow!("ANTHROPIC_API_KEY not set"))
}
```

## Single Categorization Request

```rust
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Deserialize)]
pub struct CategorizeResult {
    pub category: String,
    pub confidence: f64,
}

pub async fn categorize_one(
    client: &Client,
    api_key: &str,
    description: &str,
    system_prompt: &str,
    few_shot: &str,
) -> anyhow::Result<CategorizeResult> {
    let body = json!({
        "model": "claude-haiku-4-5-20251001",
        "max_tokens": 128,
        "system": [
            {
                "type": "text",
                "text": system_prompt,
                "cache_control": { "type": "ephemeral" }
            },
            {
                "type": "text",
                "text": few_shot,
                "cache_control": { "type": "ephemeral" }
            }
        ],
        "messages": [{
            "role": "user",
            "content": format!("Transaction: \"{}\"\n\nReturn JSON only.", description)
        }]
    });

    let resp: serde_json::Value = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let text = resp["content"][0]["text"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("no text in response"))?;
    let parsed: CategorizeResult = serde_json::from_str(text)?;
    Ok(parsed)
}
```

## Tool Use (More Reliable Than Free-Form JSON)

Tool use forces a structured response via the tool input schema. Use when free-form JSON is unreliable.

```rust
use serde_json::json;

pub fn categorize_tool_def(categories: &[&str]) -> serde_json::Value {
    json!({
        "name": "categorize_transaction",
        "description": "Assigns a bank transaction to a spending category",
        "input_schema": {
            "type": "object",
            "properties": {
                "category": {
                    "type": "string",
                    "enum": categories
                },
                "confidence": {
                    "type": "number",
                    "minimum": 0.0,
                    "maximum": 1.0
                }
            },
            "required": ["category", "confidence"]
        }
    })
}

pub async fn categorize_with_tool(
    client: &reqwest::Client,
    api_key: &str,
    description: &str,
    categories: &[&str],
) -> anyhow::Result<CategorizeResult> {
    let body = json!({
        "model": "claude-haiku-4-5-20251001",
        "max_tokens": 256,
        "tools": [categorize_tool_def(categories)],
        "tool_choice": { "type": "tool", "name": "categorize_transaction" },
        "messages": [{
            "role": "user",
            "content": format!("Categorize this bank transaction: \"{}\"", description)
        }]
    });

    let resp: serde_json::Value = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let tool_use = resp["content"]
        .as_array()
        .and_then(|arr| arr.iter().find(|b| b["type"] == "tool_use"))
        .ok_or_else(|| anyhow::anyhow!("no tool_use block"))?;

    let input = &tool_use["input"];
    Ok(CategorizeResult {
        category: input["category"].as_str().unwrap_or("Other").to_string(),
        confidence: input["confidence"].as_f64().unwrap_or(0.0),
    })
}
```

## Prompt Caching

Cache the system prompt and few-shot examples to cut repeated-call costs by ~90%. The `cache_control` marker is already shown in the single-request example above.

Cache hits cost 10% of normal input token price. For a ~300 token system prompt, caching saves ~270 tokens per request when running thousands of categorizations.

## Batch API (Historical Import)

For processing 2-3 years of historical transactions, use the Batch API for 50% cost reduction.

```rust
use reqwest::Client;
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

pub struct BatchRequest {
    pub custom_id: String,
    pub description: String,
}

pub async fn submit_batch(
    client: &Client,
    api_key: &str,
    requests: &[BatchRequest],
    system_prompt: &str,
) -> anyhow::Result<String> {
    let request_objs: Vec<_> = requests.iter().map(|r| {
        json!({
            "custom_id": r.custom_id,
            "params": {
                "model": "claude-haiku-4-5-20251001",
                "max_tokens": 128,
                "system": system_prompt,
                "messages": [{
                    "role": "user",
                    "content": format!("Transaction: \"{}\"\n\nReturn JSON only.", r.description)
                }]
            }
        })
    }).collect();

    let body = json!({ "requests": request_objs });

    let resp: serde_json::Value = client
        .post("https://api.anthropic.com/v1/messages/batches")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    Ok(resp["id"].as_str().unwrap().to_string())
}

pub async fn wait_for_batch(
    client: &Client,
    api_key: &str,
    batch_id: &str,
) -> anyhow::Result<()> {
    loop {
        let url = format!("https://api.anthropic.com/v1/messages/batches/{}", batch_id);
        let resp: serde_json::Value = client
            .get(&url)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let status = resp["processing_status"].as_str().unwrap_or("");
        if status == "ended" {
            return Ok(());
        }
        tracing::info!("batch {} status: {}", batch_id, status);
        sleep(Duration::from_secs(30)).await;
    }
}

pub async fn fetch_batch_results(
    client: &Client,
    api_key: &str,
    batch_id: &str,
) -> anyhow::Result<Vec<(String, CategorizeResult)>> {
    let url = format!("https://api.anthropic.com/v1/messages/batches/{}/results", batch_id);
    let text = client
        .get(&url)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    // Results are returned as JSONL (one JSON object per line)
    let mut results = Vec::new();
    for line in text.lines() {
        let obj: serde_json::Value = serde_json::from_str(line)?;
        let custom_id = obj["custom_id"].as_str().unwrap_or("").to_string();
        if obj["result"]["type"] == "succeeded" {
            let content_text = obj["result"]["message"]["content"][0]["text"]
                .as_str()
                .unwrap_or("{}");
            if let Ok(parsed) = serde_json::from_str::<CategorizeResult>(content_text) {
                results.push((custom_id, parsed));
            }
        }
    }
    Ok(results)
}
```

## PDF Vision Extraction

When `pdf-extract` and regex parsing fail, use Claude's document input. Code shown in `research/02_rust_crates.md`.

## Budget Analysis Prompt

Monthly insight generation using Claude Sonnet:

```rust
pub async fn monthly_insights(
    client: &reqwest::Client,
    api_key: &str,
    month: &str,
    variance_report: &str,
) -> anyhow::Result<String> {
    let prompt = format!(
        r#"Analyze my {month} spending vs budget and give me insights.

Budget vs Actual:
{variance_report}

Provide:
1. **Overall assessment** (1-2 sentences, direct and honest)
2. **Top wins** (categories where I stayed under budget)
3. **Areas to watch** (categories over budget, with specific suggestions)
4. **One key action** for next month to improve my finances

Be direct and specific. Format as markdown. Keep it under 300 words."#
    );

    let body = serde_json::json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 800,
        "messages": [{ "role": "user", "content": prompt }]
    });

    let resp: serde_json::Value = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    Ok(resp["content"][0]["text"].as_str().unwrap_or("").to_string())
}
```

## Cost Summary

For a personal finance project with ~3 years of history (~7,200 transactions):

| Operation | Model | Count | Estimated Cost |
|---|---|---|---|
| Historical categorization (batch) | Haiku Batch | 2,500 | ~$0.25 |
| PDF extraction fallback | Sonnet | ~20 PDFs | ~$0.50 |
| Monthly budget analysis | Sonnet | 12/year | ~$0.12/year |
| **Total setup** | | | **~$0.75** |
| **Ongoing/month** | | | **~$0.05** |

## Error Handling Pattern

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ClaudeError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("API returned error: {0}")]
    Api(String),
    #[error("unexpected response shape: {0}")]
    Shape(String),
}
```

Map the error at command boundaries into `anyhow::Result` using `?`.
