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

use crate::model::Agent;

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
}

const MAX_TOKENS_TEXT: u32 = 16_384;
const MAX_TOKENS_DOCUMENTS: u32 = 32_000;

// ── Progress reporting ──────────────────────────────────────────────────────

pub type ProgressTx = tokio::sync::broadcast::Sender<ProgressEvent>;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ProgressEvent {
    Phase {
        phase: ParsePhase,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        task_id: Option<String>,
    },
    LlmStart {
        model: String,
        input_tokens: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        task_id: Option<String>,
    },
    LlmProgress {
        output_tokens: u64,
        elapsed_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        task_id: Option<String>,
    },
    Done {
        total_ms: u64,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
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
    /// progress. Only `AnthropicProvider` implements this.
    fn with_progress(
        &self,
        _tx: ProgressTx,
        _task_id: Option<String>,
    ) -> Option<Arc<dyn LlmProvider>> {
        None
    }
}

// ── AnthropicProvider ─────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct AnthropicProvider {
    client: Client,
    api_key: String,
    standard_model: String,
    advanced_model: String,
    progress: Option<(ProgressTx, Option<String>)>,
}

impl AnthropicProvider {
    /// Build from environment variables.
    ///
    /// Required: `FYNANCE_ANTHROPIC_API_KEY`
    /// Optional: `FYNANCE_IMPORT_LLM_MODEL`, `FYNANCE_PARSE_PDF_MODEL`
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("FYNANCE_ANTHROPIC_API_KEY").map_err(|_| {
            anyhow!(
                "FYNANCE_ANTHROPIC_API_KEY is not set. \
                 Set it in your .env file or environment."
            )
        })?;
        let standard_model = std::env::var("FYNANCE_IMPORT_LLM_MODEL")
            .unwrap_or_else(|_| "claude-haiku-4-5-20251001".to_string());
        let advanced_model = std::env::var("FYNANCE_PARSE_PDF_MODEL")
            .unwrap_or_else(|_| "claude-sonnet-4-6".to_string());

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

    fn resolve_model(&self, tier: ModelTier, agent_override: Option<Agent>) -> String {
        match agent_override {
            Some(agent) => anthropic_model_for_agent(agent).to_string(),
            None => self.model_for_tier(tier).to_string(),
        }
    }

    pub fn clone_with_progress(
        &self,
        tx: ProgressTx,
        task_id: Option<String>,
    ) -> Self {
        Self {
            client: self.client.clone(),
            api_key: self.api_key.clone(),
            standard_model: self.standard_model.clone(),
            advanced_model: self.advanced_model.clone(),
            progress: Some((tx, task_id)),
        }
    }
}

/// Latest frontier model id per agent. Keep in sync with `pricing.rs`.
fn anthropic_model_for_agent(agent: Agent) -> &'static str {
    match agent {
        Agent::Haiku => "claude-haiku-4-5-20251001",
        Agent::Sonnet => "claude-sonnet-4-6",
        Agent::Opus => "claude-opus-4-7",
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
                    self.output_tokens =
                        usage["output_tokens"].as_u64().unwrap_or(self.output_tokens);
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

        let value: serde_json::Value =
            serde_json::from_str(&tool_json).map_err(|e| {
                ProviderError::ResponseUnreadable(format!(
                    "invalid JSON for tool {expected_tool}: {e} (preview: {})",
                    &tool_json[..tool_json.len().min(200)]
                ))
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
        })
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
        body.as_object_mut()
            .expect("request body must be an object")
            .insert("stream".to_string(), serde_json::Value::Bool(true));

        let mut req = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json");
        for (k, v) in extra_headers {
            req = req.header(*k, *v);
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
        let mut line_buf = String::new();
        let mut last_progress_emit = std::time::Instant::now();

        while let Some(chunk) = stream.next().await {
            let chunk =
                chunk.map_err(|e| ProviderError::StreamInterrupted(format!("{e}")))?;
            let text = std::str::from_utf8(&chunk)
                .map_err(|e| ProviderError::ResponseUnreadable(format!("{e}")))?;

            line_buf.push_str(text);

            while let Some(newline_pos) = line_buf.find('\n') {
                let line = line_buf[..newline_pos].trim_end_matches('\r').to_string();
                line_buf = line_buf[newline_pos + 1..].to_string();

                if let Some(data) = line.strip_prefix("data: ") {
                    if data == "[DONE]" {
                        continue;
                    }
                    match serde_json::from_str::<serde_json::Value>(data) {
                        Ok(event) => {
                            accumulator.handle_event(&event);
                            self.emit_sse_progress(
                                &accumulator,
                                &event,
                                &mut last_progress_emit,
                            );
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
            "content_block_delta" => {
                if last_emit.elapsed() >= std::time::Duration::from_secs(1) {
                    let _ = tx.send(ProgressEvent::LlmProgress {
                        output_tokens: acc.output_tokens(),
                        elapsed_ms: 0,
                        task_id: task_id.clone(),
                    });
                    *last_emit = std::time::Instant::now();
                }
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
        let model = self.resolve_model(tier, agent_override);

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
        let model = self.resolve_model(ModelTier::Advanced, agent_override);

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
        let model = self.resolve_model(ModelTier::Advanced, agent_override);

        let mut content: Vec<Value> = Vec::with_capacity(files.len() + 1);
        for (filename, mime, bytes) in files {
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
        "anthropic"
    }

    fn with_progress(
        &self,
        tx: ProgressTx,
        task_id: Option<String>,
    ) -> Option<Arc<dyn LlmProvider>> {
        Some(Arc::new(self.clone_with_progress(tx, task_id)))
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

#[derive(Debug)]
pub struct OpenAIProvider {
    client: Client,
    api_key: String,
    standard_model: String,
    #[allow(dead_code)]
    advanced_model: String,
}

impl OpenAIProvider {
    /// Build from environment variables.
    ///
    /// Required: `FYNANCE_OPENAI_API_KEY`
    /// Optional: `FYNANCE_OPENAI_TEXT_MODEL`, `FYNANCE_OPENAI_PDF_MODEL`
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("FYNANCE_OPENAI_API_KEY").map_err(|_| {
            anyhow!(
                "FYNANCE_OPENAI_API_KEY is not set. \
                 Set it in your .env file to use the OpenAI provider."
            )
        })?;
        let standard_model = std::env::var("FYNANCE_OPENAI_TEXT_MODEL")
            .unwrap_or_else(|_| "gpt-4o-mini".to_string());
        let advanced_model =
            std::env::var("FYNANCE_OPENAI_PDF_MODEL").unwrap_or_else(|_| "gpt-4o".to_string());

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
        })
    }

    fn model_for_tier(&self, tier: ModelTier) -> &str {
        match tier {
            ModelTier::Standard => &self.standard_model,
            ModelTier::Advanced => &self.advanced_model,
        }
    }
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

        let request_body = json!({
            "model": model,
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user",   "content": user_message }
            ],
            "tools": [{
                "type": "function",
                "function": {
                    "name": tool_name,
                    "description": format!("Extract structured data using the {} tool.", tool_name),
                    "parameters": tool_schema
                }
            }],
            "tool_choice": {
                "type": "function",
                "function": { "name": tool_name }
            }
        });

        tracing::debug!(
            provider = "openai",
            model = %model,
            tool_name,
            "sending text request"
        );

        let started = Instant::now();
        let response = self
            .client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
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

        let value = extract_openai_tool_input(&body, tool_name)
            .map_err(|e| ProviderError::ResponseUnreadable(format!("{e}")))?;
        let usage = extract_openai_usage(&body);
        Ok(ProviderCallResult {
            value,
            usage,
            model,
            duration_ms,
            stop_reason: None,
        })
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
        Err(ProviderError::NotSupported(
            "PDF input is not supported with the OpenAI provider in V0. \
             To import PDF documents, switch to FYNANCE_PARSE_PROVIDER=anthropic."
                .to_string(),
        ))
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
        Err(ProviderError::NotSupported(
            "Unified file input is not supported with the OpenAI provider in V0. \
             To use unified mode, switch to FYNANCE_PARSE_PROVIDER=anthropic."
                .to_string(),
        ))
    }

    fn name(&self) -> &'static str {
        "openai"
    }
}

// ── OpenAI response parsing helpers ──────────────────────────────────────────

#[derive(Deserialize)]
struct OpenAIResponse {
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

#[derive(Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessage,
}

#[derive(Deserialize)]
struct OpenAIMessage {
    tool_calls: Option<Vec<OpenAIToolCall>>,
}

#[derive(Deserialize)]
struct OpenAIToolCall {
    function: OpenAIFunctionCall,
}

#[derive(Deserialize)]
struct OpenAIFunctionCall {
    name: String,
    /// JSON string — must be parsed, not used as-is.
    arguments: String,
}

fn extract_openai_tool_input(body: &str, tool_name: &str) -> Result<Value> {
    let api_resp: OpenAIResponse = serde_json::from_str(body).map_err(|e| {
        anyhow!(
            "parsing OpenAI response (preview: {}): {e}",
            &body[..body.len().min(200)]
        )
    })?;

    let tool_call = api_resp
        .choices
        .into_iter()
        .next()
        .and_then(|c| c.message.tool_calls)
        .and_then(|calls| calls.into_iter().find(|tc| tc.function.name == tool_name))
        .ok_or_else(|| anyhow!("no {tool_name} tool call in OpenAI response"))?;

    serde_json::from_str(&tool_call.function.arguments).map_err(|e| {
        anyhow!(
            "parsing OpenAI function arguments for {tool_name}: {e} \
             (arguments: {}...)",
            &tool_call.function.arguments[..tool_call.function.arguments.len().min(200)]
        )
    })
}

fn extract_openai_usage(body: &str) -> TokenUsage {
    serde_json::from_str::<OpenAIResponse>(body)
        .ok()
        .and_then(|r| r.usage)
        .map(|u| TokenUsage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
        })
        .unwrap_or_default()
}

// ── GeminiProvider ────────────────────────────────────────────────────────────

/// Placeholder for Gemini support. Returns an error for all calls.
/// Implement in V1 using the Gemini GenerateContent API.
#[derive(Debug)]
pub struct GeminiProvider;

#[async_trait]
impl LlmProvider for GeminiProvider {
    async fn chat_with_tools(
        &self,
        _system_prompt: &str,
        _user_message: &str,
        _tool_name: &str,
        _tool_schema: Value,
        _tier: ModelTier,
        _agent_override: Option<Agent>,
    ) -> Result<ProviderCallResult, ProviderError> {
        Err(ProviderError::NotSupported(
            "Gemini provider is not yet implemented. \
             Use FYNANCE_PARSE_PROVIDER=anthropic or FYNANCE_PARSE_PROVIDER=openai."
                .to_string(),
        ))
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
        Err(ProviderError::NotSupported(
            "Gemini provider is not yet implemented. \
             Use FYNANCE_PARSE_PROVIDER=anthropic for PDF input."
                .to_string(),
        ))
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
        Err(ProviderError::NotSupported(
            "Gemini provider is not yet implemented. \
             Use FYNANCE_PARSE_PROVIDER=anthropic for unified mode."
                .to_string(),
        ))
    }

    fn name(&self) -> &'static str {
        "gemini"
    }
}

// ── Factory ───────────────────────────────────────────────────────────────────

/// Create the configured LLM provider from environment variables.
///
/// Reads `FYNANCE_PARSE_PROVIDER` (default: "anthropic") and builds the
/// corresponding provider. Returns `Err` if the required API key is not set.
///
/// # Valid values for FYNANCE_PARSE_PROVIDER
/// - `"anthropic"` (default): requires `FYNANCE_ANTHROPIC_API_KEY`
/// - `"openai"`: requires `FYNANCE_OPENAI_API_KEY`
/// - `"gemini"`: returns a stub that errors on every call
pub fn create_provider() -> Result<Arc<dyn LlmProvider>> {
    let provider_name =
        std::env::var("FYNANCE_PARSE_PROVIDER").unwrap_or_else(|_| "anthropic".to_string());

    let provider: Arc<dyn LlmProvider> = match provider_name.to_lowercase().as_str() {
        "anthropic" => Arc::new(AnthropicProvider::from_env()?),
        "openai" => Arc::new(OpenAIProvider::from_env()?),
        "gemini" => Arc::new(GeminiProvider),
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

// ── MockProvider for tests ────────────────────────────────────────────────────

#[cfg(test)]
pub mod testing {
    use super::*;

    /// Returns a pre-canned JSON Value for every call. Used in unit tests to
    /// exercise parser logic without making real API calls.
    #[derive(Debug)]
    pub struct MockProvider {
        pub tool_input: Value,
    }

    impl MockProvider {
        pub fn new(tool_input: Value) -> Arc<Self> {
            Arc::new(Self { tool_input })
        }

        fn pretend_result(&self) -> ProviderCallResult {
            ProviderCallResult {
                value: self.tool_input.clone(),
                usage: TokenUsage::default(),
                model: "mock".to_string(),
                duration_ms: 0,
                stop_reason: None,
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
    use std::sync::Mutex;

    /// Serializes tests that mutate process-global env vars
    /// (`FYNANCE_PARSE_PROVIDER`, `FYNANCE_*_API_KEY`). Cargo runs tests in
    /// parallel within one process, so without this one test's `set_var`
    /// clobbers another's between its `set_var` and `create_provider()` read.
    /// `.unwrap_or_else(into_inner)` keeps the lock usable if a guarded test
    /// panics, so the real failure surfaces instead of a poison cascade.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_create_provider_unknown_name() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe { std::env::set_var("FYNANCE_PARSE_PROVIDER", "invalid_provider") };
        let result = create_provider();
        unsafe { std::env::remove_var("FYNANCE_PARSE_PROVIDER") };
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("Unknown FYNANCE_PARSE_PROVIDER"), "got: {msg}");
    }

    #[test]
    fn test_create_provider_gemini_is_stub() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe { std::env::set_var("FYNANCE_PARSE_PROVIDER", "gemini") };
        let result = create_provider();
        unsafe { std::env::remove_var("FYNANCE_PARSE_PROVIDER") };
        assert!(result.is_ok());
        assert_eq!(result.unwrap().name(), "gemini");
    }

    #[test]
    fn test_create_provider_openai_requires_key() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::set_var("FYNANCE_PARSE_PROVIDER", "openai");
            std::env::remove_var("FYNANCE_OPENAI_API_KEY");
        }
        let result = create_provider();
        unsafe { std::env::remove_var("FYNANCE_PARSE_PROVIDER") };
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("FYNANCE_OPENAI_API_KEY"), "got: {msg}");
    }

    #[test]
    fn test_create_provider_anthropic_requires_key() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::set_var("FYNANCE_PARSE_PROVIDER", "anthropic");
            std::env::remove_var("FYNANCE_ANTHROPIC_API_KEY");
        }
        let result = create_provider();
        unsafe { std::env::remove_var("FYNANCE_PARSE_PROVIDER") };
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("FYNANCE_ANTHROPIC_API_KEY"), "got: {msg}");
    }

    #[test]
    fn test_gemini_chat_returns_error() {
        let provider = GeminiProvider;
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(provider.chat_with_tools(
            "system",
            "user",
            "tool",
            serde_json::json!({}),
            ModelTier::Standard,
            None,
        ));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not supported"));
    }

    #[test]
    fn test_openai_pdf_returns_error() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe { std::env::set_var("FYNANCE_OPENAI_API_KEY", "test-key") };
        let provider = OpenAIProvider::from_env().unwrap();
        unsafe { std::env::remove_var("FYNANCE_OPENAI_API_KEY") };

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(provider.chat_with_pdf_and_tools(
            "system",
            b"fake pdf bytes",
            "text",
            "tool",
            serde_json::json!({}),
            None,
        ));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not supported"));
    }

    #[test]
    fn test_openai_response_parsing() {
        let body = r#"{
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "function": {
                            "name": "parse_bank_statement",
                            "arguments": "{\"detected_bank\":\"monzo\",\"detection_confidence\":0.95,\"rows\":[]}"
                        }
                    }]
                }
            }]
        }"#;
        let result = extract_openai_tool_input(body, "parse_bank_statement").unwrap();
        assert_eq!(result["detected_bank"], "monzo");
        assert_eq!(result["detection_confidence"], 0.95);
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
            anthropic_model_for_agent(Agent::Haiku),
            "claude-haiku-4-5-20251001"
        );
        assert_eq!(
            anthropic_model_for_agent(Agent::Sonnet),
            "claude-sonnet-4-6"
        );
        assert_eq!(anthropic_model_for_agent(Agent::Opus), "claude-opus-4-7");
    }

    #[test]
    fn test_openai_usage_parsing() {
        let body = r#"{
            "choices": [{"message": {"tool_calls": null}}],
            "usage": {"prompt_tokens": 12, "completion_tokens": 34}
        }"#;
        let usage = extract_openai_usage(body);
        assert_eq!(usage.input_tokens, 12);
        assert_eq!(usage.output_tokens, 34);
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
}
