//! Daily token activity emitted by the bundled `ccusage` sidecar.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::TokenCounts;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelActivity {
    pub name: String,
    pub tokens: TokenCounts,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentActivity {
    pub id: String,
    pub total_tokens: u64,
    pub tokens: TokenCounts,
    pub models: Vec<ModelActivity>,
}

#[derive(Default)]
struct ActivityAccumulator {
    total_tokens: u64,
    tokens: TokenCounts,
    models: BTreeMap<String, TokenCounts>,
}

/// Parses the requested local day and skips malformed rows without failing the
/// rest of the report. Invalid JSON remains an error so callers can preserve
/// their last valid snapshot.
pub fn parse_ccusage_daily(
    contents: &str,
    expected_day: &str,
) -> Result<Vec<AgentActivity>, serde_json::Error> {
    let report = serde_json::from_str::<Value>(contents)?;
    let mut agents = BTreeMap::<String, ActivityAccumulator>::new();

    for row in report
        .get("daily")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|row| row.get("period").and_then(Value::as_str) == Some(expected_day))
    {
        for agent in row
            .get("agents")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(id) = agent.get("agent").and_then(Value::as_str) else {
                continue;
            };
            let entry = agents.entry(id.to_string()).or_default();
            let tokens = token_counts(agent);
            entry.tokens.add(&tokens);
            entry.total_tokens = entry
                .total_tokens
                .saturating_add(u64_field(agent, "totalTokens").unwrap_or_else(|| tokens.total()));

            for model in agent
                .get("modelBreakdowns")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let Some(name) = model.get("modelName").and_then(Value::as_str) else {
                    continue;
                };
                entry
                    .models
                    .entry(name.to_string())
                    .or_default()
                    .add(&token_counts(model));
            }
        }
    }

    Ok(agents
        .into_iter()
        .map(|(id, activity)| AgentActivity {
            id,
            total_tokens: activity.total_tokens,
            tokens: activity.tokens,
            models: activity
                .models
                .into_iter()
                .map(|(name, tokens)| ModelActivity { name, tokens })
                .collect(),
        })
        .collect())
}

fn token_counts(value: &Value) -> TokenCounts {
    TokenCounts {
        input: u64_field(value, "inputTokens").unwrap_or_default(),
        output: u64_field(value, "outputTokens").unwrap_or_default(),
        cache_creation: u64_field(value, "cacheCreationTokens").unwrap_or_default(),
        cache_read: u64_field(value, "cacheReadTokens").unwrap_or_default(),
    }
}

fn u64_field(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64)
}
