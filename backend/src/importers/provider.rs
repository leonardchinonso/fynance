//! Pluggable LLM provider abstraction for the parse pipeline.

use std::sync::Arc;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{Value, json};

// ── Tier hint ────────────────────────────────────────────────────────────────

/// Which model tier to use for a given call.
///
/// Each provider maps these to its own model names via env vars.
#[derive(Debug, Clone, Copy)]
pub enum ModelTier {
    /// Fast, cheap model. Used for CSV/Excel text extraction.
    /// Anthropic: FYNANCE_IMPORT_LLM_MODEL (default claude-haiku-4-5-20251001)
    /// OpenAI: FYNANCE_OPENAI_TEXT_MODEL (default gpt-4o-mini)
    Standard,
    /// More capable model. Used for PDF visual understanding.
    /// Anthropic: FYNANCE_PARSE_PDF_MODEL (default claude-sonnet-4-6)
    /// OpenAI: FYNANCE_OPENAI_PDF_MODEL (default gpt-4o)
    Advanced,
}

// ── Trait ─────────────────────────────────────────────────────────────────────

/// Pluggable LLM backend for the parse pipeline.
///
/// Implementations hide all provider-specific HTTP formatting, auth headers,
/// and response parsing. Callers pass the system prompt, user message, tool
/// name, and tool JSON Schema; they receive back the tool's input as a
/// `serde_json::Value`.
#[async_trait]
pub trait LlmProvider: Send + Sync + std::fmt::Debug {
    /// Call the LLM with a text user message and force a specific tool call.
    /// Returns the tool's input JSON on success.
    async fn chat_with_tools(
        &self,
        system_prompt: &str,
        user_message: &str,
        tool_name: &str,
        tool_schema: Value,
        tier: ModelTier,
    ) -> Result<Value>;

    /// Call the LLM with a PDF document and force a specific tool call.
    /// Returns the tool's input JSON on success.
    ///
    /// Providers that do not support PDF input (e.g., OpenAI in V0) must return
    /// `Err` with a clear message explaining the limitation and how to work
    /// around it.
    async fn chat_with_pdf_and_tools(
        &self,
        system_prompt: &str,
        pdf_bytes: &[u8],
        text_supplement: &str,
        tool_name: &str,
        tool_schema: Value,
    ) -> Result<Value>;

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
    ) -> Result<Value> {
        let model = self.model_for_tier(tier);

        let request_body = json!({
            "model": model,
            "max_tokens": 8192,
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
            model,
            tool_name,
            "sending text request"
        );

        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| anyhow!("sending request to Anthropic: {e}"))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| anyhow!("reading Anthropic response: {e}"))?;

        if !status.is_success() {
            return Err(anyhow!("Anthropic returned {status}: {body}"));
        }

        extract_anthropic_tool_input(&body, tool_name)
    }

    async fn chat_with_pdf_and_tools(
        &self,
        system_prompt: &str,
        pdf_bytes: &[u8],
        text_supplement: &str,
        tool_name: &str,
        tool_schema: Value,
    ) -> Result<Value> {
        use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
        let b64 = BASE64.encode(pdf_bytes);

        let request_body = json!({
            "model": self.advanced_model,
            "max_tokens": 8192,
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
            model = self.advanced_model,
            tool_name,
            pdf_size = pdf_bytes.len(),
            "sending PDF request"
        );

        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", "pdfs-2024-09-25")
            .header("content-type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| anyhow!("sending PDF request to Anthropic: {e}"))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| anyhow!("reading Anthropic PDF response: {e}"))?;

        if !status.is_success() {
            return Err(anyhow!("Anthropic returned {status} for PDF: {body}"));
        }

        extract_anthropic_tool_input(&body, tool_name)
    }

    fn name(&self) -> &'static str {
        "anthropic"
    }
}

// ── Anthropic response parsing helpers ────────────────────────────────────────

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
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
    ) -> Result<Value> {
        let model = self.model_for_tier(tier);

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
            model,
            tool_name,
            "sending text request"
        );

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

        if !status.is_success() {
            return Err(anyhow!("OpenAI returned {status}: {body}"));
        }

        extract_openai_tool_input(&body, tool_name)
    }

    async fn chat_with_pdf_and_tools(
        &self,
        _system_prompt: &str,
        _pdf_bytes: &[u8],
        _text_supplement: &str,
        _tool_name: &str,
        _tool_schema: Value,
    ) -> Result<Value> {
        Err(anyhow!(
            "PDF input is not supported with the OpenAI provider in V0. \
             To import PDF documents, switch to FYNANCE_PARSE_PROVIDER=anthropic."
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
    ) -> Result<Value> {
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
    ) -> Result<Value> {
        Err(anyhow!(
            "Gemini provider is not yet implemented. \
             Use FYNANCE_PARSE_PROVIDER=anthropic for PDF input."
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
        ) -> Result<Value> {
            Ok(self.tool_input.clone())
        }

        async fn chat_with_pdf_and_tools(
            &self,
            _system_prompt: &str,
            _pdf_bytes: &[u8],
            _text_supplement: &str,
            _tool_name: &str,
            _tool_schema: Value,
        ) -> Result<Value> {
            Ok(self.tool_input.clone())
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

    #[test]
    fn test_create_provider_unknown_name() {
        unsafe { std::env::set_var("FYNANCE_PARSE_PROVIDER", "invalid_provider") };
        let result = create_provider();
        unsafe { std::env::remove_var("FYNANCE_PARSE_PROVIDER") };
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("Unknown FYNANCE_PARSE_PROVIDER"), "got: {msg}");
    }

    #[test]
    fn test_create_provider_gemini_is_stub() {
        unsafe { std::env::set_var("FYNANCE_PARSE_PROVIDER", "gemini") };
        let result = create_provider();
        unsafe { std::env::remove_var("FYNANCE_PARSE_PROVIDER") };
        assert!(result.is_ok());
        assert_eq!(result.unwrap().name(), "gemini");
    }

    #[test]
    fn test_create_provider_openai_requires_key() {
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
            }]
        }"#;
        let result = extract_anthropic_tool_input(body, "parse_bank_statement").unwrap();
        assert_eq!(result["detected_bank"], "revolut");
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
}
