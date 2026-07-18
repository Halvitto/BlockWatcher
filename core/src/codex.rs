//! Codex rate-limit snapshots from local session logs.
//!
//! Only timestamps and numeric limit windows from `token_count` events are
//! retained. Conversation fields are ignored during typed deserialization.

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer};

use crate::parse::{
    FileCursor, index_requires_rebuild, lenient_object, lenient_string, read_new_lines,
};

#[derive(Debug, Clone, PartialEq)]
pub struct CodexRateWindow {
    pub used_percent: f64,
    pub window_minutes: u64,
    pub resets_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodexUsage {
    pub observed_at: DateTime<Utc>,
    pub plan_type: Option<String>,
    /// Shortest window first.
    pub windows: Vec<CodexRateWindow>,
}

#[derive(Debug, PartialEq)]
pub enum CodexParsedLine {
    Usage(CodexUsage),
    Skipped,
    Malformed,
}

#[derive(Deserialize)]
struct RawLine {
    #[serde(default, deserialize_with = "lenient_string")]
    timestamp: Option<String>,
    #[serde(default, deserialize_with = "lenient_object")]
    payload: Option<RawPayload>,
}

#[derive(Deserialize)]
struct RawPayload {
    #[serde(default, rename = "type", deserialize_with = "lenient_string")]
    kind: Option<String>,
    #[serde(default, deserialize_with = "lenient_object")]
    rate_limits: Option<RawRateLimits>,
}

#[derive(Deserialize)]
struct RawRateLimits {
    #[serde(default, deserialize_with = "lenient_string")]
    plan_type: Option<String>,
    #[serde(default, deserialize_with = "lenient_object")]
    primary: Option<RawWindow>,
    #[serde(default, deserialize_with = "lenient_object")]
    secondary: Option<RawWindow>,
}

#[derive(Deserialize)]
struct RawWindow {
    #[serde(default, deserialize_with = "lenient_f64")]
    used_percent: Option<f64>,
    #[serde(default, deserialize_with = "lenient_u64")]
    window_minutes: Option<u64>,
    #[serde(default, deserialize_with = "lenient_i64")]
    resets_at: Option<i64>,
}

pub fn parse_codex_line(line: &str) -> CodexParsedLine {
    let line = line.trim();
    if line.is_empty() {
        return CodexParsedLine::Skipped;
    }
    let Ok(raw) = serde_json::from_str::<RawLine>(line) else {
        return CodexParsedLine::Malformed;
    };
    let Some(payload) = raw.payload else {
        return CodexParsedLine::Skipped;
    };
    if payload.kind.as_deref() != Some("token_count") {
        return CodexParsedLine::Skipped;
    }
    let Some(rate_limits) = payload.rate_limits else {
        return CodexParsedLine::Skipped;
    };

    let mut windows = [rate_limits.primary, rate_limits.secondary]
        .into_iter()
        .flatten()
        .filter_map(parse_window)
        .collect::<Vec<_>>();
    if windows.is_empty() {
        return CodexParsedLine::Skipped;
    }
    windows.sort_by_key(|window| window.window_minutes);

    let Some(observed_at) = raw
        .timestamp
        .as_deref()
        .and_then(|timestamp| DateTime::parse_from_rfc3339(timestamp).ok())
        .map(|timestamp| timestamp.with_timezone(&Utc))
    else {
        return CodexParsedLine::Malformed;
    };

    CodexParsedLine::Usage(CodexUsage {
        observed_at,
        plan_type: rate_limits.plan_type,
        windows,
    })
}

fn parse_window(value: RawWindow) -> Option<CodexRateWindow> {
    let used_percent = value.used_percent?;
    let window_minutes = value.window_minutes?;
    let resets_at = value.resets_at?;
    if used_percent < 0.0 || window_minutes == 0 {
        return None;
    }
    Some(CodexRateWindow {
        used_percent,
        window_minutes,
        resets_at: DateTime::from_timestamp(resets_at, 0)?,
    })
}

fn lenient_f64<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(d)?;
    Ok(value.as_ref().and_then(serde_json::Value::as_f64))
}

fn lenient_u64<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(d)?;
    Ok(value.as_ref().and_then(serde_json::Value::as_u64))
}

fn lenient_i64<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(d)?;
    Ok(value.as_ref().and_then(serde_json::Value::as_i64))
}
#[derive(Debug, Default)]
pub struct CodexLogIndex {
    offsets: HashMap<PathBuf, FileCursor>,
    latest: Option<CodexUsage>,
    malformed: u64,
}

impl CodexLogIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn latest(&self) -> Option<&CodexUsage> {
        self.latest.as_ref()
    }

    pub fn malformed_lines(&self) -> u64 {
        self.malformed
    }

    pub fn refresh<I>(&mut self, files: I) -> usize
    where
        I: IntoIterator<Item = PathBuf>,
    {
        let mut added = 0;
        let Self {
            offsets,
            latest,
            malformed,
        } = self;
        let files = files.into_iter().collect::<Vec<_>>();
        if index_requires_rebuild(offsets, &files) {
            offsets.clear();
            *latest = None;
            *malformed = 0;
        }
        read_new_lines(offsets, files, |line| match parse_codex_line(line) {
            CodexParsedLine::Usage(usage) => {
                if latest
                    .as_ref()
                    .is_none_or(|current| usage.observed_at >= current.observed_at)
                {
                    *latest = Some(usage);
                }
                added += 1;
            }
            CodexParsedLine::Skipped => {}
            CodexParsedLine::Malformed => *malformed += 1,
        });
        added
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_orders_codex_limit_windows() {
        let line = r#"{"timestamp":"2026-07-18T02:31:55.817Z","payload":{"type":"token_count","rate_limits":{"plan_type":"plus","primary":{"used_percent":25.0,"window_minutes":300,"resets_at":1784343600},"secondary":{"used_percent":58.0,"window_minutes":10080,"resets_at":1784808941}}}}"#;
        let CodexParsedLine::Usage(usage) = parse_codex_line(line) else {
            panic!("expected usage");
        };

        assert_eq!(usage.plan_type.as_deref(), Some("plus"));
        assert_eq!(usage.windows.len(), 2);
        assert_eq!(usage.windows[0].window_minutes, 300);
        assert_eq!(usage.windows[0].used_percent, 25.0);
        assert_eq!(usage.windows[1].window_minutes, 10_080);

        let path = std::env::temp_dir().join(format!("blockwatcher-codex-{}", std::process::id()));
        std::fs::write(&path, format!("{line}\n")).unwrap();
        let mut index = CodexLogIndex::new();
        assert_eq!(index.refresh([path.clone()]), 1);
        assert_eq!(index.refresh([path.clone()]), 0);
        assert_eq!(index.latest().unwrap().windows.len(), 2);
        let _ = std::fs::remove_file(path);
    }
}
