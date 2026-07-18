//! JSONL usage-line parsing and the incremental log index.
//!
//! The log format is not a stable public API: every field is deserialized
//! leniently (wrong types read as missing, unknown fields ignored) and a
//! malformed line can never fail more than itself.

use std::collections::{HashMap, HashSet};
use std::fs::{File, Metadata};
use std::io::{Read, Seek, SeekFrom};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};

/// Token counts for one entry or an aggregate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenCounts {
    pub input: u64,
    pub output: u64,
    pub cache_creation: u64,
    pub cache_read: u64,
}

impl TokenCounts {
    pub fn total(&self) -> u64 {
        self.input
            .saturating_add(self.output)
            .saturating_add(self.cache_creation)
            .saturating_add(self.cache_read)
    }

    pub fn add(&mut self, other: &TokenCounts) {
        self.input = self.input.saturating_add(other.input);
        self.output = self.output.saturating_add(other.output);
        self.cache_creation = self.cache_creation.saturating_add(other.cache_creation);
        self.cache_read = self.cache_read.saturating_add(other.cache_read);
    }
}

/// One usage record from a session log. Carries no conversation content.
#[derive(Debug, Clone)]
pub struct UsageEntry {
    pub timestamp: DateTime<Utc>,
    /// Raw model id; `None` for synthetic/absent models.
    pub model: Option<String>,
    pub tokens: TokenCounts,
    /// Opaque ids used only to dedup the same API response logged twice.
    pub message_id: Option<String>,
    pub request_id: Option<String>,
}

/// Result of parsing one JSONL line.
#[derive(Debug)]
pub enum ParsedLine {
    Entry(UsageEntry),
    /// Valid JSON, but not a usage record (user messages, meta lines, …).
    Skipped,
    /// Not a JSON object, or a usage record with an unusable timestamp.
    Malformed,
}

// Lenient field readers, mirroring ccusage: a wrongly-typed field reads as
// missing instead of poisoning the whole line.

fn lenient_u64<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    let v = Option::<serde_json::Value>::deserialize(d)?;
    Ok(v.as_ref().and_then(serde_json::Value::as_u64).unwrap_or(0))
}

pub(crate) fn lenient_string<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let v = Option::<serde_json::Value>::deserialize(d)?;
    Ok(match v {
        Some(serde_json::Value::String(s)) if !s.trim().is_empty() => Some(s),
        _ => None,
    })
}

pub(crate) fn lenient_object<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let v = Option::<serde_json::Value>::deserialize(d)?;
    Ok(match v {
        Some(v @ serde_json::Value::Object(_)) => serde_json::from_value(v).ok(),
        _ => None,
    })
}

fn has_unsupported_null_field(line: &str) -> bool {
    [
        r#""id":null"#,
        r#""cwd":null"#,
        r#""model":null"#,
        r#""speed":null"#,
        r#""costUSD":null"#,
        r#""version":null"#,
        r#""sessionId":null"#,
        r#""requestId":null"#,
        r#""isApiErrorMessage":null"#,
        r#""cache_read_input_tokens":null"#,
        r#""cache_creation_input_tokens":null"#,
    ]
    .iter()
    .any(|field| line.contains(field))
}

#[derive(Deserialize)]
struct RawEntry {
    #[serde(default, deserialize_with = "lenient_string")]
    timestamp: Option<String>,
    #[serde(default, deserialize_with = "lenient_object")]
    message: Option<RawMessage>,
    #[serde(default, rename = "requestId", deserialize_with = "lenient_string")]
    request_id: Option<String>,
}

#[derive(Deserialize)]
struct RawMessage {
    #[serde(default, deserialize_with = "lenient_object")]
    usage: Option<RawUsage>,
    #[serde(default, deserialize_with = "lenient_string")]
    model: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    id: Option<String>,
}

#[derive(Deserialize, Default)]
struct RawUsage {
    #[serde(default, deserialize_with = "lenient_u64")]
    input_tokens: u64,
    #[serde(default, deserialize_with = "lenient_u64")]
    output_tokens: u64,
    #[serde(default, deserialize_with = "lenient_u64")]
    cache_creation_input_tokens: u64,
    #[serde(default, deserialize_with = "lenient_u64")]
    cache_read_input_tokens: u64,
    /// Newer per-TTL breakdown; when present it supersedes the flat count.
    #[serde(default, deserialize_with = "lenient_object")]
    cache_creation: Option<RawCacheCreation>,
}

#[derive(Deserialize, Default)]
struct RawCacheCreation {
    #[serde(default, deserialize_with = "lenient_u64")]
    ephemeral_5m_input_tokens: u64,
    #[serde(default, deserialize_with = "lenient_u64")]
    ephemeral_1h_input_tokens: u64,
}

/// Parse one JSONL line. Never panics.
pub fn parse_line(line: &str) -> ParsedLine {
    let line = line.trim();
    if line.is_empty() {
        return ParsedLine::Skipped;
    }
    if has_unsupported_null_field(line) {
        return ParsedLine::Skipped;
    }
    // RawEntry is fully lenient, so this only fails on non-object JSON or garbage.
    let Ok(raw) = serde_json::from_str::<RawEntry>(line) else {
        return ParsedLine::Malformed;
    };
    let Some(message) = raw.message else {
        return ParsedLine::Skipped;
    };
    let Some(usage) = message.usage else {
        return ParsedLine::Skipped;
    };
    let timestamp = raw
        .timestamp
        .as_deref()
        .and_then(|t| DateTime::parse_from_rfc3339(t).ok());
    let Some(timestamp) = timestamp else {
        // A usage record we cannot place in time is unusable.
        return ParsedLine::Malformed;
    };
    let cache_creation = match &usage.cache_creation {
        Some(c) => c
            .ephemeral_5m_input_tokens
            .saturating_add(c.ephemeral_1h_input_tokens),
        None => usage.cache_creation_input_tokens,
    };
    ParsedLine::Entry(UsageEntry {
        timestamp: timestamp.with_timezone(&Utc),
        model: message.model.filter(|m| m != "<synthetic>"),
        tokens: TokenCounts {
            input: usage.input_tokens,
            output: usage.output_tokens,
            cache_creation,
            cache_read: usage.cache_read_input_tokens,
        },
        message_id: message.id,
        request_id: raw.request_id,
    })
}

/// Incremental view over a set of JSONL log files.
///
/// Keeps a byte offset per file so each `refresh` only reads bytes appended
/// since the last one; whole files are never re-parsed. Entries are deduped
/// on (message id, request id) — the same API response can be logged in more
/// than one session file.
#[derive(Debug, Default)]
pub struct LogIndex {
    offsets: HashMap<PathBuf, FileCursor>,
    indexes: HashMap<(String, Option<String>), usize>,
    entries: Vec<UsageEntry>,
    malformed: u64,
}

impl LogIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// All entries ingested so far, in ingestion order (not sorted).
    pub fn entries(&self) -> &[UsageEntry] {
        &self.entries
    }

    /// Count of malformed lines skipped so far.
    pub fn malformed_lines(&self) -> u64 {
        self.malformed
    }

    /// Ingest new bytes from `files`; returns how many new entries appeared.
    ///
    /// Unreadable or vanished files are skipped — a partially written or
    /// rotated log must never take the whole index down.
    pub fn refresh<I>(&mut self, files: I) -> usize
    where
        I: IntoIterator<Item = PathBuf>,
    {
        let mut added = 0;
        let Self {
            offsets,
            indexes,
            entries,
            malformed,
        } = self;
        let files = files.into_iter().collect::<Vec<_>>();
        if index_requires_rebuild(offsets, &files) {
            offsets.clear();
            indexes.clear();
            entries.clear();
            *malformed = 0;
        }
        read_new_lines(offsets, files, |line| match parse_line(line) {
            ParsedLine::Entry(entry) => {
                let key = entry
                    .message_id
                    .as_ref()
                    .map(|id| (id.clone(), entry.request_id.clone()));
                if let Some(index) = key.as_ref().and_then(|key| indexes.get(key)).copied() {
                    if entry.tokens.total() > entries[index].tokens.total() {
                        entries[index] = entry;
                    }
                } else {
                    if let Some(key) = key {
                        indexes.insert(key, entries.len());
                    }
                    entries.push(entry);
                    added += 1;
                }
            }
            ParsedLine::Skipped => {}
            ParsedLine::Malformed => *malformed += 1,
        });
        added
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileIdentity {
    #[cfg(unix)]
    Unix { device: u64, inode: u64 },
    #[cfg(not(unix))]
    Created(Option<std::time::SystemTime>),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct FileCursor {
    offset: u64,
    identity: FileIdentity,
    anchor: u64,
}

fn file_identity(metadata: &Metadata) -> FileIdentity {
    #[cfg(unix)]
    {
        FileIdentity::Unix {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }
    #[cfg(not(unix))]
    {
        FileIdentity::Created(metadata.created().ok())
    }
}

fn anchor_hash(file: &mut File, offset: u64) -> std::io::Result<u64> {
    const ANCHOR_BYTES: u64 = 256;
    let start = offset.saturating_sub(ANCHOR_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = vec![0; (offset - start) as usize];
    file.read_exact(&mut bytes)?;
    Ok(bytes.into_iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    }))
}

fn cursor_matches(file: &mut File, metadata: &Metadata, cursor: FileCursor) -> bool {
    metadata.len() >= cursor.offset
        && file_identity(metadata) == cursor.identity
        && anchor_hash(file, cursor.offset).is_ok_and(|hash| hash == cursor.anchor)
}

pub(crate) fn index_requires_rebuild(
    offsets: &HashMap<PathBuf, FileCursor>,
    files: &[PathBuf],
) -> bool {
    let current = files.iter().map(PathBuf::as_path).collect::<HashSet<_>>();
    if offsets.keys().any(|path| !current.contains(path.as_path())) {
        return true;
    }

    files.iter().any(|path| {
        let Some(cursor) = offsets.get(path).copied() else {
            return false;
        };
        let Ok(metadata) = std::fs::metadata(path) else {
            return false;
        };
        let Ok(mut file) = File::open(path) else {
            return false;
        };
        !cursor_matches(&mut file, &metadata, cursor)
    })
}

pub(crate) fn read_new_lines<I, F>(
    offsets: &mut HashMap<PathBuf, FileCursor>,
    files: I,
    mut visit: F,
) where
    I: IntoIterator<Item = PathBuf>,
    F: FnMut(&str),
{
    for path in files {
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        let len = metadata.len();
        let cursor = offsets.get(&path).copied();
        let offset = cursor.map_or(0, |cursor| cursor.offset);
        if len == offset {
            continue;
        }
        let Ok(mut file) = File::open(&path) else {
            continue;
        };
        if cursor.is_some_and(|cursor| !cursor_matches(&mut file, &metadata, cursor)) {
            continue;
        }
        if file.seek(SeekFrom::Start(offset)).is_err() {
            continue;
        }
        let mut buf = Vec::with_capacity((len - offset) as usize);
        if file.read_to_end(&mut buf).is_err() {
            continue;
        }
        // Only consume complete lines; a partial trailing line stays in the
        // file until a later refresh sees its newline.
        let Some(consumed) = buf.iter().rposition(|&b| b == b'\n').map(|i| i + 1) else {
            continue;
        };
        for line in buf[..consumed].split(|&b| b == b'\n') {
            visit(&String::from_utf8_lossy(line));
        }
        let offset = offset + consumed as u64;
        let Ok(anchor) = anchor_hash(&mut file, offset) else {
            continue;
        };
        offsets.insert(
            path,
            FileCursor {
                offset,
                identity: file_identity(&metadata),
                anchor,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_fields_ccusage_rejects_when_null() {
        for field in [
            "id",
            "cwd",
            "model",
            "speed",
            "costUSD",
            "version",
            "sessionId",
            "requestId",
            "isApiErrorMessage",
            "cache_read_input_tokens",
            "cache_creation_input_tokens",
        ] {
            let line = format!(
                r#"{{"{field}":null,"timestamp":"2026-07-18T10:00:00Z","message":{{"usage":{{"input_tokens":1}}}}}}"#
            );
            assert!(
                matches!(parse_line(&line), ParsedLine::Skipped),
                "{field} should reject the line"
            );
        }

        assert!(matches!(
            parse_line(
                r#"{"unknown":null,"timestamp":"2026-07-18T10:00:00Z","message":{"usage":{"input_tokens":1}}}"#
            ),
            ParsedLine::Entry(_)
        ));
    }

    #[test]
    fn token_arithmetic_saturates() {
        let line = r#"{"timestamp":"2026-07-18T10:00:00Z","message":{"usage":{"cache_creation":{"ephemeral_5m_input_tokens":18446744073709551615,"ephemeral_1h_input_tokens":1}}}}"#;
        let ParsedLine::Entry(entry) = parse_line(line) else {
            panic!("expected entry");
        };
        assert_eq!(entry.tokens.cache_creation, u64::MAX);

        let mut total = TokenCounts {
            input: u64::MAX,
            ..Default::default()
        };
        total.add(&TokenCounts {
            input: 1,
            output: u64::MAX,
            ..Default::default()
        });
        assert_eq!(total.input, u64::MAX);
        assert_eq!(total.total(), u64::MAX);
    }
}
