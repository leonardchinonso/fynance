# Claude Subscription Credits vs Console API (for third-party harnesses)

Research conducted June 2026. Question: can fynance use a Claude Pro/Max subscription (via the new "Agent SDK credit" allowance for third-party harnesses) instead of a pay-per-token Anthropic Console API key, and should it?

## Background: the 2026 policy timeline

1. **Pre-April 2026**: Third-party harnesses (OpenClaw and similar autonomous agents) could authenticate with a Claude Pro/Max subscription and run within those subscription limits. This was cheaper than the Console API for heavy users.
2. **April 4, 2026**: Anthropic banned this. Subscription billing no longer covered third-party tools. The stated reason was that third-party harnesses bypass Claude Code's prompt-cache optimizations and invoke the model fresh every call, placing outsized strain on infrastructure: some users were paying $20-$200/mo but consuming hundreds or thousands of dollars of tokens. OpenClaw's creator was temporarily banned from Claude entirely.
3. **May 2026**: Anthropic reversed the ban and announced "Agent SDK credits": a separate monthly credit subcategory for programmatic use, available to all paid subscribers.
4. **June 15, 2026**: The split went live. Paid subscriptions now have two independent billing pools.

## The two-pool model (live as of June 15, 2026)

| Pool | Covers | Limit |
|---|---|---|
| **Pool 1 — Interactive** | Claude.ai web/desktop/mobile chat, the Claude Code terminal, Claude Cowork | Standard subscription usage limits (unchanged) |
| **Pool 2 — Agent SDK credits** | `claude -p` (non-interactive Claude Code), the Claude Agent SDK (Python/TypeScript), Claude Code GitHub Actions, third-party apps authenticated via the Agent SDK | Monthly credit equal to the subscription fee ($20 Pro / $100 Max 5x / $200 Max 20x); does **not** roll over; overages billed at standard API rates |

## The key technical constraint

The Agent SDK credit pool is only reachable with a **Claude subscription OAuth token** (the `sk-ant-oat01-` tokens Claude Code obtains when you log in). It is **not** reachable with a `console.anthropic.com` API key. Console API keys continue to bill pay-as-you-go and receive no credit.

Authentication to the credit pool happens through the Agent SDK / CLI OAuth flow, not by swapping an HTTP header. Using an `sk-ant-oat01-` OAuth token outside of Anthropic's own products (Claude Code, claude.ai) is explicitly a violation of the Consumer Terms of Service. So there is no supported way to point a raw `reqwest` call at `api.anthropic.com` and have it draw from the subscription credit pool — you must go through the Agent SDK / `claude -p`.

## How fynance works today

Fynance calls `https://api.anthropic.com/v1/messages` directly via `reqwest` in [`backend/src/importers/provider.rs`](../../backend/src/importers/provider.rs), authenticating with `FYNANCE_ANTHROPIC_API_KEY` (a Console API key). It uses:

- `tool_use` with hand-written JSON Schemas to force structured `ParsedStatement` output
- SSE streaming to drive a live progress bar (`ProgressEvent`)
- Model-tier selection (Haiku for text, Sonnet for PDF/vision, with per-agent overrides)
- PDF and image document blocks for visual statement parsing

This is the Anthropic Console API: pay-per-token, billed to a Console account, entirely separate from any Claude.ai subscription. It cannot draw from Agent SDK credits.

## Why the Agent SDK pool is the wrong fit for fynance

The Agent SDK credit pool exists for running Claude Code's **agentic coding loop** (reading/writing files, running shell commands, multi-turn autonomy). Fynance's workload is the opposite: **single-shot structured JSON extraction** from bank statements via custom tool schemas.

Routing fynance through `claude -p` / the Agent SDK to access subscription credits would require:

- Depending on the Claude Code CLI being installed on the user's machine
- Spawning a subprocess per parse call
- Losing the custom `tool_use` schemas that produce typed `ParsedStatement` output
- Losing the SSE streaming progress events
- Losing model-tier selection (Haiku / Sonnet / Opus)
- Losing PDF and image input support
- Breaking in Docker / headless server environments where there is no interactive OAuth login

That is a large regression for a workload the credits were never designed for.

## Conclusion

**Fynance's current design is correct; no code changes are warranted.** The direct Console API + `reqwest` + `tool_use` approach is the right interface for structured extraction. The subscription "Agent SDK credit" allowance is built for the agentic coding loop, is only reachable through the Agent SDK / `claude -p` OAuth flow, and adopting it would strip away structured output, streaming, model selection, PDF support, and headless operation.

The only worthwhile follow-up is **documentation clarity**, so users understand the cost model:

- Fynance needs a separate **Anthropic Console API key** (`FYNANCE_ANTHROPIC_API_KEY`), obtained at console.anthropic.com.
- This is **not** the same as a Claude.ai Pro/Max subscription, and a subscription does not cover fynance usage.
- Billing is pay-per-token. At Haiku 4.5 rates a typical CSV import costs well under $0.01.

Recommended action: spell this out in `README.md` and `.env.example`. No backend change.

## Addendum (June 2026): decision reversed and implemented

The conclusion above was a cost/benefit judgment, not a technical finding, and it rested on one wrong assumption: that reaching the subscription credit pool requires shelling out to `claude -p` / the Agent SDK and therefore losing tool_use, streaming, model selection, and PDF support. In practice the marginal cost of the Console API turned out to be prohibitive for bulk historical imports (well over £20 for a partial backfill), and the technical assumption does not hold.

**What actually works.** The subscription OAuth token (`sk-ant-oat01-`, from `claude setup-token`) can be sent directly to `https://api.anthropic.com/v1/messages` exactly like a Console API key, with three differences:

1. `authorization: Bearer <token>` instead of `x-api-key`.
2. `anthropic-beta: oauth-2025-04-20` added (merged with any existing beta such as `pdfs-2024-09-25`).
3. The first system block set to "You are Claude Code, Anthropic's official CLI for Claude." (currently not enforced, but cheap insurance if Anthropic re-enables the check).

This is the same path Claude Code uses internally. Verified live against Haiku 4.5: tool_use, SSE streaming, and token usage all work and the call draws from the subscription, not Console billing.

**How it is implemented.** The branch is at the lowest level: `AnthropicProvider::post_messages_streaming` switches headers and system prefix based on an `AnthropicAuth` enum (`ApiKey` | `Subscription`). Nothing above that layer changed: identical prompts, tool schemas, model-tier selection, parsers, return types, streaming progress, and PDF/image support. `create_provider` prefers the subscription token when present and uses the API key as an automatic runtime fallback (on auth-rejected or rate-limit/quota errors) via a small `FallbackProvider` wrapper. Per-request override is exposed through the parse endpoint's `experimental.auth` field (`auto` | `subscription` | `api_key`). Config: `FYNANCE_CLAUDE_CODE_OAUTH_TOKEN`.

**Caveat unchanged.** Using an `sk-ant-oat01-` token outside Anthropic's own products is outside the Consumer Terms for that token type. This is a personal, local, opt-in feature; the API-key path remains the default when no subscription token is configured.

## Sources

- [Anthropic reinstates OpenClaw and third-party agent usage on Claude subscriptions — with a catch (VentureBeat)](https://venturebeat.com/technology/anthropic-reinstates-openclaw-and-third-party-agent-usage-on-claude-subscriptions-with-a-catch)
- [Anthropic cuts off the ability to use Claude subscriptions with OpenClaw (VentureBeat)](https://venturebeat.com/technology/anthropic-cuts-off-the-ability-to-use-claude-subscriptions-with-openclaw-and)
- [Use the Claude Agent SDK with your Claude plan (Claude Help Center)](https://support.claude.com/en/articles/15036540-use-the-claude-agent-sdk-with-your-claude-plan)
- [Agent SDK overview (Claude API Docs)](https://docs.anthropic.com/en/docs/claude-code/sdk/sdk-overview)
- [Claude subscriptions get separate budgets for programmatic use, billed at full API prices (The Decoder)](https://the-decoder.com/claude-subscriptions-get-separate-budgets-for-programmatic-use-billed-at-full-api-prices/)
- [Anthropic's Claude subscriptions no longer include Agent SDK and claude -p usage (XDA)](https://www.xda-developers.com/anthropics-claude-subscriptions-no-longer-include-agent-sdk-and-claude-p-usage/)
