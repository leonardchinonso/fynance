//! Pluggable LLM provider abstraction for the parse pipeline.

use std::sync::Arc;
use std::time::Instant;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use futures_util::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::importers::pdf_extract::{self, PdfExtractConfig};
use crate::model::{Agent, AuthSource};

// ── Tier hint ────────────────────────────────────────────────────────────────

/// Default model tier for a call. Caller can override with `Agent`.
#[derive(Debug, Clone, Copy)]
pub enum ModelTier {
    /// CSV / Excel text extraction. Env: FYNANCE_IMPORT_LLM_MODEL.
    Standard,
    /// PDF visual understanding. Env: FYNANCE_PARSE_PDF_MODEL.
    Advanced,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone)]
pub struct ProviderCallResult {
    pub value: Value,
    pub usage: TokenUsage,
    pub model: String,
    pub duration_ms: u64,
    /// `"max_tokens"` here means the tool input is truncated.
    pub stop_reason: Option<String>,
    /// Human-readable notes about degraded input that did not fail the call
    /// (a PDF page the extract tool could not read). Routed into the parse
    /// preview's metadata notes so the user always sees them.
    pub warnings: Vec<String>,
}

const MAX_TOKENS_TEXT: u32 = 16_384;
const MAX_TOKENS_DOCUMENTS: u32 = 64_000;

// ── Progress reporting ──────────────────────────────────────────────────────

pub type ProgressTx = tokio::sync::broadcast::Sender<ProgressEvent>;

#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[serde(tag = "event", rename_all = "snake_case")]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub enum ProgressEvent {
    Phase {
        phase: ParsePhase,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        task_id: Option<String>,
    },
    LlmStart {
        model: String,
        // Serialized to JSON and read with JSON.parse on the client, so it is a
        // JS number at runtime, not a bigint.
        #[ts(type = "number")]
        input_tokens: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        task_id: Option<String>,
    },
    LlmProgress {
        #[ts(type = "number")]
        output_tokens: u64,
        #[ts(type = "number")]
        elapsed_ms: u64,
        /// Rows extracted so far, derived live from the streamed tool JSON.
        #[ts(type = "number")]
        items: u64,
        /// Section being extracted: "transactions" | "holdings" | "investments".
        #[serde(skip_serializing_if = "Option::is_none")]
        section: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        task_id: Option<String>,
    },
    Done {
        #[ts(type = "number")]
        total_ms: u64,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub enum ParsePhase {
    Preprocessing,
    BuildingContext,
    SendingToLlm,
    WaitingForLlm,
    PostProcessing,
}

pub fn emit_progress(tx: &Option<ProgressTx>, event: ProgressEvent) {
    if let Some(tx) = tx {
        let _ = tx.send(event);
    }
}

// ── Provider errors ─────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum ProviderError {
    Unreachable(String),
    Timeout(String),
    RateLimit {
        retry_after: Option<u64>,
        detail: String,
    },
    AuthRejected(String),
    UpstreamServerError {
        status: u16,
        body: String,
    },
    UpstreamClientError {
        status: u16,
        body: String,
    },
    ResponseUnreadable(String),
    NoToolUse {
        tool_name: String,
    },
    StreamInterrupted(String),
    NotSupported(String),
    /// The model ran out of output budget mid-response.
    Truncated {
        output_tokens: u64,
    },
    /// An auxiliary document-conversion step (e.g. the pdf-extract tool)
    /// failed; the message is user-facing.
    DocumentConversion(String),
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable(msg) => write!(f, "upstream unreachable: {msg}"),
            Self::Timeout(msg) => write!(f, "upstream timeout: {msg}"),
            Self::RateLimit { detail, .. } => write!(f, "rate limited: {detail}"),
            Self::AuthRejected(msg) => write!(f, "auth rejected: {msg}"),
            Self::UpstreamServerError { status, body } => {
                write!(f, "upstream server error ({status}): {body}")
            }
            Self::UpstreamClientError { status, body } => {
                write!(f, "upstream client error ({status}): {body}")
            }
            Self::ResponseUnreadable(msg) => write!(f, "unreadable response: {msg}"),
            Self::NoToolUse { tool_name } => {
                write!(f, "no {tool_name} tool_use block in response")
            }
            Self::StreamInterrupted(msg) => write!(f, "stream interrupted: {msg}"),
            Self::NotSupported(msg) => write!(f, "not supported: {msg}"),
            Self::Truncated { output_tokens } => {
                write!(f, "response truncated after {output_tokens} output tokens")
            }
            Self::DocumentConversion(msg) => write!(f, "document conversion failed: {msg}"),
        }
    }
}

impl std::error::Error for ProviderError {}

// ProviderError: Error + Send + Sync + 'static, so anyhow's blanket
// `impl<E: Error + Send + Sync + 'static> From<E> for anyhow::Error`
// covers the conversion automatically. No manual From impl needed.

// ── Trait ─────────────────────────────────────────────────────────────────────

/// Pluggable LLM backend for the parse pipeline.
#[async_trait]
pub trait LlmProvider: Send + Sync + std::fmt::Debug {
    async fn chat_with_tools(
        &self,
        system_prompt: &str,
        user_message: &str,
        tool_name: &str,
        tool_schema: Value,
        tier: ModelTier,
        agent_override: Option<Agent>,
    ) -> Result<ProviderCallResult, ProviderError>;

    /// Providers without PDF support (e.g. OpenAI v0) must return `Err`.
    async fn chat_with_pdf_and_tools(
        &self,
        system_prompt: &str,
        pdf_bytes: &[u8],
        text_supplement: &str,
        tool_name: &str,
        tool_schema: Value,
        agent_override: Option<Agent>,
    ) -> Result<ProviderCallResult, ProviderError>;

    /// Files routed by MIME: `application/pdf` → document, `image/*` →
    /// image, `text/*` → inlined text. Other MIME types may error.
    async fn chat_with_files_and_tools(
        &self,
        system_prompt: &str,
        files: &[(String, String, Vec<u8>)],
        text_supplement: &str,
        tool_name: &str,
        tool_schema: Value,
        agent_override: Option<Agent>,
    ) -> Result<ProviderCallResult, ProviderError>;

    /// Short identifier used in log messages and metrics.
    fn name(&self) -> &'static str;

    /// Return a clone of this provider that emits progress events during
    /// streaming calls. Returns `None` if the provider doesn't support
    /// progress.
    fn with_progress(
        &self,
        _tx: ProgressTx,
        _task_id: Option<String>,
    ) -> Option<Arc<dyn LlmProvider>> {
        None
    }
}

// ── AnthropicProvider ─────────────────────────────────────────────────────────

/// How an `AnthropicProvider` authenticates to `api.anthropic.com`.
#[derive(Clone)]
enum AnthropicAuth {
    /// Console API key (`x-api-key` header), pay-per-token billing.
    ApiKey(String),
    /// Claude Pro/Max subscription OAuth token (`sk-ant-oat01-`), sent as a
    /// Bearer token with the `oauth-2025-04-20` beta. Draws from the monthly
    /// Agent SDK credit pool instead of pay-per-token. This is the same auth
    /// path Claude Code itself uses; see
    /// `docs/research/08_subscription_credits_vs_api.md`.
    Subscription(String),
}

impl std::fmt::Debug for AnthropicAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the credential itself.
        match self {
            Self::ApiKey(_) => f.write_str("ApiKey(<redacted>)"),
            Self::Subscription(_) => f.write_str("Subscription(<redacted>)"),
        }
    }
}

/// Injected as the first system block on subscription calls. Anthropic gates
/// OAuth tokens to Claude Code; identifying as Claude Code keeps the request
/// valid if that check is enforced, and is harmless when it is not.
const CLAUDE_CODE_SYSTEM_IDENTITY: &str =
    "You are Claude Code, Anthropic's official CLI for Claude.";

#[derive(Debug)]
pub struct AnthropicProvider {
    client: Client,
    auth: AnthropicAuth,
    standard_model: String,
    advanced_model: String,
    progress: Option<(ProgressTx, Option<String>)>,
}

impl AnthropicProvider {
    /// Build with a Console API key from `FYNANCE_ANTHROPIC_API_KEY`
    /// (pay-per-token billing).
    ///
    /// Optional: `FYNANCE_IMPORT_LLM_MODEL`, `FYNANCE_PARSE_PDF_MODEL`
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("FYNANCE_ANTHROPIC_API_KEY")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| {
                anyhow!(
                    "FYNANCE_ANTHROPIC_API_KEY is not set. \
                     Set it in your .env file or environment."
                )
            })?;
        Self::with_auth(AnthropicAuth::ApiKey(api_key))
    }

    /// Build with a Claude subscription OAuth token from
    /// `FYNANCE_CLAUDE_CODE_OAUTH_TOKEN` (obtain one by running
    /// `claude setup-token`). Billed against the subscription's Agent SDK
    /// credit pool rather than pay-per-token.
    pub fn from_env_subscription() -> Result<Self> {
        let token = std::env::var("FYNANCE_CLAUDE_CODE_OAUTH_TOKEN")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| {
                anyhow!(
                    "FYNANCE_CLAUDE_CODE_OAUTH_TOKEN is not set. \
                     Run `claude setup-token` and set it in your .env file."
                )
            })?;
        Self::with_auth(AnthropicAuth::Subscription(token))
    }

    fn with_auth(auth: AnthropicAuth) -> Result<Self> {
        // Default to Sonnet for reliable structured extraction. Haiku can be
        // opted into via FYNANCE_IMPORT_LLM_MODEL but is never the default: it
        // intermittently drops required empty arrays (e.g. `rows: []`) from
        // tool_use output, which aborts a parse (see issue #55).
        let standard_model = std::env::var("FYNANCE_IMPORT_LLM_MODEL")
            .unwrap_or_else(|_| "claude-sonnet-4-6".to_string());
        let advanced_model = std::env::var("FYNANCE_PARSE_PDF_MODEL")
            .unwrap_or_else(|_| "claude-sonnet-4-6".to_string());

        let client = Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|e| anyhow!("failed to build HTTP client: {e}"))?;

        Ok(Self {
            client,
            auth,
            standard_model,
            advanced_model,
            progress: None,
        })
    }

    fn model_for_tier(&self, tier: ModelTier) -> &str {
        match tier {
            ModelTier::Standard => &self.standard_model,
            ModelTier::Advanced => &self.advanced_model,
        }
    }

    fn resolve_model(
        &self,
        tier: ModelTier,
        agent_override: Option<Agent>,
    ) -> Result<String, ProviderError> {
        match agent_override {
            Some(agent) => anthropic_model_for_agent(agent).map(|s| s.to_string()),
            None => Ok(self.model_for_tier(tier).to_string()),
        }
    }

    pub fn clone_with_progress(&self, tx: ProgressTx, task_id: Option<String>) -> Self {
        Self {
            client: self.client.clone(),
            auth: self.auth.clone(),
            standard_model: self.standard_model.clone(),
            advanced_model: self.advanced_model.clone(),
            progress: Some((tx, task_id)),
        }
    }
}

/// Latest frontier model id per agent. Keep in sync with `pricing.rs`.
fn anthropic_model_for_agent(agent: Agent) -> Result<&'static str, ProviderError> {
    match agent {
        Agent::Haiku => Ok("claude-haiku-4-5-20251001"),
        Agent::Sonnet => Ok("claude-sonnet-4-6"),
        Agent::Opus => Ok("claude-opus-4-7"),
        other => Err(ProviderError::NotSupported(format!(
            "agent '{other:?}' is not supported by Anthropic provider. Valid options: haiku, sonnet, opus"
        ))),
    }
}

// ── SSE streaming accumulator ───────────────────────────────────────────────

struct SseAccumulator {
    model: String,
    input_tokens: u64,
    output_tokens: u64,
    stop_reason: Option<String>,
    tool_inputs: std::collections::HashMap<u32, String>,
    tool_names: std::collections::HashMap<u32, String>,
}

impl SseAccumulator {
    fn new() -> Self {
        Self {
            model: String::new(),
            input_tokens: 0,
            output_tokens: 0,
            stop_reason: None,
            tool_inputs: std::collections::HashMap::new(),
            tool_names: std::collections::HashMap::new(),
        }
    }

    fn handle_event(&mut self, event: &serde_json::Value) {
        let event_type = event["type"].as_str().unwrap_or("");
        match event_type {
            "message_start" => {
                if let Some(msg) = event.get("message") {
                    self.model = msg["model"].as_str().unwrap_or("").to_string();
                    if let Some(usage) = msg.get("usage") {
                        self.input_tokens = usage["input_tokens"].as_u64().unwrap_or(0);
                    }
                }
            }
            "content_block_start" => {
                let index = event["index"].as_u64().unwrap_or(0) as u32;
                if let Some(block) = event.get("content_block") {
                    if block["type"].as_str() == Some("tool_use") {
                        let name = block["name"].as_str().unwrap_or("").to_string();
                        self.tool_names.insert(index, name);
                        self.tool_inputs.insert(index, String::new());
                    }
                }
            }
            "content_block_delta" => {
                let index = event["index"].as_u64().unwrap_or(0) as u32;
                if let Some(delta) = event.get("delta") {
                    if delta["type"].as_str() == Some("input_json_delta") {
                        if let Some(partial) = delta["partial_json"].as_str() {
                            if let Some(buf) = self.tool_inputs.get_mut(&index) {
                                buf.push_str(partial);
                            }
                        }
                    }
                }
            }
            "message_delta" => {
                if let Some(delta) = event.get("delta") {
                    if let Some(sr) = delta["stop_reason"].as_str() {
                        self.stop_reason = Some(sr.to_string());
                    }
                }
                if let Some(usage) = event.get("usage") {
                    self.output_tokens = usage["output_tokens"]
                        .as_u64()
                        .unwrap_or(self.output_tokens);
                }
            }
            _ => {}
        }
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn input_tokens(&self) -> u64 {
        self.input_tokens
    }

    fn output_tokens(&self) -> u64 {
        self.output_tokens
    }

    /// Best-effort live extraction progress from the streamed tool-use JSON: the
    /// section currently being filled (the unified tool returns transactions,
    /// holdings, then investments, in that order) and the number of completed rows
    /// so far. This parses partial JSON, so it is a heuristic — fine for a label.
    fn extraction_progress(&self) -> (Option<String>, u64) {
        let buf: String = self.tool_inputs.values().cloned().collect();
        streamed_extraction_progress(&buf)
    }

    fn into_result(self, expected_tool: &str) -> Result<ProviderCallResult, ProviderError> {
        let tool_names = self.tool_names;
        let tool_json = self
            .tool_inputs
            .into_iter()
            .find(|(idx, _)| {
                tool_names
                    .get(idx)
                    .map(|n| n == expected_tool)
                    .unwrap_or(false)
            })
            .map(|(_, json_str)| json_str)
            .ok_or_else(|| ProviderError::NoToolUse {
                tool_name: expected_tool.to_string(),
            })?;

        // A max_tokens stop reason means the tool input was cut off mid-JSON,
        // so the parse failure is a truncation, not a garbled model reply.
        let stop_reason = self.stop_reason.clone();
        let output_tokens = self.output_tokens;
        let value: serde_json::Value = serde_json::from_str(&tool_json).map_err(|e| {
            if stop_reason.as_deref() == Some("max_tokens") {
                ProviderError::Truncated { output_tokens }
            } else {
                ProviderError::ResponseUnreadable(format!(
                    "invalid JSON for tool {expected_tool}: {e} (preview: {})",
                    &tool_json[..tool_json.len().min(200)]
                ))
            }
        })?;

        Ok(ProviderCallResult {
            value,
            usage: TokenUsage {
                input_tokens: self.input_tokens,
                output_tokens: self.output_tokens,
            },
            model: self.model,
            duration_ms: 0,
            stop_reason: self.stop_reason,
            warnings: Vec::new(),
        })
    }
}

/// Shared progress heuristic for streamed tool JSON: the section currently
/// being filled (the unified tool returns transactions, holdings, then
/// investments, in that order) and the number of rows completed so far.
/// Parses partial JSON, so it is a heuristic — fine for a label.
fn streamed_extraction_progress(buf: &str) -> (Option<String>, u64) {
    let section = if buf.contains("\"investments\"") {
        Some("investments")
    } else if buf.contains("\"holdings\"") {
        Some("holdings")
    } else if buf.contains("\"transactions\"") {
        Some("transactions")
    } else {
        None
    };
    // Count array-element objects that have closed: each closes back to depth 2
    // (root object -> section array -> element). String bytes are skipped so
    // braces inside string values do not miscount.
    let (mut depth, mut in_str, mut esc, mut rows) = (0i32, false, false, 0u64);
    for c in buf.bytes() {
        if in_str {
            match (esc, c) {
                (true, _) => esc = false,
                (false, b'\\') => esc = true,
                (false, b'"') => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            b'"' => in_str = true,
            b'{' | b'[' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 2 {
                    rows += 1;
                }
            }
            b']' => depth -= 1,
            _ => {}
        }
    }
    (section.map(str::to_string), rows)
}

/// Byte-buffered SSE line splitter shared by the Anthropic and OpenAI
/// streams. Buffering bytes and splitting on `\n` (then trimming `\r`) means
/// a multi-byte character split across two network chunks decodes correctly,
/// which per-chunk `str::from_utf8` could not do.
#[derive(Default)]
struct SseLineSplitter {
    pending: Vec<u8>,
}

impl SseLineSplitter {
    fn new() -> Self {
        Self::default()
    }

    /// Append one network chunk and return every line completed by it.
    fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        let mut lines = Vec::new();
        let mut start = 0;
        for (i, byte) in chunk.iter().enumerate() {
            if *byte == b'\n' {
                self.pending.extend_from_slice(&chunk[start..i]);
                let line = String::from_utf8_lossy(&std::mem::take(&mut self.pending))
                    .trim_end_matches('\r')
                    .to_string();
                lines.push(line);
                start = i + 1;
            }
        }
        self.pending.extend_from_slice(&chunk[start..]);
        lines
    }
}

/// `max_tokens` returns a truncated tool input that would otherwise parse as
/// an empty success. Warn so callers see the cause.
fn warn_if_truncated(
    model: &str,
    tool_name: &str,
    stop_reason: &Option<String>,
    usage: &TokenUsage,
) {
    if stop_reason.as_deref() == Some("max_tokens") {
        tracing::warn!(
            model,
            tool_name,
            output_tokens = usage.output_tokens,
            "LLM response was truncated by max_tokens; the parsed result is likely \
             incomplete. Increase max_tokens or use a smaller input."
        );
    }
}

fn classify_reqwest_error(e: reqwest::Error) -> ProviderError {
    if e.is_timeout() {
        ProviderError::Timeout(format!("{e}"))
    } else {
        ProviderError::Unreachable(format!("{e}"))
    }
}

impl AnthropicProvider {
    async fn post_messages_streaming(
        &self,
        body: &Value,
        extra_headers: &[(&str, &str)],
        tool_name: &str,
    ) -> Result<ProviderCallResult, ProviderError> {
        let mut body = body.clone();
        {
            let obj = body
                .as_object_mut()
                .expect("request body must be an object");
            obj.insert("stream".to_string(), serde_json::Value::Bool(true));
            // Subscription (OAuth) calls must present as Claude Code: prepend the
            // Claude Code identity as the first system block. The prompt, tools,
            // and everything else are identical to the API-key path.
            if matches!(self.auth, AnthropicAuth::Subscription(_)) {
                let system = obj.get("system").cloned();
                obj.insert(
                    "system".to_string(),
                    with_claude_code_identity(system.as_ref()),
                );
            }
        }

        // Merge any caller-supplied anthropic-beta values (e.g. the PDF beta)
        // with the OAuth beta needed for subscription auth. The API accepts a
        // single comma-joined header.
        let mut beta_values: Vec<String> = extra_headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("anthropic-beta"))
            .map(|(_, v)| (*v).to_string())
            .collect();

        let mut req = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json");
        match &self.auth {
            AnthropicAuth::ApiKey(key) => {
                req = req.header("x-api-key", key);
            }
            AnthropicAuth::Subscription(token) => {
                req = req.header("authorization", format!("Bearer {token}"));
                beta_values.push("oauth-2025-04-20".to_string());
            }
        }
        if !beta_values.is_empty() {
            req = req.header("anthropic-beta", beta_values.join(","));
        }
        for (k, v) in extra_headers {
            if !k.eq_ignore_ascii_case("anthropic-beta") {
                req = req.header(*k, *v);
            }
        }

        let started = std::time::Instant::now();
        let response = req
            .json(&body)
            .send()
            .await
            .map_err(classify_reqwest_error)?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable>".to_string());
            let status_code = status.as_u16();
            return Err(match status_code {
                429 => ProviderError::RateLimit {
                    retry_after: None,
                    detail: error_body,
                },
                401 | 403 => ProviderError::AuthRejected(error_body),
                400..=499 => ProviderError::UpstreamClientError {
                    status: status_code,
                    body: error_body,
                },
                500..=599 => ProviderError::UpstreamServerError {
                    status: status_code,
                    body: error_body,
                },
                _ => ProviderError::UpstreamServerError {
                    status: status_code,
                    body: error_body,
                },
            });
        }

        let mut stream = response.bytes_stream();
        let mut accumulator = SseAccumulator::new();
        // Byte-buffered splitter: a multi-byte character split across two
        // network chunks must not fail the call.
        let mut splitter = SseLineSplitter::new();
        let mut last_progress_emit = std::time::Instant::now();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| ProviderError::StreamInterrupted(format!("{e}")))?;
            for line in splitter.push(&chunk) {
                if let Some(data) = line.strip_prefix("data: ") {
                    if data == "[DONE]" {
                        continue;
                    }
                    match serde_json::from_str::<serde_json::Value>(data) {
                        Ok(event) => {
                            accumulator.handle_event(&event);
                            self.emit_sse_progress(&accumulator, &event, &mut last_progress_emit);
                        }
                        Err(e) => {
                            tracing::warn!(
                                error = %e,
                                data_preview = &data[..data.len().min(100)],
                                "SSE: failed to parse event data, skipping"
                            );
                        }
                    }
                }
            }
        }

        let mut result = accumulator.into_result(tool_name)?;
        result.duration_ms = started.elapsed().as_millis() as u64;
        warn_if_truncated(&result.model, tool_name, &result.stop_reason, &result.usage);
        Ok(result)
    }

    fn emit_sse_progress(
        &self,
        acc: &SseAccumulator,
        event: &serde_json::Value,
        last_emit: &mut std::time::Instant,
    ) {
        let Some((tx, task_id)) = &self.progress else {
            return;
        };
        let event_type = event["type"].as_str().unwrap_or("");
        match event_type {
            "message_start" => {
                let _ = tx.send(ProgressEvent::LlmStart {
                    model: acc.model().to_string(),
                    input_tokens: acc.input_tokens(),
                    task_id: task_id.clone(),
                });
            }
            "content_block_delta" if last_emit.elapsed() >= std::time::Duration::from_secs(1) => {
                let (section, items) = acc.extraction_progress();
                let _ = tx.send(ProgressEvent::LlmProgress {
                    output_tokens: acc.output_tokens(),
                    elapsed_ms: 0,
                    items,
                    section,
                    task_id: task_id.clone(),
                });
                *last_emit = std::time::Instant::now();
            }
            _ => {}
        }
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn chat_with_tools(
        &self,
        system_prompt: &str,
        user_message: &str,
        tool_name: &str,
        tool_schema: Value,
        tier: ModelTier,
        agent_override: Option<Agent>,
    ) -> Result<ProviderCallResult, ProviderError> {
        let model = self.resolve_model(tier, agent_override)?;

        let request_body = json!({
            "model": model,
            "max_tokens": MAX_TOKENS_TEXT,
            "system": system_prompt,
            "tools": [{
                "name": tool_name,
                "description": format!("Extract structured data using the {} tool.", tool_name),
                "input_schema": tool_schema
            }],
            "tool_choice": { "type": "tool", "name": tool_name },
            "messages": [{ "role": "user", "content": user_message }]
        });

        tracing::debug!(
            provider = "anthropic",
            model = %model,
            tool_name,
            "sending text request"
        );

        self.post_messages_streaming(&request_body, &[], tool_name)
            .await
    }

    async fn chat_with_pdf_and_tools(
        &self,
        system_prompt: &str,
        pdf_bytes: &[u8],
        text_supplement: &str,
        tool_name: &str,
        tool_schema: Value,
        agent_override: Option<Agent>,
    ) -> Result<ProviderCallResult, ProviderError> {
        let b64 = BASE64.encode(pdf_bytes);
        let model = self.resolve_model(ModelTier::Advanced, agent_override)?;

        let request_body = json!({
            "model": model,
            "max_tokens": MAX_TOKENS_DOCUMENTS,
            "system": system_prompt,
            "tools": [{
                "name": tool_name,
                "description": format!("Extract structured data using the {} tool.", tool_name),
                "input_schema": tool_schema
            }],
            "tool_choice": { "type": "tool", "name": tool_name },
            "messages": [{
                "role": "user",
                "content": [
                    {
                        "type": "document",
                        "source": {
                            "type": "base64",
                            "media_type": "application/pdf",
                            "data": b64
                        }
                    },
                    {
                        "type": "text",
                        "text": text_supplement
                    }
                ]
            }]
        });

        tracing::debug!(
            provider = "anthropic",
            model = %model,
            tool_name,
            pdf_size = pdf_bytes.len(),
            "sending PDF request"
        );

        self.post_messages_streaming(
            &request_body,
            &[("anthropic-beta", "pdfs-2024-09-25")],
            tool_name,
        )
        .await
    }

    async fn chat_with_files_and_tools(
        &self,
        system_prompt: &str,
        files: &[(String, String, Vec<u8>)],
        text_supplement: &str,
        tool_name: &str,
        tool_schema: Value,
        agent_override: Option<Agent>,
    ) -> Result<ProviderCallResult, ProviderError> {
        let model = self.resolve_model(ModelTier::Advanced, agent_override)?;

        let mut content: Vec<Value> = Vec::with_capacity(files.len() * 2 + 1);
        for (filename, mime, bytes) in files {
            // Label each file by name so the model can attribute extracted rows
            // to the right document via `source_file`. PDF/image blocks carry no
            // filename of their own, so without this the model cannot tell which
            // document a row came from when several are uploaded.
            content.push(json!({
                "type": "text",
                "text": format!(
                    "Source document filename: {filename}\nFor every row you extract from the document immediately below, set its \"source_file\" field to exactly this filename."
                ),
            }));
            content.push(
                anthropic_content_block_for_file(filename, mime, bytes)
                    .map_err(|e| ProviderError::ResponseUnreadable(format!("{e}")))?,
            );
        }
        if !text_supplement.is_empty() {
            content.push(json!({ "type": "text", "text": text_supplement }));
        }

        let request_body = json!({
            "model": model,
            "max_tokens": MAX_TOKENS_DOCUMENTS,
            "system": system_prompt,
            "tools": [{
                "name": tool_name,
                "description": format!("Extract structured data using the {} tool.", tool_name),
                "input_schema": tool_schema
            }],
            "tool_choice": { "type": "tool", "name": tool_name },
            "messages": [{ "role": "user", "content": content }]
        });

        tracing::debug!(
            provider = "anthropic",
            model = %model,
            tool_name,
            files = files.len(),
            "sending files request"
        );

        self.post_messages_streaming(
            &request_body,
            &[("anthropic-beta", "pdfs-2024-09-25")],
            tool_name,
        )
        .await
    }

    fn name(&self) -> &'static str {
        match self.auth {
            AnthropicAuth::ApiKey(_) => "anthropic",
            AnthropicAuth::Subscription(_) => "anthropic-subscription",
        }
    }

    fn with_progress(
        &self,
        tx: ProgressTx,
        task_id: Option<String>,
    ) -> Option<Arc<dyn LlmProvider>> {
        Some(Arc::new(self.clone_with_progress(tx, task_id)))
    }
}

/// Prepend the Claude Code system identity as the first system block (required
/// for subscription/OAuth calls). Accepts the existing `system` value, which the
/// callers pass as a plain string, and returns a blocks array with the identity
/// first so the original system prompt is preserved verbatim.
fn with_claude_code_identity(system: Option<&Value>) -> Value {
    let identity = json!({ "type": "text", "text": CLAUDE_CODE_SYSTEM_IDENTITY });
    match system {
        Some(Value::String(s)) => json!([identity, { "type": "text", "text": s }]),
        Some(Value::Array(blocks)) => {
            let mut out = Vec::with_capacity(blocks.len() + 1);
            out.push(identity);
            out.extend(blocks.iter().cloned());
            Value::Array(out)
        }
        _ => json!([identity]),
    }
}

fn anthropic_content_block_for_file(filename: &str, mime: &str, bytes: &[u8]) -> Result<Value> {
    let mime_lower = mime.to_ascii_lowercase();
    if mime_lower == "application/pdf" {
        let b64 = BASE64.encode(bytes);
        return Ok(json!({
            "type": "document",
            "source": {
                "type": "base64",
                "media_type": "application/pdf",
                "data": b64,
            }
        }));
    }
    if mime_lower.starts_with("image/") {
        let b64 = BASE64.encode(bytes);
        return Ok(json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": mime_lower,
                "data": b64,
            }
        }));
    }
    if mime_lower.starts_with("text/")
        || mime_lower == "application/csv"
        || mime_lower == "application/json"
        || mime_lower == "application/x-ndjson"
    {
        let text = std::str::from_utf8(bytes)
            .map_err(|e| anyhow!("file '{filename}' is not valid UTF-8 text: {e}"))?;
        return Ok(json!({
            "type": "text",
            "text": format!("--- file: {filename} ({mime_lower}) ---\n{text}"),
        }));
    }
    Err(anyhow!(
        "Anthropic provider does not currently accept file '{filename}' with MIME '{mime}'. \
         Supported: application/pdf, image/*, text/*."
    ))
}

// ── Anthropic response parsing helpers (used by tests) ──────────────────────

#[allow(dead_code)]
#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
    #[serde(default)]
    usage: Option<AnthropicUsage>,
    #[serde(default)]
    stop_reason: Option<String>,
}

#[allow(dead_code)]
#[derive(Deserialize, Default)]
struct AnthropicUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(untagged)]
enum AnthropicContentBlock {
    ToolUse {
        #[serde(rename = "type")]
        block_type: String,
        name: String,
        input: Value,
    },
    Other(Value),
}

#[allow(dead_code)]
fn extract_anthropic_tool_input(body: &str, tool_name: &str) -> Result<Value> {
    let api_resp: AnthropicResponse = serde_json::from_str(body).map_err(|e| {
        anyhow!(
            "parsing Anthropic response (preview: {}): {e}",
            &body[..body.len().min(200)]
        )
    })?;

    api_resp
        .content
        .into_iter()
        .find_map(|block| match block {
            AnthropicContentBlock::ToolUse {
                block_type,
                name,
                input,
            } if block_type == "tool_use" && name == tool_name => Some(input),
            _ => None,
        })
        .ok_or_else(|| anyhow!("no {tool_name} tool_use block in Anthropic response"))
}

#[allow(dead_code)]
fn extract_anthropic_usage(body: &str) -> TokenUsage {
    serde_json::from_str::<AnthropicResponse>(body)
        .ok()
        .and_then(|r| r.usage)
        .map(|u| TokenUsage {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
        })
        .unwrap_or_default()
}

#[allow(dead_code)]
fn extract_anthropic_stop_reason(body: &str) -> Option<String> {
    serde_json::from_str::<AnthropicResponse>(body)
        .ok()
        .and_then(|r| r.stop_reason)
}

// ── OpenAIProvider ────────────────────────────────────────────────────────────

const OPENAI_DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
const OPENAI_DEFAULT_TIMEOUT_SECS: u64 = 300;
/// gpt-4o is the default model and OpenAI rejects a `max_tokens` above a
/// model's output cap, so the Anthropic 64k value cannot be the default here.
/// A local server can raise it with FYNANCE_OPENAI_MAX_TOKENS; 0 omits the
/// field entirely.
const OPENAI_DEFAULT_MAX_TOKENS: u64 = 16_384;
const OPENAI_DEFAULT_PDF_MAX_PAGES: usize = 50;

/// `FYNANCE_OPENAI_TOOL_CHOICE`. llama.cpp only honours the string forms
/// (it silently ignores the object form), so `"required"` is the default on
/// every server. `Function` keeps the spec's explicit named-tool form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenAIToolChoice {
    Required,
    Function,
    Auto,
}

/// `FYNANCE_OPENAI_PDF_MODE`. `Native` sends the PDF as an OpenAI `file`
/// content part and needs no external tool; the other modes run the
/// pdf-extract adapter and send per-page text and/or page images.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenAIPdfMode {
    Both,
    Images,
    Text,
    Native,
}

pub struct OpenAIProvider {
    client: Client,
    base_url: String,
    api_key: Option<String>,
    timeout: std::time::Duration,
    standard_model: String,
    advanced_model: String,
    tool_choice: OpenAIToolChoice,
    /// 0 means the field is omitted from the request.
    max_tokens: u64,
    /// None sends no temperature at all: some OpenAI reasoning models reject
    /// a non-default temperature.
    temperature: Option<f64>,
    /// `FYNANCE_OPENAI_EXTRA_BODY`, merged into every request last.
    extra_body: serde_json::Map<String, Value>,
    pdf_mode: OpenAIPdfMode,
    /// Page cap counted across all PDFs in a single call.
    pdf_max_pages: usize,
    pdf_extract: PdfExtractConfig,
    /// `FYNANCE_OPENAI_STREAM`: streaming is on unless explicitly disabled.
    stream: bool,
    progress: Option<(ProgressTx, Option<String>)>,
}

impl std::fmt::Debug for OpenAIProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the credential itself.
        f.debug_struct("OpenAIProvider")
            .field("client", &self.client)
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("timeout", &self.timeout)
            .field("standard_model", &self.standard_model)
            .field("advanced_model", &self.advanced_model)
            .field("tool_choice", &self.tool_choice)
            .field("max_tokens", &self.max_tokens)
            .field("temperature", &self.temperature)
            .field("extra_body", &self.extra_body)
            .field("pdf_mode", &self.pdf_mode)
            .field("pdf_max_pages", &self.pdf_max_pages)
            .field("pdf_extract", &self.pdf_extract)
            .field("stream", &self.stream)
            .field("progress", &self.progress.as_ref().map(|_| "<progress>"))
            .finish()
    }
}

/// Request keys the provider owns; an `EXTRA_BODY` entry for one of these
/// would silently fight with the provider's own value, so it is rejected at
/// startup.
const OPENAI_RESERVED_BODY_KEYS: &[&str] = &[
    "model",
    "messages",
    "tools",
    "tool_choice",
    "stream",
    "stream_options",
];

impl OpenAIProvider {
    /// Build from environment variables.
    ///
    /// Required: `FYNANCE_OPENAI_API_KEY`, unless `FYNANCE_OPENAI_BASE_URL`
    /// points at an OpenAI-compatible server that needs no key.
    /// Optional: `FYNANCE_OPENAI_BASE_URL`, `FYNANCE_OPENAI_TIMEOUT_SECS`,
    /// `FYNANCE_OPENAI_TEXT_MODEL`, `FYNANCE_OPENAI_PDF_MODEL`,
    /// `FYNANCE_OPENAI_TOOL_CHOICE`, `FYNANCE_OPENAI_MAX_TOKENS`,
    /// `FYNANCE_OPENAI_TEMPERATURE`, `FYNANCE_OPENAI_EXTRA_BODY`,
    /// `FYNANCE_OPENAI_PDF_MODE`, `FYNANCE_OPENAI_PDF_MAX_PAGES`, plus the
    /// `FYNANCE_PDF_EXTRACT_*` variables read by [`PdfExtractConfig::from_env`].
    pub fn from_env() -> Result<Self> {
        let custom_base_url = std::env::var("FYNANCE_OPENAI_BASE_URL")
            .ok()
            .filter(|v| !v.trim().is_empty());
        let raw_api_key = std::env::var("FYNANCE_OPENAI_API_KEY").ok();
        let api_key = raw_api_key.clone().filter(|v| !v.trim().is_empty());
        if api_key.is_none() && custom_base_url.is_none() {
            return Err(match raw_api_key {
                Some(_) => anyhow!(
                    "FYNANCE_OPENAI_API_KEY is set but empty. \
                     Set it to your OpenAI API key, or set FYNANCE_OPENAI_BASE_URL \
                     to use a local OpenAI-compatible server that needs no key."
                ),
                None => anyhow!(
                    "FYNANCE_OPENAI_API_KEY is not set. \
                     Set it in your .env file to use the OpenAI provider, or set \
                     FYNANCE_OPENAI_BASE_URL to use a local OpenAI-compatible server \
                     that needs no key."
                ),
            });
        }
        let base_url = custom_base_url
            .as_deref()
            .unwrap_or(OPENAI_DEFAULT_BASE_URL)
            .trim()
            .trim_end_matches('/')
            .to_string();
        validate_openai_base_url(&base_url)?;
        let timeout_secs = match std::env::var("FYNANCE_OPENAI_TIMEOUT_SECS") {
            Ok(v) => v.trim().parse::<u64>().map_err(|_| {
                anyhow!("FYNANCE_OPENAI_TIMEOUT_SECS must be a whole number of seconds, got '{v}'")
            })?,
            Err(_) => OPENAI_DEFAULT_TIMEOUT_SECS,
        };
        let standard_model = std::env::var("FYNANCE_OPENAI_TEXT_MODEL")
            .unwrap_or_else(|_| "gpt-4o-mini".to_string());
        let advanced_model =
            std::env::var("FYNANCE_OPENAI_PDF_MODEL").unwrap_or_else(|_| "gpt-4o".to_string());

        let tool_choice = match env_or_default("FYNANCE_OPENAI_TOOL_CHOICE", "required").as_str() {
            "required" => OpenAIToolChoice::Required,
            "function" => OpenAIToolChoice::Function,
            "auto" => OpenAIToolChoice::Auto,
            other => {
                return Err(anyhow!(
                    "FYNANCE_OPENAI_TOOL_CHOICE must be one of required, function or auto, \
                     got '{other}'"
                ));
            }
        };

        let max_tokens = match std::env::var("FYNANCE_OPENAI_MAX_TOKENS") {
            Ok(v) => v.trim().parse::<u64>().map_err(|_| {
                anyhow!(
                    "FYNANCE_OPENAI_MAX_TOKENS must be a whole number of tokens \
                     (0 omits the field), got '{v}'"
                )
            })?,
            Err(_) => OPENAI_DEFAULT_MAX_TOKENS,
        };

        let temperature =
            match std::env::var("FYNANCE_OPENAI_TEMPERATURE") {
                Ok(v) => Some(v.trim().parse::<f64>().map_err(|_| {
                    anyhow!("FYNANCE_OPENAI_TEMPERATURE must be a number, got '{v}'")
                })?),
                Err(_) => None,
            };

        let extra_body = match std::env::var("FYNANCE_OPENAI_EXTRA_BODY") {
            Ok(raw) => {
                let parsed: Value = serde_json::from_str(&raw).map_err(|e| {
                    anyhow!("FYNANCE_OPENAI_EXTRA_BODY must be a JSON object, got '{raw}': {e}")
                })?;
                let obj = parsed
                    .as_object()
                    .cloned()
                    .ok_or_else(|| anyhow!("FYNANCE_OPENAI_EXTRA_BODY must be a JSON object"))?;
                for key in OPENAI_RESERVED_BODY_KEYS {
                    if obj.contains_key(*key) {
                        return Err(anyhow!(
                            "FYNANCE_OPENAI_EXTRA_BODY must not set '{key}': the provider \
                             controls that request field"
                        ));
                    }
                }
                obj
            }
            Err(_) => serde_json::Map::new(),
        };

        let pdf_mode = match env_or_default("FYNANCE_OPENAI_PDF_MODE", "both").as_str() {
            "both" => OpenAIPdfMode::Both,
            "images" => OpenAIPdfMode::Images,
            "text" => OpenAIPdfMode::Text,
            "native" => OpenAIPdfMode::Native,
            other => {
                return Err(anyhow!(
                    "FYNANCE_OPENAI_PDF_MODE must be one of both, images, text or native, \
                     got '{other}'"
                ));
            }
        };

        let pdf_max_pages = match std::env::var("FYNANCE_OPENAI_PDF_MAX_PAGES") {
            Ok(v) => v.trim().parse::<usize>().map_err(|_| {
                anyhow!("FYNANCE_OPENAI_PDF_MAX_PAGES must be a whole number of pages, got '{v}'")
            })?,
            Err(_) => OPENAI_DEFAULT_PDF_MAX_PAGES,
        };

        let pdf_extract = PdfExtractConfig::from_env()?;

        let stream = match std::env::var("FYNANCE_OPENAI_STREAM") {
            Ok(v) => match v.trim().to_ascii_lowercase().as_str() {
                "true" => true,
                "false" => false,
                other => {
                    return Err(anyhow!(
                        "FYNANCE_OPENAI_STREAM must be true or false, got '{other}'"
                    ));
                }
            },
            Err(_) => true,
        };

        let client = Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| anyhow!("failed to build HTTP client: {e}"))?;

        Ok(Self {
            client,
            base_url,
            api_key,
            timeout: std::time::Duration::from_secs(timeout_secs),
            standard_model,
            advanced_model,
            tool_choice,
            max_tokens,
            temperature,
            extra_body,
            pdf_mode,
            pdf_max_pages,
            pdf_extract,
            stream,
            progress: None,
        })
    }

    pub fn clone_with_progress(&self, tx: ProgressTx, task_id: Option<String>) -> Self {
        Self {
            client: self.client.clone(),
            base_url: self.base_url.clone(),
            api_key: self.api_key.clone(),
            timeout: self.timeout,
            standard_model: self.standard_model.clone(),
            advanced_model: self.advanced_model.clone(),
            tool_choice: self.tool_choice,
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            extra_body: self.extra_body.clone(),
            pdf_mode: self.pdf_mode,
            pdf_max_pages: self.pdf_max_pages,
            pdf_extract: self.pdf_extract.clone(),
            stream: self.stream,
            progress: Some((tx, task_id)),
        }
    }

    fn model_for_tier(&self, tier: ModelTier) -> &str {
        match tier {
            ModelTier::Standard => &self.standard_model,
            ModelTier::Advanced => &self.advanced_model,
        }
    }

    /// The one request builder for every transport (streamed or not): system
    /// message, one function tool, `tool_choice` per config, `max_tokens`
    /// unless 0, `temperature` when set, then the `EXTRA_BODY` keys merged
    /// last so they win.
    fn build_request(
        &self,
        model: &str,
        system_prompt: &str,
        user_content: Value,
        tool_name: &str,
        tool_schema: &Value,
    ) -> Value {
        let tool_choice = match self.tool_choice {
            OpenAIToolChoice::Required => json!("required"),
            OpenAIToolChoice::Function => {
                json!({"type": "function", "function": { "name": tool_name }})
            }
            OpenAIToolChoice::Auto => json!("auto"),
        };
        let mut request = json!({
            "model": model,
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": user_content }
            ],
            "tools": [{
                "type": "function",
                "function": {
                    "name": tool_name,
                    "description": format!("Extract structured data using the {} tool.", tool_name),
                    "parameters": tool_schema
                }
            }],
            "tool_choice": tool_choice
        });
        if self.max_tokens != 0 {
            request["max_tokens"] = json!(self.max_tokens);
        }
        if let Some(temperature) = self.temperature {
            request["temperature"] = json!(temperature);
        }
        if let Some(obj) = request.as_object_mut() {
            for (key, value) in &self.extra_body {
                obj.insert(key.clone(), value.clone());
            }
        }
        request
    }
}

fn env_or_default(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
}

/// Rejects base URLs that would silently build a wrong request URL: a missing
/// scheme (`localhost:8080/v1` parses with scheme `localhost`), or a query or
/// fragment, which would end up in front of the appended `/chat/completions`.
fn validate_openai_base_url(base_url: &str) -> Result<()> {
    let invalid = |reason: &str| {
        anyhow!(
            "FYNANCE_OPENAI_BASE_URL is not a valid base URL ({reason}), got '{base_url}'. \
             Use a full http(s) URL such as http://127.0.0.1:8080/v1"
        )
    };
    let url = reqwest::Url::parse(base_url).map_err(|e| invalid(&e.to_string()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(invalid("scheme must be http or https"));
    }
    if url.query().is_some() {
        return Err(invalid("it must not contain a query string"));
    }
    if url.fragment().is_some() {
        return Err(invalid("it must not contain a fragment"));
    }
    Ok(())
}

#[async_trait]
impl LlmProvider for OpenAIProvider {
    async fn chat_with_tools(
        &self,
        system_prompt: &str,
        user_message: &str,
        tool_name: &str,
        tool_schema: Value,
        tier: ModelTier,
        agent_override: Option<Agent>,
    ) -> Result<ProviderCallResult, ProviderError> {
        if agent_override.is_some() {
            return Err(ProviderError::NotSupported(
                "agent override is only supported with the Anthropic provider. \
                 Set FYNANCE_PARSE_PROVIDER=anthropic to use agent overrides."
                    .to_string(),
            ));
        }
        let model = self.model_for_tier(tier).to_string();
        let request_body = self.build_request(
            &model,
            system_prompt,
            json!(user_message),
            tool_name,
            &tool_schema,
        );

        tracing::debug!(
            provider = "openai",
            model = %model,
            tool_name,
            "sending text request"
        );

        self.call_model(&request_body, &model, tool_name, &tool_schema)
            .await
    }

    async fn chat_with_pdf_and_tools(
        &self,
        system_prompt: &str,
        pdf_bytes: &[u8],
        text_supplement: &str,
        tool_name: &str,
        tool_schema: Value,
        agent_override: Option<Agent>,
    ) -> Result<ProviderCallResult, ProviderError> {
        let files = vec![(
            "document.pdf".to_string(),
            "application/pdf".to_string(),
            pdf_bytes.to_vec(),
        )];
        self.chat_with_files_and_tools(
            system_prompt,
            &files,
            text_supplement,
            tool_name,
            tool_schema,
            agent_override,
        )
        .await
    }

    async fn chat_with_files_and_tools(
        &self,
        system_prompt: &str,
        files: &[(String, String, Vec<u8>)],
        text_supplement: &str,
        tool_name: &str,
        tool_schema: Value,
        agent_override: Option<Agent>,
    ) -> Result<ProviderCallResult, ProviderError> {
        if agent_override.is_some() {
            return Err(ProviderError::NotSupported(
                "agent override is only supported with the Anthropic provider. \
                 Set FYNANCE_PARSE_PROVIDER=anthropic to use agent overrides."
                    .to_string(),
            ));
        }
        let model = self.model_for_tier(ModelTier::Advanced).to_string();

        let mut content: Vec<Value> = Vec::with_capacity(files.len() * 2 + 1);
        let mut warnings: Vec<String> = Vec::new();
        let mut pdf_pages_total = 0usize;
        for (filename, mime, bytes) in files {
            // Same per-file label the Anthropic and Gemini providers use, so
            // the model can attribute rows via `source_file`.
            content.push(json!({
                "type": "text",
                "text": format!(
                    "Source document filename: {filename}\nFor every row you extract from the document immediately below, set its \"source_file\" field to exactly this filename."
                ),
            }));

            let mime_lower = mime.to_ascii_lowercase();
            if mime_lower == "application/pdf" {
                if self.pdf_mode == OpenAIPdfMode::Native {
                    content.push(json!({
                        "type": "file",
                        "file": {
                            "filename": filename,
                            "file_data": format!(
                                "data:application/pdf;base64,{}",
                                BASE64.encode(bytes)
                            ),
                        }
                    }));
                    continue;
                }
                let extracted = pdf_extract::extract_pdf(bytes, filename, &self.pdf_extract)
                    .await
                    .map_err(pdf_extract_err_to_provider_err)?;
                if !extracted.page_errors.is_empty() {
                    // Some pages came out; send what we have rather than
                    // failing the whole call, but tell the model which pages
                    // are missing and record it for the preview notes.
                    tracing::warn!(
                        filename,
                        failed_pages = extracted.page_errors.len(),
                        "pdf-extract: some pages failed; continuing with the pages that worked"
                    );
                    for (page, error) in &extracted.page_errors {
                        content.push(json!({
                            "type": "text",
                            "text": format!(
                                "Page {page} of {filename} could not be extracted: {error}"
                            ),
                        }));
                        warnings.push(format!(
                            "page {page} of {filename} could not be extracted: {error}"
                        ));
                    }
                }
                pdf_pages_total += extracted.pages.len();
                if pdf_pages_total > self.pdf_max_pages {
                    return Err(ProviderError::DocumentConversion(format!(
                        "the uploaded PDF documents expand to {pdf_pages_total} page images, \
                         more than FYNANCE_OPENAI_PDF_MAX_PAGES ({}) allows in one call: \
                         split the upload into smaller files and try again",
                        self.pdf_max_pages
                    )));
                }
                // The document's real page count from the manifest: a page
                // label like "Page 3 of 2" (failed or filtered pages) would
                // mislead the model. Fall back to the pages we got when the
                // manifest omits it.
                let page_count = extracted.page_count.unwrap_or(extracted.pages.len() as u32);
                for page in &extracted.pages {
                    content.push(json!({
                        "type": "text",
                        "text": format!("Page {} of {page_count} of {filename}", page.page),
                    }));
                    let has_text = page.has_text_layer && !page.text.is_empty();
                    let has_image = !page.png.is_empty();
                    let send_text = match self.pdf_mode {
                        OpenAIPdfMode::Both => has_text,
                        OpenAIPdfMode::Text => has_text || !has_image,
                        OpenAIPdfMode::Images => !has_image && has_text,
                        OpenAIPdfMode::Native => unreachable!(),
                    };
                    let send_image = match self.pdf_mode {
                        OpenAIPdfMode::Both | OpenAIPdfMode::Images => has_image,
                        OpenAIPdfMode::Text => has_image && !has_text,
                        OpenAIPdfMode::Native => unreachable!(),
                    };
                    if send_text {
                        content.push(json!({ "type": "text", "text": page.text }));
                    }
                    if send_image {
                        content.push(image_url_part("image/png", &page.png));
                    }
                }
                continue;
            }

            if mime_lower.starts_with("image/") {
                content.push(image_url_part(&mime_lower, bytes));
                continue;
            }
            if mime_lower.starts_with("text/")
                || mime_lower == "application/csv"
                || mime_lower == "application/json"
                || mime_lower == "application/x-ndjson"
            {
                // Lossy decode, like Gemini: a stray non-UTF-8 byte in a CSV
                // must not fail the whole parse.
                let text = String::from_utf8_lossy(bytes);
                content.push(json!({
                    "type": "text",
                    "text": format!("--- file: {filename} ({mime_lower}) ---\n{text}"),
                }));
                continue;
            }
            return Err(ProviderError::NotSupported(format!(
                "OpenAI provider does not accept file '{filename}' with MIME '{mime}'. \
                 Supported: application/pdf, image/*, text/*, CSV, JSON and NDJSON."
            )));
        }
        if !text_supplement.is_empty() {
            content.push(json!({ "type": "text", "text": text_supplement }));
        }

        let request_body = self.build_request(
            &model,
            system_prompt,
            json!(content),
            tool_name,
            &tool_schema,
        );

        tracing::debug!(
            provider = "openai",
            model = %model,
            tool_name,
            files = files.len(),
            "sending files request"
        );

        let mut result = self
            .call_model(&request_body, &model, tool_name, &tool_schema)
            .await?;
        result.warnings.extend(warnings);
        Ok(result)
    }

    fn name(&self) -> &'static str {
        "openai"
    }

    fn with_progress(
        &self,
        tx: ProgressTx,
        task_id: Option<String>,
    ) -> Option<Arc<dyn LlmProvider>> {
        Some(Arc::new(self.clone_with_progress(tx, task_id)))
    }
}

fn image_url_part(mime: &str, bytes: &[u8]) -> Value {
    json!({
        "type": "image_url",
        "image_url": { "url": format!("data:{mime};base64,{}", BASE64.encode(bytes)) }
    })
}

/// pdf-extract failures surfaced to the user: an unconfigured or missing tool
/// is a setup problem (`provider_not_supported`, whose message names the fix),
/// anything else the tool ran and failed (`document_conversion_failed`).
fn pdf_extract_err_to_provider_err(e: pdf_extract::PdfExtractError) -> ProviderError {
    match e {
        pdf_extract::PdfExtractError::NotConfigured
        | pdf_extract::PdfExtractError::NotFound { .. } => {
            ProviderError::NotSupported(e.to_string())
        }
        other => ProviderError::DocumentConversion(other.to_string()),
    }
}

/// Non-streaming transport: POST, status mapping, then the shared response
/// post-processing.
impl OpenAIProvider {
    async fn post_chat_completion(
        &self,
        request_body: &Value,
        model: &str,
        tool_name: &str,
        tool_schema: &Value,
    ) -> Result<ProviderCallResult, ProviderError> {
        let started = Instant::now();
        let mut request = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .timeout(self.timeout)
            .header("Content-Type", "application/json");
        if let Some(api_key) = &self.api_key {
            request = request.header("Authorization", format!("Bearer {api_key}"));
        }
        let response = request
            .json(request_body)
            .send()
            .await
            .map_err(classify_reqwest_error)?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| ProviderError::ResponseUnreadable(format!("{e}")))?;
        let duration_ms = started.elapsed().as_millis() as u64;

        if !status.is_success() {
            let status_code = status.as_u16();
            return Err(match status_code {
                429 => ProviderError::RateLimit {
                    retry_after: None,
                    detail: body,
                },
                401 | 403 => ProviderError::AuthRejected(body),
                400..=499 => ProviderError::UpstreamClientError {
                    status: status_code,
                    body,
                },
                _ => ProviderError::UpstreamServerError {
                    status: status_code,
                    body,
                },
            });
        }

        let api_resp: OpenAIResponse = serde_json::from_str(&body).map_err(|e| {
            ProviderError::ResponseUnreadable(format!(
                "parsing OpenAI response (preview: {}): {e}",
                preview_chars(&body, 200)
            ))
        })?;
        let usage = api_resp
            .usage
            .map(|u| TokenUsage {
                input_tokens: u.prompt_tokens,
                output_tokens: u.completion_tokens,
            })
            .unwrap_or_default();
        let reply = api_resp
            .choices
            .first()
            .map(reply_from_choice)
            .ok_or_else(|| {
                ProviderError::ResponseUnreadable("no choices in OpenAI response".to_string())
            })?;
        let (value, stop_reason) =
            post_process_openai_reply(&reply, tool_name, tool_schema, usage.output_tokens)?;

        Ok(ProviderCallResult {
            value,
            usage,
            model: model.to_string(),
            duration_ms,
            stop_reason,
            warnings: Vec::new(),
        })
    }
}

// ── OpenAI streamed transport ────────────────────────────────────────────────

/// Accumulates chat-completion SSE chunks into the same `OpenAIReply` the
/// non-streaming body produces, so post-processing happens in one place.
#[derive(Default)]
struct OpenAIStreamAccumulator {
    model: Option<String>,
    usage: Option<OpenAIUsage>,
    finish_reason: Option<String>,
    content: String,
    /// Tool-call parts keyed by delta index, in arrival order: the first
    /// non-empty `name` wins, `arguments` strings concatenate.
    parts: Vec<(usize, String, String)>,
    delta_count: u64,
}

impl OpenAIStreamAccumulator {
    /// Feed one parsed SSE chunk. Returns whether it carried any content or
    /// tool-call delta (the progress trigger). `reasoning_content` is
    /// deliberately ignored.
    fn handle_chunk(&mut self, chunk: &Value) -> bool {
        if let Some(model) = chunk["model"].as_str() {
            self.model = Some(model.to_string());
        }
        if let Some(usage) = chunk.get("usage").filter(|u| !u.is_null()) {
            if let Ok(parsed) = serde_json::from_value::<OpenAIUsage>(usage.clone()) {
                self.usage = Some(parsed);
            }
        }
        let mut saw_delta = false;
        if let Some(choice) = chunk["choices"].get(0) {
            let delta = &choice["delta"];
            if let Some(text) = delta["content"].as_str() {
                self.content.push_str(text);
                saw_delta = true;
            }
            if let Some(calls) = delta["tool_calls"].as_array() {
                for call in calls {
                    let index = call["index"].as_u64().unwrap_or(0) as usize;
                    let name = call["function"]["name"].as_str().unwrap_or("");
                    let args = call["function"]["arguments"].as_str().unwrap_or("");
                    match self.parts.iter_mut().find(|(i, _, _)| *i == index) {
                        Some(entry) => {
                            if entry.1.is_empty() {
                                entry.1 = name.to_string();
                            }
                            entry.2.push_str(args);
                        }
                        None => self.parts.push((index, name.to_string(), args.to_string())),
                    }
                    if !name.is_empty() || !args.is_empty() {
                        saw_delta = true;
                    }
                }
            }
            if let Some(reason) = choice["finish_reason"].as_str() {
                self.finish_reason = Some(reason.to_string());
            }
        }
        if saw_delta {
            self.delta_count += 1;
        }
        saw_delta
    }

    fn tool_args(&self) -> String {
        self.parts
            .iter()
            .map(|(_, _, args)| args.as_str())
            .collect()
    }

    fn into_reply(self) -> OpenAIReply {
        let mut reply = OpenAIReply {
            content: (!self.content.is_empty()).then_some(self.content),
            tool_calls: Vec::new(),
            finish_reason: self.finish_reason,
            invalid_arguments: None,
        };
        for (_, name, args) in &self.parts {
            match serde_json::from_str::<Value>(args) {
                Ok(parsed) => reply.tool_calls.push((name.clone(), parsed)),
                Err(e) => {
                    if reply.invalid_arguments.is_none() {
                        reply.invalid_arguments = Some(format!(
                            "parsing streamed OpenAI function arguments for {name}: {e} \
                             (arguments: {})",
                            preview_chars(args, 200)
                        ));
                    }
                }
            }
        }
        reply
    }
}

impl OpenAIProvider {
    /// The one transport every chat call goes through: streamed unless
    /// `FYNANCE_OPENAI_STREAM=false`.
    async fn call_model(
        &self,
        request_body: &Value,
        model: &str,
        tool_name: &str,
        tool_schema: &Value,
    ) -> Result<ProviderCallResult, ProviderError> {
        if self.stream {
            self.post_chat_completion_streamed(request_body, model, tool_name, tool_schema)
                .await
        } else {
            self.post_chat_completion(request_body, model, tool_name, tool_schema)
                .await
        }
    }

    /// Streaming transport: the same request plus `stream` and
    /// `stream_options.include_usage`, an SSE read loop with an overall
    /// deadline, then the shared response post-processing. Dropping this
    /// future drops the body stream, which closes the connection and cancels
    /// generation upstream.
    async fn post_chat_completion_streamed(
        &self,
        request_body: &Value,
        model: &str,
        tool_name: &str,
        tool_schema: &Value,
    ) -> Result<ProviderCallResult, ProviderError> {
        let started = Instant::now();
        let mut body = request_body.clone();
        {
            let obj = body
                .as_object_mut()
                .expect("request body must be an object");
            obj.insert("stream".to_string(), json!(true));
            obj.insert(
                "stream_options".to_string(),
                json!({ "include_usage": true }),
            );
        }

        let mut request = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .timeout(self.timeout)
            .header("Content-Type", "application/json");
        if let Some(api_key) = &self.api_key {
            request = request.header("Authorization", format!("Bearer {api_key}"));
        }
        let response = request
            .json(&body)
            .send()
            .await
            .map_err(classify_reqwest_error)?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable>".to_string());
            let status_code = status.as_u16();
            return Err(match status_code {
                429 => ProviderError::RateLimit {
                    retry_after: None,
                    detail: error_body,
                },
                401 | 403 => ProviderError::AuthRejected(error_body),
                400..=499 => ProviderError::UpstreamClientError {
                    status: status_code,
                    body: error_body,
                },
                _ => ProviderError::UpstreamServerError {
                    status: status_code,
                    body: error_body,
                },
            });
        }

        let mut stream = response.bytes_stream();
        let mut splitter = SseLineSplitter::new();
        let mut accumulator = OpenAIStreamAccumulator::default();
        let mut last_progress_emit = Instant::now();
        let mut first_chunk = true;

        tokio::time::timeout(self.timeout, async {
            while let Some(chunk) = stream.next().await {
                let chunk = match chunk {
                    Ok(chunk) => chunk,
                    Err(e) if e.is_timeout() => {
                        return Err(ProviderError::Timeout(format!("{e}")));
                    }
                    Err(e) => {
                        return Err(ProviderError::StreamInterrupted(format!("{e}")));
                    }
                };
                for line in splitter.push(&chunk) {
                    if line.is_empty() || line.starts_with(':') || line.starts_with("event:") {
                        continue;
                    }
                    // Some servers (llama.cpp builds among them) report
                    // in-stream failures as an `error:` event line instead of
                    // a data payload.
                    if let Some(payload) = line.strip_prefix("error:") {
                        return Err(ProviderError::UpstreamServerError {
                            status: 0,
                            body: payload.trim().to_string(),
                        });
                    }
                    let Some(data) = line.strip_prefix("data:").map(str::trim_start) else {
                        continue;
                    };
                    if data == "[DONE]" {
                        return Ok(());
                    }
                    let Ok(event) = serde_json::from_str::<Value>(data) else {
                        tracing::warn!(
                            data_preview = preview_chars(data, 100),
                            "OpenAI SSE: failed to parse chunk, skipping"
                        );
                        continue;
                    };
                    if !event["error"].is_null() {
                        return Err(ProviderError::UpstreamServerError {
                            status: 0,
                            body: event["error"].to_string(),
                        });
                    }
                    if first_chunk {
                        first_chunk = false;
                        if let Some((tx, task_id)) = &self.progress {
                            let _ = tx.send(ProgressEvent::LlmStart {
                                model: model.to_string(),
                                input_tokens: 0,
                                task_id: task_id.clone(),
                            });
                        }
                    }
                    let saw_delta = accumulator.handle_chunk(&event);
                    if saw_delta
                        && last_progress_emit.elapsed() >= std::time::Duration::from_secs(1)
                    {
                        if let Some((tx, task_id)) = &self.progress {
                            let (section, items) =
                                streamed_extraction_progress(&accumulator.tool_args());
                            let _ = tx.send(ProgressEvent::LlmProgress {
                                output_tokens: accumulator.delta_count,
                                elapsed_ms: 0,
                                items,
                                section,
                                task_id: task_id.clone(),
                            });
                        }
                        last_progress_emit = Instant::now();
                    }
                }
            }
            Ok(())
        })
        .await
        .map_err(|_| {
            ProviderError::Timeout(format!(
                "streamed response stalled: no completion within {}s",
                self.timeout.as_secs()
            ))
        })??;

        let usage = accumulator
            .usage
            .as_ref()
            .map(|u| TokenUsage {
                input_tokens: u.prompt_tokens,
                output_tokens: u.completion_tokens,
            })
            .unwrap_or_default();
        let model = accumulator
            .model
            .clone()
            .unwrap_or_else(|| model.to_string());
        let reply = accumulator.into_reply();
        let (value, stop_reason) =
            post_process_openai_reply(&reply, tool_name, tool_schema, usage.output_tokens)?;

        Ok(ProviderCallResult {
            value,
            usage,
            model,
            duration_ms: started.elapsed().as_millis() as u64,
            stop_reason,
            warnings: Vec::new(),
        })
    }
}

// ── OpenAI response parsing helpers ──────────────────────────────────────────

#[derive(Deserialize)]
struct OpenAIResponse {
    #[serde(default)]
    choices: Vec<OpenAIChoice>,
    #[serde(default)]
    usage: Option<OpenAIUsage>,
}

#[derive(Deserialize, Default)]
struct OpenAIUsage {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
}

#[derive(Deserialize, Default)]
struct OpenAIChoice {
    #[serde(default)]
    message: OpenAIMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize, Default)]
struct OpenAIMessage {
    #[serde(default)]
    tool_calls: Option<Vec<OpenAIToolCall>>,
    #[serde(default)]
    content: Option<String>,
}

#[derive(Deserialize)]
struct OpenAIToolCall {
    function: OpenAIFunctionCall,
}

#[derive(Deserialize)]
struct OpenAIFunctionCall {
    #[serde(default)]
    name: String,
    /// Some servers send the arguments as a JSON string, others (and streamed
    /// deltas once accumulated) as an object. Accept both.
    #[serde(default)]
    arguments: Value,
}

/// The provider-neutral view of one chat reply, the same for the non-stream
/// body and the accumulated stream, so post-processing happens in one place.
struct OpenAIReply {
    content: Option<String>,
    /// (tool name, tool input) for every tool call, arguments already parsed.
    tool_calls: Vec<(String, Value)>,
    finish_reason: Option<String>,
    /// First unparsable `arguments` string, if any: only reported when no
    /// correctly-named tool call exists anyway.
    invalid_arguments: Option<String>,
}

fn reply_from_choice(choice: &OpenAIChoice) -> OpenAIReply {
    let mut reply = OpenAIReply {
        content: choice.message.content.clone(),
        tool_calls: Vec::new(),
        finish_reason: choice.finish_reason.clone(),
        invalid_arguments: None,
    };
    if let Some(calls) = &choice.message.tool_calls {
        for call in calls {
            match &call.function.arguments {
                Value::String(raw) => match serde_json::from_str(raw) {
                    Ok(parsed) => reply.tool_calls.push((call.function.name.clone(), parsed)),
                    Err(e) => {
                        if reply.invalid_arguments.is_none() {
                            reply.invalid_arguments = Some(format!(
                                "parsing OpenAI function arguments for {}: {e} (arguments: {})",
                                call.function.name,
                                preview_chars(raw, 200)
                            ));
                        }
                    }
                },
                other => reply
                    .tool_calls
                    .push((call.function.name.clone(), other.clone())),
            }
        }
    }
    reply
}

/// finish_reason for the API surface: the OpenAI spelling of truncation is
/// `"length"`, the rest of the pipeline speaks `"max_tokens"`.
fn map_openai_finish_reason(finish_reason: Option<&str>) -> Option<String> {
    match finish_reason {
        Some("length") => Some("max_tokens".to_string()),
        other => other.map(str::to_string),
    }
}

/// Shared result handling for both transports. A `length` finish_reason is a
/// truncation, never a parse; a correctly-named tool call wins; otherwise the
/// tool input may still be recoverable from the message content (thinking
/// models that emit the call as text when `tool_choice` is not enforced).
fn post_process_openai_reply(
    reply: &OpenAIReply,
    tool_name: &str,
    tool_schema: &Value,
    output_tokens: u64,
) -> Result<(Value, Option<String>), ProviderError> {
    if reply.finish_reason.as_deref() == Some("length") {
        return Err(ProviderError::Truncated { output_tokens });
    }

    if !reply.tool_calls.is_empty() {
        if let Some((_, input)) = reply.tool_calls.iter().find(|(name, _)| name == tool_name) {
            return Ok((
                input.clone(),
                map_openai_finish_reason(reply.finish_reason.as_deref()),
            ));
        }
    }

    if let Some(content) = &reply.content {
        if let Some(input) = recover_tool_input_from_content(content, tool_name, tool_schema) {
            tracing::warn!(
                tool_name,
                "OpenAI reply carried no tool call; recovered the tool input from message content"
            );
            return Ok((
                input,
                map_openai_finish_reason(reply.finish_reason.as_deref()),
            ));
        }
    }

    if let Some(detail) = &reply.invalid_arguments {
        // The most common local-model failure: slightly broken JSON
        // arguments. That is an unreadable reply, not a refusal to call the
        // tool, and the parse detail is worth keeping.
        return Err(ProviderError::ResponseUnreadable(detail.clone()));
    }

    Err(ProviderError::NoToolUse {
        tool_name: tool_name.to_string(),
    })
}

/// Only a plausible tool input is ever accepted from message content: prose,
/// arrays and scalars must produce `NoToolUse`, not an empty extraction. An
/// envelope-shaped object counts only when it names this tool; a bare object
/// counts only when it actually carries properties of the tool's schema, so
/// `{}` or an error payload never becomes a "successful" zero-row parse.
fn recover_tool_input_from_content(
    content: &str,
    tool_name: &str,
    tool_schema: &Value,
) -> Option<Value> {
    let cleaned = strip_think_tags(content);
    let candidate = extract_between(&cleaned, "<tool_call>", "</tool_call>")
        .or_else(|| extract_fenced_json(&cleaned))
        .unwrap_or_else(|| cleaned.trim().to_string());
    let parsed: Value = serde_json::from_str(&candidate).ok()?;
    let obj = parsed.as_object()?;
    if obj.get("name").and_then(Value::as_str) == Some(tool_name) {
        match obj.get("arguments") {
            Some(Value::String(raw)) => serde_json::from_str(raw).ok(),
            Some(arguments @ Value::Object(_)) => Some(arguments.clone()),
            _ => None,
        }
    } else if obj.contains_key("name") || obj.contains_key("arguments") {
        // Another tool's call, or a half-envelope: not ours to recover.
        None
    } else {
        // A bare object counts only when it carries properties of this tool's
        // schema, so `{}` and error payloads never become an extraction. Every
        // real call site passes a schema with properties; one without cannot
        // vouch for anything.
        tool_schema
            .get("properties")
            .and_then(Value::as_object)
            .and_then(|properties| {
                obj.keys()
                    .any(|k| properties.contains_key(k))
                    .then(|| Value::Object(obj.clone()))
            })
    }
}

/// First `max_chars` characters of `s`, on char boundaries (byte-slicing can
/// panic mid multi-byte character in model output).
fn preview_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let head: String = s.chars().take(max_chars).collect();
    format!("{head}\u{2026}")
}

fn strip_think_tags(content: &str) -> String {
    let mut out = content.to_string();
    while let Some(start) = out.find("<think>") {
        let after = &out[start + "<think>".len()..];
        match after.find("</think>") {
            Some(end) => out = format!("{}{}", &out[..start], &after[end + "</think>".len()..]),
            None => {
                out.truncate(start);
                break;
            }
        }
    }
    out
}

fn extract_between(hay: &str, open: &str, close: &str) -> Option<String> {
    let start = hay.find(open)? + open.len();
    let end = hay[start..].find(close)?;
    Some(hay[start..start + end].to_string())
}

/// Body of the first fenced code block (```` ```json ```` or a plain fence;
/// a language tag on the opening line is skipped).
fn extract_fenced_json(hay: &str) -> Option<String> {
    let open = hay.find("```")? + 3;
    let after = &hay[open..];
    let body_start = after.find('\n').map_or(0, |i| i + 1);
    let body = &after[body_start..];
    let end = body.find("```")?;
    Some(body[..end].to_string())
}

// ── GeminiProvider ────────────────────────────────────────────────────────────

/// Gemini LLM provider using Google's GenerateContent API.
/// Defaults to gemini-3.8-flash for standard/PDF tiers and gemini-3.5-flash-lite for lite tier.
pub struct GeminiProvider {
    client: Client,
    api_key: String,
    standard_model: String,
    advanced_model: String,
    progress: Option<(ProgressTx, Option<String>)>,
}

impl std::fmt::Debug for GeminiProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the credential itself.
        f.debug_struct("GeminiProvider")
            .field("client", &self.client)
            .field("api_key", &"<redacted>")
            .field("standard_model", &self.standard_model)
            .field("advanced_model", &self.advanced_model)
            .field("progress", &self.progress)
            .finish()
    }
}

impl GeminiProvider {
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("FYNANCE_GEMINI_API_KEY")
            .or_else(|_| std::env::var("GEMINI_API_KEY"))
            .ok()
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| {
                anyhow!(
                    "FYNANCE_GEMINI_API_KEY (or GEMINI_API_KEY) is not set. \
                     Set it in your .env file or environment."
                )
            })?;

        let standard_model = std::env::var("FYNANCE_GEMINI_TEXT_MODEL")
            .unwrap_or_else(|_| "gemini-3.8-flash".to_string());
        let advanced_model = std::env::var("FYNANCE_GEMINI_PDF_MODEL")
            .unwrap_or_else(|_| "gemini-3.8-flash".to_string());

        let client = Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|e| anyhow!("failed to build HTTP client: {e}"))?;

        Ok(Self {
            client,
            api_key,
            standard_model,
            advanced_model,
            progress: None,
        })
    }

    fn model_for_tier(&self, tier: ModelTier) -> &str {
        match tier {
            ModelTier::Standard => &self.standard_model,
            ModelTier::Advanced => &self.advanced_model,
        }
    }

    fn resolve_model(
        &self,
        tier: ModelTier,
        agent_override: Option<Agent>,
    ) -> Result<String, ProviderError> {
        match agent_override {
            Some(agent) => gemini_model_for_agent(agent).map(|s| s.to_string()),
            None => Ok(self.model_for_tier(tier).to_string()),
        }
    }

    pub fn clone_with_progress(&self, tx: ProgressTx, task_id: Option<String>) -> Self {
        Self {
            client: self.client.clone(),
            api_key: self.api_key.clone(),
            standard_model: self.standard_model.clone(),
            advanced_model: self.advanced_model.clone(),
            progress: Some((tx, task_id)),
        }
    }

    async fn post_generate_content(
        &self,
        model: &str,
        request_body: &Value,
        tool_name: &str,
    ) -> Result<ProviderCallResult, ProviderError> {
        if let Some((tx, task_id)) = &self.progress {
            let _ = tx.send(ProgressEvent::LlmStart {
                model: model.to_string(),
                input_tokens: 0,
                task_id: task_id.clone(),
            });
        }

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            model
        );

        let started = Instant::now();
        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("x-goog-api-key", &self.api_key)
            .json(request_body)
            .send()
            .await
            .map_err(classify_reqwest_error)?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| ProviderError::ResponseUnreadable(format!("{e}")))?;
        let duration_ms = started.elapsed().as_millis() as u64;

        if !status.is_success() {
            let status_code = status.as_u16();
            return Err(match status_code {
                429 => ProviderError::RateLimit {
                    retry_after: None,
                    detail: body,
                },
                401 | 403 => ProviderError::AuthRejected(body),
                400..=499 => ProviderError::UpstreamClientError {
                    status: status_code,
                    body,
                },
                _ => ProviderError::UpstreamServerError {
                    status: status_code,
                    body,
                },
            });
        }

        let (value, finish_reason) = extract_gemini_tool_input(&body, tool_name)?;
        let usage = extract_gemini_usage(&body);

        warn_if_truncated(model, tool_name, &finish_reason, &usage);

        Ok(ProviderCallResult {
            value,
            usage,
            model: model.to_string(),
            duration_ms,
            stop_reason: finish_reason,
            warnings: Vec::new(),
        })
    }
}

#[async_trait]
impl LlmProvider for GeminiProvider {
    async fn chat_with_tools(
        &self,
        system_prompt: &str,
        user_message: &str,
        tool_name: &str,
        tool_schema: Value,
        tier: ModelTier,
        agent_override: Option<Agent>,
    ) -> Result<ProviderCallResult, ProviderError> {
        let model = self.resolve_model(tier, agent_override)?;
        let sanitized_schema = sanitize_schema_for_gemini(&tool_schema);

        let request_body = json!({
            "system_instruction": {
                "parts": [{ "text": system_prompt }]
            },
            "contents": [
                {
                    "role": "user",
                    "parts": [{ "text": user_message }]
                }
            ],
            "tools": [
                {
                    "function_declarations": [
                        {
                            "name": tool_name,
                            "description": format!("Extract structured data using the {} tool.", tool_name),
                            "parameters": sanitized_schema
                        }
                    ]
                }
            ],
            "tool_config": {
                "function_calling_config": {
                    "mode": "ANY",
                    "allowed_function_names": [tool_name]
                }
            },
            "generation_config": {
                "max_output_tokens": MAX_TOKENS_TEXT
            }
        });

        tracing::debug!(
            provider = "gemini",
            model = %model,
            tool_name,
            "sending text request"
        );

        self.post_generate_content(&model, &request_body, tool_name)
            .await
    }

    async fn chat_with_pdf_and_tools(
        &self,
        system_prompt: &str,
        pdf_bytes: &[u8],
        text_supplement: &str,
        tool_name: &str,
        tool_schema: Value,
        agent_override: Option<Agent>,
    ) -> Result<ProviderCallResult, ProviderError> {
        let model = self.resolve_model(ModelTier::Advanced, agent_override)?;
        let sanitized_schema = sanitize_schema_for_gemini(&tool_schema);
        let b64 = BASE64.encode(pdf_bytes);

        let mut parts = vec![json!({
            "inline_data": {
                "mime_type": "application/pdf",
                "data": b64
            }
        })];
        if !text_supplement.is_empty() {
            parts.push(json!({ "text": text_supplement }));
        }

        let request_body = json!({
            "system_instruction": {
                "parts": [{ "text": system_prompt }]
            },
            "contents": [
                {
                    "role": "user",
                    "parts": parts
                }
            ],
            "tools": [
                {
                    "function_declarations": [
                        {
                            "name": tool_name,
                            "description": format!("Extract structured data using the {} tool.", tool_name),
                            "parameters": sanitized_schema
                        }
                    ]
                }
            ],
            "tool_config": {
                "function_calling_config": {
                    "mode": "ANY",
                    "allowed_function_names": [tool_name]
                }
            },
            "generation_config": {
                "max_output_tokens": MAX_TOKENS_DOCUMENTS
            }
        });

        tracing::debug!(
            provider = "gemini",
            model = %model,
            tool_name,
            pdf_size = pdf_bytes.len(),
            "sending PDF request"
        );

        self.post_generate_content(&model, &request_body, tool_name)
            .await
    }

    async fn chat_with_files_and_tools(
        &self,
        system_prompt: &str,
        files: &[(String, String, Vec<u8>)],
        text_supplement: &str,
        tool_name: &str,
        tool_schema: Value,
        agent_override: Option<Agent>,
    ) -> Result<ProviderCallResult, ProviderError> {
        let model = self.resolve_model(ModelTier::Advanced, agent_override)?;
        let sanitized_schema = sanitize_schema_for_gemini(&tool_schema);

        let mut parts: Vec<Value> = Vec::with_capacity(files.len() * 2 + 1);
        for (filename, mime, bytes) in files {
            parts.push(json!({
                "text": format!(
                    "Source document filename: {filename}\nFor every row you extract from the document immediately below, set its \"source_file\" field to exactly this filename."
                )
            }));
            if mime == "application/pdf" || mime.starts_with("image/") {
                parts.push(json!({
                    "inline_data": {
                        "mime_type": mime,
                        "data": BASE64.encode(bytes)
                    }
                }));
            } else {
                let text = match std::str::from_utf8(bytes) {
                    Ok(s) => s.to_string(),
                    Err(_) => String::from_utf8_lossy(bytes).to_string(),
                };
                parts.push(json!({ "text": text }));
            }
        }
        if !text_supplement.is_empty() {
            parts.push(json!({ "text": text_supplement }));
        }

        let request_body = json!({
            "system_instruction": {
                "parts": [{ "text": system_prompt }]
            },
            "contents": [
                {
                    "role": "user",
                    "parts": parts
                }
            ],
            "tools": [
                {
                    "function_declarations": [
                        {
                            "name": tool_name,
                            "description": format!("Extract structured data using the {} tool.", tool_name),
                            "parameters": sanitized_schema
                        }
                    ]
                }
            ],
            "tool_config": {
                "function_calling_config": {
                    "mode": "ANY",
                    "allowed_function_names": [tool_name]
                }
            },
            "generation_config": {
                "max_output_tokens": MAX_TOKENS_DOCUMENTS
            }
        });

        tracing::debug!(
            provider = "gemini",
            model = %model,
            tool_name,
            files_count = files.len(),
            "sending files request"
        );

        self.post_generate_content(&model, &request_body, tool_name)
            .await
    }

    fn name(&self) -> &'static str {
        "gemini"
    }

    fn with_progress(
        &self,
        tx: ProgressTx,
        task_id: Option<String>,
    ) -> Option<Arc<dyn LlmProvider>> {
        Some(Arc::new(self.clone_with_progress(tx, task_id)))
    }
}

/// Latest frontier model id per agent for Gemini. Keep in sync with `pricing.rs`.
fn gemini_model_for_agent(agent: Agent) -> Result<&'static str, ProviderError> {
    match agent {
        Agent::FlashLite => Ok("gemini-3.5-flash-lite"),
        Agent::Flash => Ok("gemini-3.8-flash"),
        other => Err(ProviderError::NotSupported(format!(
            "agent '{other:?}' is not supported by Gemini provider. Valid options: flash, flash_lite"
        ))),
    }
}

// ── Gemini response parsing helpers ──────────────────────────────────────────

#[derive(Deserialize)]
struct GeminiResponse {
    #[serde(default)]
    candidates: Option<Vec<GeminiCandidate>>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<GeminiUsageMetadata>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiContent>,
    #[serde(rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct GeminiContent {
    parts: Option<Vec<GeminiPart>>,
}

#[derive(Deserialize)]
struct GeminiPart {
    #[serde(rename = "functionCall")]
    function_call: Option<GeminiFunctionCall>,
}

#[derive(Deserialize)]
struct GeminiFunctionCall {
    name: String,
    args: Option<Value>,
}

#[derive(Deserialize, Default)]
struct GeminiUsageMetadata {
    #[serde(rename = "promptTokenCount", default)]
    prompt_token_count: u64,
    #[serde(rename = "candidatesTokenCount", default)]
    candidates_token_count: u64,
}

fn extract_gemini_tool_input(
    body: &str,
    tool_name: &str,
) -> Result<(Value, Option<String>), ProviderError> {
    let resp: GeminiResponse = serde_json::from_str(body).map_err(|e| {
        ProviderError::ResponseUnreadable(format!(
            "parsing Gemini response: {e} (preview: {})",
            &body[..body.len().min(200)]
        ))
    })?;

    let candidate = resp
        .candidates
        .and_then(|mut cs| {
            if cs.is_empty() {
                None
            } else {
                Some(cs.remove(0))
            }
        })
        .ok_or_else(|| {
            ProviderError::ResponseUnreadable("no candidates in Gemini response".to_string())
        })?;

    let finish_reason = candidate.finish_reason;

    let parts = candidate.content.and_then(|c| c.parts).ok_or_else(|| {
        ProviderError::ResponseUnreadable("no content parts in Gemini candidate".to_string())
    })?;

    let function_call = parts
        .into_iter()
        .find_map(|p| p.function_call.filter(|fc| fc.name == tool_name))
        .ok_or_else(|| ProviderError::NoToolUse {
            tool_name: tool_name.to_string(),
        })?;

    let args = function_call.args.unwrap_or(Value::Null);
    Ok((args, finish_reason))
}

fn extract_gemini_usage(body: &str) -> TokenUsage {
    serde_json::from_str::<GeminiResponse>(body)
        .ok()
        .and_then(|r| r.usage_metadata)
        .map(|u| TokenUsage {
            input_tokens: u.prompt_token_count,
            output_tokens: u.candidates_token_count,
        })
        .unwrap_or_default()
}

pub fn sanitize_schema_for_gemini(schema: &Value) -> Value {
    match schema {
        Value::Object(map) => {
            let mut new_map = serde_json::Map::new();
            let mut is_nullable = false;
            for (k, v) in map {
                if k == "$schema" {
                    continue;
                }
                if k == "type" {
                    if let Value::Array(arr) = v {
                        let mut non_null_type = None;
                        for item in arr {
                            if let Value::String(s) = item {
                                if s == "null" {
                                    is_nullable = true;
                                } else {
                                    non_null_type = Some(s.clone());
                                }
                            }
                        }
                        if let Some(t) = non_null_type {
                            new_map.insert("type".to_string(), Value::String(t));
                        }
                        continue;
                    }
                }
                new_map.insert(k.clone(), sanitize_schema_for_gemini(v));
            }
            if is_nullable {
                new_map.insert("nullable".to_string(), Value::Bool(true));
            }
            Value::Object(new_map)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(sanitize_schema_for_gemini).collect()),
        other => other.clone(),
    }
}

// ── FallbackProvider ───────────────────────────────────────────────────────────

/// Tries `primary` first and, on an auth or quota failure, retries the same call
/// against `fallback`. Used to run subscription (OAuth) auth as the primary path
/// and transparently fall back to the Console API key when the subscription
/// token is rejected or its monthly credit pool is exhausted.
#[derive(Debug)]
struct FallbackProvider {
    primary: Arc<dyn LlmProvider>,
    fallback: Arc<dyn LlmProvider>,
}

/// Retry on the fallback only for failures a different credential could plausibly
/// fix: a rejected/expired token or an exhausted credit pool. Network, schema,
/// and upstream-bug errors would fail identically on the fallback, so they
/// propagate unchanged.
fn should_fall_back(err: &ProviderError) -> bool {
    matches!(
        err,
        ProviderError::AuthRejected(_) | ProviderError::RateLimit { .. }
    )
}

#[async_trait]
impl LlmProvider for FallbackProvider {
    async fn chat_with_tools(
        &self,
        system_prompt: &str,
        user_message: &str,
        tool_name: &str,
        tool_schema: Value,
        tier: ModelTier,
        agent_override: Option<Agent>,
    ) -> Result<ProviderCallResult, ProviderError> {
        match self
            .primary
            .chat_with_tools(
                system_prompt,
                user_message,
                tool_name,
                tool_schema.clone(),
                tier,
                agent_override,
            )
            .await
        {
            Err(e) if should_fall_back(&e) => {
                tracing::warn!(error = %e, "subscription provider failed; retrying with API key");
                self.fallback
                    .chat_with_tools(
                        system_prompt,
                        user_message,
                        tool_name,
                        tool_schema,
                        tier,
                        agent_override,
                    )
                    .await
            }
            other => other,
        }
    }

    async fn chat_with_pdf_and_tools(
        &self,
        system_prompt: &str,
        pdf_bytes: &[u8],
        text_supplement: &str,
        tool_name: &str,
        tool_schema: Value,
        agent_override: Option<Agent>,
    ) -> Result<ProviderCallResult, ProviderError> {
        match self
            .primary
            .chat_with_pdf_and_tools(
                system_prompt,
                pdf_bytes,
                text_supplement,
                tool_name,
                tool_schema.clone(),
                agent_override,
            )
            .await
        {
            Err(e) if should_fall_back(&e) => {
                tracing::warn!(error = %e, "subscription provider failed; retrying with API key");
                self.fallback
                    .chat_with_pdf_and_tools(
                        system_prompt,
                        pdf_bytes,
                        text_supplement,
                        tool_name,
                        tool_schema,
                        agent_override,
                    )
                    .await
            }
            other => other,
        }
    }

    async fn chat_with_files_and_tools(
        &self,
        system_prompt: &str,
        files: &[(String, String, Vec<u8>)],
        text_supplement: &str,
        tool_name: &str,
        tool_schema: Value,
        agent_override: Option<Agent>,
    ) -> Result<ProviderCallResult, ProviderError> {
        match self
            .primary
            .chat_with_files_and_tools(
                system_prompt,
                files,
                text_supplement,
                tool_name,
                tool_schema.clone(),
                agent_override,
            )
            .await
        {
            Err(e) if should_fall_back(&e) => {
                tracing::warn!(error = %e, "subscription provider failed; retrying with API key");
                self.fallback
                    .chat_with_files_and_tools(
                        system_prompt,
                        files,
                        text_supplement,
                        tool_name,
                        tool_schema,
                        agent_override,
                    )
                    .await
            }
            other => other,
        }
    }

    fn name(&self) -> &'static str {
        self.primary.name()
    }

    fn with_progress(
        &self,
        tx: ProgressTx,
        task_id: Option<String>,
    ) -> Option<Arc<dyn LlmProvider>> {
        let primary = self
            .primary
            .with_progress(tx.clone(), task_id.clone())
            .unwrap_or_else(|| self.primary.clone());
        let fallback = self
            .fallback
            .with_progress(tx, task_id)
            .unwrap_or_else(|| self.fallback.clone());
        Some(Arc::new(FallbackProvider { primary, fallback }))
    }
}

// ── Factory ───────────────────────────────────────────────────────────────────

/// Create the configured LLM provider from environment variables, using the
/// default credential selection ([`AuthSource::Auto`]).
pub fn create_provider() -> Result<Arc<dyn LlmProvider>> {
    create_provider_with_auth(AuthSource::Auto)
}

/// Create the configured LLM provider, overriding which Anthropic credential to
/// use. `auth` only affects the Anthropic provider; OpenAI and Gemini ignore it.
///
/// Reads `FYNANCE_PARSE_PROVIDER` (default: "anthropic").
///
/// # Valid values for FYNANCE_PARSE_PROVIDER
/// - `"anthropic"` (default): credential chosen per `auth` (see below)
/// - `"openai"`: requires `FYNANCE_OPENAI_API_KEY` unless `FYNANCE_OPENAI_BASE_URL` is set
/// - `"gemini"`: requires `FYNANCE_GEMINI_API_KEY` or `GEMINI_API_KEY`
///
/// # Anthropic credential selection
/// - [`AuthSource::Auto`]: prefer the subscription OAuth token; when both it and
///   the API key are set, use the subscription with the API key as a runtime
///   fallback; when only one is set, use that one.
/// - [`AuthSource::Subscription`]: require `FYNANCE_CLAUDE_CODE_OAUTH_TOKEN`.
/// - [`AuthSource::ApiKey`]: require `FYNANCE_ANTHROPIC_API_KEY`.
pub fn create_provider_with_auth(auth: AuthSource) -> Result<Arc<dyn LlmProvider>> {
    let provider_name =
        std::env::var("FYNANCE_PARSE_PROVIDER").unwrap_or_else(|_| "anthropic".to_string());

    let provider: Arc<dyn LlmProvider> = match provider_name.to_lowercase().as_str() {
        "anthropic" => create_anthropic_provider(auth)?,
        "openai" => Arc::new(OpenAIProvider::from_env()?),
        "gemini" => Arc::new(GeminiProvider::from_env()?),
        other => {
            return Err(anyhow!(
                "Unknown FYNANCE_PARSE_PROVIDER value: '{}'. \
                 Valid values are: anthropic, openai, gemini.",
                other
            ));
        }
    };

    tracing::info!(provider = provider.name(), "LLM provider initialized");
    Ok(provider)
}

fn env_present(name: &str) -> bool {
    std::env::var(name)
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
}

fn create_anthropic_provider(auth: AuthSource) -> Result<Arc<dyn LlmProvider>> {
    match auth {
        AuthSource::Subscription => Ok(Arc::new(AnthropicProvider::from_env_subscription()?)),
        AuthSource::ApiKey => Ok(Arc::new(AnthropicProvider::from_env()?)),
        AuthSource::Auto => {
            match (
                env_present("FYNANCE_CLAUDE_CODE_OAUTH_TOKEN"),
                env_present("FYNANCE_ANTHROPIC_API_KEY"),
            ) {
                (true, true) => Ok(Arc::new(FallbackProvider {
                    primary: Arc::new(AnthropicProvider::from_env_subscription()?),
                    fallback: Arc::new(AnthropicProvider::from_env()?),
                })),
                (true, false) => Ok(Arc::new(AnthropicProvider::from_env_subscription()?)),
                (false, true) => Ok(Arc::new(AnthropicProvider::from_env()?)),
                (false, false) => Err(anyhow!(
                    "No Anthropic credential is set. Set FYNANCE_CLAUDE_CODE_OAUTH_TOKEN \
                     (preferred; run `claude setup-token`) or FYNANCE_ANTHROPIC_API_KEY in \
                     your .env file or environment."
                )),
            }
        }
    }
}

// ── MockProvider for tests ────────────────────────────────────────────────────

#[cfg(test)]
pub mod testing {
    use super::*;

    /// Returns a pre-canned JSON Value for every call. Used in unit tests to
    /// exercise parser logic without making real API calls.
    #[derive(Debug)]
    pub struct MockProvider {
        pub tool_input: Value,
        pub warnings: Vec<String>,
    }

    impl MockProvider {
        pub fn new(tool_input: Value) -> Arc<Self> {
            Arc::new(Self {
                tool_input,
                warnings: Vec::new(),
            })
        }

        /// A mock whose call result carries degraded-input warnings, for
        /// tests of how parsers surface them.
        pub fn with_warnings(tool_input: Value, warnings: Vec<String>) -> Arc<Self> {
            Arc::new(Self {
                tool_input,
                warnings,
            })
        }

        fn pretend_result(&self) -> ProviderCallResult {
            ProviderCallResult {
                value: self.tool_input.clone(),
                usage: TokenUsage::default(),
                model: "mock".to_string(),
                duration_ms: 0,
                stop_reason: None,
                warnings: self.warnings.clone(),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        async fn chat_with_tools(
            &self,
            _system_prompt: &str,
            _user_message: &str,
            _tool_name: &str,
            _tool_schema: Value,
            _tier: ModelTier,
            _agent_override: Option<Agent>,
        ) -> Result<ProviderCallResult, ProviderError> {
            Ok(self.pretend_result())
        }

        async fn chat_with_pdf_and_tools(
            &self,
            _system_prompt: &str,
            _pdf_bytes: &[u8],
            _text_supplement: &str,
            _tool_name: &str,
            _tool_schema: Value,
            _agent_override: Option<Agent>,
        ) -> Result<ProviderCallResult, ProviderError> {
            Ok(self.pretend_result())
        }

        async fn chat_with_files_and_tools(
            &self,
            _system_prompt: &str,
            _files: &[(String, String, Vec<u8>)],
            _text_supplement: &str,
            _tool_name: &str,
            _tool_schema: Value,
            _agent_override: Option<Agent>,
        ) -> Result<ProviderCallResult, ProviderError> {
            Ok(self.pretend_result())
        }

        fn name(&self) -> &'static str {
            "mock"
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::importers::TEST_ENV_LOCK;

    // Every test here that mutates process-global env vars (`FYNANCE_PARSE_PROVIDER`,
    // `FYNANCE_*_API_KEY`, `FYNANCE_OPENAI_*`, `FYNANCE_PDF_EXTRACT_*`) holds the
    // crate-wide test env lock from `importers::mod`. Cargo runs tests in parallel
    // in one process, so one lock (not one per module) is what stops a
    // `pdf_extract` env test from racing an `OpenAIProvider::from_env` read.
    // `.unwrap_or_else(into_inner)` keeps the lock usable if a guarded test
    // panics, so the real failure surfaces instead of a poison cascade.

    #[test]
    fn test_create_provider_unknown_name() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe { std::env::set_var("FYNANCE_PARSE_PROVIDER", "invalid_provider") };
        let result = create_provider();
        unsafe { std::env::remove_var("FYNANCE_PARSE_PROVIDER") };
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("Unknown FYNANCE_PARSE_PROVIDER"), "got: {msg}");
    }

    #[test]
    fn test_create_provider_gemini_requires_key() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::set_var("FYNANCE_PARSE_PROVIDER", "gemini");
            std::env::remove_var("FYNANCE_GEMINI_API_KEY");
            std::env::remove_var("GEMINI_API_KEY");
        }
        let result = create_provider();
        unsafe { std::env::remove_var("FYNANCE_PARSE_PROVIDER") };
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("FYNANCE_GEMINI_API_KEY")
        );
    }

    #[test]
    fn test_create_provider_gemini_success() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::set_var("FYNANCE_PARSE_PROVIDER", "gemini");
            std::env::set_var("FYNANCE_GEMINI_API_KEY", "test-key");
        }
        let result = create_provider();
        unsafe {
            std::env::remove_var("FYNANCE_PARSE_PROVIDER");
            std::env::remove_var("FYNANCE_GEMINI_API_KEY");
        }
        assert!(result.is_ok());
        assert_eq!(result.unwrap().name(), "gemini");
    }

    fn clear_openai_env() {
        for key in OPENAI_ENV_KEYS {
            unsafe { std::env::remove_var(key) };
        }
    }

    const OPENAI_ENV_KEYS: &[&str] = &[
        "FYNANCE_PARSE_PROVIDER",
        "FYNANCE_OPENAI_API_KEY",
        "FYNANCE_OPENAI_BASE_URL",
        "FYNANCE_OPENAI_TIMEOUT_SECS",
        "FYNANCE_OPENAI_TEXT_MODEL",
        "FYNANCE_OPENAI_PDF_MODEL",
        "FYNANCE_OPENAI_TOOL_CHOICE",
        "FYNANCE_OPENAI_MAX_TOKENS",
        "FYNANCE_OPENAI_TEMPERATURE",
        "FYNANCE_OPENAI_EXTRA_BODY",
        "FYNANCE_OPENAI_PDF_MODE",
        "FYNANCE_OPENAI_PDF_MAX_PAGES",
        "FYNANCE_OPENAI_STREAM",
        // Read by PdfExtractConfig::from_env inside OpenAIProvider::from_env:
        // a value left over from another test must not reach this one.
        "FYNANCE_PDF_EXTRACT_CMD",
        "FYNANCE_PDF_EXTRACT_ARGS",
        "FYNANCE_PDF_EXTRACT_TIMEOUT_SECS",
    ];

    /// One-shot HTTP server on an ephemeral loopback port. Answers the first
    /// request with `body` as JSON and returns the raw request it received.
    fn serve_once(body: &'static str) -> (String, std::thread::JoinHandle<String>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut raw = Vec::new();
            let mut buf = [0u8; 4096];
            while !http_request_complete(&raw) {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                raw.extend_from_slice(&buf[..n]);
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8_lossy(&raw).into_owned()
        });
        (format!("http://{addr}"), handle)
    }

    fn http_request_complete(raw: &[u8]) -> bool {
        let Some(header_end) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
            return false;
        };
        let content_length = String::from_utf8_lossy(&raw[..header_end])
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())?
            })
            .unwrap_or(0);
        raw.len() >= header_end + 4 + content_length
    }

    const OPENAI_TOOL_CALL_RESPONSE: &str = r#"{
        "choices": [{
            "message": {
                "tool_calls": [{
                    "function": {
                        "name": "parse_bank_statement",
                        "arguments": "{\"detected_bank\":\"monzo\",\"rows\":[]}"
                    }
                }]
            }
        }],
        "usage": {"prompt_tokens": 11, "completion_tokens": 22}
    }"#;

    fn call_openai(provider: &OpenAIProvider) -> Result<ProviderCallResult, ProviderError> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(provider.chat_with_tools(
            "system",
            "user",
            "parse_bank_statement",
            json!({"type": "object"}),
            ModelTier::Standard,
            None,
        ))
    }

    #[test]
    fn test_create_provider_openai_requires_key() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_openai_env();
        unsafe { std::env::set_var("FYNANCE_PARSE_PROVIDER", "openai") };
        let result = create_provider();
        clear_openai_env();
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("FYNANCE_OPENAI_API_KEY"), "got: {msg}");
    }

    #[test]
    fn test_create_provider_openai_custom_base_url_needs_no_key() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_openai_env();
        unsafe {
            std::env::set_var("FYNANCE_PARSE_PROVIDER", "openai");
            std::env::set_var("FYNANCE_OPENAI_BASE_URL", "http://127.0.0.1:8080/v1");
        }
        let result = create_provider();
        clear_openai_env();
        assert_eq!(result.unwrap().name(), "openai");
    }

    #[test]
    fn test_openai_custom_base_url_without_key_sends_no_auth_header() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (server_url, server) = serve_once(OPENAI_TOOL_CALL_RESPONSE);
        clear_openai_env();
        unsafe {
            std::env::set_var("FYNANCE_OPENAI_BASE_URL", format!("{server_url}/v1/"));
            // The fake answers with a JSON body: keep this on the
            // non-streaming transport.
            std::env::set_var("FYNANCE_OPENAI_STREAM", "false");
        }
        let provider = OpenAIProvider::from_env();
        clear_openai_env();

        let result = call_openai(&provider.unwrap()).unwrap();
        let request = server.join().unwrap();

        assert!(
            request.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"),
            "got: {request}"
        );
        assert!(
            !request.to_ascii_lowercase().contains("\r\nauthorization:"),
            "got: {request}"
        );
        assert_eq!(result.value["detected_bank"], "monzo");
        assert_eq!(result.usage.input_tokens, 11);
        assert_eq!(result.usage.output_tokens, 22);
    }

    #[test]
    fn test_openai_custom_base_url_with_key_sends_bearer() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (server_url, server) = serve_once(OPENAI_TOOL_CALL_RESPONSE);
        clear_openai_env();
        unsafe {
            std::env::set_var("FYNANCE_OPENAI_BASE_URL", format!("{server_url}/v1"));
            std::env::set_var("FYNANCE_OPENAI_API_KEY", "local-key");
            // The fake answers with a JSON body: keep this on the
            // non-streaming transport.
            std::env::set_var("FYNANCE_OPENAI_STREAM", "false");
        }
        let provider = OpenAIProvider::from_env().unwrap();
        clear_openai_env();

        call_openai(&provider).unwrap();
        let request = server.join().unwrap().to_ascii_lowercase();

        assert!(
            request.contains("\r\nauthorization: bearer local-key\r\n"),
            "got: {request}"
        );
    }

    #[test]
    fn test_openai_defaults_to_openai_host() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_openai_env();
        unsafe { std::env::set_var("FYNANCE_OPENAI_API_KEY", "test-key") };
        let provider = OpenAIProvider::from_env().unwrap();
        clear_openai_env();

        assert_eq!(provider.base_url, "https://api.openai.com/v1");
        assert_eq!(provider.api_key.as_deref(), Some("test-key"));
        assert_eq!(provider.timeout, std::time::Duration::from_secs(300));
    }

    #[test]
    fn test_openai_custom_base_url_and_timeout() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_openai_env();
        unsafe {
            std::env::set_var("FYNANCE_OPENAI_BASE_URL", "http://localhost:8080/v1/");
            std::env::set_var("FYNANCE_OPENAI_TIMEOUT_SECS", "900");
        }
        let provider = OpenAIProvider::from_env().unwrap();
        clear_openai_env();

        assert_eq!(provider.base_url, "http://localhost:8080/v1");
        assert_eq!(provider.api_key, None);
        assert_eq!(provider.timeout, std::time::Duration::from_secs(900));
    }

    #[test]
    fn test_openai_rejects_invalid_timeout() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_openai_env();
        unsafe {
            std::env::set_var("FYNANCE_OPENAI_API_KEY", "test-key");
            std::env::set_var("FYNANCE_OPENAI_TIMEOUT_SECS", "soon");
        }
        let result = OpenAIProvider::from_env();
        clear_openai_env();

        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("FYNANCE_OPENAI_TIMEOUT_SECS"), "got: {msg}");
    }

    #[test]
    fn test_openai_request_times_out() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Accepts the connection and holds it open without ever replying,
        // until the test drops `release`.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (release, held) = std::sync::mpsc::channel::<()>();
        let server = std::thread::spawn(move || {
            let (_stream, _) = listener.accept().unwrap();
            let _ = held.recv();
        });
        clear_openai_env();
        unsafe {
            std::env::set_var("FYNANCE_OPENAI_BASE_URL", format!("http://{addr}/v1"));
            std::env::set_var("FYNANCE_OPENAI_TIMEOUT_SECS", "1");
        }
        let provider = OpenAIProvider::from_env().unwrap();
        clear_openai_env();

        // The outer 5s cap turns a missing per-request timeout into a test
        // failure instead of a hang.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async {
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                provider.chat_with_tools(
                    "system",
                    "user",
                    "parse_bank_statement",
                    json!({"type": "object"}),
                    ModelTier::Standard,
                    None,
                ),
            )
            .await
        });
        drop(release);
        server.join().unwrap();

        let result = result.expect("request was not cut off by the 1s timeout within 5s");
        assert!(
            matches!(result, Err(ProviderError::Timeout(_))),
            "got: {result:?}"
        );
    }

    #[test]
    fn test_openai_rejects_invalid_base_url() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for bad in [
            "localhost:8080/v1",
            "http://127.0.0.1:8080/v1?x=1",
            "http://127.0.0.1:8080/v1#frag",
            "ftp://127.0.0.1/v1",
        ] {
            clear_openai_env();
            unsafe { std::env::set_var("FYNANCE_OPENAI_BASE_URL", bad) };
            let result = OpenAIProvider::from_env();
            clear_openai_env();

            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("FYNANCE_OPENAI_BASE_URL is not a valid base URL"),
                "{bad}: got: {msg}"
            );
        }
    }

    #[test]
    fn test_openai_empty_key_error_mentions_base_url() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_openai_env();
        unsafe { std::env::set_var("FYNANCE_OPENAI_API_KEY", "   ") };
        let result = OpenAIProvider::from_env();
        clear_openai_env();

        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("FYNANCE_OPENAI_API_KEY is set but empty"),
            "got: {msg}"
        );
        assert!(msg.contains("FYNANCE_OPENAI_BASE_URL"), "got: {msg}");
    }

    #[test]
    fn test_openai_and_gemini_debug_redact_api_key() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_openai_env();
        unsafe {
            std::env::set_var("FYNANCE_OPENAI_API_KEY", "sk-openai-secret");
            std::env::set_var("FYNANCE_GEMINI_API_KEY", "gemini-secret");
        }
        let openai = OpenAIProvider::from_env().unwrap();
        let gemini = GeminiProvider::from_env().unwrap();
        clear_openai_env();
        unsafe { std::env::remove_var("FYNANCE_GEMINI_API_KEY") };

        let openai_debug = format!("{openai:?}");
        assert!(
            !openai_debug.contains("sk-openai-secret"),
            "got: {openai_debug}"
        );
        assert!(openai_debug.contains("<redacted>"), "got: {openai_debug}");
        let gemini_debug = format!("{gemini:?}");
        assert!(
            !gemini_debug.contains("gemini-secret"),
            "got: {gemini_debug}"
        );
        assert!(gemini_debug.contains("<redacted>"), "got: {gemini_debug}");
    }

    #[test]
    fn test_create_provider_anthropic_requires_some_credential() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::set_var("FYNANCE_PARSE_PROVIDER", "anthropic");
            std::env::remove_var("FYNANCE_ANTHROPIC_API_KEY");
            std::env::remove_var("FYNANCE_CLAUDE_CODE_OAUTH_TOKEN");
        }
        let result = create_provider();
        unsafe { std::env::remove_var("FYNANCE_PARSE_PROVIDER") };
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        // Names both env vars so the user knows either one resolves it.
        assert!(msg.contains("FYNANCE_ANTHROPIC_API_KEY"), "got: {msg}");
        assert!(
            msg.contains("FYNANCE_CLAUDE_CODE_OAUTH_TOKEN"),
            "got: {msg}"
        );
    }

    #[test]
    fn test_create_provider_auto_prefers_subscription() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::set_var("FYNANCE_PARSE_PROVIDER", "anthropic");
            std::env::set_var("FYNANCE_ANTHROPIC_API_KEY", "sk-ant-test");
            std::env::set_var("FYNANCE_CLAUDE_CODE_OAUTH_TOKEN", "sk-ant-oat01-test");
        }
        let result = create_provider_with_auth(AuthSource::Auto);
        unsafe {
            std::env::remove_var("FYNANCE_PARSE_PROVIDER");
            std::env::remove_var("FYNANCE_ANTHROPIC_API_KEY");
            std::env::remove_var("FYNANCE_CLAUDE_CODE_OAUTH_TOKEN");
        }
        // FallbackProvider reports its primary's name, which is the subscription.
        assert_eq!(result.unwrap().name(), "anthropic-subscription");
    }

    #[test]
    fn test_create_provider_auto_uses_api_key_without_subscription() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::set_var("FYNANCE_PARSE_PROVIDER", "anthropic");
            std::env::set_var("FYNANCE_ANTHROPIC_API_KEY", "sk-ant-test");
            std::env::remove_var("FYNANCE_CLAUDE_CODE_OAUTH_TOKEN");
        }
        let result = create_provider_with_auth(AuthSource::Auto);
        unsafe {
            std::env::remove_var("FYNANCE_PARSE_PROVIDER");
            std::env::remove_var("FYNANCE_ANTHROPIC_API_KEY");
        }
        assert_eq!(result.unwrap().name(), "anthropic");
    }

    #[test]
    fn test_create_provider_subscription_requires_token() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::set_var("FYNANCE_PARSE_PROVIDER", "anthropic");
            std::env::set_var("FYNANCE_ANTHROPIC_API_KEY", "sk-ant-test");
            std::env::remove_var("FYNANCE_CLAUDE_CODE_OAUTH_TOKEN");
        }
        let result = create_provider_with_auth(AuthSource::Subscription);
        unsafe {
            std::env::remove_var("FYNANCE_PARSE_PROVIDER");
            std::env::remove_var("FYNANCE_ANTHROPIC_API_KEY");
        }
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("FYNANCE_CLAUDE_CODE_OAUTH_TOKEN")
        );
    }

    #[test]
    fn test_with_claude_code_identity_wraps_string() {
        let wrapped = with_claude_code_identity(Some(&json!("Parse the statement.")));
        let arr = wrapped.as_array().expect("should be an array of blocks");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["text"], json!(CLAUDE_CODE_SYSTEM_IDENTITY));
        assert_eq!(arr[1]["text"], json!("Parse the statement."));
    }

    #[test]
    fn test_with_claude_code_identity_prepends_to_array() {
        let existing = json!([{ "type": "text", "text": "block one" }]);
        let wrapped = with_claude_code_identity(Some(&existing));
        let arr = wrapped.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["text"], json!(CLAUDE_CODE_SYSTEM_IDENTITY));
        assert_eq!(arr[1]["text"], json!("block one"));
    }

    #[test]
    fn test_gemini_from_env_requires_key() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::remove_var("FYNANCE_GEMINI_API_KEY");
            std::env::remove_var("GEMINI_API_KEY");
        };
        let err = GeminiProvider::from_env().unwrap_err();
        assert!(err.to_string().contains("FYNANCE_GEMINI_API_KEY"));
    }

    #[test]
    fn test_gemini_resolve_model() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::set_var("FYNANCE_GEMINI_API_KEY", "test-key");
            std::env::remove_var("FYNANCE_GEMINI_TEXT_MODEL");
            std::env::remove_var("FYNANCE_GEMINI_PDF_MODEL");
        };
        let provider = GeminiProvider::from_env().unwrap();
        unsafe {
            std::env::remove_var("FYNANCE_GEMINI_API_KEY");
        };

        assert_eq!(
            provider.resolve_model(ModelTier::Standard, None).unwrap(),
            "gemini-3.8-flash"
        );
        assert_eq!(
            provider.resolve_model(ModelTier::Advanced, None).unwrap(),
            "gemini-3.8-flash"
        );
        assert_eq!(
            provider
                .resolve_model(ModelTier::Standard, Some(Agent::FlashLite))
                .unwrap(),
            "gemini-3.5-flash-lite"
        );
        assert_eq!(
            provider
                .resolve_model(ModelTier::Standard, Some(Agent::Flash))
                .unwrap(),
            "gemini-3.8-flash"
        );
        assert!(
            provider
                .resolve_model(ModelTier::Standard, Some(Agent::Haiku))
                .is_err()
        );
        assert!(
            provider
                .resolve_model(ModelTier::Standard, Some(Agent::Sonnet))
                .is_err()
        );
        assert!(
            provider
                .resolve_model(ModelTier::Standard, Some(Agent::Opus))
                .is_err()
        );
    }

    #[test]
    fn test_anthropic_resolve_model() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::set_var("FYNANCE_ANTHROPIC_API_KEY", "test-key");
            std::env::remove_var("FYNANCE_IMPORT_LLM_MODEL");
            std::env::remove_var("FYNANCE_PARSE_PDF_MODEL");
        };
        let provider = AnthropicProvider::from_env().unwrap();
        unsafe {
            std::env::remove_var("FYNANCE_ANTHROPIC_API_KEY");
        };

        // None defaults to tier model
        assert_eq!(
            provider.resolve_model(ModelTier::Standard, None).unwrap(),
            "claude-sonnet-4-6"
        );
        assert_eq!(
            provider.resolve_model(ModelTier::Advanced, None).unwrap(),
            "claude-sonnet-4-6"
        );

        // Anthropic agents match
        assert_eq!(
            provider
                .resolve_model(ModelTier::Standard, Some(Agent::Haiku))
                .unwrap(),
            "claude-haiku-4-5-20251001"
        );
        assert_eq!(
            provider
                .resolve_model(ModelTier::Standard, Some(Agent::Sonnet))
                .unwrap(),
            "claude-sonnet-4-6"
        );
        assert_eq!(
            provider
                .resolve_model(ModelTier::Standard, Some(Agent::Opus))
                .unwrap(),
            "claude-opus-4-7"
        );

        // Other agents throw error
        assert!(
            provider
                .resolve_model(ModelTier::Standard, Some(Agent::Flash))
                .is_err()
        );
        assert!(
            provider
                .resolve_model(ModelTier::Standard, Some(Agent::FlashLite))
                .is_err()
        );
    }

    #[test]
    fn test_openai_pdf_without_tool_configured_is_not_supported() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_openai_env();
        unsafe {
            std::env::remove_var("FYNANCE_PDF_EXTRACT_CMD");
            std::env::set_var("FYNANCE_OPENAI_BASE_URL", "http://127.0.0.1:9/v1");
        }
        let provider = OpenAIProvider::from_env().unwrap();
        clear_openai_env();

        let result = call_openai_pdf(&provider);
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("PDF without the extract tool must fail"),
        };
        assert!(matches!(err, ProviderError::NotSupported(_)), "got: {err}");
        assert!(
            err.to_string().contains("FYNANCE_PDF_EXTRACT_CMD"),
            "got: {err}"
        );
    }

    fn call_openai_pdf(provider: &OpenAIProvider) -> Result<ProviderCallResult, ProviderError> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(provider.chat_with_pdf_and_tools(
            "system",
            b"%PDF-1.4 fake",
            "text",
            "extract_unified",
            json!({"type": "object"}),
            None,
        ))
    }

    #[test]
    fn test_openai_response_tool_call_string_arguments() {
        let api_resp: OpenAIResponse =
            serde_json::from_str(OPENAI_EXTRACT_UNIFIED_RESPONSE).unwrap();
        let reply = reply_from_choice(api_resp.choices.first().unwrap());
        assert_eq!(reply.finish_reason.as_deref(), Some("tool_calls"));
        let (value, stop) =
            post_process_openai_reply(&reply, "extract_unified", &json!({"type": "object"}), 20)
                .unwrap();
        assert_eq!(value["transactions"], json!([]));
        assert_eq!(stop.as_deref(), Some("tool_calls"));
    }

    #[test]
    fn test_anthropic_response_parsing() {
        let body = r#"{
            "content": [{
                "type": "tool_use",
                "name": "parse_bank_statement",
                "input": {"detected_bank": "revolut", "detection_confidence": 0.88, "rows": []}
            }],
            "usage": {"input_tokens": 1234, "output_tokens": 567}
        }"#;
        let result = extract_anthropic_tool_input(body, "parse_bank_statement").unwrap();
        assert_eq!(result["detected_bank"], "revolut");
        let usage = extract_anthropic_usage(body);
        assert_eq!(usage.input_tokens, 1234);
        assert_eq!(usage.output_tokens, 567);
    }

    #[test]
    fn test_anthropic_response_missing_tool_use() {
        let body = r#"{"content": [{"type": "text", "text": "hello"}]}"#;
        let result = extract_anthropic_tool_input(body, "parse_bank_statement");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("parse_bank_statement")
        );
    }

    #[test]
    fn test_anthropic_model_for_agent_mapping() {
        assert_eq!(
            anthropic_model_for_agent(Agent::Haiku).unwrap(),
            "claude-haiku-4-5-20251001"
        );
        assert_eq!(
            anthropic_model_for_agent(Agent::Sonnet).unwrap(),
            "claude-sonnet-4-6"
        );
        assert_eq!(
            anthropic_model_for_agent(Agent::Opus).unwrap(),
            "claude-opus-4-7"
        );
        assert!(anthropic_model_for_agent(Agent::Flash).is_err());
        assert!(anthropic_model_for_agent(Agent::FlashLite).is_err());
    }

    #[test]
    fn test_gemini_model_for_agent_mapping() {
        assert_eq!(
            gemini_model_for_agent(Agent::FlashLite).unwrap(),
            "gemini-3.5-flash-lite"
        );
        assert_eq!(
            gemini_model_for_agent(Agent::Flash).unwrap(),
            "gemini-3.8-flash"
        );
        assert!(gemini_model_for_agent(Agent::Haiku).is_err());
        assert!(gemini_model_for_agent(Agent::Sonnet).is_err());
        assert!(gemini_model_for_agent(Agent::Opus).is_err());
    }

    #[test]
    fn test_openai_usage_parsing() {
        let body = r#"{
            "choices": [{"message": {"tool_calls": null}}],
            "usage": {"prompt_tokens": 12, "completion_tokens": 34}
        }"#;
        let api_resp: OpenAIResponse = serde_json::from_str(body).unwrap();
        let usage = api_resp.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 12);
        assert_eq!(usage.completion_tokens, 34);
    }

    #[test]
    fn test_sse_accumulator_reconstructs_tool_input() {
        let events = vec![
            json!({"type":"message_start","message":{"id":"msg_1","model":"claude-sonnet-4-6","usage":{"input_tokens":500}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"tu_1","name":"extract_unified","input":{}}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"transactions\":"}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"[]}"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":42}}),
            json!({"type":"message_stop"}),
        ];

        let mut acc = SseAccumulator::new();
        for event in &events {
            acc.handle_event(event);
        }
        assert_eq!(acc.model, "claude-sonnet-4-6");
        assert_eq!(acc.input_tokens, 500);
        assert_eq!(acc.output_tokens, 42);
        assert_eq!(acc.stop_reason.as_deref(), Some("tool_use"));

        let result = acc.into_result("extract_unified").unwrap();
        assert_eq!(result.value, json!({"transactions": []}));
        assert_eq!(result.usage.input_tokens, 500);
        assert_eq!(result.usage.output_tokens, 42);
    }

    #[test]
    fn test_sse_accumulator_missing_tool_errors() {
        let acc = SseAccumulator::new();
        let result = acc.into_result("missing_tool");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing_tool"));
    }

    #[test]
    fn test_provider_error_display() {
        let err = ProviderError::Timeout("connection timed out".to_string());
        assert_eq!(err.to_string(), "upstream timeout: connection timed out");

        let err = ProviderError::RateLimit {
            retry_after: Some(30),
            detail: "too many requests".to_string(),
        };
        assert_eq!(err.to_string(), "rate limited: too many requests");

        let err = ProviderError::NoToolUse {
            tool_name: "extract_unified".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "no extract_unified tool_use block in response"
        );
    }

    #[test]
    fn test_provider_error_converts_to_anyhow() {
        let err = ProviderError::Timeout("test".to_string());
        let anyhow_err: anyhow::Error = err.into();
        assert!(anyhow_err.to_string().contains("upstream timeout"));
        assert!(anyhow_err.downcast_ref::<ProviderError>().is_some());
    }

    #[test]
    fn test_sanitize_schema_for_gemini() {
        let draft07_schema = json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "notes": { "type": ["string", "null"], "description": "Optional notes" },
                "amount": { "type": ["number", "null"] },
                "items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "code": { "type": ["string", "null"] }
                        }
                    }
                }
            },
            "required": ["name"]
        });

        let sanitized = sanitize_schema_for_gemini(&draft07_schema);

        assert!(sanitized.get("$schema").is_none());
        assert_eq!(sanitized["type"], "object");
        assert_eq!(sanitized["properties"]["name"]["type"], "string");
        assert_eq!(sanitized["properties"]["name"].get("nullable"), None);

        assert_eq!(sanitized["properties"]["notes"]["type"], "string");
        assert_eq!(sanitized["properties"]["notes"]["nullable"], true);
        assert_eq!(
            sanitized["properties"]["notes"]["description"],
            "Optional notes"
        );

        assert_eq!(sanitized["properties"]["amount"]["type"], "number");
        assert_eq!(sanitized["properties"]["amount"]["nullable"], true);

        assert_eq!(
            sanitized["properties"]["items"]["items"]["properties"]["code"]["type"],
            "string"
        );
        assert_eq!(
            sanitized["properties"]["items"]["items"]["properties"]["code"]["nullable"],
            true
        );
    }

    #[test]
    fn test_extract_gemini_tool_input_success() {
        let body = json!({
            "candidates": [
                {
                    "content": {
                        "parts": [
                            {
                                "functionCall": {
                                    "name": "parse_statement",
                                    "args": {
                                        "transactions": [
                                            { "date": "2026-01-01", "amount": "-10.50" }
                                        ]
                                    }
                                }
                            }
                        ],
                        "role": "model"
                    },
                    "finishReason": "STOP"
                }
            ],
            "usageMetadata": {
                "promptTokenCount": 120,
                "candidatesTokenCount": 45
            }
        })
        .to_string();

        let (val, finish_reason) = extract_gemini_tool_input(&body, "parse_statement").unwrap();
        assert_eq!(finish_reason.as_deref(), Some("STOP"));
        assert_eq!(
            val,
            json!({
                "transactions": [
                    { "date": "2026-01-01", "amount": "-10.50" }
                ]
            })
        );

        let usage = extract_gemini_usage(&body);
        assert_eq!(usage.input_tokens, 120);
        assert_eq!(usage.output_tokens, 45);
    }

    #[test]
    fn test_extract_gemini_tool_input_missing_tool() {
        let body = json!({
            "candidates": [
                {
                    "content": {
                        "parts": [
                            {
                                "text": "I could not extract any data."
                            }
                        ]
                    },
                    "finishReason": "STOP"
                }
            ]
        })
        .to_string();

        let err = extract_gemini_tool_input(&body, "parse_statement").unwrap_err();
        assert!(matches!(err, ProviderError::NoToolUse { .. }));
    }

    // ── OpenAI provider parity (plan 30 Part 2) ─────────────────────────────

    const OPENAI_EXTRACT_UNIFIED_RESPONSE: &str = r#"{
        "choices": [{
            "message": {
                "tool_calls": [{
                    "function": {
                        "name": "extract_unified",
                        "arguments": "{\"transactions\":[],\"holdings\":[],\"investments\":[]}"
                    }
                }]
            },
            "finish_reason": "tool_calls"
        }],
        "usage": {"prompt_tokens": 10, "completion_tokens": 20}
    }"#;

    const OPENAI_TRUNCATED_RESPONSE: &str = r#"{
        "choices": [{
            "message": {
                "tool_calls": [{
                    "function": {
                        "name": "extract_unified",
                        "arguments": "{\"transactions\":[{\"date\":\"2026-11-0"
                    }
                }]
            },
            "finish_reason": "length"
        }],
        "usage": {"prompt_tokens": 5, "completion_tokens": 999}
    }"#;

    fn openai_content_response(content: &str) -> String {
        serde_json::to_string(&json!({
            "choices": [{ "message": { "content": content }, "finish_reason": "stop" }],
            "usage": {"prompt_tokens": 1, "completion_tokens": 2}
        }))
        .unwrap()
    }

    /// The JSON body of the raw HTTP request captured by `serve_once`.
    fn request_json(raw: &str) -> Value {
        let body = raw
            .split_once("\r\n\r\n")
            .expect("request had no header terminator")
            .1;
        serde_json::from_str(body).expect("request body must be JSON")
    }

    /// A provider wired for tests only. `base_url` defaults to an unreachable
    /// loopback port: every test that sends a request points it at a
    /// `serve_once` fake first, so no test can reach api.openai.com.
    #[derive(Default)]
    struct TestConfig {
        base_url: Option<String>,
        tool_choice: Option<OpenAIToolChoice>,
        max_tokens: Option<u64>,
        temperature: Option<f64>,
        extra_body: Option<Value>,
        pdf_mode: Option<OpenAIPdfMode>,
        pdf_max_pages: Option<usize>,
        pdf_extract_cmd: Option<std::path::PathBuf>,
        /// None keeps the transport non-streaming for the Part 2 fakes;
        /// streaming tests pass `Some(true)`.
        stream: Option<bool>,
        progress: Option<(ProgressTx, Option<String>)>,
        timeout_secs: Option<u64>,
    }

    impl OpenAIProvider {
        fn for_test(cfg: TestConfig) -> Self {
            let extra_body = match cfg.extra_body {
                Some(value) => value
                    .as_object()
                    .expect("extra_body must be an object")
                    .clone(),
                None => serde_json::Map::new(),
            };
            Self {
                client: Client::builder()
                    .connect_timeout(std::time::Duration::from_secs(5))
                    .build()
                    .unwrap(),
                base_url: cfg
                    .base_url
                    .unwrap_or_else(|| "http://127.0.0.1:9/v1".to_string()),
                api_key: None,
                timeout: std::time::Duration::from_secs(cfg.timeout_secs.unwrap_or(5)),
                standard_model: "test-model".to_string(),
                advanced_model: "test-model".to_string(),
                tool_choice: cfg.tool_choice.unwrap_or(OpenAIToolChoice::Required),
                max_tokens: cfg.max_tokens.unwrap_or(OPENAI_DEFAULT_MAX_TOKENS),
                temperature: cfg.temperature,
                extra_body,
                pdf_mode: cfg.pdf_mode.unwrap_or(OpenAIPdfMode::Both),
                pdf_max_pages: cfg.pdf_max_pages.unwrap_or(OPENAI_DEFAULT_PDF_MAX_PAGES),
                pdf_extract: PdfExtractConfig {
                    cmd: cfg.pdf_extract_cmd,
                    extra_args: Vec::new(),
                    timeout: std::time::Duration::from_secs(30),
                },
                stream: cfg.stream.unwrap_or(false),
                progress: cfg.progress,
            }
        }
    }

    fn call_extract_unified(provider: &OpenAIProvider) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(provider.chat_with_tools(
            "system",
            "user",
            "extract_unified",
            json!({"type": "object"}),
            ModelTier::Standard,
            None,
        ))
        .expect("fake server returns a valid tool call");
    }

    /// A schema shaped like `build_unified_tool_schema`, enough for the
    /// content-recovery path to check a bare object's top-level keys.
    fn unified_test_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "transactions": {},
                "holdings": {},
                "investments": {}
            }
        })
    }

    fn call_openai_files(
        provider: &OpenAIProvider,
        files: &[(String, String, Vec<u8>)],
    ) -> Result<ProviderCallResult, ProviderError> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(provider.chat_with_files_and_tools(
            "system",
            files,
            "supplement text",
            "extract_unified",
            unified_test_schema(),
            None,
        ))
    }

    fn pdf_file() -> (String, String, Vec<u8>) {
        (
            "statement.pdf".to_string(),
            "application/pdf".to_string(),
            pdf_extract::tests::sample_pdf_bytes(),
        )
    }

    fn png_file() -> (String, String, Vec<u8>) {
        (
            "receipt.png".to_string(),
            "image/png".to_string(),
            vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a],
        )
    }

    fn csv_file() -> (String, String, Vec<u8>) {
        (
            "statement.csv".to_string(),
            "text/csv".to_string(),
            b"date,description,amount\n2026-11-02,Corner Shop,-12.00\n".to_vec(),
        )
    }

    fn user_content_parts(request: &Value) -> Vec<Value> {
        request["messages"][1]["content"]
            .as_array()
            .expect("user message content must be an array")
            .clone()
    }

    fn fake_tool_in_temp_dir(
        behaviour: pdf_extract::tests::Behaviour,
    ) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::TempDir::new().unwrap();
        let tool = pdf_extract::tests::write_fake_tool(dir.path(), behaviour);
        (dir, tool)
    }

    fn image_url_parts(parts: &[Value]) -> Vec<String> {
        parts
            .iter()
            .filter(|p| p["type"] == "image_url")
            .map(|p| p["image_url"]["url"].as_str().unwrap_or("").to_string())
            .collect()
    }

    #[test]
    fn test_openai_tool_choice_forms() {
        for (choice, expected) in [
            (OpenAIToolChoice::Required, json!("required")),
            (
                OpenAIToolChoice::Function,
                json!({"type": "function", "function": { "name": "extract_unified" }}),
            ),
            (OpenAIToolChoice::Auto, json!("auto")),
        ] {
            let (url, server) = serve_once(OPENAI_EXTRACT_UNIFIED_RESPONSE);
            let provider = OpenAIProvider::for_test(TestConfig {
                base_url: Some(format!("{url}/v1")),
                tool_choice: Some(choice),
                ..Default::default()
            });
            call_extract_unified(&provider);
            let request = request_json(&server.join().unwrap());
            assert_eq!(request["tool_choice"], expected, "form: {choice:?}");
        }
    }

    #[test]
    fn test_openai_max_tokens_default_and_zero_omits() {
        let (url, server) = serve_once(OPENAI_EXTRACT_UNIFIED_RESPONSE);
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            ..Default::default()
        });
        call_extract_unified(&provider);
        let request = request_json(&server.join().unwrap());
        assert_eq!(request["max_tokens"], json!(16_384));

        let (url, server) = serve_once(OPENAI_EXTRACT_UNIFIED_RESPONSE);
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            max_tokens: Some(0),
            ..Default::default()
        });
        call_extract_unified(&provider);
        let request = request_json(&server.join().unwrap());
        assert!(
            request.get("max_tokens").is_none(),
            "max_tokens must be absent when 0: {request}"
        );
    }

    #[test]
    fn test_openai_temperature_absent_unless_set() {
        let (url, server) = serve_once(OPENAI_EXTRACT_UNIFIED_RESPONSE);
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            ..Default::default()
        });
        call_extract_unified(&provider);
        let request = request_json(&server.join().unwrap());
        assert!(
            request.get("temperature").is_none(),
            "no temperature unless set: {request}"
        );

        let (url, server) = serve_once(OPENAI_EXTRACT_UNIFIED_RESPONSE);
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            temperature: Some(0.2),
            ..Default::default()
        });
        call_extract_unified(&provider);
        let request = request_json(&server.join().unwrap());
        assert_eq!(request["temperature"], json!(0.2));
    }

    #[test]
    fn test_openai_extra_body_is_merged_into_every_request() {
        let (url, server) = serve_once(OPENAI_EXTRACT_UNIFIED_RESPONSE);
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            extra_body: Some(json!({
                "chat_template_kwargs": { "enable_thinking": false },
                "top_k": 5
            })),
            ..Default::default()
        });
        call_extract_unified(&provider);
        let request = request_json(&server.join().unwrap());
        assert_eq!(
            request["chat_template_kwargs"]["enable_thinking"],
            json!(false)
        );
        assert_eq!(request["top_k"], json!(5));
    }

    #[test]
    fn test_openai_from_env_rejects_bad_knobs() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for (key, value) in [
            ("FYNANCE_OPENAI_TOOL_CHOICE", "sometimes"),
            ("FYNANCE_OPENAI_MAX_TOKENS", "lots"),
            ("FYNANCE_OPENAI_TEMPERATURE", "hot"),
            ("FYNANCE_OPENAI_PDF_MODE", "scanned"),
            ("FYNANCE_OPENAI_PDF_MAX_PAGES", "many"),
            ("FYNANCE_OPENAI_EXTRA_BODY", r#"[1,2]"#),
            ("FYNANCE_OPENAI_EXTRA_BODY", r#"{"messages": []}"#),
            ("FYNANCE_OPENAI_STREAM", "sometimes"),
        ] {
            clear_openai_env();
            unsafe {
                std::env::set_var("FYNANCE_OPENAI_BASE_URL", "http://127.0.0.1:9/v1");
                std::env::set_var(key, value);
            }
            let result = OpenAIProvider::from_env();
            clear_openai_env();
            let msg = match result {
                Err(e) => e.to_string(),
                Ok(_) => panic!("{key}={value} must be rejected"),
            };
            assert!(msg.contains(key), "{key}={value}: got {msg}");
        }
    }

    #[test]
    fn test_openai_files_part_order() {
        let (url, server) = serve_once(OPENAI_EXTRACT_UNIFIED_RESPONSE);
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            ..Default::default()
        });
        call_openai_files(&provider, &[csv_file(), png_file()]).unwrap();
        let request = request_json(&server.join().unwrap());
        let parts = user_content_parts(&request);

        assert_eq!(
            parts[0]["text"]
                .as_str()
                .unwrap_or("")
                .strip_prefix("Source document filename: ")
                .unwrap_or(""),
            "statement.csv\nFor every row you extract from the document immediately below, set its \"source_file\" field to exactly this filename.",
            "label part: {}",
            parts[0]
        );
        assert!(
            parts[1]["text"]
                .as_str()
                .unwrap_or("")
                .starts_with("--- file: statement.csv (text/csv) ---\ndate,description,amount"),
            "csv part: {}",
            parts[1]
        );
        assert!(
            parts[2]["text"]
                .as_str()
                .unwrap_or("")
                .starts_with("Source document filename: receipt.png"),
            "label part: {}",
            parts[2]
        );
        assert_eq!(parts[3]["type"], "image_url");
        assert!(
            parts[3]["image_url"]["url"]
                .as_str()
                .unwrap_or("")
                .starts_with("data:image/png;base64,"),
            "image part: {}",
            parts[3]
        );
        assert_eq!(parts[4]["text"], json!("supplement text"));
        assert_eq!(parts.len(), 5, "parts: {parts:?}");
    }

    #[test]
    fn test_openai_pdf_mode_both() {
        let (_dir, tool) = fake_tool_in_temp_dir(pdf_extract::tests::Behaviour::Ok);
        let (url, server) = serve_once(OPENAI_EXTRACT_UNIFIED_RESPONSE);
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            pdf_extract_cmd: Some(tool),
            ..Default::default()
        });
        call_openai_files(&provider, &[pdf_file()]).unwrap();
        let parts = user_content_parts(&request_json(&server.join().unwrap()));

        let images = image_url_parts(&parts);
        assert_eq!(images.len(), 3, "every page sends its image: {parts:?}");
        assert!(
            images
                .iter()
                .all(|u| u.starts_with("data:image/png;base64,")),
            "{images:?}"
        );

        let page_labels: Vec<&str> = parts
            .iter()
            .filter(|p| p["text"].as_str().unwrap_or("").starts_with("Page "))
            .map(|p| p["text"].as_str().unwrap())
            .collect();
        assert_eq!(
            page_labels,
            [
                "Page 1 of 3 of statement.pdf",
                "Page 2 of 3 of statement.pdf",
                "Page 3 of 3 of statement.pdf"
            ]
        );

        // Pages 1-2 carry their text layer; page 3 (scanned) is image-only:
        // the part after its label is the image, not text.
        let page1_label = parts
            .iter()
            .position(|p| p["text"] == json!("Page 1 of 3 of statement.pdf"))
            .unwrap();
        assert!(
            parts[page1_label + 1]["text"]
                .as_str()
                .unwrap_or("")
                .contains("Corner Shop"),
            "page 1 text part: {}",
            parts[page1_label + 1]
        );
        let page3_label = parts
            .iter()
            .position(|p| p["text"] == json!("Page 3 of 3 of statement.pdf"))
            .unwrap();
        assert_eq!(parts[page3_label + 1]["type"], "image_url");
    }

    #[test]
    fn test_openai_pdf_mode_images_sends_no_page_text() {
        let (_dir, tool) = fake_tool_in_temp_dir(pdf_extract::tests::Behaviour::Ok);
        let (url, server) = serve_once(OPENAI_EXTRACT_UNIFIED_RESPONSE);
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            pdf_extract_cmd: Some(tool),
            pdf_mode: Some(OpenAIPdfMode::Images),
            ..Default::default()
        });
        call_openai_files(&provider, &[pdf_file()]).unwrap();
        let parts = user_content_parts(&request_json(&server.join().unwrap()));
        assert_eq!(image_url_parts(&parts).len(), 3);
        assert!(
            !parts
                .iter()
                .any(|p| { p["text"].as_str().unwrap_or("").contains("Corner Shop") }),
            "images mode must not send page text"
        );
    }

    #[test]
    fn test_openai_pdf_mode_text_uses_images_only_for_scanned_pages() {
        let (_dir, tool) = fake_tool_in_temp_dir(pdf_extract::tests::Behaviour::Ok);
        let (url, server) = serve_once(OPENAI_EXTRACT_UNIFIED_RESPONSE);
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            pdf_extract_cmd: Some(tool),
            pdf_mode: Some(OpenAIPdfMode::Text),
            ..Default::default()
        });
        call_openai_files(&provider, &[pdf_file()]).unwrap();
        let parts = user_content_parts(&request_json(&server.join().unwrap()));
        assert!(
            parts
                .iter()
                .any(|p| { p["text"].as_str().unwrap_or("").contains("Corner Shop") })
        );
        let images = image_url_parts(&parts);
        assert_eq!(
            images.len(),
            1,
            "only the no-text-layer page sends an image"
        );
    }

    #[test]
    fn test_openai_pdf_page_cap_is_document_conversion() {
        let (_dir, tool) = fake_tool_in_temp_dir(pdf_extract::tests::Behaviour::Ok);
        let provider = OpenAIProvider::for_test(TestConfig {
            pdf_extract_cmd: Some(tool),
            pdf_max_pages: Some(2),
            ..Default::default()
        });
        let err = call_openai_files(&provider, &[pdf_file()])
            .expect_err("3 pages over a cap of 2 must fail");
        match err {
            ProviderError::DocumentConversion(msg) => {
                assert!(msg.contains("FYNANCE_OPENAI_PDF_MAX_PAGES"), "got: {msg}");
            }
            other => panic!("expected DocumentConversion, got {other}"),
        }
    }

    #[test]
    fn test_openai_pdf_page_failures_are_surfaced_to_model_and_notes() {
        let (_dir, tool) = fake_tool_in_temp_dir(pdf_extract::tests::Behaviour::PartialFail);
        let (url, server) = serve_once(OPENAI_EXTRACT_UNIFIED_RESPONSE);
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            pdf_extract_cmd: Some(tool),
            ..Default::default()
        });
        let result = call_openai_files(&provider, &[pdf_file()]).unwrap();
        let parts = user_content_parts(&request_json(&server.join().unwrap()));
        let texts: Vec<&str> = parts.iter().filter_map(|p| p["text"].as_str()).collect();
        assert!(
            texts
                .iter()
                .any(|t| *t == "Page 2 of statement.pdf could not be extracted: render failed"),
            "the model must be told which page is missing: {texts:?}"
        );
        // Surviving pages are labelled against the document's real page count
        // from the manifest, not the number of pages that came back.
        assert!(
            texts.iter().any(|t| *t == "Page 3 of 3 of statement.pdf"),
            "page labels: {texts:?}"
        );
        assert_eq!(
            result.warnings,
            vec!["page 2 of statement.pdf could not be extracted: render failed".to_string()]
        );
    }

    #[test]
    fn test_openai_pdf_native_mode_never_runs_the_tool() {
        // The Fail fake exits 3: reaching it would fail the call, so a green
        // call proves native mode skipped the tool entirely.
        let (_dir, tool) = fake_tool_in_temp_dir(pdf_extract::tests::Behaviour::Fail);
        let (url, server) = serve_once(OPENAI_EXTRACT_UNIFIED_RESPONSE);
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            pdf_extract_cmd: Some(tool),
            pdf_mode: Some(OpenAIPdfMode::Native),
            ..Default::default()
        });
        call_openai_files(&provider, &[pdf_file()]).unwrap();
        let parts = user_content_parts(&request_json(&server.join().unwrap()));
        let file_parts: Vec<&Value> = parts.iter().filter(|p| p["type"] == "file").collect();
        assert_eq!(file_parts.len(), 1, "exactly one file part: {parts:?}");
        assert_eq!(file_parts[0]["file"]["filename"], json!("statement.pdf"));
        assert!(
            file_parts[0]["file"]["file_data"]
                .as_str()
                .unwrap_or("")
                .starts_with("data:application/pdf;base64,"),
            "file part: {}",
            file_parts[0]
        );
        assert!(image_url_parts(&parts).is_empty());
    }

    #[test]
    fn test_openai_finish_reason_length_returns_truncated() {
        let (url, server) = serve_once(OPENAI_TRUNCATED_RESPONSE);
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            ..Default::default()
        });
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(provider.chat_with_tools(
            "system",
            "user",
            "extract_unified",
            json!({"type": "object"}),
            ModelTier::Standard,
            None,
        ));
        server.join().unwrap();
        match result {
            Err(ProviderError::Truncated { output_tokens }) => {
                assert_eq!(output_tokens, 999);
            }
            other => panic!("expected Truncated, got {other:?}"),
        }
    }

    #[test]
    fn test_openai_arguments_sent_as_object_are_accepted() {
        let body = json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "function": {
                            "name": "extract_unified",
                            "arguments": {"transactions": [], "holdings": [], "investments": []}
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 1, "completion_tokens": 2}
        })
        .to_string();
        let (url, server) = serve_once(Box::leak(body.into_boxed_str()));
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            ..Default::default()
        });
        call_extract_unified(&provider);
        server.join().unwrap();
    }

    #[test]
    fn test_openai_content_fallback_recovers_tool_call_block() {
        let content = "<think>pondering\nsome hidden reasoning\n</think><tool_call>{\"transactions\":[],\"holdings\":[],\"investments\":[]}</tool_call>";
        let body = openai_content_response(content);
        let (url, server) = serve_once(Box::leak(body.into_boxed_str()));
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            ..Default::default()
        });
        let result = call_openai_files(&provider, &[csv_file()]).unwrap();
        assert_eq!(result.value["transactions"], json!([]));
        server.join().unwrap();
    }

    #[test]
    fn test_openai_content_fallback_recovers_fenced_json() {
        let content = "Sure, here is the extraction:\n```json\n{\"transactions\": [], \"holdings\": [], \"investments\": []}\n```";
        let body = openai_content_response(content);
        let (url, server) = serve_once(Box::leak(body.into_boxed_str()));
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            ..Default::default()
        });
        let result = call_openai_files(&provider, &[csv_file()]).unwrap();
        assert_eq!(result.value["holdings"], json!([]));
        server.join().unwrap();
    }

    #[test]
    fn test_openai_prose_only_returns_no_tool_use() {
        let body = openai_content_response("I could not find any transactions in that file.");
        let (url, server) = serve_once(Box::leak(body.into_boxed_str()));
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            ..Default::default()
        });
        let err = call_openai_files(&provider, &[csv_file()])
            .expect_err("prose must not produce an extraction");
        assert!(matches!(err, ProviderError::NoToolUse { .. }), "got: {err}");
        server.join().unwrap();
    }

    #[test]
    fn test_openai_wrong_named_tool_call_returns_no_tool_use() {
        let body = json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "function": {
                            "name": "some_other_tool",
                            "arguments": "{}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        })
        .to_string();
        let (url, server) = serve_once(Box::leak(body.into_boxed_str()));
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            ..Default::default()
        });
        let err = call_extract_unified_result(&provider).expect_err("wrong name must fail");
        assert!(matches!(err, ProviderError::NoToolUse { .. }), "got: {err}");
        server.join().unwrap();
    }

    #[test]
    fn test_openai_content_fallback_rejects_empty_object() {
        let body = openai_content_response("{}");
        let (url, server) = serve_once(Box::leak(body.into_boxed_str()));
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            ..Default::default()
        });
        let err = call_extract_unified_result(&provider)
            .expect_err("an empty recovered object must not become an empty extraction");
        assert!(matches!(err, ProviderError::NoToolUse { .. }), "got: {err}");
        server.join().unwrap();
    }

    #[test]
    fn test_openai_content_fallback_rejects_error_payload() {
        let body =
            openai_content_response("```json\n{\"error\": \"cannot read the document\"}\n```");
        let (url, server) = serve_once(Box::leak(body.into_boxed_str()));
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            ..Default::default()
        });
        let err = call_extract_unified_result(&provider)
            .expect_err("an error payload must not become an extraction");
        assert!(matches!(err, ProviderError::NoToolUse { .. }), "got: {err}");
        server.join().unwrap();
    }

    #[test]
    fn test_openai_content_fallback_rejects_wrong_tool_envelope() {
        let body = openai_content_response(
            r#"{"name": "some_other_tool", "arguments": {"transactions": []}}"#,
        );
        let (url, server) = serve_once(Box::leak(body.into_boxed_str()));
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            ..Default::default()
        });
        let err = call_extract_unified_result(&provider)
            .expect_err("another tool's call must not be recovered as ours");
        assert!(matches!(err, ProviderError::NoToolUse { .. }), "got: {err}");
        server.join().unwrap();
    }

    #[test]
    fn test_openai_content_fallback_bare_object_must_overlap_schema_properties() {
        let schema = json!({
            "type": "object",
            "properties": {
                "transactions": {},
                "holdings": {},
                "investments": {}
            }
        });
        let call = |content: &str| -> Result<Value, ProviderError> {
            let reply = OpenAIReply {
                content: Some(content.to_string()),
                tool_calls: Vec::new(),
                finish_reason: Some("stop".to_string()),
                invalid_arguments: None,
            };
            post_process_openai_reply(&reply, "extract_unified", &schema, 5).map(|(v, _)| v)
        };
        // A bare object that carries the tool's own top-level keys recovers.
        let value = call("```json\n{\"transactions\": [], \"holdings\": []}\n```")
            .expect("schema-overlapping bare object must recover");
        assert_eq!(value["transactions"], json!([]));
        // One that shares nothing with the schema does not.
        let err = call("```json\n{\"statements\": [{\"rows\": []}]}\n```")
            .expect_err("non-overlapping bare object must not recover");
        assert!(matches!(err, ProviderError::NoToolUse { .. }), "got: {err}");
    }

    #[test]
    fn test_openai_unparsable_tool_arguments_returns_response_unreadable() {
        let body = json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "function": {
                            "name": "extract_unified",
                            "arguments": "{\"transactions\":["
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        })
        .to_string();
        let (url, server) = serve_once(Box::leak(body.into_boxed_str()));
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            ..Default::default()
        });
        let err = call_extract_unified_result(&provider)
            .expect_err("broken JSON arguments must be an unreadable reply, not NoToolUse");
        match err {
            ProviderError::ResponseUnreadable(detail) => {
                assert!(
                    detail.contains("parsing OpenAI function arguments"),
                    "got: {detail}"
                );
            }
            other => panic!("expected ResponseUnreadable, got {other}"),
        }
        server.join().unwrap();
    }

    fn call_extract_unified_result(
        provider: &OpenAIProvider,
    ) -> Result<ProviderCallResult, ProviderError> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(provider.chat_with_tools(
            "system",
            "user",
            "extract_unified",
            unified_test_schema(),
            ModelTier::Standard,
            None,
        ))
    }

    #[test]
    fn test_anthropic_max_tokens_with_truncated_json_returns_truncated() {
        let mut acc = SseAccumulator::new();
        acc.handle_event(&json!({
            "type": "content_block_start", "index": 0,
            "content_block": { "type": "tool_use", "id": "tu_1", "name": "extract_unified" }
        }));
        acc.handle_event(&json!({
            "type": "content_block_delta", "index": 0,
            "delta": { "type": "input_json_delta", "partial_json": "{\"transactions\":[" }
        }));
        acc.handle_event(&json!({
            "type": "message_delta",
            "delta": { "stop_reason": "max_tokens" },
            "usage": { "output_tokens": 1234 }
        }));
        match acc.into_result("extract_unified") {
            Err(ProviderError::Truncated { output_tokens }) => {
                assert_eq!(output_tokens, 1234);
            }
            other => panic!("expected Truncated, got {other:?}"),
        }
    }

    #[test]
    fn test_openai_from_env_reads_knobs() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_openai_env();
        unsafe {
            std::env::set_var("FYNANCE_OPENAI_BASE_URL", "http://127.0.0.1:9/v1");
            std::env::set_var("FYNANCE_OPENAI_TOOL_CHOICE", "auto");
            std::env::set_var("FYNANCE_OPENAI_MAX_TOKENS", "64000");
            std::env::set_var("FYNANCE_OPENAI_TEMPERATURE", "0");
            std::env::set_var("FYNANCE_OPENAI_PDF_MODE", "native");
            std::env::set_var("FYNANCE_OPENAI_PDF_MAX_PAGES", "10");
            std::env::set_var(
                "FYNANCE_OPENAI_EXTRA_BODY",
                r#"{"chat_template_kwargs":{"enable_thinking":false}}"#,
            );
        }
        let provider = OpenAIProvider::from_env().unwrap();
        clear_openai_env();

        assert_eq!(provider.tool_choice, OpenAIToolChoice::Auto);
        assert_eq!(provider.max_tokens, 64000);
        assert_eq!(provider.temperature, Some(0.0));
        assert_eq!(provider.pdf_mode, OpenAIPdfMode::Native);
        assert_eq!(provider.pdf_max_pages, 10);
        assert_eq!(
            provider.extra_body["chat_template_kwargs"]["enable_thinking"],
            json!(false)
        );
        assert!(provider.stream, "streaming is on unless disabled");

        clear_openai_env();
        unsafe {
            std::env::set_var("FYNANCE_OPENAI_BASE_URL", "http://127.0.0.1:9/v1");
            std::env::set_var("FYNANCE_OPENAI_STREAM", "false");
        }
        let provider = OpenAIProvider::from_env().unwrap();
        clear_openai_env();
        assert!(!provider.stream);
    }

    // ── Part 3: streaming transport ─────────────────────────────────────────

    /// One-shot SSE fake: answers the first request with a chunked
    /// `text/event-stream` body made of `chunks`, written one at a time with
    /// `gap` between writes when set. Stops writing when the client hangs up,
    /// so cancellation is observable. The handle returns the raw request, the
    /// milliseconds from the first write until the connection died, and
    /// whether every chunk got out.
    fn serve_sse(
        chunks: Vec<Vec<u8>>,
        gap: Option<std::time::Duration>,
    ) -> (String, std::thread::JoinHandle<(String, u128, bool)>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                .unwrap();
            let mut raw = Vec::new();
            let mut buf = [0u8; 4096];
            while !http_request_complete(&raw) {
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => raw.extend_from_slice(&buf[..n]),
                }
            }
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                       Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
            let started = std::time::Instant::now();
            let mut wrote_all = true;
            for (i, chunk) in chunks.iter().enumerate() {
                if i > 0 {
                    if let Some(gap) = gap {
                        std::thread::sleep(gap);
                    }
                }
                let frame = format!("{:X}\r\n", chunk.len());
                if stream
                    .write_all(frame.as_bytes())
                    .and_then(|()| {
                        stream.write_all(chunk)?;
                        stream.write_all(b"\r\n")?;
                        stream.flush()
                    })
                    .is_err()
                {
                    wrote_all = false;
                    break;
                }
            }
            if wrote_all {
                let _ = stream.write_all(b"0\r\n\r\n");
                let _ = stream.flush();
                // Wait for the client to hang up (it does when the call
                // finishes, times out, or is cancelled).
                loop {
                    match stream.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => continue,
                    }
                }
            }
            (
                String::from_utf8_lossy(&raw).into_owned(),
                started.elapsed().as_millis(),
                wrote_all,
            )
        });
        (format!("http://{addr}"), handle)
    }

    fn sse_chunk(event: Value) -> Vec<u8> {
        format!("data: {event}\n\n").into_bytes()
    }

    const STREAM_TOOL_ARGS: &str = r#"{"transactions":[{"date":"2026-11-02","description":"Corner Shop £5.00","amount":"-5.00","currency":"GBP","row_confidence":0.9}],"holdings":[],"investments":[]}"#;

    fn stream_delta(args_fragment: &str) -> Value {
        json!({
            "choices": [{
                "index": 0,
                "delta": { "tool_calls": [{ "index": 0, "function": { "arguments": args_fragment } }] },
                "finish_reason": null
            }]
        })
    }

    fn stream_openai(provider: &OpenAIProvider) -> Result<ProviderCallResult, ProviderError> {
        call_extract_unified_result(provider)
    }

    #[test]
    fn test_openai_stream_accumulates_tool_calls_usage_and_split_multibyte_bytes() {
        // One argument fragment carries a multi-byte £; its wire bytes are
        // split through the middle of the character across two writes.
        let mut full = sse_chunk(stream_delta(STREAM_TOOL_ARGS));
        let pound = full
            .windows(2)
            .position(|w| w[0] == 0xc2 && w[1] == 0xa3)
            .unwrap();
        let tail = full.split_off(pound + 1);

        let chunks = vec![
            sse_chunk(json!({
                "model": "fake-stream",
                "choices": [{
                    "index": 0,
                    "delta": {
                        "role": "assistant",
                        "tool_calls": [{ "index": 0, "id": "c0",
                            "function": { "name": "extract_unified", "arguments": "" } }]
                    },
                    "finish_reason": null
                }]
            })),
            full,
            tail,
            sse_chunk(json!({
                "choices": [{ "index": 0, "delta": {}, "finish_reason": "tool_calls" }]
            })),
            sse_chunk(json!({ "usage": { "prompt_tokens": 30, "completion_tokens": 120 } })),
            b"data: [DONE]\n\n".to_vec(),
        ];
        let (url, server) = serve_sse(chunks, None);
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            stream: Some(true),
            ..Default::default()
        });
        let result = stream_openai(&provider).expect("streamed tool call must parse");
        let (request, _ms, wrote_all) = server.join().unwrap();
        assert!(wrote_all, "fake must have sent every chunk");

        assert_eq!(
            result.value,
            serde_json::from_str::<Value>(STREAM_TOOL_ARGS).unwrap()
        );
        assert_eq!(result.usage.input_tokens, 30);
        assert_eq!(result.usage.output_tokens, 120);
        assert_eq!(result.model, "fake-stream");
        assert_eq!(result.stop_reason.as_deref(), Some("tool_calls"));

        let request = request_json(&request);
        assert_eq!(request["stream"], json!(true));
        assert_eq!(request["stream_options"]["include_usage"], json!(true));
    }

    #[test]
    fn test_openai_stream_in_stream_error_is_upstream_server_error() {
        let chunks = vec![
            sse_chunk(stream_delta("{\"transactions\":[")),
            sse_chunk(json!({ "error": { "message": "model crashed", "type": "server_error" } })),
            b"data: [DONE]\n\n".to_vec(),
        ];
        let (url, server) = serve_sse(chunks, None);
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            stream: Some(true),
            ..Default::default()
        });
        let err = stream_openai(&provider).expect_err("in-stream error must fail");
        match err {
            ProviderError::UpstreamServerError { status, body } => {
                assert_eq!(status, 0);
                assert!(body.contains("model crashed"), "got: {body}");
            }
            other => panic!("expected UpstreamServerError, got {other}"),
        }
        server.join().unwrap();
    }

    #[test]
    fn test_openai_stream_finish_reason_length_returns_truncated() {
        let chunks = vec![
            sse_chunk(stream_delta("{\"transactions\":[{\"date\":\"2026-11-")),
            sse_chunk(json!({
                "choices": [{ "index": 0, "delta": {}, "finish_reason": "length" }]
            })),
            sse_chunk(json!({ "usage": { "prompt_tokens": 50, "completion_tokens": 16384 } })),
            b"data: [DONE]\n\n".to_vec(),
        ];
        let (url, server) = serve_sse(chunks, None);
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            stream: Some(true),
            ..Default::default()
        });
        let err = stream_openai(&provider).expect_err("length must truncate");
        match err {
            ProviderError::Truncated { output_tokens } => assert_eq!(output_tokens, 16384),
            other => panic!("expected Truncated, got {other}"),
        }
        server.join().unwrap();
    }

    #[test]
    fn test_openai_stream_content_fallback_recovers_tool_input() {
        let pieces = [
            "Here you go:\n```json\n{\"transactions\":[]",
            ",\"holdings\":[],\"investments\":[]}\n```",
        ];
        let mut chunks: Vec<Vec<u8>> = pieces
            .iter()
            .map(|p| {
                sse_chunk(json!({
                    "choices": [{ "index": 0, "delta": { "content": p }, "finish_reason": null }]
                }))
            })
            .collect();
        chunks.push(sse_chunk(json!({
            "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }]
        })));
        chunks.push(b"data: [DONE]\n\n".to_vec());
        let (url, server) = serve_sse(chunks, None);
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            stream: Some(true),
            ..Default::default()
        });
        let result = stream_openai(&provider).expect("content fallback must recover");
        assert_eq!(
            result.value,
            json!({ "transactions": [], "holdings": [], "investments": [] })
        );
        server.join().unwrap();
    }

    #[test]
    fn test_openai_stream_prose_only_returns_no_tool_use() {
        let chunks = vec![
            sse_chunk(json!({
                "choices": [{ "index": 0, "delta": { "content": "I could not read it." },
                              "finish_reason": null }]
            })),
            sse_chunk(json!({
                "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }]
            })),
            b"data: [DONE]\n\n".to_vec(),
        ];
        let (url, server) = serve_sse(chunks, None);
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            stream: Some(true),
            ..Default::default()
        });
        let err = stream_openai(&provider).expect_err("prose is not a tool call");
        assert!(matches!(err, ProviderError::NoToolUse { .. }), "got: {err}");
        server.join().unwrap();
    }

    #[test]
    fn test_openai_stream_unparsable_arguments_returns_response_unreadable() {
        let chunks = vec![
            sse_chunk(stream_delta("{\"transactions\":[")),
            sse_chunk(json!({
                "choices": [{ "index": 0, "delta": {}, "finish_reason": "tool_calls" }]
            })),
            b"data: [DONE]\n\n".to_vec(),
        ];
        let (url, server) = serve_sse(chunks, None);
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            stream: Some(true),
            ..Default::default()
        });
        let err = stream_openai(&provider)
            .expect_err("broken arguments must be unreadable, not NoToolUse");
        match err {
            ProviderError::ResponseUnreadable(detail) => {
                assert!(
                    detail.contains("parsing streamed OpenAI function arguments"),
                    "got: {detail}"
                );
            }
            other => panic!("expected ResponseUnreadable, got {other}"),
        }
        server.join().unwrap();
    }

    #[test]
    fn test_openai_stream_error_event_line_is_upstream_server_error() {
        // llama.cpp-style in-stream failure: an `error:` event line, not a
        // `data:` payload.
        let chunks = vec![
            sse_chunk(stream_delta("{\"transactions\":[")),
            b"event: error\nerror: {\"message\": \"model crashed\"}\n\n".to_vec(),
        ];
        let (url, server) = serve_sse(chunks, None);
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            stream: Some(true),
            ..Default::default()
        });
        let err = stream_openai(&provider).expect_err("an error: line must fail the call");
        match err {
            ProviderError::UpstreamServerError { status, body } => {
                assert_eq!(status, 0);
                assert!(body.contains("model crashed"), "got: {body}");
            }
            other => panic!("expected UpstreamServerError, got {other}"),
        }
        server.join().unwrap();
    }

    #[test]
    fn test_openai_stream_stall_times_out() {
        let chunks = vec![
            sse_chunk(stream_delta("{\"transactions\":[")),
            sse_chunk(stream_delta("{\"date\":\"later\"}]}")),
        ];
        // The fake sleeps 2 s between the two writes; the provider deadline
        // is 1 s, so the fake's next write lands after the hang-up.
        let (url, server) = serve_sse(chunks, Some(std::time::Duration::from_secs(2)));
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            stream: Some(true),
            timeout_secs: Some(1),
            ..Default::default()
        });
        let err = stream_openai(&provider).expect_err("stalled stream must time out");
        assert!(matches!(err, ProviderError::Timeout(_)), "got: {err}");
        // The plan asks for more than the timeout itself: the fake must see
        // the connection close within 3 s of its first write, which is what
        // lets it finish at all.
        let (_request, disconnected_ms, _wrote_all) = server.join().unwrap();
        assert!(
            disconnected_ms < 3000,
            "hang-up took {disconnected_ms} ms to reach the fake"
        );
    }

    #[test]
    fn test_openai_stream_dropping_the_call_cancels_upstream() {
        // A slow fake: 12 chunks, 200 ms apart, so the client hangs up well
        // before the stream would end.
        let chunks = vec![sse_chunk(stream_delta("{\"transactions\":[")); 12];
        let (url, server) = serve_sse(chunks, Some(std::time::Duration::from_millis(200)));
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            stream: Some(true),
            ..Default::default()
        });
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let fut = provider.chat_with_tools(
                "system",
                "user",
                "extract_unified",
                json!({"type": "object"}),
                ModelTier::Standard,
                None,
            );
            tokio::pin!(fut);
            let outcome = tokio::time::timeout(std::time::Duration::from_millis(300), fut).await;
            assert!(outcome.is_err(), "call must still be running at 300 ms");
            // The timeout dropping the pinned future is what hangs up.
        });
        let (_request, disconnected_ms, wrote_all) = server.join().unwrap();
        assert!(!wrote_all, "fake must have seen the hang-up mid-stream");
        assert!(
            disconnected_ms < 3000,
            "disconnect took {disconnected_ms} ms to reach the fake"
        );
    }

    #[test]
    fn test_openai_stream_emits_progress_events() {
        let (tx, mut rx) = tokio::sync::broadcast::channel::<ProgressEvent>(32);
        // Deltas 1.1 s apart so at least one second passes between two
        // progress emissions.
        let chunks = vec![
            sse_chunk(json!({
                "choices": [{
                    "index": 0,
                    "delta": { "tool_calls": [{ "index": 0,
                        "function": { "name": "extract_unified", "arguments": "{\"transactions\":[" } }] },
                    "finish_reason": null
                }]
            })),
            sse_chunk(stream_delta(
                "{\"date\":\"2026-11-01\",\"description\":\"A\",\"amount\":\"-1.00\",\"currency\":\"GBP\",\"row_confidence\":0.9},",
            )),
            sse_chunk(stream_delta(
                "{\"date\":\"2026-11-02\",\"description\":\"B\",\"amount\":\"-2.00\",\"currency\":\"GBP\",\"row_confidence\":0.9}",
            )),
            sse_chunk(stream_delta("],\"holdings\":[],\"investments\":[]}")),
            sse_chunk(json!({
                "choices": [{ "index": 0, "delta": {}, "finish_reason": "tool_calls" }]
            })),
            b"data: [DONE]\n\n".to_vec(),
        ];
        let (url, server) = serve_sse(chunks, Some(std::time::Duration::from_millis(1100)));
        let provider = OpenAIProvider::for_test(TestConfig {
            base_url: Some(format!("{url}/v1")),
            stream: Some(true),
            progress: Some((tx, Some("task-1".to_string()))),
            // The fake needs ~5.5 s to drip the chunks out; the deadline is
            // not what this test is checking.
            timeout_secs: Some(15),
            ..Default::default()
        });
        let result = stream_openai(&provider).expect("streamed tool call must parse");
        server.join().unwrap();
        assert_eq!(result.value["transactions"].as_array().unwrap().len(), 2);

        let mut started = 0;
        let mut progress = Vec::new();
        while let Ok(event) = rx.try_recv() {
            match event {
                ProgressEvent::LlmStart {
                    model,
                    input_tokens,
                    task_id,
                } => {
                    assert_eq!(model, "test-model");
                    assert_eq!(input_tokens, 0);
                    assert_eq!(task_id.as_deref(), Some("task-1"));
                    started += 1;
                }
                ProgressEvent::LlmProgress {
                    output_tokens,
                    items,
                    section,
                    ..
                } => progress.push((output_tokens, items, section)),
                other => panic!("unexpected progress event: {other:?}"),
            }
        }
        assert_eq!(started, 1, "exactly one LlmStart");
        assert!(
            progress.len() >= 2,
            "expected at least two LlmProgress events, got {progress:?}"
        );
        // The final emission sees the completed buffer, whose last section
        // keyword is "investments".
        let (tokens, items, section) = progress.last().unwrap();
        assert!(*items >= 2, "rows must grow: {progress:?}");
        assert_eq!(section.as_deref(), Some("investments"));
        let first_tokens = progress[0].0;
        assert!(
            *tokens > first_tokens,
            "delta count must grow: {progress:?}"
        );
    }

    #[test]
    fn test_sse_line_splitter_edge_cases() {
        let mut splitter = SseLineSplitter::new();
        let mut lines = Vec::new();
        lines.extend(splitter.push(b"data:{\"a\":1}\r\n")); // no space after colon
        lines.extend(splitter.push(b": ping\n")); // SSE comment
        lines.extend(splitter.push(b"data: [DONE]")); // line not yet ended
        lines.extend(splitter.push(b"\r\n")); // completed by the next write
        lines.extend(splitter.push(b"event: message_start\n\n")); // blank line
        assert_eq!(
            lines,
            [
                "data:{\"a\":1}",
                ": ping",
                "data: [DONE]",
                "event: message_start",
                "",
            ]
        );

        // A multi-byte character split across two writes must survive.
        let bytes = "data: \"Café\"\n".as_bytes().to_vec();
        let acute = bytes
            .windows(2)
            .position(|w| w[0] == 0xc3 && w[1] == 0xa9)
            .unwrap();
        let (first, second) = bytes.split_at(acute + 1);
        let mut splitter = SseLineSplitter::new();
        assert!(splitter.push(first).is_empty());
        assert_eq!(splitter.push(second), ["data: \"Café\""]);
    }

    #[test]
    fn test_anthropic_sse_splitter_keeps_split_multibyte_tool_input() {
        // The same network-shaped split fed through the Anthropic path: the
        // accumulator must see the intact characters.
        let mut acc = SseAccumulator::new();
        acc.handle_event(&json!({
            "type": "content_block_start", "index": 0,
            "content_block": { "type": "tool_use", "id": "tu_1", "name": "extract_unified" }
        }));
        let event = json!({
            "type": "content_block_delta", "index": 0,
            "delta": { "type": "input_json_delta", "partial_json": "{\"note\":\"Café ☕\"}" }
        });
        let bytes = format!("data: {event}\n\n").into_bytes();
        let split = bytes
            .windows(2)
            .position(|w| w[0] == 0xe2 && w[1] == 0x98)
            .unwrap();
        let (first, second) = bytes.split_at(split + 1);
        let mut splitter = SseLineSplitter::new();
        let mut lines = splitter.push(first);
        lines.extend(splitter.push(second));
        // The data line, then the blank line ending the event.
        assert_eq!(lines.len(), 2);
        let parsed: Value =
            serde_json::from_str(lines[0].strip_prefix("data: ").expect("data line"))
                .expect("line must decode and parse");
        acc.handle_event(&parsed);
        let result = acc.into_result("extract_unified").expect("tool input");
        assert_eq!(result.value["note"], json!("Café ☕"));
    }
}
