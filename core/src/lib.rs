//! `blockwatcher-core` - pure logic for the BlockWatcher menu bar app.
//!
//! Reads aggregate Claude plan usage when Claude Desktop exports it, or
//! groups Claude Code JSONL usage into 5-hour blocks as a fallback.
//!
//! Privacy: only timestamps, model ids, token counts, and opaque
//! message/request ids (for dedup) are ever extracted or kept in memory.
//! Conversation content is never stored or logged. No network access.
//!
//! Block-boundary logic is ported from ccusage (MIT,
//! <https://github.com/ryoppippi/ccusage>) — see NOTICE.

pub mod activity;
pub mod blocks;
pub mod claude;
pub mod codex;
pub mod parse;
pub mod source;

pub use activity::{AgentActivity, ModelActivity, parse_ccusage_daily};
pub use blocks::{
    BurnRate, DEFAULT_SESSION_HOURS, Projection, SessionBlock, burn_rate, eta_to_limit,
    identify_blocks, max_block_tokens, projection,
};
pub use claude::{ClaudeRateWindow, ClaudeUsage, parse_claude_usage_history};
pub use codex::{CodexLogIndex, CodexParsedLine, CodexRateWindow, CodexUsage, parse_codex_line};
pub use parse::{LogIndex, ParsedLine, TokenCounts, UsageEntry, parse_line};
pub use source::{CodexLogSource, LogSource, MacLogSource};

/// Working name — referenced by UI, docs, and bundle config. Rename here first.
pub const APP_NAME: &str = "BlockWatcher";
