# 24: Streaming, Progress Reporting, and Error Classification

**Design doc:** `docs/design/08_streaming_and_error_classification.md`
**Status:** Ready for implementation
**Last updated:** 2026-05-29

## Decisions

These were resolved before writing this plan:

1. **Progress handle on LlmProvider:** Wrapper struct (`ProgressAwareProvider`) that
   holds `Arc<dyn LlmProvider>` + `Option<ProgressTx>`. No trait signature changes.
2. **ProviderError:** Replaces `anyhow::Result` as the return type on all
   `LlmProvider` trait methods. Provides `impl From<ProviderError> for anyhow::Error`
   so callers that just `?` still compile.
3. **Split-mode progress:** Per-task events. Each parallel LLM call gets its own
   `llm_start`/`llm_progress` stream tagged with `task_id` (e.g.
   `"statement.pdf:transactions"`).
4. **Channel cleanup:** TTL via `tokio::spawn`. A 10-minute background task removes
   the channel if still present.

---

## File inventory

Every file that will be created or modified, with the change each receives:

| File | Change |
|------|--------|
| `backend/Cargo.toml` | Add `"stream"` feature to `reqwest`, add `futures-util`, add `tokio-stream` |
| `backend/src/importers/provider.rs` | Rewrite `post_messages` to stream SSE; add `ProviderError` enum; change `LlmProvider` trait return types; add `ProgressAwareProvider` wrapper; update `AnthropicProvider`, `OpenAIProvider`, `GeminiProvider`, `MockProvider` impls |
| `backend/src/importers/mod.rs` | Update `get_importer` call site (return type change) |
| `backend/src/importers/llm_parser.rs` | Update `LlmStatementParser::parse` call site (return type change) |
| `backend/src/importers/holdings_parser.rs` | Update `LlmHoldingsParser::extract_holdings` call site |
| `backend/src/importers/pdf_parser.rs` | Update 4 parser structs' call sites |
| `backend/src/importers/investments_parser.rs` | Update `LlmInvestmentsParser::extract_investments` call site |
| `backend/src/importers/periodic_holdings_parser.rs` | Update `LlmPeriodicHoldingsParser` call site |
| `backend/src/importers/unified_parser.rs` | Update `extract_all` call site |
| `backend/src/importers/document_parser.rs` | Update `extract_all_parallel` to accept + thread progress through |
| `backend/src/server/state.rs` | Add `progress_channels` field to `AppState` |
| `backend/src/server/mod.rs` | Construct `progress_channels` in `build_router`; register SSE route |
| `backend/src/server/routes/parse.rs` | Add `parse_id` field; create/emit progress events; add SSE handler; map `ProviderError` to `AppError` |
| `backend/src/server/error.rs` | Add `BadGateway`, `GatewayTimeout`, `TooManyRequests` variants |

---

## Change 1: Streaming API

### Step 1.1: Update `Cargo.toml`

In `backend/Cargo.toml`, change the `reqwest` line and add `futures-util`:

**Before:**
```toml
reqwest = { version = "0.12", features = ["json"] }
```

**After:**
```toml
reqwest = { version = "0.12", features = ["json", "stream"] }
futures-util = "0.3"
```

`futures-util` provides `StreamExt::next()` for consuming the byte stream.

### Step 1.2: Add SSE accumulator to `provider.rs`

Add this struct and function after the `warn_if_truncated` function (after line 342)
and before the `impl AnthropicProvider` block (line 344):

```rust
use futures_util::StreamExt;

/// Accumulates Anthropic streaming SSE events into a complete
/// `ProviderCallResult`. Replaces the old `post_messages` which waited
/// for the entire response body.
struct SseAccumulator {
    model: String,
    input_tokens: u64,
    output_tokens: u64,
    stop_reason: Option<String>,
    /// Per-tool_use-block accumulator: index -> partial JSON string.
    tool_inputs: std::collections::HashMap<u32, String>,
    /// Maps content block index to tool name.
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

    /// Process one SSE `data:` line (already JSON-parsed).
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
                    self.output_tokens = usage["output_tokens"].as_u64().unwrap_or(self.output_tokens);
                }
            }
            _ => {}
        }
    }

    /// Extract the accumulated tool input for the named tool. Returns the
    /// parsed JSON Value and usage/model info.
    fn into_result(self, expected_tool: &str) -> Result<ProviderCallResult> {
        let tool_json = self
            .tool_inputs
            .into_iter()
            .find(|(idx, _)| {
                self.tool_names
                    .get(idx)
                    .map(|n| n == expected_tool)
                    .unwrap_or(false)
            })
            .map(|(_, json_str)| json_str)
            .ok_or_else(|| anyhow!("no {expected_tool} tool_use block in streaming response"))?;

        let value: serde_json::Value = serde_json::from_str(&tool_json).map_err(|e| {
            anyhow!(
                "streaming SSE: invalid JSON for tool {expected_tool}: {e} (preview: {})",
                &tool_json[..tool_json.len().min(200)]
            )
        })?;

        Ok(ProviderCallResult {
            value,
            usage: TokenUsage {
                input_tokens: self.input_tokens,
                output_tokens: self.output_tokens,
            },
            model: self.model,
            duration_ms: 0, // caller sets this from Instant
            stop_reason: self.stop_reason,
        })
    }
}
```

**Note:** The `into_result` method moves `self`, but it needs to read `self.tool_names`
after `self.tool_inputs` is consumed by `into_iter()`. Fix: collect tool_names into a
local before the iterator:

```rust
fn into_result(self, expected_tool: &str) -> Result<ProviderCallResult> {
    let tool_names = self.tool_names;
    let tool_json = self
        .tool_inputs
        .into_iter()
        .find(|(idx, _)| {
            tool_names.get(idx).map(|n| n == expected_tool).unwrap_or(false)
        })
        .map(|(_, json_str)| json_str)
        .ok_or_else(|| anyhow!("no {expected_tool} tool_use block in streaming response"))?;

    // ... rest unchanged
}
```

### Step 1.3: Rewrite `post_messages` to use streaming

Replace the current `impl AnthropicProvider` block (lines 344-372) with:

```rust
impl AnthropicProvider {
    /// Send a request to the Anthropic Messages API using streaming.
    ///
    /// Injects `"stream": true` into the body, consumes SSE events, and
    /// reconstructs the complete tool_use result. Returns the accumulated
    /// result for the named tool.
    async fn post_messages_streaming(
        &self,
        body: &Value,
        extra_headers: &[(&str, &str)],
        tool_name: &str,
    ) -> Result<ProviderCallResult> {
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
            .map_err(|e| anyhow!("sending request to Anthropic: {e}"))?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable>".to_string());
            return Err(anyhow!("Anthropic returned {status}: {error_body}"));
        }

        let mut stream = response.bytes_stream();
        let mut accumulator = SseAccumulator::new();
        let mut line_buf = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| anyhow!("reading Anthropic stream chunk: {e}"))?;
            let text = std::str::from_utf8(&chunk)
                .map_err(|e| anyhow!("Anthropic stream chunk is not UTF-8: {e}"))?;

            line_buf.push_str(text);

            // SSE protocol: events are separated by blank lines.
            // Process complete lines from the buffer.
            while let Some(newline_pos) = line_buf.find('\n') {
                let line = line_buf[..newline_pos].trim_end_matches('\r').to_string();
                line_buf = line_buf[newline_pos + 1..].to_string();

                if let Some(data) = line.strip_prefix("data: ") {
                    if data == "[DONE]" {
                        continue;
                    }
                    match serde_json::from_str::<serde_json::Value>(data) {
                        Ok(event) => accumulator.handle_event(&event),
                        Err(e) => {
                            tracing::warn!(
                                error = %e,
                                data_preview = &data[..data.len().min(100)],
                                "SSE: failed to parse event data, skipping"
                            );
                        }
                    }
                }
                // Lines starting with "event:", "id:", "retry:", or empty
                // lines are part of the SSE protocol but carry no data we
                // need beyond what's in the "data:" lines.
            }
        }

        let mut result = accumulator.into_result(tool_name)?;
        result.duration_ms = started.elapsed().as_millis() as u64;
        warn_if_truncated(&result.model, tool_name, &result.stop_reason, &result.usage);
        Ok(result)
    }
}
```

### Step 1.4: Update the three `AnthropicProvider` trait method impls

Each method currently calls `self.post_messages(...)` then manually extracts
tool input, usage, and stop reason. Replace with a single call to
`self.post_messages_streaming(...)` which returns the fully-assembled
`ProviderCallResult`.

**`chat_with_tools` (lines 148-193):** Replace the body starting at `let started = Instant::now();` (line 179):

**Before:**
```rust
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
```

**After:**
```rust
        self.post_messages_streaming(&request_body, &[], tool_name).await
```

**`chat_with_pdf_and_tools` (lines 195-260):** Replace from `let started = Instant::now();` (line 244):

**Before:**
```rust
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
```

**After:**
```rust
        self.post_messages_streaming(
            &request_body,
            &[("anthropic-beta", "pdfs-2024-09-25")],
            tool_name,
        )
        .await
```

**`chat_with_files_and_tools` (lines 262-318):** Same pattern. Replace from
`let started = Instant::now();` (line 302):

**Before:**
```rust
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
```

**After:**
```rust
        self.post_messages_streaming(
            &request_body,
            &[("anthropic-beta", "pdfs-2024-09-25")],
            tool_name,
        )
        .await
```

### Step 1.5: Remove dead code

The old `post_messages` method (lines 345-372) is now unused. Delete it.

The following helper functions are no longer called by any `AnthropicProvider`
method (they were only used by `post_messages` callers):

- `extract_anthropic_tool_input` (line 448)
- `extract_anthropic_usage` (line 470)
- `extract_anthropic_stop_reason` (line 481)
- `AnthropicResponse`, `AnthropicUsage`, `AnthropicContentBlock` structs (lines 418-446)

**Do not delete them yet.** They are used by tests (`test_anthropic_response_parsing`,
`test_anthropic_response_missing_tool_use`). Keep them and their tests. They test
non-streaming response parsing which could still be useful as a fallback reference.
Mark them with `#[allow(dead_code)]` if clippy warns.

### Step 1.6: Add the `Client` timeout

In `AnthropicProvider::from_env()` (line 114), change:

**Before:**
```rust
        Ok(Self {
            client: Client::new(),
            api_key,
            standard_model,
            advanced_model,
        })
```

**After:**
```rust
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
```

Do the same for `OpenAIProvider::from_env()` (around line 518):

**Before:**
```rust
        Ok(Self {
            client: Client::new(),
            api_key,
            standard_model,
            advanced_model,
        })
```

**After:**
```rust
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
```

### Step 1.7: Add streaming accumulator test

Add to the `#[cfg(test)] mod tests` block in `provider.rs`:

```rust
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
```

### Step 1.8: Verify

Run `cargo clippy --all-targets -- -D warnings && cargo test` and fix any issues.

---

## Change 2: Progress Reporting

### Step 2.1: Add `tokio-stream` to `Cargo.toml`

In `backend/Cargo.toml`, add after `futures-util`:

```toml
tokio-stream = "0.1"
```

### Step 2.2: Create progress types in `provider.rs`

Add these types near the top of `provider.rs`, after the existing `TokenUsage` and
`ProviderCallResult` types (around line 40):

```rust
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

/// Send a progress event, ignoring errors (no subscribers is fine).
pub fn emit_progress(tx: &Option<ProgressTx>, event: ProgressEvent) {
    if let Some(tx) = tx {
        let _ = tx.send(event);
    }
}
```

### Step 2.3: Create `ProgressAwareProvider` wrapper

Add this after the `MockProvider` section in `provider.rs` (before `// ── Tests`):

```rust
// ── Progress-aware provider wrapper ─────────────────────────────────────────

/// Wraps an `LlmProvider` and emits progress events during streaming.
/// Used by the parse route to forward Anthropic SSE events to the frontend.
/// Callers that don't need progress use the inner provider directly.
#[derive(Debug)]
pub struct ProgressAwareProvider {
    inner: Arc<dyn LlmProvider>,
    progress: ProgressTx,
    task_id: Option<String>,
}

impl ProgressAwareProvider {
    pub fn new(
        inner: Arc<dyn LlmProvider>,
        progress: ProgressTx,
        task_id: Option<String>,
    ) -> Arc<Self> {
        Arc::new(Self {
            inner,
            progress,
            task_id,
        })
    }
}

#[async_trait]
impl LlmProvider for ProgressAwareProvider {
    async fn chat_with_tools(
        &self,
        system_prompt: &str,
        user_message: &str,
        tool_name: &str,
        tool_schema: Value,
        tier: ModelTier,
        agent_override: Option<Agent>,
    ) -> Result<ProviderCallResult> {
        self.inner
            .chat_with_tools(system_prompt, user_message, tool_name, tool_schema, tier, agent_override)
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
    ) -> Result<ProviderCallResult> {
        self.inner
            .chat_with_pdf_and_tools(system_prompt, pdf_bytes, text_supplement, tool_name, tool_schema, agent_override)
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
    ) -> Result<ProviderCallResult> {
        self.inner
            .chat_with_files_and_tools(system_prompt, files, text_supplement, tool_name, tool_schema, agent_override)
            .await
    }

    fn name(&self) -> &'static str {
        self.inner.name()
    }
}
```

**Important:** For the `ProgressAwareProvider` to actually emit `LlmStart` and
`LlmProgress` events, the streaming accumulator needs access to the progress
channel. This is handled by making `post_messages_streaming` accept an optional
callback. See Step 2.5.

### Step 2.4: Add `on_sse_event` callback to `post_messages_streaming`

Modify `post_messages_streaming` to accept an optional callback that fires on every
SSE event. This is how the `ProgressAwareProvider` injects progress reporting into
the streaming loop without changing the `LlmProvider` trait.

Change the signature of `post_messages_streaming`:

**Before:**
```rust
    async fn post_messages_streaming(
        &self,
        body: &Value,
        extra_headers: &[(&str, &str)],
        tool_name: &str,
    ) -> Result<ProviderCallResult> {
```

**After:**
```rust
    async fn post_messages_streaming(
        &self,
        body: &Value,
        extra_headers: &[(&str, &str)],
        tool_name: &str,
        on_sse_event: Option<&dyn Fn(&SseAccumulator, &serde_json::Value)>,
    ) -> Result<ProviderCallResult> {
```

Inside the streaming loop, after `accumulator.handle_event(&event)`, add:

```rust
                    match serde_json::from_str::<serde_json::Value>(data) {
                        Ok(event) => {
                            accumulator.handle_event(&event);
                            if let Some(cb) = &on_sse_event {
                                cb(&accumulator, &event);
                            }
                        }
                        Err(e) => {
                            // ... existing warn
                        }
                    }
```

Update all three `AnthropicProvider` trait method calls to pass `None`:

```rust
        self.post_messages_streaming(&request_body, &[], tool_name, None).await
```

```rust
        self.post_messages_streaming(
            &request_body,
            &[("anthropic-beta", "pdfs-2024-09-25")],
            tool_name,
            None,
        )
        .await
```

### Step 2.5: Make `SseAccumulator` fields readable

Add public getter methods to `SseAccumulator` so the callback can read state:

```rust
impl SseAccumulator {
    pub fn model(&self) -> &str { &self.model }
    pub fn input_tokens(&self) -> u64 { self.input_tokens }
    pub fn output_tokens(&self) -> u64 { self.output_tokens }
}
```

### Step 2.6: Implement progress-aware Anthropic calls

The `ProgressAwareProvider` needs to call `post_messages_streaming` with a callback
instead of delegating to `self.inner`. But `post_messages_streaming` is a private
method on `AnthropicProvider`, not on the trait. So the `ProgressAwareProvider` cannot
call it directly.

**Solution:** Add a second trait method `chat_with_files_and_tools_streaming` that
accepts the callback. No: this defeats the wrapper purpose.

**Better solution:** Make `AnthropicProvider` itself check for a progress channel.
Add an `Option<(ProgressTx, Option<String>)>` field to `AnthropicProvider`:

This introduces shared mutable state which we rejected. Instead, use the following
approach:

**Final approach:** `AnthropicProvider` stores a `std::sync::Mutex<Option<(ProgressTx, Option<String>)>>`.
The `ProgressAwareProvider` wrapper sets it before each call and clears it after.
The streaming loop reads it.

Actually this is getting complicated. Let me simplify. The cleanest path:

**Revised approach:** Add a `progress_tx: Option<ProgressTx>` and
`progress_task_id: Option<String>` to `AnthropicProvider` via an `RwLock`. But since
`AnthropicProvider` is behind `Arc<dyn LlmProvider>` and shared across calls, this
would leak state between parallel calls.

**Simplest correct approach:** Instead of a wrapper struct, create a standalone
async function `streaming_call_with_progress` that:
1. Takes a `reqwest::Client`, API key, request body, headers, tool name, and an
   `Option<(ProgressTx, Option<String>)>`
2. Does the streaming SSE loop with progress emission
3. Returns `Result<ProviderCallResult>`

Both `AnthropicProvider` (no progress) and the parse route (with progress) can call
this. The parse route builds the request body the same way `AnthropicProvider` does,
then calls this function directly instead of going through the trait.

**This is too much duplication.** Let me go back to the simplest thing that works:

**Final design: `AnthropicProvider` gains a `clone_with_progress` method.**

```rust
impl AnthropicProvider {
    /// Create a new provider instance that shares the HTTP client but emits
    /// progress events during streaming calls.
    pub fn clone_with_progress(&self, tx: ProgressTx, task_id: Option<String>) -> Self {
        Self {
            client: self.client.clone(),
            api_key: self.api_key.clone(),
            standard_model: self.standard_model.clone(),
            advanced_model: self.advanced_model.clone(),
            progress: Some((tx, task_id)),
        }
    }
}
```

Add a `progress` field to `AnthropicProvider`:

```rust
#[derive(Debug)]
pub struct AnthropicProvider {
    client: Client,
    api_key: String,
    standard_model: String,
    advanced_model: String,
    progress: Option<(ProgressTx, Option<String>)>,
}
```

In `from_env()`, initialize it as `progress: None`.

In `post_messages_streaming`, after `accumulator.handle_event(&event)`:

```rust
                        Ok(event) => {
                            accumulator.handle_event(&event);
                            self.emit_sse_progress(&accumulator, &event, &mut last_progress_emit);
                        }
```

Where `last_progress_emit` is a `std::time::Instant` declared before the loop:

```rust
        let mut last_progress_emit = std::time::Instant::now();
```

And `emit_sse_progress` is:

```rust
    fn emit_sse_progress(
        &self,
        acc: &SseAccumulator,
        event: &serde_json::Value,
        last_emit: &mut std::time::Instant,
    ) {
        let Some((tx, task_id)) = &self.progress else { return };
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
                        elapsed_ms: 0, // route fills in wall-clock time
                        task_id: task_id.clone(),
                    });
                    *last_emit = std::time::Instant::now();
                }
            }
            _ => {}
        }
    }
```

**However**, `clone_with_progress` returns `AnthropicProvider`, not `Arc<dyn LlmProvider>`.
The parse route needs `Arc<dyn LlmProvider>` to pass to the pipeline. Fix: wrap it:

```rust
let provider_with_progress: Arc<dyn LlmProvider> = Arc::new(
    anthropic_provider.clone_with_progress(progress_tx.clone(), Some(task_id))
);
```

But the route currently gets the provider from `create_provider()` which returns
`Arc<dyn LlmProvider>`, not a concrete `AnthropicProvider`. The route cannot downcast.

**Final final design:** Add a `with_progress` method to the `LlmProvider` trait:

```rust
    fn with_progress(&self, _tx: ProgressTx, _task_id: Option<String>) -> Option<Arc<dyn LlmProvider>> {
        None
    }
```

`AnthropicProvider` overrides it to return `Some(Arc::new(self.clone_with_progress(tx, task_id)))`.
Other providers return `None` (no progress support). The parse route calls
`provider.with_progress(tx, task_id).unwrap_or_else(|| provider.clone())`.

This is the cleanest design: no wrapper struct, no shared mutable state, no trait
signature changes on the three call methods, one new defaulted method on the trait.

### Step 2.6 (revised): Add `progress` field to `AnthropicProvider`

**`AnthropicProvider` struct (line 89-95):**

**Before:**
```rust
#[derive(Debug)]
pub struct AnthropicProvider {
    client: Client,
    api_key: String,
    standard_model: String,
    advanced_model: String,
}
```

**After:**
```rust
#[derive(Debug)]
pub struct AnthropicProvider {
    client: Client,
    api_key: String,
    standard_model: String,
    advanced_model: String,
    progress: Option<(ProgressTx, Option<String>)>,
}
```

**`from_env()` (line 114):** Add `progress: None` to the struct literal.

**Add `clone_with_progress` method** after `resolve_model` (line 135):

```rust
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
```

### Step 2.7: Add `with_progress` to `LlmProvider` trait

Add to the trait (after `fn name()`):

```rust
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
```

**`AnthropicProvider` impl:** Override it:

```rust
    fn with_progress(
        &self,
        tx: ProgressTx,
        task_id: Option<String>,
    ) -> Option<Arc<dyn LlmProvider>> {
        Some(Arc::new(self.clone_with_progress(tx, task_id)))
    }
```

**Delete the `ProgressAwareProvider` struct** from Step 2.3. It is no longer needed.

### Step 2.8: Add `emit_sse_progress` to `AnthropicProvider`

Add this method to `impl AnthropicProvider` (the inherent impl, not the trait impl):

```rust
    fn emit_sse_progress(
        &self,
        acc: &SseAccumulator,
        event: &serde_json::Value,
        last_emit: &mut std::time::Instant,
    ) {
        let Some((tx, task_id)) = &self.progress else { return };
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
```

### Step 2.9: Wire progress into `post_messages_streaming`

In `post_messages_streaming`, add `let mut last_progress_emit = std::time::Instant::now();`
before the streaming loop. In the event handling block:

**Before:**
```rust
                        Ok(event) => accumulator.handle_event(&event),
```

**After:**
```rust
                        Ok(event) => {
                            accumulator.handle_event(&event);
                            self.emit_sse_progress(&accumulator, &event, &mut last_progress_emit);
                        }
```

### Step 2.10: Add `progress_channels` to `AppState`

**`server/state.rs`:**

**Before:**
```rust
use std::sync::{Arc, Mutex};

use crate::storage::Db;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Db>>,
    pub loopback_only: bool,
}
```

**After:**
```rust
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::importers::provider::ProgressTx;
use crate::storage::Db;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Db>>,
    pub loopback_only: bool,
    pub progress_channels: Arc<Mutex<HashMap<String, ProgressTx>>>,
}
```

### Step 2.11: Construct `progress_channels` in `build_router`

**`server/mod.rs` line 28:**

**Before:**
```rust
    let state = AppState { db, loopback_only };
```

**After:**
```rust
    let state = AppState {
        db,
        loopback_only,
        progress_channels: Arc::new(Mutex::new(std::collections::HashMap::new())),
    };
```

### Step 2.12: Register the SSE route

**`server/mod.rs`**, after the existing `/parse` route (line 107), add:

```rust
        .route(
            "/parse/progress/:parse_id",
            get(routes::parse::parse_progress),
        )
```

### Step 2.13: Add `parse_id` multipart field to the parse handler

In `parse_documents` in `server/routes/parse.rs`, add a new local:

```rust
    let mut parse_id: Option<String> = None;
```

Add a match arm in the multipart loop (alongside `"account_id"` and `"hints"`):

```rust
            "parse_id" => {
                let bytes = field.bytes().await.map_err(|e| {
                    AppError::bad_request(format!("failed to read parse_id: {e}"), "field_error")
                })?;
                let val = String::from_utf8(bytes.to_vec()).map_err(|_| {
                    AppError::bad_request("parse_id is not valid UTF-8", "field_error")
                })?;
                if !val.is_empty() {
                    parse_id = Some(val);
                }
            }
```

### Step 2.14: Create progress channel and emit phase events in `parse_documents`

After the `parse_id` extraction (after the `hints` validation, before preprocessing),
create the channel and store it:

```rust
    use crate::importers::provider::{ProgressEvent, ProgressTx, ParsePhase, emit_progress};

    // Create progress channel if the client supplied a parse_id.
    let progress_tx: Option<ProgressTx> = parse_id.as_ref().map(|pid| {
        let (tx, _rx) = tokio::sync::broadcast::channel::<ProgressEvent>(64);
        state
            .progress_channels
            .lock()
            .expect("progress_channels mutex poisoned")
            .insert(pid.clone(), tx.clone());

        // TTL cleanup: remove the channel after 10 minutes regardless.
        let channels = state.progress_channels.clone();
        let pid_clone = pid.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(600)).await;
            channels
                .lock()
                .expect("progress_channels mutex poisoned")
                .remove(&pid_clone);
        });

        tx
    });
```

Emit phase events at each stage. Before preprocessing:

```rust
    emit_progress(&progress_tx, ProgressEvent::Phase {
        phase: ParsePhase::Preprocessing,
        message: format!("Processing {} uploaded file(s)", files.len()),
        task_id: None,
    });
```

Before the unified/split mode branch (after `create_provider()`):

```rust
    emit_progress(&progress_tx, ProgressEvent::Phase {
        phase: ParsePhase::SendingToLlm,
        message: format!("Sending {} file(s) to AI model", documents.len()),
        task_id: None,
    });
```

### Step 2.15: Wire progress into unified-mode path

In `run_unified_path`, emit `BuildingContext` before the DB lookup:

```rust
    emit_progress(&progress_tx, ProgressEvent::Phase {
        phase: ParsePhase::BuildingContext,
        message: "Loading categories and holdings".to_string(),
        task_id: None,
    });
```

Pass `progress_tx` to `run_unified_path`. Change its signature:

**Before:**
```rust
async fn run_unified_path(
    state: AppState,
    hints: ParseHints,
    documents: Vec<DocumentInput>,
    account_id: String,
    account_institution: String,
    provider: std::sync::Arc<dyn crate::importers::provider::LlmProvider>,
    start: std::time::Instant,
) -> Result<IngestionPreview, AppError> {
```

**After:**
```rust
async fn run_unified_path(
    state: AppState,
    hints: ParseHints,
    documents: Vec<DocumentInput>,
    account_id: String,
    account_institution: String,
    provider: std::sync::Arc<dyn crate::importers::provider::LlmProvider>,
    start: std::time::Instant,
    progress_tx: Option<ProgressTx>,
) -> Result<IngestionPreview, AppError> {
```

Before calling `unified_extract_all`, wrap the provider with progress:

```rust
    let provider_for_call: Arc<dyn LlmProvider> = match &progress_tx {
        Some(tx) => provider
            .with_progress(tx.clone(), Some("unified".to_string()))
            .unwrap_or_else(|| provider.clone()),
        None => provider,
    };
```

Then call `unified_extract_all` with `provider_for_call.as_ref()` instead of
`provider.as_ref()`.

After the LLM call completes, emit `PostProcessing`:

```rust
    emit_progress(&progress_tx, ProgressEvent::Phase {
        phase: ParsePhase::PostProcessing,
        message: "Checking for duplicates".to_string(),
        task_id: None,
    });
```

At the very end, before returning the preview, emit `Done`:

```rust
    emit_progress(&progress_tx, ProgressEvent::Done {
        total_ms: start.elapsed().as_millis() as u64,
    });
```

Update the call site in `parse_documents`:

**Before:**
```rust
    if hints.mode() == ParseMode::Unified {
        return run_unified_path(state, hints, documents, account_id, account.institution, provider, start)
            .await
            .map(Json);
    }
```

**After:**
```rust
    if hints.mode() == ParseMode::Unified {
        return run_unified_path(
            state, hints, documents, account_id, account.institution,
            provider, start, progress_tx,
        )
        .await
        .map(Json);
    }
```

### Step 2.16: Wire progress into split-mode path

For split mode, pass `progress_tx` and the provider to `run_multi_file_pipeline`.

**`document_parser.rs` — `run_multi_file_pipeline` signature change:**

**Before:**
```rust
pub async fn run_multi_file_pipeline(
    documents: &[DocumentInput],
    hints: &ParseHints,
    _account_institution: &str,
    provider: Arc<dyn LlmProvider>,
) -> Result<PipelineOutcome> {
```

**After:**
```rust
pub async fn run_multi_file_pipeline(
    documents: &[DocumentInput],
    hints: &ParseHints,
    _account_institution: &str,
    provider: Arc<dyn LlmProvider>,
    progress_tx: Option<ProgressTx>,
) -> Result<PipelineOutcome> {
```

Pass `progress_tx` into `extract_all_parallel`.

**`extract_all_parallel` signature change:**

**Before:**
```rust
async fn extract_all_parallel(
    documents: &[DocumentInput],
    content_types: &[ContentType],
    hints: &ParseHints,
    provider: Arc<dyn LlmProvider>,
) -> Result<ExtractionResult> {
```

**After:**
```rust
async fn extract_all_parallel(
    documents: &[DocumentInput],
    content_types: &[ContentType],
    hints: &ParseHints,
    provider: Arc<dyn LlmProvider>,
    progress_tx: Option<ProgressTx>,
) -> Result<ExtractionResult> {
```

Inside the `for doc in documents` / `for ct in content_types` loop, before
`join_set.spawn(...)`, create a progress-aware provider per task:

```rust
            let task_id = format!("{}:{:?}", doc.filename, ct).to_lowercase();
            let task_provider = match &progress_tx {
                Some(tx) => provider_clone
                    .with_progress(tx.clone(), Some(task_id))
                    .unwrap_or(provider_clone),
                None => provider_clone,
            };
```

Then pass `task_provider` instead of `provider_clone` to `extract_single_file`.

Update the call site in `parse_documents` (split mode path):

**Before:**
```rust
    let pipeline_result =
        run_multi_file_pipeline(&documents, &hints, &account.institution, provider)
            .await
            .map_err(|e| AppError::bad_request(e.to_string(), "parse_error"))?;
```

**After:**
```rust
    let pipeline_result =
        run_multi_file_pipeline(&documents, &hints, &account.institution, provider, progress_tx.clone())
            .await
            .map_err(|e| AppError::bad_request(e.to_string(), "parse_error"))?;
```

Add post-processing and done events to the split-mode path too (after `build_multi_preview`):

```rust
    emit_progress(&progress_tx, ProgressEvent::Phase {
        phase: ParsePhase::PostProcessing,
        message: "Checking for duplicates".to_string(),
        task_id: None,
    });
```

And before the final `Ok(Json(preview))`:

```rust
    emit_progress(&progress_tx, ProgressEvent::Done {
        total_ms: route_started.elapsed().as_millis() as u64,
    });
```

### Step 2.17: Clean up channel on completion

After emitting `Done` or on error, remove the channel from the map. In
`parse_documents`, add a helper closure at the top (after `progress_tx` creation):

```rust
    let cleanup_progress = {
        let channels = state.progress_channels.clone();
        let pid = parse_id.clone();
        move || {
            if let Some(pid) = &pid {
                channels
                    .lock()
                    .expect("progress_channels mutex poisoned")
                    .remove(pid);
            }
        }
    };
```

Call `cleanup_progress()` before each `return Ok(Json(preview))` and in error paths.
Alternatively, use a scope guard, but an explicit call is clearer.

### Step 2.18: Add the SSE handler

Add to `server/routes/parse.rs`:

```rust
use std::convert::Infallible;

use axum::extract::Path;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::Stream;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

pub async fn parse_progress(
    State(state): State<AppState>,
    Path(parse_id): Path<String>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state
        .progress_channels
        .lock()
        .expect("progress_channels mutex poisoned")
        .get(&parse_id)
        .map(|tx| tx.subscribe());

    let stream = async_stream::stream! {
        let Some(rx) = rx else {
            yield Ok(Event::default()
                .event("error")
                .data(r#"{"code":"not_found","message":"No active parse with this ID"}"#));
            return;
        };
        let mut stream = BroadcastStream::new(rx);
        while let Some(Ok(event)) = stream.next().await {
            let data = serde_json::to_string(&event).unwrap_or_default();
            let event_name = match &event {
                crate::importers::provider::ProgressEvent::Phase { .. } => "phase",
                crate::importers::provider::ProgressEvent::LlmStart { .. } => "llm_start",
                crate::importers::provider::ProgressEvent::LlmProgress { .. } => "llm_progress",
                crate::importers::provider::ProgressEvent::Done { .. } => "done",
                crate::importers::provider::ProgressEvent::Error { .. } => "error",
            };
            yield Ok(Event::default().event(event_name).data(data));
            if matches!(
                &event,
                crate::importers::provider::ProgressEvent::Done { .. }
                    | crate::importers::provider::ProgressEvent::Error { .. }
            ) {
                return;
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}
```

This requires `async-stream` crate. Add to `Cargo.toml`:

```toml
async-stream = "0.3"
```

**Alternative without `async-stream`:** Use `tokio_stream::wrappers::BroadcastStream`
directly and `.map()` it. This avoids a new dependency:

```rust
pub async fn parse_progress(
    State(state): State<AppState>,
    Path(parse_id): Path<String>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state
        .progress_channels
        .lock()
        .expect("progress_channels mutex poisoned")
        .get(&parse_id)
        .map(|tx| tx.subscribe());

    let stream = match rx {
        Some(rx) => {
            let broadcast_stream = BroadcastStream::new(rx);
            let mapped = broadcast_stream.filter_map(|item| {
                let Ok(event) = item else { return None };
                let data = serde_json::to_string(&event).unwrap_or_default();
                let event_name = match &event {
                    ProgressEvent::Phase { .. } => "phase",
                    ProgressEvent::LlmStart { .. } => "llm_start",
                    ProgressEvent::LlmProgress { .. } => "llm_progress",
                    ProgressEvent::Done { .. } => "done",
                    ProgressEvent::Error { .. } => "error",
                };
                Some(Ok::<_, Infallible>(Event::default().event(event_name).data(data)))
            });
            // Wrap in Either so both arms have the same type
            futures_util::stream::Either::Left(mapped)
        }
        None => {
            let once = futures_util::stream::once(async {
                Ok::<_, Infallible>(
                    Event::default()
                        .event("error")
                        .data(r#"{"code":"not_found","message":"No active parse with this ID"}"#),
                )
            });
            futures_util::stream::Either::Right(once)
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}
```

**Use the alternative (no `async-stream` dependency).** No new crate needed.

### Step 2.19: Verify

Run `cargo clippy --all-targets -- -D warnings && cargo test` and fix any issues.

---

## Change 3: Error Classification

### Step 3.1: Define `ProviderError` in `provider.rs`

Add after the `ProgressEvent` types:

```rust
// ── Provider errors ─────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum ProviderError {
    /// DNS resolution failed, TCP connection refused, TLS handshake failed.
    Unreachable(String),
    /// Request or response timed out (reqwest timeout or OS-level).
    Timeout(String),
    /// Anthropic/OpenAI returned HTTP 429.
    RateLimit {
        retry_after: Option<u64>,
        detail: String,
    },
    /// Anthropic/OpenAI returned 401 or 403.
    AuthRejected(String),
    /// Upstream returned 5xx.
    UpstreamServerError { status: u16, body: String },
    /// Upstream returned 4xx (not 401/403/429).
    UpstreamClientError { status: u16, body: String },
    /// Response body was not valid UTF-8 or not valid JSON.
    ResponseUnreadable(String),
    /// Response did not contain the expected tool_use block.
    NoToolUse { tool_name: String },
    /// Streaming SSE error (connection dropped mid-stream).
    StreamInterrupted(String),
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
        }
    }
}

impl std::error::Error for ProviderError {}

impl From<ProviderError> for anyhow::Error {
    fn from(e: ProviderError) -> Self {
        anyhow::anyhow!(e)
    }
}
```

### Step 3.2: Change `LlmProvider` trait return types

**Before (all 3 methods):**
```rust
    ) -> Result<ProviderCallResult>;
```

**After:**
```rust
    ) -> Result<ProviderCallResult, ProviderError>;
```

This affects:
- `chat_with_tools`
- `chat_with_pdf_and_tools`
- `chat_with_files_and_tools`

### Step 3.3: Update `AnthropicProvider` — classify errors in `post_messages_streaming`

**The `.send().await` error (currently line ~375):**

**Before:**
```rust
        let response = req
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow!("sending request to Anthropic: {e}"))?;
```

**After:**
```rust
        let response = req
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    ProviderError::Timeout(format!("{e}"))
                } else if e.is_connect() {
                    ProviderError::Unreachable(format!("{e}"))
                } else {
                    ProviderError::Unreachable(format!("{e}"))
                }
            })?;
```

**The non-success status check:**

**Before:**
```rust
        if !status.is_success() {
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable>".to_string());
            return Err(anyhow!("Anthropic returned {status}: {error_body}"));
        }
```

**After:**
```rust
        if !status.is_success() {
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable>".to_string());
            let status_code = status.as_u16();
            return Err(match status_code {
                429 => {
                    let retry_after = None; // could parse Retry-After header
                    ProviderError::RateLimit {
                        retry_after,
                        detail: error_body,
                    }
                }
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
```

**Chunk read error in streaming loop:**

**Before:**
```rust
            let chunk = chunk.map_err(|e| anyhow!("reading Anthropic stream chunk: {e}"))?;
```

**After:**
```rust
            let chunk = chunk.map_err(|e| ProviderError::StreamInterrupted(format!("{e}")))?;
```

**UTF-8 error in streaming loop:**

**Before:**
```rust
            let text = std::str::from_utf8(&chunk)
                .map_err(|e| anyhow!("Anthropic stream chunk is not UTF-8: {e}"))?;
```

**After:**
```rust
            let text = std::str::from_utf8(&chunk)
                .map_err(|e| ProviderError::ResponseUnreadable(format!("{e}")))?;
```

**`into_result` errors:** Change the two error sites in `SseAccumulator::into_result`:

```rust
        let tool_json = /* ... */
            .ok_or_else(|| ProviderError::NoToolUse {
                tool_name: expected_tool.to_string(),
            })?;

        let value: serde_json::Value = serde_json::from_str(&tool_json)
            .map_err(|e| ProviderError::ResponseUnreadable(format!(
                "invalid JSON for tool {expected_tool}: {e}"
            )))?;
```

Change `into_result` return type from `Result<ProviderCallResult>` to
`Result<ProviderCallResult, ProviderError>`.

Change `post_messages_streaming` return type from `Result<ProviderCallResult>` to
`Result<ProviderCallResult, ProviderError>`.

The three `AnthropicProvider` trait methods already delegate to
`post_messages_streaming`, so their return types match automatically.

### Step 3.4: Update `OpenAIProvider`

Change the three trait method return types to `Result<ProviderCallResult, ProviderError>`.

In `chat_with_tools`, classify errors the same way:

**`.send().await`:**
```rust
            .map_err(|e| {
                if e.is_timeout() {
                    ProviderError::Timeout(format!("{e}"))
                } else if e.is_connect() {
                    ProviderError::Unreachable(format!("{e}"))
                } else {
                    ProviderError::Unreachable(format!("{e}"))
                }
            })?;
```

**Response body read:**
```rust
            .map_err(|e| ProviderError::ResponseUnreadable(format!("{e}")))?;
```

**Status check:**
```rust
        if !status.is_success() {
            let status_code = status.as_u16();
            return Err(match status_code {
                429 => ProviderError::RateLimit { retry_after: None, detail: body },
                401 | 403 => ProviderError::AuthRejected(body),
                400..=499 => ProviderError::UpstreamClientError { status: status_code, body },
                _ => ProviderError::UpstreamServerError { status: status_code, body },
            });
        }
```

**Tool input extraction:**
```rust
        let value = extract_openai_tool_input(&body, tool_name)
            .map_err(|e| ProviderError::ResponseUnreadable(format!("{e}")))?;
```

**PDF and files methods:** These already return `Err(anyhow!(...))`. Change to
`Err(ProviderError::UpstreamClientError { status: 0, body: "...".into() })`. Or
better, add a convenience variant. Actually these are "not supported" errors, not
upstream errors. Keep them as `anyhow::Error` by... wait, the return type is now
`Result<_, ProviderError>`. Add a generic variant or convert:

Simplest: these are config errors, not runtime errors. Add to `ProviderError`:

```rust
    /// Feature not supported by this provider.
    NotSupported(String),
```

With Display: `write!(f, "not supported: {msg}")`

Then the OpenAI PDF/files methods become:
```rust
        Err(ProviderError::NotSupported(
            "PDF input is not supported with the OpenAI provider. \
             Switch to FYNANCE_PARSE_PROVIDER=anthropic.".to_string()
        ))
```

Same for GeminiProvider methods.

### Step 3.5: Update `GeminiProvider`

Change all three method return types. Return `ProviderError::NotSupported(...)`.

### Step 3.6: Update `MockProvider`

Change all three method return types to `Result<ProviderCallResult, ProviderError>`.
The body stays the same (`Ok(self.pretend_result())`).

### Step 3.7: Update all caller sites

Every file that calls an `LlmProvider` method currently uses `?` to propagate
`anyhow::Error`. After the change, `?` propagates `ProviderError`. Since the callers
return `Result<T>` (i.e. `Result<T, anyhow::Error>`), and we defined
`impl From<ProviderError> for anyhow::Error`, the `?` operator still works.
**No changes needed at the 8 call sites.**

The call sites are:
- `llm_parser.rs:124` — `chat_with_tools` → `?` into `anyhow::Result` ✓
- `holdings_parser.rs:118` — `chat_with_tools` → `?` into `anyhow::Result` ✓
- `investments_parser.rs:147` — `chat_with_tools` → `?` into `anyhow::Result` ✓
- `periodic_holdings_parser.rs:67` — `chat_with_tools` → `?` into `anyhow::Result` ✓
- `pdf_parser.rs:52` — `chat_with_pdf_and_tools` → `?` into `anyhow::Result` ✓
- `pdf_parser.rs:110` — `chat_with_pdf_and_tools` → `?` into `anyhow::Result` ✓
- `pdf_parser.rs:175` — `chat_with_pdf_and_tools` → `?` into `anyhow::Result` ✓
- `pdf_parser.rs:231` — `chat_with_pdf_and_tools` → `?` into `anyhow::Result` ✓
- `unified_parser.rs:154` — `chat_with_files_and_tools` → `?` into `anyhow::Result` ✓

**However**, the `with_progress` method returns `Option<Arc<dyn LlmProvider>>`. Since
the trait now has different return types, this still works since the trait itself is
the contract.

### Step 3.8: Add `AppError` variants

**`server/error.rs`:** Add to the `AppError` enum:

```rust
    /// 502: upstream service error. code is a specific upstream error type.
    BadGateway { message: String, code: &'static str },
    /// 504: upstream service timed out.
    GatewayTimeout { message: String, code: &'static str },
    /// 429: rate limited by upstream service.
    TooManyRequests { message: String, code: &'static str },
```

Add status codes in `status_code()`:

```rust
            Self::BadGateway { .. } => StatusCode::BAD_GATEWAY,
            Self::GatewayTimeout { .. } => StatusCode::GATEWAY_TIMEOUT,
            Self::TooManyRequests { .. } => StatusCode::TOO_MANY_REQUESTS,
```

Add code strings in `code_str()`:

```rust
            Self::BadGateway { code, .. } => code,
            Self::GatewayTimeout { code, .. } => code,
            Self::TooManyRequests { code, .. } => code,
```

Add messages in `message()`:

```rust
            Self::BadGateway { message, .. }
            | Self::GatewayTimeout { message, .. }
            | Self::TooManyRequests { message, .. } => message.clone(),
```

Add constructor helpers:

```rust
    pub fn bad_gateway(message: impl Into<String>, code: &'static str) -> Self {
        Self::BadGateway { message: message.into(), code }
    }

    pub fn gateway_timeout(message: impl Into<String>, code: &'static str) -> Self {
        Self::GatewayTimeout { message: message.into(), code }
    }

    pub fn too_many_requests(message: impl Into<String>, code: &'static str) -> Self {
        Self::TooManyRequests { message: message.into(), code }
    }
```

### Step 3.9: Map `ProviderError` to `AppError` in the parse route

Add a helper function in `server/routes/parse.rs`:

```rust
use crate::importers::provider::ProviderError;

fn provider_err_to_app_error(e: anyhow::Error) -> AppError {
    // Try to downcast to ProviderError for classified errors.
    // If the error chain contains a ProviderError (set as the source via
    // From<ProviderError> for anyhow::Error), extract it.
    if let Some(pe) = e.downcast_ref::<ProviderError>() {
        return match pe {
            ProviderError::Timeout(msg) => {
                AppError::gateway_timeout(msg.clone(), "upstream_timeout")
            }
            ProviderError::Unreachable(msg) => {
                AppError::bad_gateway(msg.clone(), "upstream_unreachable")
            }
            ProviderError::RateLimit { detail, .. } => {
                AppError::too_many_requests(detail.clone(), "upstream_rate_limit")
            }
            ProviderError::AuthRejected(msg) => {
                AppError::bad_gateway(msg.clone(), "upstream_auth")
            }
            ProviderError::UpstreamServerError { body, .. } => {
                AppError::bad_gateway(body.clone(), "upstream_error")
            }
            ProviderError::UpstreamClientError { body, .. } => {
                AppError::bad_request(body.clone(), "upstream_rejected")
            }
            ProviderError::ResponseUnreadable(msg) => {
                AppError::bad_gateway(msg.clone(), "upstream_garbled")
            }
            ProviderError::NoToolUse { tool_name } => {
                AppError::bad_gateway(
                    format!("AI service did not return structured data (expected {tool_name} tool)"),
                    "upstream_no_tool_use",
                )
            }
            ProviderError::StreamInterrupted(msg) => {
                AppError::bad_gateway(msg.clone(), "upstream_stream_interrupted")
            }
            ProviderError::NotSupported(msg) => {
                AppError::bad_request(msg.clone(), "provider_not_supported")
            }
        };
    }
    // Unclassified error: treat as bad request with generic code.
    AppError::bad_request(e.to_string(), "parse_error")
}
```

**Wait:** Since `ProviderError` implements `Into<anyhow::Error>`, and callers use `?`
which converts via this impl, `downcast_ref` will work because `anyhow` stores the
original error as the source. This works correctly.

Replace error mappings in `parse_documents`:

**Unified-mode error mapping (line 334):**

**Before:**
```rust
        .map_err(|e| {
            tracing::error!(
                error = %e,
                elapsed_ms = llm_started.elapsed().as_millis() as u64,
                "parse: unified LLM call failed"
            );
            AppError::bad_request(e.to_string(), "parse_error")
        })?;
```

**After:**
```rust
        .map_err(|e| {
            tracing::error!(
                error = %e,
                elapsed_ms = llm_started.elapsed().as_millis() as u64,
                "parse: unified LLM call failed"
            );
            emit_progress(&progress_tx, ProgressEvent::Error {
                code: "parse_error".to_string(),
                message: e.to_string(),
            });
            provider_err_to_app_error(e)
        })?;
```

**Split-mode error mapping (line 229):**

**Before:**
```rust
    let pipeline_result =
        run_multi_file_pipeline(&documents, &hints, &account.institution, provider, progress_tx.clone())
            .await
            .map_err(|e| AppError::bad_request(e.to_string(), "parse_error"))?;
```

**After:**
```rust
    let pipeline_result =
        run_multi_file_pipeline(&documents, &hints, &account.institution, provider, progress_tx.clone())
            .await
            .map_err(|e| {
                emit_progress(&progress_tx, ProgressEvent::Error {
                    code: "parse_error".to_string(),
                    message: e.to_string(),
                });
                provider_err_to_app_error(e)
            })?;
```

### Step 3.10: Verify

Run `cargo clippy --all-targets -- -D warnings && cargo test` and fix any issues.

---

## Verification checklist

After all three changes are implemented:

- [ ] `cargo clippy --all-targets -- -D warnings` passes
- [ ] `cargo test` passes (all existing tests + new accumulator tests)
- [ ] Manual test: upload a small CSV in split mode → parse succeeds, no regression
- [ ] Manual test: upload a large PDF + CSV in unified mode → parse succeeds (no timeout)
- [ ] Manual test: connect to `GET /api/parse/progress/:id` via curl/EventSource while
      a parse is running → receive phase, llm_start, llm_progress, done events
- [ ] Manual test: disconnect the SSE client mid-parse → parse still completes, channel
      is cleaned up after TTL
- [ ] Manual test: set an invalid API key → frontend receives `upstream_auth` error code
- [ ] Manual test: temporarily block api.anthropic.com in /etc/hosts → frontend receives
      `upstream_unreachable` error code within ~10 seconds (connect timeout)
