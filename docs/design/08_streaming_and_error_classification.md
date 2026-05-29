# 08: Streaming API, Progress Reporting, and Error Classification for LLM Calls

## Problem

Large file imports (e.g. a 97.5 KB PDF + 35.1 KB CSV in unified mode) fail with a
connection-level timeout after ~60 seconds:

```
error=sending request to Anthropic: error sending request for url
(https://api.anthropic.com/v1/messages) elapsed_ms=60857
```

The error surfaces as a generic string to the frontend with no way to distinguish
"Anthropic is down" from "the request was too large" from "the model is still
thinking." During the (potentially multi-minute) wait, the frontend has no visibility
into whether the backend is making progress or stalled.

This document covers three changes: streaming to fix the timeout, progress reporting
to give the frontend real-time visibility, and error classification for actionable
diagnostics.


## Current architecture

All LLM calls go through `AnthropicProvider::post_messages` (`provider.rs:345-371`),
which makes a synchronous (non-streaming) HTTP POST to `https://api.anthropic.com/v1/messages`:

```
frontend  ──POST /api/parse──>  backend (Axum)
                                   │
                                   ├─ preprocess files
                                   ├─ build prompt + tool schema
                                   │
                                   └─ POST https://api.anthropic.com/v1/messages
                                          (blocking, no streaming, no timeout)
                                          │
                                          ├─ waits for full model response
                                          └─ returns complete JSON body
```

The reqwest `Client` is created with `Client::new()` (`provider.rs:115`), which sets
no explicit timeout. Three things combine to cause the failure:

1. **No connect or read timeout on the HTTP client.** The OS/network layer decides
   when to give up, typically at ~60 seconds on macOS.

2. **Anthropic's API gateway has a time-to-first-byte limit.** If the model takes too
   long to produce the first output token (common for large multi-file inputs), the
   gateway closes the connection before any response headers arrive.

3. **Every error is a flat string.** The `map_err` at `provider.rs:359` discards
   reqwest's structured error info (`is_timeout()`, `is_connect()`, `status()`) and
   wraps everything in `anyhow!("sending request to Anthropic: {e}")`. The frontend
   receives the same opaque message regardless of cause.


## Change 1: Switch to the Streaming API

### What streaming changes

Anthropic supports `"stream": true` in the request body. Instead of waiting for the
full response and returning a single JSON object, the server sends a sequence of
Server-Sent Events (SSE):

```
event: message_start
data: {"type":"message_start","message":{"id":"msg_...","model":"...","usage":{...}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"...","name":"extract_unified","input":{}}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"..."}}
... (many deltas) ...

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":1234}}

event: message_stop
data: {"type":"message_stop"}
```

The critical difference: `message_start` arrives within **seconds** of the request
being sent, regardless of how long the model takes to finish. Every subsequent
`content_block_delta` keeps the connection alive. The 60-second gateway timeout is
a time-to-first-byte limit: streaming defeats it entirely because the first byte
arrives almost immediately.

### Why streaming fixes the timeout

The current non-streaming flow:

```
request sent ──────── 60s silence ────────> gateway kills connection
```

With streaming:

```
request sent ── ~2s ──> message_start ── deltas every ~1s ──> message_stop
                 ^                                                ^
                 first byte (gateway satisfied)                   done (maybe 3 min later)
```

The model can take as long as it needs. Each SSE event resets any TCP idle timer. A
unified-mode parse of a large PDF + CSV that produces hundreds of rows might take
2-3 minutes of model time, and streaming handles this without any timeout risk.

### How `post_messages` changes

`post_messages` currently returns `Result<String>` (the full response body). With
streaming it would:

1. Add `"stream": true` to the request JSON body.
2. Consume the response as a byte stream (`response.bytes_stream()`), parsing SSE
   events line by line.
3. Accumulate `input_json_delta` fragments into a single JSON string per tool_use
   block.
4. Extract `usage` from `message_start` (input tokens) and `message_delta` (output
   tokens).
5. Return the same `Result<String>` (reconstructed full body) or, better, return a
   parsed struct directly so callers don't re-parse.

The `LlmProvider` trait and all callers (`llm_parser.rs`, `unified_parser.rs`,
`holdings_parser.rs`, `pdf_parser.rs`, `investments_parser.rs`,
`periodic_holdings_parser.rs`) do **not** change their signatures. Streaming is
entirely internal to `AnthropicProvider::post_messages`. Callers still
`await` a single `Result<ProviderCallResult>`.

### Scope of code changes

| Area | Files touched | Nature of change |
|------|--------------|------------------|
| `post_messages` rewrite | `provider.rs` | Replace the 25-line `post_messages` method with a ~80-line streaming SSE parser. Existing response-parsing helpers (`extract_anthropic_tool_input`, `extract_anthropic_usage`, `extract_anthropic_stop_reason`) are either reused or inlined into the streaming accumulator. |
| New dependency | `Cargo.toml` | Add `reqwest` feature `"stream"`. Optionally add `futures` or `tokio-stream` if needed for `StreamExt` combinators (tokio is already present). No new crates strictly required; reqwest's `bytes_stream()` returns an `impl Stream`. |
| Tests | `provider.rs` (test module) | Add a unit test that feeds a canned SSE byte sequence to the accumulator and asserts correct JSON reconstruction. The mock provider is unaffected. |
| Trait / callers | None | `LlmProvider` trait signatures stay the same. All 8 call sites remain untouched. |

Estimated net diff: ~100 lines added, ~25 removed in `provider.rs`. One line in
`Cargo.toml`.

### Pros

- Eliminates the gateway timeout for arbitrarily large inputs and long model runs.
- No signature changes to the `LlmProvider` trait or any caller.
- Enables progress reporting (Change 2): SSE deltas provide real-time token counts
  that the backend can forward to the frontend as progress events.

### Cons

- SSE parsing adds complexity to `post_messages`. Edge cases: partial lines across
  chunk boundaries, events with no `data:` field, unexpected event types.
- Slightly harder to unit test (need to mock a stream of SSE bytes vs. a single
  JSON body).
- The `input_json_delta` fragments must be concatenated as raw strings, then parsed
  as JSON once at the end. If Anthropic ever changes the delta encoding, this breaks.
  (Unlikely: the streaming format is stable and versioned by `anthropic-version`.)


## Change 2: Progress Reporting

### The problem

A unified-mode parse of a large PDF + CSV can take 1-3 minutes. During this time the
frontend has zero visibility: it fired `POST /api/parse` and is waiting for the
response. The user sees a spinner with no indication of whether the backend is making
progress, stalled, or about to finish. Streaming (Change 1) solves the backend-to-
Anthropic timeout, but the frontend-to-backend connection is still a single long-lived
HTTP request that returns nothing until the very end.

### What the backend needs to support

The backend exposes a new SSE endpoint that the frontend can connect to in parallel
with the parse request. The parse request itself remains a normal `POST` that returns
`IngestionPreview` on completion: it does not become a streaming response. This keeps
the two concerns cleanly separated (data vs. progress) and avoids changing the
existing `parseDocuments` contract.

### Architecture

```
frontend                              backend (Axum)                      Anthropic
   │                                     │                                    │
   ├─ GET /api/parse/progress/:id ──────>│ (SSE connection, held open)        │
   │    <── event: phase ───────────────<│                                    │
   │                                     │                                    │
   ├─ POST /api/parse (with parse_id) ──>│                                    │
   │                                     ├─ preprocess files                  │
   │    <── event: phase ───────────────<│   ("preprocessing")                │
   │                                     ├─ build context                     │
   │    <── event: phase ───────────────<│   ("building_context")             │
   │                                     ├─ POST /v1/messages (stream:true) ─>│
   │                                     │                                    │
   │                                     │<── message_start ────────────────<│
   │    <── event: llm_start ──────────<│                                    │
   │                                     │                                    │
   │                                     │<── content_block_delta ──────────<│
   │    <── event: llm_progress ───────<│   (output_tokens so far)           │
   │                                     │    ... repeated ...                │
   │                                     │                                    │
   │                                     │<── message_stop ─────────────────<│
   │    <── event: phase ───────────────<│   ("post_processing")             │
   │                                     ├─ dedup, build preview              │
   │    <── event: done ────────────────<│                                    │
   │                                     │                                    │
   │<── 200 IngestionPreview ──────────<│ (POST /api/parse returns)          │
```

### SSE endpoint: `GET /api/parse/progress/:parse_id`

Returns a text/event-stream. The frontend connects before (or concurrently with)
firing the parse request. Events are:

```
event: phase
data: {"phase":"preprocessing","message":"Processing uploaded files"}

event: phase
data: {"phase":"building_context","message":"Loading categories and holdings"}

event: phase
data: {"phase":"sending_to_llm","message":"Sending 2 files to AI model"}

event: llm_start
data: {"model":"claude-sonnet-4-6","input_tokens":45231}

event: llm_progress
data: {"output_tokens":1200,"elapsed_ms":8500}

event: llm_progress
data: {"output_tokens":3800,"elapsed_ms":17200}

event: phase
data: {"phase":"post_processing","message":"Checking for duplicates"}

event: done
data: {"total_ms":42000}

event: error
data: {"code":"upstream_timeout","message":"The AI service took too long to respond"}
```

The `parse_id` is a short random string (e.g. `nanoid`) generated by the frontend and
sent as a field in the `POST /api/parse` multipart body. This ties the progress stream
to the specific parse operation.

### Event types

| Event | When sent | Data fields |
|-------|-----------|-------------|
| `phase` | At each pipeline stage transition | `phase` (enum string), `message` (human-readable) |
| `llm_start` | When `message_start` SSE arrives from Anthropic | `model`, `input_tokens` |
| `llm_progress` | Periodically during Anthropic streaming (throttled to ~1/sec) | `output_tokens` (cumulative), `elapsed_ms` |
| `done` | Parse completed successfully | `total_ms` |
| `error` | Parse failed | `code`, `message` (matches error classification codes from Change 3) |

### Phase enum

Phases the backend reports, in order:

| Phase | When | What is happening |
|-------|------|-------------------|
| `preprocessing` | Files received, format detection starting | Detecting CSV/PDF, base64 encoding |
| `building_context` | After preprocessing | Loading categories tree and open holdings from DB |
| `sending_to_llm` | Context built, about to call Anthropic | The request is being sent |
| `waiting_for_llm` | Anthropic streaming in progress | Model is generating output; `llm_progress` events carry token counts |
| `post_processing` | Anthropic response complete | Deduplication, preview assembly |

Split mode has additional granularity: each `(file, content_type)` extraction task
can report its own sub-phase. For split mode, the `phase` event includes optional
`file` and `content_type` fields:

```
event: phase
data: {"phase":"waiting_for_llm","file":"statement.pdf","content_type":"transactions","message":"Extracting transactions from statement.pdf"}
```

### Backend implementation

**Progress channel.** A `tokio::sync::broadcast` channel per active parse, keyed by
`parse_id`. The parse route creates the channel when the request arrives; the SSE
endpoint subscribes to it. Events are lightweight `serde_json::Value` objects.

```rust
type ProgressTx = tokio::sync::broadcast::Sender<ProgressEvent>;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProgressEvent {
    Phase { phase: ParsePhase, message: String, file: Option<String>, content_type: Option<String> },
    LlmStart { model: String, input_tokens: u64 },
    LlmProgress { output_tokens: u64, elapsed_ms: u64 },
    Done { total_ms: u64 },
    Error { code: String, message: String },
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ParsePhase {
    Preprocessing,
    BuildingContext,
    SendingToLlm,
    WaitingForLlm,
    PostProcessing,
}
```

**Channel storage.** A `DashMap<String, ProgressTx>` (or `Arc<Mutex<HashMap>>`) on
`AppState`. The parse route inserts its channel; the SSE route looks it up. Channels
are removed on `Done` or `Error`, or after a TTL (e.g. 5 minutes) to prevent leaks
from abandoned requests.

**Sending progress from the streaming accumulator.** The SSE accumulator in
`post_messages` (from Change 1) accepts an optional `ProgressTx`. When it receives a
`message_start` event from Anthropic, it sends `LlmStart`. On each
`content_block_delta`, it increments an internal `output_tokens` counter and, if
>= 1 second has passed since the last emit, sends `LlmProgress`. This throttling
prevents flooding the channel on fast models.

The `LlmProvider` trait gains an optional progress handle:

```rust
async fn chat_with_files_and_tools(
    &self,
    system_prompt: &str,
    files: &[(String, String, Vec<u8>)],
    text_supplement: &str,
    tool_name: &str,
    tool_schema: Value,
    agent_override: Option<Agent>,
    progress: Option<ProgressTx>,       // new, optional
) -> Result<ProviderCallResult>;
```

Callers that don't need progress pass `None`. The unified-mode path in `parse.rs`
passes the channel. The `MockProvider` ignores it.

**SSE route handler.** Uses `axum::response::Sse` with `tokio_stream::wrappers::BroadcastStream`:

```rust
pub async fn parse_progress(
    State(state): State<AppState>,
    Path(parse_id): Path<String>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.progress_channels
        .get(&parse_id)
        .map(|entry| entry.value().subscribe());
    // ... wrap rx into SSE Event stream, or send a single "not_found" event and close
}
```

The route is registered as:
```rust
.route("/parse/progress/:parse_id", get(routes::parse::parse_progress))
```

### Scope of code changes

| Area | Files touched | Nature of change |
|------|--------------|------------------|
| `ProgressEvent` + `ParsePhase` enums | `provider.rs` or new `progress.rs` | ~40 lines: event types + phase enum |
| Progress channel on `AppState` | `server/state.rs` | ~5 lines: add `DashMap<String, ProgressTx>` field |
| SSE route handler | `server/routes/parse.rs` | ~30 lines: new `parse_progress` function |
| Route registration | `server/mod.rs` | 1 line: add `/api/parse/progress/:parse_id` |
| Parse route: create channel + emit phases | `server/routes/parse.rs` | ~25 lines: insert channel, send phase events at each stage |
| Streaming accumulator: emit LLM events | `provider.rs` (`post_messages`) | ~15 lines: send `LlmStart`/`LlmProgress` from SSE loop |
| `LlmProvider` trait: add `progress` param | `provider.rs` | Signature change on 3 trait methods. Both real impls + mock update. |
| Channel cleanup | `server/routes/parse.rs` or `state.rs` | ~10 lines: remove channel on done/error, TTL sweep |
| New dependency | `Cargo.toml` | `dashmap` (or use existing `Arc<Mutex<HashMap>>`), `tokio-stream` for `BroadcastStream` |
| Tests | `provider.rs`, `parse.rs` | ~20 lines: test that progress events are emitted in order |

Estimated net diff: ~160 lines added across 4-5 files.

### Pros

- The frontend gets real-time visibility into parse progress without polling.
- `output_tokens` count gives a meaningful proxy for "how much work is done" since
  token generation is the dominant cost. The frontend can derive a percentage from
  `output_tokens / max_tokens` (the backend sets `MAX_TOKENS_DOCUMENTS = 32_000`).
- Phase events let the frontend show distinct messages ("Processing files",
  "Waiting for AI model", "Checking for duplicates") instead of a generic spinner.
- The `POST /api/parse` response contract does not change. `parseDocuments` still
  returns `Promise<IngestionPreview>`. The SSE connection is additive.
- Split mode benefits too: the frontend can show per-file progress when multiple
  LLM calls run in parallel.

### Cons

- Adds a coordination mechanism (broadcast channel + DashMap) to `AppState`. This is
  simple but is new infrastructure that must be cleaned up on abandonment.
- The `LlmProvider` trait signature changes to accept `Option<ProgressTx>`. This is
  the same kind of mechanical propagation as the error classification change: all
  3 methods, both impls, mock, 8 call sites. Most pass `None`.
- `output_tokens` as a progress metric is approximate. The model may front-load
  reasoning tokens (in extended thinking mode) before producing output, making
  progress appear stalled at 0% then jump. This is a cosmetic issue, not a
  correctness one; the frontend can handle it with an indeterminate state until the
  first `llm_progress` event arrives.
- If the frontend never connects to the SSE endpoint (e.g. old client, mock mode),
  the channel is created and cleaned up with no subscribers. Harmless but slightly
  wasteful. The backend should check subscriber count before serializing events.

### Alternative considered: streaming the parse response itself

Instead of a separate SSE endpoint, the `POST /api/parse` response could itself be a
streaming response that sends progress events followed by the final JSON. This was
rejected because:

- It changes the `parseDocuments` return type from `Promise<IngestionPreview>` to a
  stream consumer, breaking the existing frontend contract and every mock.
- Mixing progress events and the final payload in one stream complicates parsing: the
  frontend must buffer events, then detect the "final" event and parse it differently.
- The separate SSE endpoint can be connected to optionally. Clients that don't care
  about progress (scripts, agents, tests) just call `POST /api/parse` and ignore the
  progress endpoint entirely.


## Change 3: Error Classification

### What we have now

Every failure in `post_messages` is wrapped the same way:

```rust
// provider.rs:359
.map_err(|e| anyhow!("sending request to Anthropic: {e}"))?;

// provider.rs:365
.map_err(|e| anyhow!("reading Anthropic response: {e}"))?;

// provider.rs:368
return Err(anyhow!("Anthropic returned {status}: {body_str}"));
```

The route handler in `parse.rs:334` then maps everything to a single error code:

```rust
AppError::bad_request(e.to_string(), "parse_error")
```

The frontend receives `{ "error": "sending request to Anthropic: ...", "code": "parse_error" }`
for every failure, whether it was a timeout, a missing API key, a 429 rate limit, or
a 500 from Anthropic.

### What we want

Distinct error codes that the frontend can match on to show appropriate messages:

| Scenario | HTTP status | `code` field | Frontend message |
|----------|-------------|-------------|------------------|
| Can't connect to Anthropic (DNS, TCP) | 502 | `upstream_unreachable` | "Could not reach the AI service. Check your internet connection." |
| Request timed out (no response in time) | 504 | `upstream_timeout` | "The AI service took too long to respond. Try a smaller file or split mode." |
| Anthropic returned 429 | 429 | `upstream_rate_limit` | "Rate limited by the AI service. Wait a moment and retry." |
| Anthropic returned 401/403 | 502 | `upstream_auth` | "API key is invalid or expired. Check FYNANCE_ANTHROPIC_API_KEY." |
| Anthropic returned 5xx | 502 | `upstream_error` | "The AI service returned an error. Try again shortly." |
| Anthropic returned 4xx (other) | 400 | `upstream_rejected` | "The AI service rejected the request: {details}" |
| Response body unreadable | 502 | `upstream_garbled` | "Received an unreadable response from the AI service." |
| Response missing tool_use block | 502 | `upstream_no_tool_use` | "The AI service did not return structured data. Try again." |

### How to implement

**Step 1: Define a `ProviderError` enum in `provider.rs`.**

```rust
#[derive(Debug)]
pub enum ProviderError {
    Unreachable(String),
    Timeout(String),
    RateLimit { retry_after: Option<u64>, detail: String },
    AuthRejected(String),
    UpstreamServerError { status: u16, body: String },
    UpstreamClientError { status: u16, body: String },
    ResponseUnreadable(String),
    NoToolUse { tool_name: String },
}
```

**Step 2: Classify in `post_messages`.**

Replace the flat `map_err` calls with structured inspection:

```rust
let response = req.json(body).send().await.map_err(|e| {
    if e.is_timeout() {
        ProviderError::Timeout(format!("request timed out: {e}"))
    } else if e.is_connect() {
        ProviderError::Unreachable(format!("connection failed: {e}"))
    } else {
        ProviderError::Unreachable(format!("request failed: {e}"))
    }
})?;
```

For HTTP-level errors, inspect the status code:

```rust
match status.as_u16() {
    429 => ProviderError::RateLimit { ... },
    401 | 403 => ProviderError::AuthRejected(body_str),
    400..=499 => ProviderError::UpstreamClientError { status, body },
    500..=599 => ProviderError::UpstreamServerError { status, body },
    _ => ...
}
```

**Step 3: Map `ProviderError` to `AppError` in the route handler.**

Instead of the current catch-all:
```rust
.map_err(|e| AppError::bad_request(e.to_string(), "parse_error"))
```

A helper function maps each variant to the right HTTP status and code:

```rust
fn provider_err_to_app_error(e: ProviderError) -> AppError {
    match e {
        ProviderError::Timeout(msg) => AppError::gateway_timeout(msg, "upstream_timeout"),
        ProviderError::RateLimit { detail, .. } => AppError::too_many_requests(detail, "upstream_rate_limit"),
        ProviderError::Unreachable(msg) => AppError::bad_gateway(msg, "upstream_unreachable"),
        // ...
    }
}
```

This requires adding `BadGateway` (502), `GatewayTimeout` (504), and `TooManyRequests` (429)
variants to `AppError` in `server/error.rs`.

**Step 4: Add explicit timeouts to the reqwest `Client`.**

```rust
client: Client::builder()
    .connect_timeout(Duration::from_secs(10))
    .timeout(Duration::from_secs(300))
    .build()?
```

The connect timeout (10s) catches "Anthropic is unreachable" quickly. The overall
timeout (5 min) is a safety net: with streaming, it should never fire unless the
connection is truly dead.

### Scope of code changes

| Area | Files touched | Nature of change |
|------|--------------|------------------|
| `ProviderError` enum | `provider.rs` | ~30 lines: new enum + Display impl |
| `post_messages` error handling | `provider.rs` | ~20 lines changed: replace 3 `map_err` calls with structured classification |
| `extract_*` helpers | `provider.rs` | ~10 lines: return `ProviderError::NoToolUse` / `ResponseUnreadable` instead of `anyhow!` |
| `AppError` new variants | `server/error.rs` | ~25 lines: add `BadGateway`, `GatewayTimeout`, `TooManyRequests` variants + status codes |
| Route handler mapping | `server/routes/parse.rs` | ~20 lines: replace `map_err(bad_request)` with `map_err(provider_err_to_app_error)` |
| Split-mode route handlers | `server/routes/parse.rs`, `document_parser.rs` | ~5 lines each: same `map_err` replacement at 2-3 other call sites |
| Client builder | `provider.rs` | 3 lines: `Client::builder()` with timeouts |
| `LlmProvider` trait | `provider.rs` | Return type changes from `Result<ProviderCallResult>` to `Result<ProviderCallResult, ProviderError>`. All 3 trait methods, both impls (Anthropic, OpenAI), and the mock impl update their signatures. |
| Tests | `provider.rs` | ~15 lines: test that reqwest error types map to correct `ProviderError` variants |

Estimated net diff: ~120 lines added across 4 files.

### Pros

- Frontend can show specific, actionable messages instead of raw error strings.
- Operators (you) can distinguish "my API key expired" from "Anthropic is having an
  outage" from "the file is too big" at a glance in the server logs.
- The explicit connect timeout (10s) surfaces unreachable errors quickly instead of
  waiting 60 seconds.
- `ProviderError` is a proper enum with variants, not stringly-typed. Code that
  handles provider errors can `match` on variants instead of parsing substrings.

### Cons

- Touches more files than the streaming change: `provider.rs`, `server/error.rs`,
  `parse.rs`, plus the trait signature change propagates to both real providers
  (Anthropic, OpenAI) and the mock.
- The `LlmProvider` trait return type change is a breaking change within the
  codebase. Every call site must handle `ProviderError` instead of `anyhow::Error`.
  This is mechanical but touches 8 call sites across 6 files.
- Anthropic might return error shapes that don't fit the classification (e.g. a 400
  with a body that looks like a 429). The classification is best-effort; edge cases
  go to the catch-all `UpstreamClientError`.


## Implementation order

1. **Streaming first.** This eliminates the timeout, which is the user-facing
   blocker. It's a contained change (one method in one file + Cargo.toml).

2. **Progress reporting second.** Builds directly on the streaming accumulator from
   Change 1. The SSE endpoint and broadcast channel are additive; the only
   cross-cutting change is the `Option<ProgressTx>` parameter on `LlmProvider`.

3. **Error classification third.** This is additive, wider, and benefits from both
   streaming and progress being in place. Streaming-specific errors ("stream
   interrupted mid-response") and progress-aware errors (sending an `error` event
   before closing the SSE connection) can be classified from the start.

4. **Frontend updates (out of scope).** Once the backend ships all three changes,
   the frontend can:
   - Connect to `GET /api/parse/progress/:parse_id` and render a progress bar.
   - Switch on error `code` instead of showing raw strings.
   - These are follow-ups, not prerequisites for the backend work.


## Total estimated diff

| Component | Lines added | Lines removed | Files touched |
|-----------|------------ |---------------|---------------|
| Streaming | ~100 | ~25 | 2 (`provider.rs`, `Cargo.toml`) |
| Progress reporting | ~160 | ~5 | 5 (`provider.rs`, `parse.rs`, `state.rs`, `mod.rs`, `Cargo.toml`) |
| Error classification | ~120 | ~20 | 4 (`provider.rs`, `error.rs`, `parse.rs`, `document_parser.rs`) |
| **Total** | **~380** | **~50** | **6 files** |

The `LlmProvider` trait, mock provider, and all 8 call sites need signature updates
for both progress reporting (`Option<ProgressTx>`) and error classification (swap
`Result<T>` for `Result<T, ProviderError>`). These can be combined into a single
pass. Most call sites pass `None` for progress and propagate the new error type
mechanically.
