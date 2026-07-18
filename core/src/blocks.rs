//! 5-hour billing-block computation.
//!
//! `identify_blocks` is a direct port of ccusage's `identify_session_blocks`
//! (MIT, <https://github.com/ryoppippi/ccusage>, rust/crates/ccusage/src/blocks.rs)
//! so block boundaries match `ccusage blocks` exactly:
//! - a block starts at the first entry's timestamp floored to the UTC hour
//!   and lasts the session duration (default 5h);
//! - a new block starts when an entry falls strictly more than the duration
//!   after the block start, or after the previous entry;
//! - an idle stretch longer than the duration yields a synthetic gap block
//!   (start = last entry + duration, end = next entry);
//! - a block is active when now is before its end and within the duration
//!   of its last entry.

use std::collections::BTreeMap;

use chrono::{DateTime, DurationRound, TimeDelta, Utc};

use crate::parse::{TokenCounts, UsageEntry};

/// Claude Code usage windows are 5 hours long.
pub const DEFAULT_SESSION_HOURS: f64 = 5.0;

#[derive(Debug, Clone)]
pub struct SessionBlock {
    pub start: DateTime<Utc>,
    /// Nominal end (start + session duration); for gap blocks, the next entry's time.
    pub end: DateTime<Utc>,
    /// Timestamp of the last entry, if any.
    pub actual_end: Option<DateTime<Utc>>,
    /// Timestamp of the first entry, if any (burn rate is measured from here).
    pub first_entry: Option<DateTime<Utc>>,
    pub is_active: bool,
    pub is_gap: bool,
    pub entry_count: usize,
    pub tokens: TokenCounts,
    /// Per-model token totals (entries without a model id count only in `tokens`).
    pub per_model: BTreeMap<String, TokenCounts>,
}

impl SessionBlock {
    /// Time until this block's reset, clamped to zero.
    pub fn remaining(&self, now: DateTime<Utc>) -> TimeDelta {
        (self.end - now).max(TimeDelta::zero())
    }
}

/// Group usage entries into session blocks. `now` decides which block is active
/// (pass `Utc::now()` outside tests). Entries need not be sorted.
pub fn identify_blocks(
    mut entries: Vec<UsageEntry>,
    session_hours: f64,
    now: DateTime<Utc>,
) -> Vec<SessionBlock> {
    if entries.is_empty() {
        return Vec::new();
    }
    let duration = TimeDelta::milliseconds((session_hours * 3_600_000.0) as i64);
    entries.sort_by_key(|e| e.timestamp);

    let mut blocks = Vec::new();
    let mut current_start: Option<DateTime<Utc>> = None;
    let mut current: Vec<UsageEntry> = Vec::new();

    for entry in entries {
        if let Some(start) = current_start {
            let last_time = current.last().map(|e| e.timestamp).unwrap_or(start);
            let since_start = entry.timestamp - start;
            let since_last = entry.timestamp - last_time;
            if since_start > duration || since_last > duration {
                blocks.push(create_block(
                    start,
                    std::mem::take(&mut current),
                    now,
                    duration,
                ));
                if since_last > duration {
                    blocks.push(gap_block(last_time, entry.timestamp, duration));
                }
                current_start = Some(floor_to_hour(entry.timestamp));
            }
        } else {
            current_start = Some(floor_to_hour(entry.timestamp));
        }
        current.push(entry);
    }

    if let Some(start) = current_start {
        if !current.is_empty() {
            blocks.push(create_block(start, current, now, duration));
        }
    }
    blocks
}

fn floor_to_hour(ts: DateTime<Utc>) -> DateTime<Utc> {
    ts.duration_trunc(TimeDelta::hours(1)).unwrap_or(ts)
}

fn create_block(
    start: DateTime<Utc>,
    entries: Vec<UsageEntry>,
    now: DateTime<Utc>,
    duration: TimeDelta,
) -> SessionBlock {
    let end = start + duration;
    let actual_end = entries.last().map(|e| e.timestamp);
    let is_active = actual_end.is_some_and(|last| now - last < duration && now < end);
    let mut tokens = TokenCounts::default();
    let mut per_model: BTreeMap<String, TokenCounts> = BTreeMap::new();
    for e in &entries {
        tokens.add(&e.tokens);
        if let Some(model) = &e.model {
            per_model.entry(model.clone()).or_default().add(&e.tokens);
        }
    }
    SessionBlock {
        start,
        end,
        actual_end,
        first_entry: entries.first().map(|e| e.timestamp),
        is_active,
        is_gap: false,
        entry_count: entries.len(),
        tokens,
        per_model,
    }
}

fn gap_block(last: DateTime<Utc>, next: DateTime<Utc>, duration: TimeDelta) -> SessionBlock {
    SessionBlock {
        start: last + duration,
        end: next,
        actual_end: None,
        first_entry: None,
        is_active: false,
        is_gap: true,
        entry_count: 0,
        tokens: TokenCounts::default(),
        per_model: BTreeMap::new(),
    }
}

/// Tokens per minute over the block's entry span. Estimate — labeled so in UI.
#[derive(Debug, Clone, Copy)]
pub struct BurnRate {
    /// All tokens (incl. cache) per minute.
    pub tokens_per_minute: f64,
    /// Input+output only — steadier indicator of real activity.
    pub indicator_tokens_per_minute: f64,
}

/// `None` for gap blocks and blocks whose entries span no measurable time.
pub fn burn_rate(block: &SessionBlock) -> Option<BurnRate> {
    if block.is_gap {
        return None;
    }
    let minutes = (block.actual_end? - block.first_entry?).num_milliseconds() as f64 / 60_000.0;
    if minutes <= 0.0 {
        return None;
    }
    Some(BurnRate {
        tokens_per_minute: block.tokens.total() as f64 / minutes,
        indicator_tokens_per_minute: (block.tokens.input + block.tokens.output) as f64 / minutes,
    })
}

/// Linear projection of usage to the block's reset. Estimate.
#[derive(Debug, Clone, Copy)]
pub struct Projection {
    pub total_tokens: u64,
    pub remaining_minutes: u64,
}

pub fn projection(block: &SessionBlock, now: DateTime<Utc>) -> Option<Projection> {
    if !block.is_active || block.is_gap {
        return None;
    }
    let rate = burn_rate(block)?;
    let remaining = ((block.end - now).num_milliseconds() as f64 / 60_000.0).round();
    Some(Projection {
        total_tokens: (block.tokens.total() as f64 + rate.tokens_per_minute * remaining).round()
            as u64,
        remaining_minutes: remaining.max(0.0) as u64,
    })
}

/// Estimated token limit: the largest total of any completed block, matching
/// ccusage's `--token-limit max`. 0 when no block has completed yet.
pub fn max_block_tokens(blocks: &[SessionBlock]) -> u64 {
    blocks
        .iter()
        .filter(|b| !b.is_gap && !b.is_active)
        .map(|b| b.tokens.total())
        .max()
        .unwrap_or(0)
}

/// Estimated wall-clock time the active block reaches `limit` tokens at the
/// current burn rate. `None` if not active, already at/over the limit, or the
/// block resets first. Estimate — actual limits vary by plan and load.
pub fn eta_to_limit(block: &SessionBlock, limit: u64, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if !block.is_active || limit == 0 || block.tokens.total() >= limit {
        return None;
    }
    let rate = burn_rate(block)?;
    if rate.tokens_per_minute <= 0.0 {
        return None;
    }
    let minutes = (limit - block.tokens.total()) as f64 / rate.tokens_per_minute;
    let delta = TimeDelta::try_milliseconds((minutes * 60_000.0) as i64)?;
    let eta = now.checked_add_signed(delta)?;
    (eta <= block.end).then_some(eta)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn entry(when: &str, output: u64) -> UsageEntry {
        UsageEntry {
            timestamp: ts(when),
            model: Some("claude-sonnet-5".into()),
            tokens: TokenCounts {
                output,
                ..Default::default()
            },
            message_id: None,
            request_id: None,
        }
    }

    const H: f64 = DEFAULT_SESSION_HOURS;

    #[test]
    fn empty_entries_yield_no_blocks() {
        assert!(identify_blocks(vec![], H, ts("2026-07-17T12:00:00Z")).is_empty());
    }

    #[test]
    fn block_start_floors_to_utc_hour() {
        let blocks = identify_blocks(
            vec![entry("2026-07-17T14:37:45Z", 10)],
            H,
            ts("2026-07-17T15:00:00Z"),
        );
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].start, ts("2026-07-17T14:00:00Z"));
        assert_eq!(blocks[0].end, ts("2026-07-17T19:00:00Z"));
        assert!(blocks[0].is_active);
    }

    #[test]
    fn entries_within_duration_share_a_block() {
        let blocks = identify_blocks(
            vec![
                entry("2026-07-17T14:00:00Z", 1),
                entry("2026-07-17T15:30:00Z", 2),
                entry("2026-07-17T18:59:00Z", 3),
            ],
            H,
            ts("2026-07-17T18:59:30Z"),
        );
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].entry_count, 3);
        assert_eq!(blocks[0].tokens.output, 6);
    }

    #[test]
    fn entry_exactly_at_duration_boundary_stays_in_block() {
        // ccusage uses strict >, so an entry exactly 5h after the (floored)
        // start still belongs to the block.
        let blocks = identify_blocks(
            vec![
                entry("2026-07-17T10:00:00Z", 1),
                entry("2026-07-17T15:00:00Z", 1),
            ],
            H,
            ts("2026-07-17T16:00:00Z"),
        );
        assert_eq!(blocks.len(), 1);
    }

    #[test]
    fn long_idle_gap_creates_gap_block() {
        let blocks = identify_blocks(
            vec![
                entry("2026-07-17T10:00:00Z", 1),
                entry("2026-07-17T16:30:00Z", 2),
            ],
            H,
            ts("2026-07-17T16:31:00Z"),
        );
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].start, ts("2026-07-17T10:00:00Z"));
        assert!(blocks[1].is_gap);
        // Gap spans last entry + 5h → next entry.
        assert_eq!(blocks[1].start, ts("2026-07-17T15:00:00Z"));
        assert_eq!(blocks[1].end, ts("2026-07-17T16:30:00Z"));
        assert_eq!(blocks[2].start, ts("2026-07-17T16:00:00Z"));
        assert!(blocks[2].is_active);
    }

    #[test]
    fn rollover_past_block_start_without_gap() {
        // 15:30 is >5h after the 10:00 block start but only 1.5h after the
        // previous entry: new block, no gap block.
        let blocks = identify_blocks(
            vec![
                entry("2026-07-17T10:00:00Z", 1),
                entry("2026-07-17T12:00:00Z", 1),
                entry("2026-07-17T14:00:00Z", 1),
                entry("2026-07-17T15:30:00Z", 1),
            ],
            H,
            ts("2026-07-17T15:35:00Z"),
        );
        assert_eq!(blocks.len(), 2);
        assert!(!blocks.iter().any(|b| b.is_gap));
        assert_eq!(blocks[0].entry_count, 3);
        assert_eq!(blocks[1].start, ts("2026-07-17T15:00:00Z"));
    }

    #[test]
    fn block_is_inactive_after_its_end() {
        let blocks = identify_blocks(
            vec![entry("2026-07-17T10:40:00Z", 1)],
            H,
            ts("2026-07-17T15:30:00Z"), // block ended 15:00
        );
        assert!(!blocks[0].is_active);
    }

    #[test]
    fn block_is_inactive_when_last_entry_is_stale() {
        // now < end but last entry more than 5h ago can only happen with a
        // custom duration; verify the `now - last < duration` leg with 1h blocks.
        let blocks = identify_blocks(
            vec![entry("2026-07-17T10:00:00Z", 1)],
            1.0,
            ts("2026-07-17T10:59:00Z"),
        );
        assert!(blocks[0].is_active);
        let blocks = identify_blocks(
            vec![entry("2026-07-17T10:00:00Z", 1)],
            1.0,
            ts("2026-07-17T11:01:00Z"),
        );
        assert!(!blocks[0].is_active);
    }

    #[test]
    fn unsorted_entries_are_sorted_first() {
        let blocks = identify_blocks(
            vec![
                entry("2026-07-17T15:30:00Z", 2),
                entry("2026-07-17T14:00:00Z", 1),
            ],
            H,
            ts("2026-07-17T15:31:00Z"),
        );
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].first_entry, Some(ts("2026-07-17T14:00:00Z")));
    }

    #[test]
    fn per_model_aggregation() {
        let mut e1 = entry("2026-07-17T14:00:00Z", 100);
        e1.model = Some("claude-opus-4-8".into());
        let e2 = entry("2026-07-17T14:05:00Z", 50);
        let mut e3 = entry("2026-07-17T14:06:00Z", 7);
        e3.model = None; // synthetic — counts in totals only
        let blocks = identify_blocks(vec![e1, e2, e3], H, ts("2026-07-17T14:10:00Z"));
        let b = &blocks[0];
        assert_eq!(b.tokens.output, 157);
        assert_eq!(b.per_model["claude-opus-4-8"].output, 100);
        assert_eq!(b.per_model["claude-sonnet-5"].output, 50);
        assert_eq!(b.per_model.len(), 2);
    }

    #[test]
    fn burn_rate_over_entry_span() {
        let blocks = identify_blocks(
            vec![
                entry("2026-07-17T14:00:00Z", 100),
                entry("2026-07-17T14:10:00Z", 900),
            ],
            H,
            ts("2026-07-17T14:10:00Z"),
        );
        let rate = burn_rate(&blocks[0]).unwrap();
        assert_eq!(rate.tokens_per_minute, 100.0);
        assert_eq!(rate.indicator_tokens_per_minute, 100.0);
    }

    #[test]
    fn burn_rate_none_for_single_entry() {
        let blocks = identify_blocks(
            vec![entry("2026-07-17T14:00:00Z", 100)],
            H,
            ts("2026-07-17T14:01:00Z"),
        );
        assert!(burn_rate(&blocks[0]).is_none());
    }

    #[test]
    fn projection_extends_linearly_to_reset() {
        let now = ts("2026-07-17T14:10:00Z");
        let blocks = identify_blocks(
            vec![
                entry("2026-07-17T14:00:00Z", 500),
                entry("2026-07-17T14:10:00Z", 500),
            ],
            H,
            now,
        );
        let p = projection(&blocks[0], now).unwrap();
        // 100 tok/min × 290 min remaining + 1000 current
        assert_eq!(p.remaining_minutes, 290);
        assert_eq!(p.total_tokens, 30_000);
    }

    #[test]
    fn max_block_tokens_ignores_gaps_and_active() {
        let now = ts("2026-07-18T09:10:00Z");
        let blocks = identify_blocks(
            vec![
                entry("2026-07-17T10:00:00Z", 4_000),
                entry("2026-07-17T20:00:00Z", 9_000),
                entry("2026-07-18T09:00:00Z", 50_000), // active
            ],
            H,
            now,
        );
        assert!(blocks.last().unwrap().is_active);
        assert_eq!(max_block_tokens(&blocks), 9_000);
    }

    #[test]
    fn eta_to_limit_math_and_reset_cap() {
        let now = ts("2026-07-17T14:10:00Z");
        let blocks = identify_blocks(
            vec![
                entry("2026-07-17T14:00:00Z", 500),
                entry("2026-07-17T14:10:00Z", 500),
            ],
            H,
            now,
        );
        // 100 tok/min, 1000 used → 2000 hits in 10 min.
        assert_eq!(
            eta_to_limit(&blocks[0], 2_000, now),
            Some(ts("2026-07-17T14:20:00Z"))
        );
        // Would only hit after reset → None.
        assert_eq!(eta_to_limit(&blocks[0], 40_000, now), None);
        // Already over → None.
        assert_eq!(eta_to_limit(&blocks[0], 900, now), None);
    }

    #[test]
    fn eta_to_limit_rejects_out_of_range_datetime() {
        let now = ts("2026-07-17T14:10:00Z");
        let blocks = identify_blocks(
            vec![
                entry("2026-07-17T14:00:00Z", 1),
                entry("2026-07-17T14:10:00Z", 1),
            ],
            H,
            now,
        );

        assert_eq!(eta_to_limit(&blocks[0], u64::MAX, now), None);
    }
}
