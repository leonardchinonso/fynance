# 30 — OpenAI-compatible parse provider parity (local llama.cpp, PDFs via an external pdf-extract tool)

Status: **implemented on `feat/openai-parity`, in review.** All five parts are on the branch; benchmarks live outside the repo.

## Goal

Make `FYNANCE_PARSE_PROVIDER=openai` as capable as the Anthropic provider for `/api/parse`: the default unified mode, PDFs, images and CSVs, with clear errors and live progress. Imports can then run on any OpenAI-compatible `/v1/chat/completions` server, for example a local llama.cpp server running a Qwen vision model, and the results can be benchmarked against Claude.

The provider stays generic. It must work with any OpenAI-compatible server, including the real OpenAI API. The repo contains no deployment-specific hostnames, ports or model names. Tests use loopback fakes only and never call a real model server.

**PDFs:** Fynance does not render PDFs itself. Per the owner and coordinator (2026-11-13), the PDF reader is being built **outside Fynance**, by the Helm coordinator, as a reusable `pdf-extract <file> --out <dir>` CLI. For each page it outputs the text layer, a rendered PNG and the numbers found, as JSON plus PNGs. Fynance gets a **thin adapter** that calls that tool during a parse, for providers that cannot take PDFs, and turns its output into text plus image content. **The tool's exact JSON schema and CLI flags are still to be confirmed from the Helm tool.** Everything that depends on them is isolated in one adapter module and marked *(to confirm)* below.

## What I read, and what I only assumed

Read in the `feat/openai-base-url` worktree (`C:\w\fynance\openai-base-url`, HEAD `f61bb14`):

- `backend/src/importers/provider.rs` (all of it)
- `unified_parser.rs`, `unified.rs`, `pdf_parser.rs` (head)
- `format_detection.rs` (`preprocess_file`, `validate_pdf`)
- the relevant parts of `document_parser.rs` (`ParseHints`/`ParseMode`, `build_multi_preview`, `convert_parsed_holdings`)
- `llm_parser.rs` (call site) and `pricing.rs` (head)
- `server/routes/parse.rs` (all of it)
- the commit-time category check in `storage/db.rs` (~L2020)
- `cli.rs`, `main.rs`, `lib.rs`, `Cargo.toml`
- `Dockerfile`, `docker-compose.yml`
- `.github/workflows/ci.yml`
- `.env.example`, the env table in `backend/RUNNING.md`
- the `/api/parse` error table in `docs/api.html`
- the headers of `scripts/check_api_docs.py` and `scripts/check_pii.py`
- `backend/tests/fixtures/`
- the researcher's notes, which live outside the repo

Web facts I checked:

- llama-server reads `tool_choice` only as a string (`auto`/`none`/`required`). It silently ignores the object form and falls back to `auto` ([llama.cpp #30369](https://github.com/ggml-org/llama.cpp/issues/30369), [#26535](https://github.com/ggml-org/llama.cpp/issues/26535)).
- `"required"` is not enforced on some reasoning-preserving templates ([#27217](https://github.com/ggml-org/llama.cpp/issues/27217)).
- A non-streaming request may keep generating after the client disconnects, while a streaming one is cancelled when a write fails ([#24496](https://github.com/ggml-org/llama.cpp/issues/24496)).

Assumed and not verified (builders must confirm):

- the Helm tool's CLI flags, output file name and JSON field names
- that the llama.cpp build in use accepts `stream_options` and `chat_template_kwargs`

## Corrections to the research gap list (verified in code)

| # | Research claim | What the code actually does |
|---|---|---|
| 1 | OpenAI rejects unified mode | **Confirmed.** `OpenAIProvider::chat_with_files_and_tools` returns `NotSupported` (provider.rs ~L1220). Split mode's PDF/image path (`pdf_parser.rs`) also calls `chat_with_files_and_tools`, so split-mode PDFs fail too. Only split-mode CSV works. `chat_with_pdf_and_tools` has no callers outside provider.rs. |
| 2 | No PDF support | **Confirmed.** `MAX_FILES = 5` (parse.rs L30) caps *uploaded files*, not content parts. Expanding a PDF into page parts inside the provider does not hit it. |
| 3 | `tool_choice` sent as an object | **Confirmed** (provider.rs L1138). |
| 4 | No `max_tokens`, and truncation surfaces as a parse error | **Confirmed.** `stop_reason: None` is hard-coded (L1200). A truncated tool-call JSON fails in `extract_openai_tool_input` as `ResponseUnreadable` *before* the route's `response_truncated` check (parse.rs L550) runs. Anthropic has the same ordering problem: `SseAccumulator::into_result` fails on invalid JSON before the stop reason is looked at. |
| 5 | 300 s non-streaming timeout | PR #138 already made it `FYNANCE_OPENAI_TIMEOUT_SECS` (a per-request total). The request is still not streamed. |
| 6 | No temperature and no thinking control | **Confirmed.** |
| 7 | "One bad date/amount fails the whole document" | **Partly wrong.** In unified mode, `post_validate` does `filter_map(\|t\| convert_transaction(t).ok())`, so a bad date or amount **silently drops that row and adds no note**. The data loss is real, it is just silent. The whole-document failures do exist elsewhere: (a) a type mismatch at the serde level (`amount` as a JSON number, a missing `row_confidence`, a null `description`) fails `UnifiedRaw` deserialisation and returns 400 `parse_error` with no rows; (b) a bad holding quantity, price or value hits `?` in `convert_parsed_holdings` and fails the whole call. Category ids are not checked at parse time. At commit, an unknown or parent id **rejects that row** (`db.rs` ~L2023), after the user has already approved the preview. |

Other findings:

- **Anthropic SSE decoding bug.** `post_messages_streaming` calls `std::str::from_utf8(&chunk)` on each network chunk (L651). A multi-byte character such as `£` that is split across two chunks fails the whole call with `ResponseUnreadable`. The OpenAI streaming code must not copy this. Part 3 fixes it for both providers with one shared splitter.
- **Wrong cost estimate for some local models.** `pricing::agent_from_model` matches substrings, so a model id containing "flash" (for example a Qwen "Flash" build) is costed at Gemini Flash rates, and any other unknown id at Sonnet rates. This is cosmetic and out of scope. Benchmark scoring must use token counts, not `estimated_price`.
- **`tempfile` is only a dev-dependency today.** The adapter needs a temporary working directory at runtime, so it moves to `[dependencies]`. The crate is already in `Cargo.lock`.

## Decisions taken

1. **PDFs go through the external Helm `pdf-extract` tool, through a thin adapter.**
   - `FYNANCE_PDF_EXTRACT_CMD` is the path to the tool's executable, or to a wrapper script.
   - `FYNANCE_PDF_EXTRACT_ARGS` is an optional JSON array of extra arguments (for example `["--dpi","150"]`), so Fynance does not hard-code tool flags it does not need.
   - `FYNANCE_PDF_EXTRACT_TIMEOUT_SECS` defaults to 120.
   - Fynance renders nothing and has no PDF library dependency.
   - Licence: the tool runs as a separate process, so its licence does not affect Fynance's MIT code. Bundling it into the Docker image would still mean distributing it (see question 1). The Helm tool should avoid AGPL engines (MuPDF/PyMuPDF); PDFium via `pdfium-render` (MIT/Apache, with BSD/Apache PDFium binaries) is the sensible pick there.
2. **Which providers call the adapter.** Only the OpenAI-compatible provider. Anthropic and Gemini keep sending native PDFs. The adapter is a plain module with no provider coupling, so other providers could use it later.
3. **The default PDF mode for the OpenAI provider is `both`: the text layer plus the page image for each page.**
   - This mirrors Claude's native PDF handling, which gives the model page images plus extracted text, so the benchmark is fair.
   - `FYNANCE_OPENAI_PDF_MODE` also takes `images`, `text` and `native`:
     - `text` falls back to the image for pages with no text layer, such as scanned pages.
     - `native` sends an OpenAI `file` content part and needs no tool. It only works on servers that accept PDFs, such as api.openai.com.
   - The tool's "numbers found" output is parsed if present but **not sent to the model**, because Claude does not get it either. It stays available for later use.
4. **`tool_choice` defaults to the string `"required"` on every server, including real OpenAI.** With exactly one tool this is the same as naming it, and it is the only form llama.cpp honours. `FYNANCE_OPENAI_TOOL_CHOICE=required|function|auto` is an escape hatch; `function` keeps today's object form. Fynance does not sniff hostnames.
5. **`max_tokens` comes from `FYNANCE_OPENAI_MAX_TOKENS`, default 16384.**
   - The default model is gpt-4o, and OpenAI rejects a value above a model's output cap, so the Anthropic value of 64000 cannot be the default.
   - `0` means the field is not sent.
   - A local server would set something like 64000.
6. **Sampling and server-specific knobs.**
   - `FYNANCE_OPENAI_TEMPERATURE` is optional. When unset, nothing is sent, because some OpenAI reasoning models reject a non-default temperature.
   - `FYNANCE_OPENAI_EXTRA_BODY` is a JSON object merged into every request. It is the generic way to turn Qwen thinking off (`{"chat_template_kwargs":{"enable_thinking":false}}`) or set `top_k`. A model that wants `max_completion_tokens` can set `MAX_TOKENS=0` and pass it here.
   - Reserved keys (`model`, `messages`, `tools`, `tool_choice`, `stream`, `stream_options`) and non-object JSON are rejected at startup.
7. **Streaming is on by default (`FYNANCE_OPENAI_STREAM=true`).**
   - Streaming is the reliable way to make llama-server stop generating when Fynance gives up or the caller goes away.
   - It also gives the progress UI parity with Anthropic.
   - `false` keeps the non-streaming path.
   - The timeout stays a total deadline (`FYNANCE_OPENAI_TIMEOUT_SECS`, default 300, unchanged).
8. **Typed errors.**
   - `ProviderError::Truncated { output_tokens }` maps to the existing 400 `response_truncated`, with the existing "split your upload" message.
   - PDF conversion problems map to:
     - **`NotSupported`** (existing 400 `provider_not_supported`) when the tool is not configured or cannot be found. The message names `FYNANCE_PDF_EXTRACT_CMD` and the `native` mode.
     - **A new `ProviderError::DocumentConversion(String)`**, mapped to 400 `document_conversion_failed`, when the tool runs but fails: non-zero exit, timeout, unreadable or encrypted PDF, output not matching the schema, a page image path escaping the output directory, or more pages than the page cap. This new error code is documented in `docs/api.html`.
9. **A malformed row drops that row, not the whole document, and every drop is surfaced.**
   - Each drop is reported in `metadata.notes`, which the import UI already shows (`metadata_header.tsx`).
   - Amount and date parsing becomes moderately lenient, but never guesses on ambiguous formats.
   - Unknown or parent category ids are cleared to "uncategorised" at parse time, with a note, rather than rejected at commit.
10. **Optional raw-output dump.** `FYNANCE_PARSE_DUMP_DIR` is off by default. When set, it writes each unified call's raw tool JSON to a local directory for diagnosis and scoring. This is the user's financial data, so it is documented as such.
11. **Branching.**
    - Work on a new branch stacked on `feat/openai-base-url` (suggested name `feat/openai-parity`). Do not grow PR #138.
    - Rebase onto `master` after #138 merges.
    - Work only in the `C:\w\fynance\...` worktrees. Never build in the owner's main checkout, where their `cargo watch` dev server runs.
12. **Not changing:**
    - agent overrides (still rejected for OpenAI)
    - pricing and `ParserCallCost.agent`
    - split-mode prompts
    - the `MAX_FILES` and size caps
    - `validate_pdf`
    - the frontend
    - the Dockerfile and CI workflows (see question 1)

## Questions for the owner (each has a recommended answer; "yes" accepts all)

1. **Should the Docker image bundle the Helm `pdf-extract` tool?**
   - **Recommended: not in this plan.** The tool's packaging (language, binary, dependencies) is not known yet, and existing long-running deployments use the Anthropic provider.
   - If the openai provider is ever run in Docker, mount the tool and set `FYNANCE_PDF_EXTRACT_CMD`. Revisit bundling as its own small change once the Helm tool ships.
2. **Stacked PRs off `feat/openai-base-url`, rather than more commits on #138?** **Recommended: yes.** One PR per part, or Parts 1–2 together and then 3–5.
3. **Accept a behaviour change for real-OpenAI users?** The changes are `tool_choice` `"required"`, streamed requests and `max_tokens` 16384. There are likely no such users today, since unified mode never worked. **Recommended: accept.**
4. **The Helm tool's output contract.** Fynance needs four things from the Helm coordinator:
   - the output file name inside `--out`, or whether the JSON is on stdout
   - the page array field names (page number, text, image path)
   - whether image paths are relative to `--out`
   - how a page without a text layer is marked

   **Recommended:** adopt the assumed shape in Part 1 as the proposal to Helm. The adapter can absorb small differences in one struct.

---

## Parts

Order and parallelism:

```
Part 1 (pdf-extract adapter) ──┐
                               ├─► Part 2 (OpenAI unified parity; wires Part 1 in) ─► Part 3 (streaming/progress/cancel) ─► Part 5 (docs)
Part 4 (parse robustness) — independent; build any time, in parallel with 1–3
```

- Part 1 only adds new files and two one-line edits, so it can be built in parallel with the start of Part 2.
- Part 2's PDF step needs Part 1 merged into the branch.
- Parts 2 and 3 both edit `provider.rs`, so they are strictly sequential, ideally by the same builder.
- Part 4 owns `unified_parser.rs` and `document_parser.rs` and touches nothing else.

**Common verification**, run in the stacked worktree's `backend/` at the end of every part:

```
cargo fmt
cargo fmt --check
cargo +1.85 clippy --all-targets -- -D warnings
cargo +1.85 test
python3 ../scripts/check_pii.py
```

Also run `python3 ../scripts/check_api_docs.py` when `docs/api.html`, `routes/docs.rs` or the router changes. No part adds routes or query params, so it should stay green.

Ground rules for tests and manual runs:

- Never point a test or manual run at a real model server, at a production deployment, or at the default serve port (7433).
- Env-mutating tests hold `ENV_LOCK` in `provider.rs`, or a file-local mutex in integration tests.
- Any test that builds an `OpenAIProvider` sets `FYNANCE_OPENAI_BASE_URL` to a loopback fake **before** constructing it. The default base URL is the real OpenAI API.
- No Docker impact in this plan: no native dependency is added, and the Dockerfile is unchanged.

---

### Part 1 — `pdf-extract` adapter: run the external tool, read its pages, clear errors

**Builder:** adapter builder. **Can start immediately.** It must be merged into the branch before Part 2 step 4.

**Owns:**

- new `backend/src/importers/pdf_extract.rs`
- a one-line `pub mod pdf_extract;` in `backend/src/importers/mod.rs`
- `backend/Cargo.toml` and `Cargo.lock` (move `tempfile = "3"` from dev to normal dependencies)
- new fixtures:
  - `backend/tests/fixtures/pdf_extract/extract.json`
  - `backend/tests/fixtures/pdf_extract/page-001.png`, `page-002.png`, `page-003.png`
  - `backend/tests/fixtures/synthetic_statement.pdf`

**Out of scope:**

- rendering or reading PDFs in Fynance
- the Helm tool itself
- provider wiring (that is Part 2)
- Docker and CI changes
- docs (that is Part 5)

**Tool contract** *(FINAL, confirmed from the Helm tool 2026-11-13; supersedes the earlier assumed shape)*. It is captured in one place in the module: the constants `OUTPUT_FILE`, the arg builder, and the `ToolOutput` serde structs.

```
invocation:  <FYNANCE_PDF_EXTRACT_CMD> <input.pdf> --out <dir> [FYNANCE_PDF_EXTRACT_ARGS...]
output:      <dir>/extract.json  plus page-NNN.png (NNN = REAL page number, zero-padded)
exit codes:  0 ok; 10 finished with some pages failed (extract.json IS written);
             1 unexpected error, 2 usage, 3 file not found,
             4 password-protected with no password, 5 wrong password,
             6 not a readable PDF, 7 cannot write the output folder.
             Every non-zero code other than 10 writes no extract.json; the
             reason is on stderr as "pdf-extract: error: <reason>".
```

```json
{
  "schema_version": 1,
  "page_count": 3,
  "pages": [
    {
      "page": 1,
      "text": "…text layer, always a string, may be empty…",
      "has_text_layer": true,
      "image": { "path": "page-001.png", "width_px": 1240, "height_px": 1754 },
      "numbers": [ { "raw": "£1,234.50", "value": "1234.50", "currency": "GBP" } ],
      "dates": [ { "raw": "2026-11-02", "iso": "2026-11-02" } ],
      "width_pt": 595.0,
      "height_pt": 842.0,
      "error": null
    }
  ],
  "tool": {}, "source": "…", "document": {}, "options": {}
}
```

Fynance relies only on:

- `pages[].page`: the REAL page number. Pages are in strictly ascending order, but need NOT be 1..n: the tool's `--pages` flag skips pages, and `page_count` is the document total, not `pages.len()`. The adapter requires strictly ascending numbers, not contiguity.
- `pages[].text` (always a string; a missing value or null is treated as empty)
- `pages[].image.path` (relative to `--out`, `..`-free by contract; still validated defensively). `image` can be `null` (`--no-images`): the adapter returns empty png bytes for that page.
- `pages[].has_text_layer`, which the tool always sets. The adapter keeps the fallback: when it is missing, a page has a text layer if its text has at least 20 non-whitespace characters.
- `pages[].error`: null/absent on success; a non-null string or object means that page failed.

Exit 10 is a **partial success**: the adapter returns the good pages plus their errors (`ExtractedPdf::page_errors: Vec<(u32, String)>`); every page failed is `BadOutput`. Exit codes 4/5 get a `Failed` message that says the PDF is password protected. Unknown fields are ignored (within schema_version 1 the tool only adds fields). `numbers` and `dates` are heuristics; no Fynance code reads them.

**Public API:**

- `PdfExtractConfig { cmd: Option<PathBuf>, extra_args: Vec<String>, timeout: Duration }`
  - `PdfExtractConfig::from_env() -> anyhow::Result<Self>` validates that `FYNANCE_PDF_EXTRACT_ARGS` is a JSON array of strings and that the timeout is a whole number.
- `pub async fn extract_pdf(bytes: &[u8], filename: &str, cfg: &PdfExtractConfig) -> Result<ExtractedPdf, PdfExtractError>`
  - `ExtractedPdf { pages: Vec<ExtractedPage>, page_errors: Vec<(u32, String)> }`, `page_errors` non-empty only on an exit-10 partial success
  - `ExtractedPage { page: u32, text: String, has_text_layer: bool, png: Vec<u8> }` (`png` empty when `image` is null)

**Steps (tests first):**

1. `PdfExtractError` has these variants:
   - `NotConfigured`
   - `NotFound { cmd }` (spawn failed with not-found or permission errors)
   - `Failed { status, stderr_tail }`
   - `TimedOut { secs }`
   - `BadOutput(String)` (missing or invalid JSON, schema mismatch, a missing or unreadable PNG, a PNG without the PNG signature, a path that is absolute or contains `..`, page numbers not strictly ascending, or an exit-10 run where every page failed)

   Messages are user-facing:
   - `NotConfigured` / `NotFound` name `FYNANCE_PDF_EXTRACT_CMD` and the `FYNANCE_OPENAI_PDF_MODE=native` alternative.
   - `Failed` includes the last 500 characters of stderr.
   - `BadOutput` says the tool's output did not match the expected format and names the field.
2. Run the tool:
   - Create a `tempfile::TempDir`, write the PDF to `input.pdf` inside it, and create the `out/` directory.
   - Run `tokio::process::Command` with `kill_on_drop(true)`, args `[input.pdf, "--out", out, extra_args...]`, and stdin set to null.
   - Capture stdout and stderr, and wrap the call in `tokio::time::timeout`.
   - On timeout, the child is killed when it is dropped.
   - The TempDir is removed on every path, success or error.
3. Parse `out/extract.json` into `ToolOutput`, check the page numbering, resolve each `image.path` inside `out/` only, and read the PNG bytes.
4. Never log page text or file contents. Log only the page count, total PNG bytes and duration at `debug`.
5. Fixtures (all synthetic):
   - **`pdf_extract/extract.json`:** 3 pages. Pages 1–2 have the text of an invented "Example Bank" statement, with rows such as "Corner Shop", "Salary ACME Ltd", `£1,234.50` and `(12.00)`, ISO dates only, and nothing shaped like a sort code, IBAN, NI number or UTR. Page 3 has `"text": ""` and `"has_text_layer": false`, standing for a scanned page.
   - **Three tiny valid PNGs**, for example 8×8 grey. They may be generated by a `#[test] #[ignore]` helper, or written by hand. Nothing decodes them.
   - **`synthetic_statement.pdf`:** a small, uncompressed, hand-built 3-page PDF matching the JSON, made by a dependency-free generator in the test module. The drift guard is a normal test that asserts the committed file equals the generator's output, plus a `#[ignore]` test that regenerates it. Its page 3 holds only an embedded raster image and no text operators. It is the upload in Part 2's route test and the input for the manual smoke against the real Helm tool.
6. **Fake tool for tests.** A test helper `write_fake_tool(dir, Behaviour) -> PathBuf` writes a platform script:
   - Unix: an `sh` script with mode 0755.
   - Windows: a `.cmd` script.

   Each behaviour does one thing:
   - `Ok`: copy the fixture JSON and PNGs into the `--out` directory, taken from the third argument.
   - `Fail`: write to stderr and exit 3.
   - `Garbage`: write `extract.json` containing `not json`.
   - `MissingPng`: write the JSON but omit `page-002.png`.
   - `EscapingPath`: write the JSON with `"path": "../x.png"`.
   - `Slow`: sleep 30 s.
   - `EchoArgs`: write its argv to a file, then behave like `Ok`.
   - `PartialFail`: write the JSON with one page's `error` set, exit 10.

   Scripts get their fixture directory as a literal path baked in when they are written. They do not depend on env vars, so tests stay parallel-safe.

**Tests (in `pdf_extract.rs`; they run on Linux CI and on the owner's Windows machine):**

- `Ok` gives 3 pages, numbered 1–3:
  - page 1 text contains "Corner Shop"
  - page 3 has empty text and `has_text_layer == false`
  - every `png` starts with the PNG signature and equals the fixture bytes
  - the temp directory is gone afterwards
- `EchoArgs` with `FYNANCE_PDF_EXTRACT_ARGS=["--dpi","150"]` (set through config, not env): the received argv is exactly `input.pdf --out <dir> --dpi 150`.
- `cmd: None` gives `NotConfigured`, and the message names `FYNANCE_PDF_EXTRACT_CMD`. A nonexistent path gives `NotFound`.
- `Fail` gives `Failed`, with the stderr text included.
- `Garbage` and `MissingPng` give `BadOutput`.
- `EscapingPath` gives `BadOutput` and never reads outside `out/`.
- `Slow` with a 1 s timeout gives `TimedOut` within about 3 s.
- Missing `has_text_layer`: derived from the text length (20-character threshold, tested at the boundary, spaces not counted).
- `PartialFail`: exit 10 returns `Ok` with the two good pages and one recorded `page_errors` entry.
- Page numbers not starting at 1 (simulating `--pages`) are accepted; duplicate or out-of-order numbers give `BadOutput`.
- `from_env`: `FYNANCE_PDF_EXTRACT_ARGS='{"a":1}'` is an error, and a bad timeout is an error.
- The fixture drift guard passes.

**Done when:**

- these tests pass on Windows and in CI
- `check_pii.py` passes on the new fixtures
- the common verification is green

---

### Part 2 — OpenAI provider: unified mode, files (text, images, PDFs via Part 1), request knobs, truncation, tool-call fallback

**Builder:** provider builder. **Steps 1–3 can start in parallel with Part 1. Step 4 needs Part 1.**

**Owns:**

- `backend/src/importers/provider.rs`: the OpenAI section, the `ProviderError` additions, a small Anthropic change in `SseAccumulator::into_result`, and the tests
- two new match arms in `provider_err_to_app_error` in `backend/src/server/routes/parse.rs`
- a new integration test, `backend/tests/parse_openai_compatible.rs`

**Out of scope:**

- streaming (Part 3)
- parser leniency (Part 4)
- docs (Part 5)

**Steps (tests first):**

1. **Config in `OpenAIProvider::from_env`.** Parse and validate once. An invalid value is an `Err`, which the route turns into 400 `provider_config`, the same way `FYNANCE_OPENAI_TIMEOUT_SECS` already works.
   - `FYNANCE_OPENAI_TOOL_CHOICE`: `required` (default), `function` or `auto`.
   - `FYNANCE_OPENAI_MAX_TOKENS`: default 16384; `0` means omit.
   - `FYNANCE_OPENAI_TEMPERATURE`: optional.
   - `FYNANCE_OPENAI_EXTRA_BODY`: a JSON object; reserved keys are rejected.
   - `FYNANCE_OPENAI_PDF_MODE`: `both` (default), `images`, `text` or `native`.
   - `FYNANCE_OPENAI_PDF_MAX_PAGES`: the total across one call, default 50.
   - The `PdfExtractConfig` from Part 1.

   Also remove `#[allow(dead_code)]` from `advanced_model`. Unified mode now uses it; it is set by `FYNANCE_OPENAI_PDF_MODEL`.
2. **One request builder**, `build_request(&self, model, system, user_content, tool_name, schema) -> Value`, shared by all three trait methods. It builds:
   - a system message
   - one function tool
   - `tool_choice` per config (`"required"`, `{"type":"function","function":{"name":…}}`, or `"auto"`)
   - `max_tokens` unless it is 0
   - `temperature` when set
   - then the `EXTRA_BODY` keys, merged in last
3. **`chat_with_files_and_tools` for text and images.** This mirrors Anthropic's loop. Each file gets the same text part, "Source document filename: … set its source_file …", followed by:
   - **text, CSV, JSON or NDJSON:** a text part `--- file: {filename} ({mime}) ---\n{text}`. Decode as lossy UTF-8, as Gemini does, and do not error.
   - **`image/*`:** `{"type":"image_url","image_url":{"url":"data:<mime>;base64,…"}}`.

   The `text_supplement` goes last.
4. **PDFs** (needs Part 1):
   - **`native`:** one `{"type":"file","file":{"filename":…,"file_data":"data:application/pdf;base64,…"}}` part.
   - **`both`, `images` or `text`:**
     - Call `pdf_extract::extract_pdf`, and check that the total pages across all PDFs in the call stay within `PDF_MAX_PAGES`. If not, return `DocumentConversion` and tell the user to split the upload.
     - For each page, add a text part `Page {i} of {n} of {filename}`, then the content for the mode:
       - `both`: the page text (only if the page has a text layer), then the PNG as an `image_url`.
       - `images`: the image only.
       - `text`: the text, or the image if the page has no text layer.
   - Error mapping: `NotConfigured` and `NotFound` become `ProviderError::NotSupported(msg)`. Every other `PdfExtractError` becomes `ProviderError::DocumentConversion(msg)`.
5. **`chat_with_pdf_and_tools`** passes the bytes to `chat_with_files_and_tools` as the file `("document.pdf", "application/pdf", bytes)`. Replace `test_openai_pdf_returns_error` accordingly.
6. **Response handling.** Add `finish_reason` to `OpenAIChoice` and `content: Option<String>` to `OpenAIMessage`. Make `arguments` a `serde_json::Value`: parse it if it is a string, use it as-is if it is an object. Then, in order:
   - `finish_reason == "length"` returns `Truncated { output_tokens }`.
   - A tool call named `tool_name` returns its arguments.
   - Tool calls are present but none has that name: return `NoToolUse`.
   - No tool calls: **content fallback**:
     - strip `<think>…</think>`
     - take the body of a `<tool_call>…</tool_call>` block, else a fenced ```` ```json ```` block, else the trimmed content
     - parse it as JSON
     - if it is `{"name": tool_name, "arguments": …}`, use the arguments; if it is any other object, use it as the tool input
     - log a `warn!`
     - anything else returns `NoToolUse`, which replaces today's `ResponseUnreadable`
   - `stop_reason` maps `"length"` to `"max_tokens"` and passes any other value through.
7. **New `ProviderError` variants** `Truncated { output_tokens: u64 }` and `DocumentConversion(String)`:
   - Add both to `Display`.
   - In `provider_err_to_app_error`, map `Truncated` to 400 `response_truncated`. Move the existing message into a shared `const` that the L550 check also uses.
   - Map `DocumentConversion` to 400 `document_conversion_failed`.
   - Anthropic: in `SseAccumulator::into_result`, if the JSON is invalid and `stop_reason == "max_tokens"`, return `Truncated`.

**Tests.** Add `request_json(raw)` to the existing `serve_once` helper so tests can read the captured request body.

Unit tests in `provider.rs`:

- **Request knobs:**
  - `tool_choice` is `"required"` by default, the object form with `function`, and `"auto"` with `auto`.
  - `max_tokens` is 16384 by default and absent when set to 0.
  - `temperature` is absent unless set.
  - The thinking-off `EXTRA_BODY` is merged into the request.
  - `EXTRA_BODY` with `messages` in it, or a non-object, fails `from_env`.
- **File order:** CSV + PNG produce, in order: label, CSV text, label, an `image_url` starting `data:image/png;base64,`, then the supplement.
- **PDF modes,** using the Part 1 fake tool:
  - `both`: page 3 has an image and no text part; pages 1–2 have text and an image.
  - `images`: no page text.
  - `text`: pages 1–2 have text only; page 3 is an image.
  - `PDF_MAX_PAGES=2` returns `DocumentConversion`.
  - No tool configured returns `NotSupported` naming `FYNANCE_PDF_EXTRACT_CMD`.
  - `native` produces one `file` part and never runs the tool (a config that points at the `Fail` fake still succeeds).
- **Responses:**
  - `finish_reason:"length"` with half-written arguments returns `Truncated`.
  - Arguments sent as an object are accepted.
  - `<think>…</think><tool_call>{…}</tool_call>` content is recovered.
  - A fenced JSON block is recovered.
  - Prose only returns `NoToolUse`.
  - Anthropic `max_tokens` with truncated JSON returns `Truncated`.
- **Route mapping:** `Truncated` gives 400 `response_truncated`; `DocumentConversion` gives 400 `document_conversion_failed`.

`tests/parse_openai_compatible.rs`. Each case runs a loopback fake chat server (a std `TcpListener` thread, like `serve_once`). Env: `FYNANCE_PARSE_PROVIDER=openai`, `FYNANCE_OPENAI_BASE_URL=http://127.0.0.1:<port>/v1`, `FYNANCE_OPENAI_STREAM=false` (Part 3 adds the streamed case). Each case creates a temp DB with a profile and an account, then calls `POST /api/parse` through `build_router(…, true)` with **no** `experimental` field. Cases run one at a time under a file mutex.

1. **CSV:** the fake returns `extract_unified` with 2 transactions and 1 holding. Expect 200 with 2 transaction rows and 1 holding, and `calls[0].model` equal to the configured model.
2. **Truncated:** `finish_reason:"length"` gives 400 `response_truncated`.
3. **Synthetic PDF with the `Ok` fake tool:** expect 200. The captured chat request contains 3 `image_url` parts and the page-1 text.
4. **PDF with no tool configured:** 400 `provider_not_supported`.

**Done when:** these tests pass, the existing provider and parse tests pass, and the common verification is green.

---

### Part 3 — Streaming, progress events and cancellation for the OpenAI provider

**Builder:** the provider builder, after Part 2.

**Owns:** `provider.rs` (the OpenAI transport, a shared SSE line splitter, progress) and `tests/parse_openai_compatible.rs` (one streamed case).

**Out of scope:** changes to the `ProgressEvent` shape (no ts-rs binding changes) and the frontend.

**Steps (tests first):**

1. **Shared SSE line splitter.**
   - Buffer **bytes**, split on `\n`, trim `\r`, and decode each complete line as UTF-8.
   - Use it for the OpenAI stream **and** in `AnthropicProvider::post_messages_streaming`. This fixes the split-character bug described above.
2. **Shared progress heuristic.** Factor `SseAccumulator::extraction_progress` into a free function `streamed_extraction_progress(buf) -> (Option<String>, u64)`. Anthropic behaviour must not change; its existing tests guard this.
3. **Streaming request** (unless `FYNANCE_OPENAI_STREAM=false`):
   - Send `"stream": true, "stream_options": {"include_usage": true}`.
   - Read `data:` lines whether or not a space follows the colon. Skip `:` comments and `event:` lines. Stop at `[DONE]`.
   - An in-stream `{"error":…}` returns `UpstreamServerError`.
   - Accumulate:
     - for each `tool_calls[i].index`: the first non-empty name and the concatenated arguments
     - `delta.content`
     - the last non-null `finish_reason`
     - `usage`
     - `model`
   - Ignore `reasoning_content`.
   - Then run the same post-processing as Part 2 step 6, factored into one shared function.
4. **Errors and deadline.**
   - A body-read error with `is_timeout()` returns `Timeout`; any other returns `StreamInterrupted`.
   - The total deadline stays `.timeout(self.timeout)`.
   - Dropping the future, on timeout or when the route is dropped, closes the socket, and llama-server cancels at its next write.
5. **Progress.**
   - Add `progress: Option<(ProgressTx, Option<String>)>` and `with_progress`, as Gemini has them.
   - On the first chunk, send `LlmStart { model, input_tokens: 0 }`.
   - At most once per second, send `LlmProgress { output_tokens: <delta count, approximate>, items, section }`.
6. Keep the non-streaming path behind `FYNANCE_OPENAI_STREAM=false`.

**Tests.** Add `serve_sse_once(chunks with delays)`, which can also stall.

- Arguments split across 5 chunks, with usage and `finish_reason` in the last chunk: value, usage and `stop_reason` are all correct.
- A `£` split across two writes parses correctly. Add the same test for the Anthropic accumulator path.
- A streamed `"length"` returns `Truncated`.
- A content-only `<tool_call>` triggers the fallback.
- An in-stream error object returns `UpstreamServerError`.
- **Cancellation:**
  - The fake sends one chunk, then stalls. With `TIMEOUT_SECS=1`, the call returns `Timeout` within about 2 s, and the fake sees the connection close within 3 s.
  - Dropping the future mid-stream (`tokio::time::timeout` around the call) also closes the socket.
- **Progress:** `LlmStart` is sent, then at least one `LlmProgress` whose `items` count grows.
- **Integration:** the CSV case is streamed by default, and a separate case covers `STREAM=false`.

**Done when:** the tests pass, Anthropic's existing streaming tests pass unchanged, and the common verification is green.

---

### Part 4 — Unified parse robustness: per-row tolerance, lenient values, category check, raw dump

**Builder:** parser builder. **Independent:** it can be built in parallel with Parts 1–3.

**Owns:**

- `backend/src/importers/unified_parser.rs`
- `backend/src/importers/document_parser.rs`, only to refactor `convert_parsed_holdings` into a per-row helper and add a lenient variant. The strict function keeps its signature and its behaviour for split mode.

**Out of scope:**

- split-mode behaviour
- the shared `ParsedHoldingRow` / `ParsedInvestmentRow` structs (do not add serde attributes)
- the frontend
- commit

**Steps (tests first).** Feed crafted tool JSON through `extract_all` using the existing `provider::testing::MockProvider`.

1. **Per-row deserialisation.**
   - `UnifiedRaw` holds a `Vec<Value>` per section.
   - Before deserialising each row, convert JSON numbers to strings in these fields:
     - transactions: `amount`
     - holdings: `quantity`, `price_per_unit`, `value`
     - investments: `quantity`, `price_per_share`, `total_value`, `fee`
   - A row that still fails is dropped, with one note per section, e.g. `dropped 2 transaction rows … (first: missing field 'description')`.
   - If the top-level value is not an object, the whole parse still fails.
2. **`parse_amount_lenient`:**
   - accept a currency symbol (`£ $ € ₦`) or a 3-letter ISO code, before or after the number
   - turn a unicode minus into `-`; accept `-£5.00` and `£-5.00`
   - treat parentheses as negative
   - strip `,` only when the number uses strict thousands grouping
   - reject `1.234,56`, `CR`/`DR` suffixes and empty strings
3. **`parse_date_lenient`:**
   - accept `%Y-%m-%d`, `T` or space datetimes, fractional seconds, a trailing `Z` and RFC 3339 offsets (keep the wall-clock time)
   - **do not** accept `DD/MM/YYYY` or `MM/DD/YYYY`
4. **Rows that fail conversion are dropped with a note**, giving the count and up to 3 example values. This replaces the silent `.ok()`.
5. **Holdings.**
   - Add `convert_holding_row(row, now) -> Result<Option<Holding>>` and use it in both functions.
   - The strict function keeps `?`.
   - The new `convert_parsed_holdings_lenient(&ParsedHoldings) -> (Vec<Holding>, Vec<String>)` is used by `post_validate`, so a bad holding no longer fails the parse.
6. **Category check.**
   - Add `scrub_unknown_categories(rows, &HashSet<&str>) -> usize`, using the ids in `ctx.categories`.
   - When that set is non-empty, clear `category_id` and `category_confidence` on rows whose id is not in it, and add a note.
7. **`FYNANCE_PARSE_DUMP_DIR`.**
   - Right after the provider call, before any conversion, write `{dir}/{YYYYMMDDTHHMMSS}-{account_id}-unified.json` containing `{model, stop_reason, usage, duration_ms, value}`.
   - An I/O error only logs a `warn!`.
   - Log the path at `info`.

**Tests:**

- **Amounts:**
  - the number `-12.5` is accepted
  - `"£1,234.50"` → 1234.50
  - `"(12.00)"` → -12.00
  - `"−5.00"` → -5.00
  - `"5.00 GBP"` → 5.00
  - `"1.234,56"` is dropped with a note
- **Dates:**
  - `"12/08/2026"` is dropped with a note
  - `"2026-08-12 10:11:12"` is accepted
  - `"2026-08-12T10:11:12.5Z"` is accepted
- **Bad rows:**
  - 1 of 3 transactions missing `description`: 2 are kept, plus a note.
  - A holding with `quantity:"N/A"`: first add a test proving it fails the whole parse today; after the fix, the other rows are kept, plus a note.
- **Categories:**
  - an unknown category is cleared with a note
  - a valid category is kept
  - an empty context list means no scrubbing
- **Dump:**
  - the dump writes one file containing `value`
  - an unwritable dump directory does not fail the parse
- **Regression:** the existing `unified_parser` and `document_parser` tests stay green.

**Done when:** the tests pass and the common verification is green.

---

### Part 5 — Documentation and plan index

**Builder:** any builder, after Parts 1–4.

**Owns:**

- `.env.example`
- `backend/RUNNING.md`
- `docs/api.html` (the `/api/parse` section)
- `docs/design/08_streaming_and_error_classification.md` (the `ProviderError` list)
- `docs/plans/README.md` (a row for plan 30)

**Steps:**

1. In `.env.example` and the `RUNNING.md` env table, list every new variable with its default and a one-line effect:
   - `FYNANCE_OPENAI_TOOL_CHOICE`
   - `_MAX_TOKENS`
   - `_TEMPERATURE`
   - `_EXTRA_BODY`
   - `_STREAM`
   - `_PDF_MODE`
   - `_PDF_MAX_PAGES`
   - `FYNANCE_PDF_EXTRACT_CMD`
   - `_ARGS`
   - `_TIMEOUT_SECS`
   - `FYNANCE_PARSE_DUMP_DIR`
2. Add a generic `RUNNING.md` section, "Imports on a local OpenAI-compatible server". It must contain no deployment-specific values. It covers:
   - **server flags:** `--jinja` (tool calls) and `--mmproj` (images), plus enough context for page images
   - **an example env:** `FYNANCE_PARSE_PROVIDER=openai`, `FYNANCE_OPENAI_BASE_URL=http://127.0.0.1:8080/v1`, `FYNANCE_OPENAI_PDF_MODEL=<model id>`, `MAX_TOKENS=64000`, `TIMEOUT_SECS=1800`, `TEMPERATURE=0`, `EXTRA_BODY={"chat_template_kwargs":{"enable_thinking":false}}`
   - **PDFs:** the PDF modes, and how to install the external `pdf-extract` tool and point `FYNANCE_PDF_EXTRACT_CMD` at it on Windows, Linux and Docker (by mounting it)
   - **single-slot servers:** they run imports one at a time
3. Update `docs/api.html` for `/api/parse`:
   - Add `document_conversion_failed`.
   - `response_truncated` can now come from any provider.
   - `provider_not_supported` now also covers an unconfigured or missing pdf-extract tool.
   - `upstream_no_tool_use` covers OpenAI replies that have neither a tool call nor JSON that can be recovered.
4. Add `Truncated` and `DocumentConversion` to the design doc.
5. Add the plan 30 row to `docs/plans/README.md`.

**Done when:** `check_api_docs.py` and `check_pii.py` pass, and a new reader can configure a local server from `RUNNING.md` alone.

---

## Checks for the whole thing

1. **CI is green** on the stacked PR(s): fmt, clippy 1.85, tests, api-docs and pii-guard. Docker and actionlint do not trigger, because neither the Dockerfile nor the workflows change.
2. **Manual smoke** against a **local** OpenAI-compatible server and the **real Helm `pdf-extract` tool**, using synthetic fixtures only:
   - Set up:
     - run the worktree build from a scratch directory with no `.env` holding real credentials
     - use a port other than 7433
     - use a temp `FYNANCE_DB_PATH` (never a live database)
   - Parse each of these in default unified mode, with no `experimental`:
     - `synthetic_statement.pdf`
     - a synthetic CSV
     - one of the fixture PNGs
   - Confirm:
     - 200 responses with rows
     - progress on `/api/parse/progress/:id`
     - the server log shows thinking off and `tool_choice` honoured
     - page 3 (scanned) is sent as an image
   - Run the same parse with `FYNANCE_PDF_EXTRACT_CMD` unset: it should return 400 `provider_not_supported`.
   - Run with `FYNANCE_OPENAI_TIMEOUT_SECS=5` on the PDF: Fynance should return 504 `upstream_timeout`, and the server's `/slots` should go idle shortly after.
3. **Anthropic regression:** the existing tests, plus one manual unified parse of `synthetic_statement.pdf` with `FYNANCE_PARSE_PROVIDER=anthropic`. It should behave as before. Anthropic still receives the native PDF, never the tool's output.
4. Once the Helm tool's schema is final, update the `ToolOutput` structs and the fixture JSON, then re-run Part 1's tests and the manual smoke. That should be the only change needed.
5. The real-data benchmark lives outside the repo and is not part of this plan.

## Risks, and where reviewers should look hardest

- **The Helm tool's contract may differ from the assumed one.** Anything that depends on it lives in `pdf_extract.rs` (constants, argument builder, `ToolOutput`) and the fixture JSON. Check that no other file knows about the tool's field names.
- **Running an external process.** Look at four things:
  - `kill_on_drop` plus the timeout, so no orphaned tool processes are left behind
  - temp-directory cleanup on every path
  - path-traversal checks on image paths
  - stderr ends up in user-facing messages, but it is truncated and comes from a local tool the user configured
- **Windows `.cmd` invocation.** Rust's `Command` can run `.cmd` and `.bat` files, with argument escaping rules. The fake-tool tests prove this on the owner's machine. If the Helm tool ships as a script, document using a wrapper.
- **Tool calls not forced on reasoning templates** (llama.cpp #27217). The mitigations are the content fallback and turning thinking off. Review the fallback for over-eager acceptance: it should accept JSON objects only, and prose must produce `NoToolUse`, not an empty extraction.
- **Token and request size.** 40 page images at about 150 DPI come to roughly 80–120k prompt tokens and a request body over 10 MB. The page cap counts across all PDFs in a call; check this. DPI is set through `FYNANCE_PDF_EXTRACT_ARGS`.
- **Streaming edge cases.** Split UTF-8, `data:` with no space after the colon, `\r\n` line endings, usage arriving in a final chunk of its own, and multiple tool-call indices. The shared splitter now carries Anthropic too, so give it a close look.
- **Lenient amounts flipping a sign or scale.** The thousands-separator rule must be strict and parentheses must mean negative; CR and DR are rejected on purpose. Review the tables in the tests.
- **Category scrubbing hiding model errors.** The note counts must be accurate, and scrubbing must be skipped when the context list is empty.
- **The raw dump writes personal financial data to disk.** It is opt-in, logged when used, and never on by default.
- **Behaviour changes for real-OpenAI users.** The escape-hatch env vars cover them.
- **Env-mutating tests racing.** Use `ENV_LOCK` or the file mutex, and always set a loopback base URL before building the provider, so no test can reach api.openai.com.
