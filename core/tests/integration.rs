//! End-to-end: fixture JSONL files → LogIndex → blocks.

use std::path::PathBuf;

use blockwatcher_core::{
    DEFAULT_SESSION_HOURS, LogIndex, TokenCounts, burn_rate, identify_blocks, max_block_tokens,
};
use chrono::{DateTime, Utc};

fn fixtures() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "jsonl")
        })
        .collect();
    files.sort();
    files
}

fn ts(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
}

#[test]
fn fixtures_parse_dedup_and_block_correctly() {
    let mut index = LogIndex::new();
    let added = index.refresh(fixtures());

    // 7 usage lines across the fixtures; msg_003 is logged in both session
    // files and must be deduped.
    assert_eq!(added, 6);
    assert_eq!(index.entries().len(), 6);
    assert_eq!(index.malformed_lines(), 4);

    // Nested cache_creation breakdown supersedes the flat count.
    let msg_002 = index
        .entries()
        .iter()
        .find(|e| e.message_id.as_deref() == Some("msg_002"))
        .unwrap();
    assert_eq!(msg_002.tokens.cache_creation, 2_000);

    // Synthetic model becomes None but the entry still counts.
    assert!(
        index
            .entries()
            .iter()
            .any(|e| e.message_id.as_deref() == Some("msg_004") && e.model.is_none())
    );

    let now = ts("2026-07-17T17:10:00Z");
    let blocks = identify_blocks(index.entries().to_vec(), DEFAULT_SESSION_HOURS, now);

    assert_eq!(blocks.len(), 3, "morning block, gap, active block");

    let morning = &blocks[0];
    assert_eq!(morning.start, ts("2026-07-17T10:00:00Z"));
    assert_eq!(morning.end, ts("2026-07-17T15:00:00Z"));
    assert!(!morning.is_active);
    assert_eq!(morning.entry_count, 2);
    assert_eq!(
        morning.tokens,
        TokenCounts {
            input: 2_000,
            output: 1_000,
            cache_creation: 6_000,
            cache_read: 30_000
        }
    );

    let gap = &blocks[1];
    assert!(gap.is_gap);
    assert_eq!(gap.start, ts("2026-07-17T15:15:00Z"));
    assert_eq!(gap.end, ts("2026-07-17T16:30:00Z"));

    let active = &blocks[2];
    assert!(active.is_active);
    assert_eq!(active.start, ts("2026-07-17T16:00:00Z"));
    assert_eq!(active.entry_count, 4);
    assert_eq!(active.tokens.total(), 12_125);
    assert_eq!(active.per_model.len(), 2);
    assert_eq!(active.per_model["claude-opus-4-8"].output, 1_950);
    assert_eq!(active.per_model["claude-sonnet-5"].output, 25);

    // Historical limit estimate comes from the completed block only.
    assert_eq!(max_block_tokens(&blocks), 39_000);
    assert!(burn_rate(active).is_some());

    // Refreshing again reads nothing new (offsets, not re-parsing).
    assert_eq!(index.refresh(fixtures()), 0);
    assert_eq!(
        index.malformed_lines(),
        4,
        "malformed lines are not re-counted"
    );
}

#[test]
fn incremental_refresh_appends_and_handles_partial_lines() {
    use std::io::Write;

    let dir = std::env::temp_dir().join(format!("blockwatcher-int-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("live.jsonl");

    // No message ids on purpose: dedup can't mask an offset bug here.
    let line = |t: &str, out: u64| {
        format!(
            "{{\"timestamp\":\"2026-07-17T{t}:00.000Z\",\"message\":{{\"model\":\"claude-sonnet-5\",\"usage\":{{\"input_tokens\":10,\"output_tokens\":{out}}}}}}}\n"
        )
    };

    std::fs::write(&log, line("10:00", 1) + &line("10:01", 2)).unwrap();
    let mut index = LogIndex::new();
    assert_eq!(index.refresh([log.clone()]), 2);

    // Append a complete line plus the first half of another (no newline yet).
    let half = line("10:03", 4);
    let (half_a, half_b) = half.split_at(30);
    let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
    f.write_all((line("10:02", 3) + half_a).as_bytes()).unwrap();
    f.sync_all().unwrap();
    assert_eq!(index.refresh([log.clone()]), 1, "partial line must wait");

    f.write_all(half_b.as_bytes()).unwrap();
    f.sync_all().unwrap();
    assert_eq!(
        index.refresh([log.clone()]),
        1,
        "completed line parses once"
    );

    assert_eq!(index.entries().len(), 4);
    let outputs: Vec<u64> = index.entries().iter().map(|e| e.tokens.output).collect();
    assert_eq!(outputs, [1, 2, 3, 4], "no line lost or double-counted");
    assert_eq!(index.malformed_lines(), 0);
}

#[test]
fn incremental_refresh_rebuilds_after_replacement_or_truncation() {
    let dir = std::env::temp_dir().join(format!("blockwatcher-rotate-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("rotating.jsonl");
    let line = |minute: u8, output: u64| {
        format!(
            "{{\"timestamp\":\"2026-07-17T10:{minute:02}:00Z\",\"message\":{{\"usage\":{{\"output_tokens\":{output}}}}}}}\n"
        )
    };

    std::fs::write(&log, line(0, 1) + &line(1, 2)).unwrap();
    let mut index = LogIndex::new();
    assert_eq!(index.refresh([log.clone()]), 2);

    let replacement = dir.join("replacement.jsonl");
    std::fs::write(&replacement, line(2, 9) + &line(3, 10) + &line(4, 11)).unwrap();
    std::fs::remove_file(&log).unwrap();
    std::fs::rename(&replacement, &log).unwrap();
    assert_eq!(index.refresh([log.clone()]), 3);
    assert_eq!(
        index
            .entries()
            .iter()
            .map(|entry| entry.tokens.output)
            .collect::<Vec<_>>(),
        [9, 10, 11]
    );

    std::fs::write(&log, line(5, 20)).unwrap();
    assert_eq!(index.refresh([log]), 1);
    assert_eq!(index.entries().len(), 1);
    assert_eq!(index.entries()[0].tokens.output, 20);
}

#[test]
fn duplicate_revisions_keep_largest_usage() {
    let dir = std::env::temp_dir().join(format!("blockwatcher-dedup-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("revisions.jsonl");
    let line = |output| {
        format!(
            "{{\"timestamp\":\"2026-07-18T10:00:00Z\",\"requestId\":\"req_1\",\"message\":{{\"id\":\"msg_1\",\"usage\":{{\"output_tokens\":{output}}}}}}}\n"
        )
    };
    std::fs::write(&log, line(10) + &line(20)).unwrap();

    let mut index = LogIndex::new();
    assert_eq!(index.refresh([log]), 1);
    assert_eq!(index.entries().len(), 1);
    assert_eq!(index.entries()[0].tokens.output, 20);
}
