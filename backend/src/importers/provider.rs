//! Pluggable LLM provider abstraction for the parse pipeline.

use std::sync::Arc;
use std::time::Instant;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
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
    ) -> Result<ProviderCallResult>;

    /// Providers without PDF support (e.g. OpenAI v0) must return `Err`.
    async fn chat_with_pdf_and_tools(
        &self,
        system_prompt: &str,
        pdf_bytes: &[u8],
        text_supplement: &str,
        tool_name: &str,
        tool_schema: Value,
        agent_override: Option<Agent>,
    ) -> Result<ProviderCallResult>;

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
    ) -> Result<ProviderCallResult>;

    /// Short identifier used in log messages and metrics.
    fn name(&self) -> &'static str;
}

// ── AnthropicProvider ─────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct AnthropicProvider {
    client: Client,
    api_key: String,
    standard_model: String,
    advanced_model: String,
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

        Ok(Self {
            client: Client::new(),
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

    fn resolve_model(&self, tier: ModelTier, agent_override: Option<Agent>) -> String {
        match agent_override {
            Some(agent) => anthropic_model_for_agent(agent).to_string(),
            None => self.model_for_tier(tier).to_string(),
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
    ) -> Result<ProviderCallResult> {
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

        let started = Instant::now();
        let body = self.post_messages(&request_body, &[]).await?;
        let duration_ms = started.elapsed().as_millis() as u64;
        let value = extract_anthropic_tool_input(&body, tool_name)?;
        let usage = extract_anthropic_usage(&body);
        let stop_reason = extract_anthropic_stop_reason(&body);
        warn_if_truncated(&model, tool_name, &stop_reason, &usage);
        Ok(ProviderCallResult {
            value,
            usage,
            model,
            duration_ms,
            stop_reason,
        })
    }

    async fn chat_with_pdf_and_tools(
        &self,
        system_prompt: &str,
        pdf_bytes: &[u8],
        text_supplement: &str,
        tool_name: &str,
        tool_schema: Value,
        agent_override: Option<Agent>,
    ) -> Result<ProviderCallResult> {
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

        let started = Instant::now();
        let body = self
            .post_messages(&request_body, &[("anthropic-beta", "pdfs-2024-09-25")])
            .await?;
        let duration_ms = started.elapsed().as_millis() as u64;
        let value = extract_anthropic_tool_input(&body, tool_name)?;
        let usage = extract_anthropic_usage(&body);
        let stop_reason = extract_anthropic_stop_reason(&body);
        warn_if_truncated(&model, tool_name, &stop_reason, &usage);
        Ok(ProviderCallResult {
            value,
            usage,
            model,
            duration_ms,
            stop_reason,
        })
    }

    async fn chat_with_files_and_tools(
        &self,
        system_prompt: &str,
        files: &[(String, String, Vec<u8>)],
        text_supplement: &str,
        tool_name: &str,
        tool_schema: Value,
        agent_override: Option<Agent>,
    ) -> Result<ProviderCallResult> {
        let model = self.resolve_model(ModelTier::Advanced, agent_override);

        let mut content: Vec<Value> = Vec::with_capacity(files.len() + 1);
        for (filename, mime, bytes) in files {
            content.push(anthropic_content_block_for_file(filename, mime, bytes)?);
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

        let started = Instant::now();
        let body = self
            .post_messages(&request_body, &[("anthropic-beta", "pdfs-2024-09-25")])
            .await?;
        let duration_ms = started.elapsed().as_millis() as u64;
        let value = extract_anthropic_tool_input(&body, tool_name)?;
        let usage = extract_anthropic_usage(&body);
        let stop_reason = extract_anthropic_stop_reason(&body);
        warn_if_truncated(&model, tool_name, &stop_reason, &usage);
        Ok(ProviderCallResult {
            value,
            usage,
            model,
            duration_ms,
            stop_reason,
        })
    }

    fn name(&self) -> &'static str {
        "anthropic"
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

impl AnthropicProvider {
    async fn post_messages(&self, body: &Value, extra_headers: &[(&str, &str)]) -> Result<String> {
        let mut req = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json");
        for (k, v) in extra_headers {
            req = req.header(*k, *v);
        }
        let response = req
            .json(body)
            .send()
            .await
            .map_err(|e| anyhow!("sending request to Anthropic: {e}"))?;

        let status = response.status();
        let body_str = response
            .text()
            .await
            .map_err(|e| anyhow!("reading Anthropic response: {e}"))?;

        if !status.is_success() {
            return Err(anyhow!("Anthropic returned {status}: {body_str}"));
        }
        Ok(body_str)
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

// ── Anthropic response parsing helpers ────────────────────────────────────────

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
    #[serde(default)]
    usage: Option<AnthropicUsage>,
    #[serde(default)]
    stop_reason: Option<String>,
}

#[derive(Deserialize, Default)]
struct AnthropicUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum AnthropicContentBlock {
    ToolUse {
        #[serde(rename = "type")]
        block_type: String,
        name: String,
        input: Value,
    },
    #[allow(dead_code)]
    Other(Value),
}

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

        Ok(Self {
            client: Client::new(),
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
    ) -> Result<ProviderCallResult> {
        if agent_override.is_some() {
            return Err(anyhow!(
                "agent override is only supported with the Anthropic provider. \
                 Set FYNANCE_PARSE_PROVIDER=anthropic to use agent overrides."
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
            .map_err(|e| anyhow!("sending request to OpenAI: {e}"))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| anyhow!("reading OpenAI response: {e}"))?;
        let duration_ms = started.elapsed().as_millis() as u64;

        if !status.is_success() {
            return Err(anyhow!("OpenAI returned {status}: {body}"));
        }

        let value = extract_openai_tool_input(&body, tool_name)?;
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
    ) -> Result<ProviderCallResult> {
        Err(anyhow!(
            "PDF input is not supported with the OpenAI provider in V0. \
             To import PDF documents, switch to FYNANCE_PARSE_PROVIDER=anthropic."
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
    ) -> Result<ProviderCallResult> {
        Err(anyhow!(
            "Unified file input is not supported with the OpenAI provider in V0. \
             To use unified mode, switch to FYNANCE_PARSE_PROVIDER=anthropic."
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
    ) -> Result<ProviderCallResult> {
        Err(anyhow!(
            "Gemini provider is not yet implemented. \
             Use FYNANCE_PARSE_PROVIDER=anthropic or FYNANCE_PARSE_PROVIDER=openai."
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
    ) -> Result<ProviderCallResult> {
        Err(anyhow!(
            "Gemini provider is not yet implemented. \
             Use FYNANCE_PARSE_PROVIDER=anthropic for PDF input."
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
    ) -> Result<ProviderCallResult> {
        Err(anyhow!(
            "Gemini provider is not yet implemented. \
             Use FYNANCE_PARSE_PROVIDER=anthropic for unified mode."
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
        ) -> Result<ProviderCallResult> {
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
        ) -> Result<ProviderCallResult> {
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
        ) -> Result<ProviderCallResult> {
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
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("not yet implemented")
        );
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
        assert_eq!(anthropic_model_for_agent(Agent::Haiku), "claude-haiku-4-5-20251001");
        assert_eq!(anthropic_model_for_agent(Agent::Sonnet), "claude-sonnet-4-6");
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
}
