//! Tiny CLI harness: print the current block state once.
//!
//!    cargo run --example status

use blockwatcher_core::{
    DEFAULT_SESSION_HOURS, LogIndex, LogSource, MacLogSource, burn_rate, eta_to_limit,
    identify_blocks, max_block_tokens, projection,
};
use chrono::{DateTime, Local, Utc};

fn local_hm(ts: DateTime<Utc>) -> String {
    ts.with_timezone(&Local).format("%-I:%M %p").to_string()
}

fn main() {
    let source = MacLogSource::discover();
    let files = source.log_files();
    if files.is_empty() {
        println!("No Claude Code logs found (looked for <config dir>/projects/**/*.jsonl).");
        return;
    }

    let mut index = LogIndex::new();
    index.refresh(files);
    let now = Utc::now();
    let blocks = identify_blocks(index.entries().to_vec(), DEFAULT_SESSION_HOURS, now);
    let est_limit = max_block_tokens(&blocks);

    let Some(block) = blocks.iter().rev().find(|b| b.is_active) else {
        println!("No active session.");
        if let Some(last) = blocks.iter().rev().find(|b| !b.is_gap) {
            println!("Last block ended {}.", local_hm(last.end));
        }
        return;
    };

    let remaining = block.remaining(now);
    println!(
        "Active block   {} → {}  (first message {})",
        local_hm(block.start),
        local_hm(block.end),
        block.first_entry.map(local_hm).unwrap_or_default(),
    );
    println!(
        "Resets in      {}h {:02}m",
        remaining.num_hours(),
        remaining.num_minutes() % 60
    );
    let pct = if est_limit > 0 {
        format!(
            " ({:.0}% of est. {} limit)",
            block.tokens.total() as f64 / est_limit as f64 * 100.0,
            est_limit
        )
    } else {
        String::new()
    };
    println!("Tokens         {}{pct}", block.tokens.total());
    for (model, t) in &block.per_model {
        println!(
            "  {model}: {} in / {} out / {} cache write / {} cache read",
            t.input, t.output, t.cache_creation, t.cache_read
        );
    }
    if let Some(rate) = burn_rate(block) {
        println!(
            "Burn rate      {:.0} tok/min (estimate)",
            rate.tokens_per_minute
        );
        if let Some(p) = projection(block, now) {
            println!(
                "Projected      ~{} tokens by reset (estimate)",
                p.total_tokens
            );
        }
        if let Some(eta) = eta_to_limit(block, est_limit, now) {
            println!("Est. limit hit ~{} (estimate)", local_hm(eta));
        }
    }
    println!(
        "\n{} entries parsed, {} malformed lines skipped.",
        index.entries().len(),
        index.malformed_lines()
    );
}
