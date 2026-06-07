//! LLM cost computation. Prices per million tokens from
//! <https://platform.claude.com/docs/en/about-claude/pricing>.

use rust_decimal::Decimal;

use crate::importers::provider::ProviderCallResult;
use crate::model::{Agent, EstimatedPrice, ParserCallCost};

struct AgentRates {
    input_per_mtok: Decimal,
    output_per_mtok: Decimal,
}

fn rates_for(agent: Agent) -> AgentRates {
    match agent {
        Agent::Haiku => AgentRates {
            input_per_mtok: Decimal::from(1u64),
            output_per_mtok: Decimal::from(5u64),
        },
        Agent::Sonnet => AgentRates {
            input_per_mtok: Decimal::from(3u64),
            output_per_mtok: Decimal::from(15u64),
        },
        Agent::Opus => AgentRates {
            input_per_mtok: Decimal::from(5u64),
            output_per_mtok: Decimal::from(25u64),
        },
    }
}

/// `None` for non-frontier model ids; caller decides the fallback.
pub fn agent_from_model(model: &str) -> Option<Agent> {
    let m = model.to_ascii_lowercase();
    if m.contains("haiku") {
        Some(Agent::Haiku)
    } else if m.contains("sonnet") {
        Some(Agent::Sonnet)
    } else if m.contains("opus") {
        Some(Agent::Opus)
    } else {
        None
    }
}

pub fn cost_for(agent: Agent, input_tokens: u64, output_tokens: u64) -> Decimal {
    let rates = rates_for(agent);
    let million = Decimal::from(1_000_000u64);
    (rates.input_per_mtok * Decimal::from(input_tokens)
        + rates.output_per_mtok * Decimal::from(output_tokens))
        / million
}

pub fn parser_call_cost(parser_id: &str, call: &ProviderCallResult) -> ParserCallCost {
    let agent = agent_from_model(&call.model).unwrap_or_else(|| {
        tracing::warn!(
            model = %call.model,
            parser = parser_id,
            "model id does not match a known frontier family; cost reporting will use Sonnet rates as a fallback",
        );
        Agent::Sonnet
    });
    let amount = cost_for(agent, call.usage.input_tokens, call.usage.output_tokens);
    ParserCallCost {
        parser: parser_id.to_string(),
        agent,
        model: call.model.clone(),
        input_tokens: call.usage.input_tokens,
        output_tokens: call.usage.output_tokens,
        duration_ms: call.duration_ms,
        amount,
        currency: "USD".to_string(),
    }
}

pub fn estimated_price(calls: Vec<ParserCallCost>) -> EstimatedPrice {
    let total = calls.iter().map(|c| c.amount).sum();
    EstimatedPrice {
        calls,
        total,
        currency: "USD".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[test]
    fn test_agent_from_model_matches() {
        assert_eq!(
            agent_from_model("claude-haiku-4-5-20251001"),
            Some(Agent::Haiku)
        );
        assert_eq!(agent_from_model("claude-sonnet-4-6"), Some(Agent::Sonnet));
        assert_eq!(agent_from_model("claude-opus-4-7"), Some(Agent::Opus));
    }

    #[test]
    fn test_agent_from_model_unknown_is_none() {
        assert_eq!(agent_from_model("gpt-4o-mini"), None);
        assert_eq!(agent_from_model("mock"), None);
    }

    #[test]
    fn test_cost_for_haiku() {
        // 1M input @ $1, 500k output @ $5 => 1.00 + 2.50 = 3.50
        let cost = cost_for(Agent::Haiku, 1_000_000, 500_000);
        assert_eq!(cost, d("3.50"));
    }

    #[test]
    fn test_cost_for_sonnet() {
        // 100k input @ $3/M, 50k output @ $15/M => 0.30 + 0.75 = 1.05
        let cost = cost_for(Agent::Sonnet, 100_000, 50_000);
        assert_eq!(cost, d("1.05"));
    }

    #[test]
    fn test_cost_for_opus() {
        // 200k input @ $5/M, 20k output @ $25/M => 1.00 + 0.50 = 1.50
        let cost = cost_for(Agent::Opus, 200_000, 20_000);
        assert_eq!(cost, d("1.50"));
    }

    #[test]
    fn test_estimated_price_totals() {
        let calls = vec![
            ParserCallCost {
                parser: "csv_transactions".into(),
                agent: Agent::Haiku,
                model: "claude-haiku-4-5-20251001".into(),
                input_tokens: 100,
                output_tokens: 50,
                duration_ms: 1234,
                amount: d("0.10"),
                currency: "USD".into(),
            },
            ParserCallCost {
                parser: "unified".into(),
                agent: Agent::Sonnet,
                model: "claude-sonnet-4-6".into(),
                input_tokens: 1000,
                output_tokens: 500,
                duration_ms: 5678,
                amount: d("0.25"),
                currency: "USD".into(),
            },
        ];
        let price = estimated_price(calls);
        assert_eq!(price.total, d("0.35"));
        assert_eq!(price.calls.len(), 2);
        assert_eq!(price.currency, "USD");
    }
}
