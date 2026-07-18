//! Aggregate Claude plan usage exported by Claude Desktop.
//!
//! The history contains timestamps and utilization percentages only.
//! Conversation content is never read.

use chrono::{DateTime, Utc};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeRateWindow {
    pub used_percent: f64,
    pub window_minutes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeUsage {
    pub observed_at: DateTime<Utc>,
    /// Shortest window first.
    pub windows: Vec<ClaudeRateWindow>,
}

pub fn parse_claude_usage_history(contents: &str) -> Option<ClaudeUsage> {
    let value = serde_json::from_str::<Value>(contents).ok()?;
    value
        .get("samples")?
        .as_array()?
        .iter()
        .filter_map(parse_sample)
        .max_by_key(|usage| usage.observed_at)
}

fn parse_sample(sample: &Value) -> Option<ClaudeUsage> {
    let observed_at = DateTime::from_timestamp_millis(sample.get("t")?.as_i64()?)?;
    let usage = sample.get("u").unwrap_or(sample);
    let mut windows = [("fh", 300), ("sd", 10_080)]
        .into_iter()
        .filter_map(|(key, window_minutes)| {
            let used_percent = usage.get(key)?.as_f64()?;
            (used_percent.is_finite() && used_percent >= 0.0).then_some(ClaudeRateWindow {
                used_percent,
                window_minutes,
            })
        })
        .collect::<Vec<_>>();
    if windows.is_empty() {
        return None;
    }
    windows.sort_by_key(|window| window.window_minutes);
    Some(ClaudeUsage {
        observed_at,
        windows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_latest_v1_or_v2_plan_sample() {
        let v2 = r#"{"version":2,"samples":[
            {"t":1784371000000,"u":{"fh":12,"sd":47,"future":99}},
            {"t":1784372000000,"org":"ignored","u":{"fh":100,"sd":48}}
        ]}"#;
        let usage = parse_claude_usage_history(v2).unwrap();
        assert_eq!(usage.observed_at.timestamp_millis(), 1_784_372_000_000);
        assert_eq!(
            usage.windows,
            vec![
                ClaudeRateWindow {
                    used_percent: 100.0,
                    window_minutes: 300,
                },
                ClaudeRateWindow {
                    used_percent: 48.0,
                    window_minutes: 10_080,
                },
            ]
        );

        let v1 = r#"{"version":1,"samples":[{"t":1784372000000,"fh":7,"sd":9}]}"#;
        assert_eq!(
            parse_claude_usage_history(v1).unwrap().windows[0].used_percent,
            7.0
        );
        assert!(parse_claude_usage_history("not json").is_none());
    }
}
