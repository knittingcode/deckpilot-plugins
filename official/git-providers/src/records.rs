//! The records the operations return (`contributes.recordSchemas` of the
//! manifest): flat objects of typed fields. Text a person wrote (titles,
//! descriptions, names) is marked `sensitive` in the schemas.
//!
//! ABI 1 outcomes carry no objects, so a `records` output travels as a
//! list of strings, each one record's JSON object
//! ([`to_output`]); the host parses them against the schema.

use serde::Serialize;

use crate::provider::Capabilities;

/// Most records one output carries (DeckPilot's `MAX_RECORDS`).
pub const MAX_RECORDS: usize = 200;
/// Largest serialized `records` output (`MAX_RECORDS_BYTES`).
pub const MAX_RECORDS_BYTES: usize = 64 * 1024;

/// A page of records and whether more exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page<T> {
    pub items: Vec<T>,
    /// The next page's number, when the provider says there is one.
    pub next: Option<u32>,
}

/// The pipeline states (design §2.2): a closed set; anything else is
/// `unknown`, with the provider's own word kept in `providerStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PipelineState {
    Pending,
    Running,
    Success,
    Failed,
    Cancelled,
    Skipped,
    Manual,
    Unknown,
}

impl PipelineState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Success => "success",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Skipped => "skipped",
            Self::Manual => "manual",
            Self::Unknown => "unknown",
        }
    }

    /// Whether the pipeline will not change any more.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Success | Self::Failed | Self::Cancelled | Self::Skipped
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryRecord {
    /// `gl:4711`: the identity, never the path.
    pub id: String,
    /// `group/sub/project`, for display only.
    pub path: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<String>,
    pub archived: bool,
    pub web_url: String,
    /// Clone URLs, to match a local git remote (`resolve-remote`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchRecord {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha: Option<String>,
    pub default: bool,
    pub protected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitRecord {
    pub sha: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub committed_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeRequestRecord {
    pub number: u64,
    pub title: String,
    /// `open`, `closed`, `merged` or the provider's word.
    pub state: String,
    pub source: String,
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub draft: bool,
    pub web_url: String,
    /// The repository, for lists across repositories.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    /// For `create-change-request`: one was already open for the same
    /// source and target, and nothing was created.
    pub existing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineRecord {
    pub id: String,
    pub state: PipelineState,
    /// The provider's own status word, cleaned.
    pub provider_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueRecord {
    pub number: u64,
    pub title: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub web_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<u64>,
}

/// What a connection test found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Probe {
    /// The signed-in user's login.
    pub account: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// The token's scopes, when the server says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
    /// When the token expires (Unix ms), when the server says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
    pub capabilities: Capabilities,
    /// Scopes the token has beyond what DeckPilot needs (admin, sudo…):
    /// the app warns about them.
    pub excess_scopes: Vec<String>,
}

/// A `records` output: one JSON object text per record, at most
/// [`MAX_RECORDS`] and [`MAX_RECORDS_BYTES`] in all; `truncated` when some
/// did not fit.
pub fn to_output<T: Serialize>(records: &[T]) -> (Vec<String>, bool) {
    let mut out = Vec::new();
    let mut bytes = 0;
    for record in records.iter().take(MAX_RECORDS) {
        let Ok(text) = serde_json::to_string(record) else {
            continue;
        };
        if bytes + text.len() > MAX_RECORDS_BYTES {
            return (out, true);
        }
        bytes += text.len();
        out.push(text);
    }
    let truncated = records.len() > out.len();
    (out, truncated)
}

/// An ISO 8601 / RFC 3339 time (`2026-11-24T21:05:00.123Z`,
/// `2026-11-24T22:05:00+01:00`) as Unix milliseconds. `None` for anything
/// else: a record then leaves the field out.
pub fn timestamp(text: &str) -> Option<u64> {
    let text = text.trim();
    let (date, rest) = text.split_at_checked(10)?;
    let mut parts = date.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    let rest = rest.strip_prefix(['T', 't', ' '])?;
    let (time, zone) = rest.split_at(rest.find(['Z', 'z', '+', '-'])?);
    let (hms, millis) = match time.split_once('.') {
        Some((hms, fraction)) => {
            let digits: String = fraction.chars().take(3).collect();
            let padded = format!("{digits:0<3}");
            (hms, padded.parse::<i64>().ok()?)
        }
        None => (time, 0),
    };
    let mut t = hms.split(':');
    let hour: i64 = t.next()?.parse().ok()?;
    let minute: i64 = t.next()?.parse().ok()?;
    let second: i64 = t.next()?.parse().ok()?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let offset = match zone {
        "Z" | "z" => 0,
        _ => {
            let sign = if zone.starts_with('-') { -1 } else { 1 };
            let (h, m) = zone[1..].split_once(':')?;
            sign * (h.parse::<i64>().ok()? * 60 + m.parse::<i64>().ok()?)
        }
    };
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let seconds = days * 86_400 + hour * 3_600 + minute * 60 + second - offset * 60;
    u64::try_from(seconds * 1_000 + millis).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_are_read_as_unix_ms() {
        assert_eq!(timestamp("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            timestamp("2026-11-24T21:05:00.123Z"),
            Some(1_795_554_300_123)
        );
        assert_eq!(
            timestamp("2026-11-24T22:05:00.123+01:00"),
            timestamp("2026-11-24T21:05:00.123Z")
        );
        assert_eq!(timestamp("2026-11-24T21:05:00.5Z"), Some(1_795_554_300_500));
        for bad in [
            "",
            "yesterday",
            "2026-13-01T00:00:00Z",
            "2026-11-24",
            "1969-12-31T23:59:59Z",
        ] {
            assert_eq!(timestamp(bad), None, "{bad}");
        }
    }

    #[test]
    fn records_are_bounded() {
        let many: Vec<u32> = (0..250).collect();
        let (out, truncated) = to_output(&many);
        assert_eq!(out.len(), MAX_RECORDS);
        assert!(truncated);
        let big = vec!["x".repeat(20_000); 5];
        let (out, truncated) = to_output(&big);
        assert_eq!(out.len(), 3);
        assert!(truncated);
        let (out, truncated) = to_output(&[1, 2]);
        assert_eq!(out, ["1", "2"]);
        assert!(!truncated);
    }
}
