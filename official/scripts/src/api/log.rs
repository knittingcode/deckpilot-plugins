//! `deckpilot.log.*` (§5.4 "Logs": 1,000 lines or 256 KiB per run, then
//! one "…truncated" marker). The limit is kept here, before any host call,
//! so a log flood costs neither host calls nor agent memory; the agent's
//! ring buffer applies the same bounds again.

use serde_json::json;

use crate::bridge::{Session, limits};

pub const LEVELS: &[&str] = &["debug", "info", "warn", "error"];
pub const TRUNCATED: &str = "…truncated (the log limit of this run was reached)";

/// Cuts `s` to at most `max` bytes on a char boundary.
pub fn cut(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Sends one log line, or the truncation marker once, or nothing.
/// Logging never fails the script: a refused or failed `script.log` is
/// dropped.
pub fn line(session: &mut Session, level: &str, message: &str) {
    let level = if LEVELS.contains(&level) {
        level
    } else {
        "info"
    };
    let b = &mut session.budget;
    if b.log_truncated || b.cancelled {
        return;
    }
    let message = cut(message, limits::LOG_LINE);
    if b.log_lines >= limits::LOG_LINES || b.log_bytes + message.len() > limits::LOG_BYTES {
        b.log_truncated = true;
        let _ = session.call(
            "script.log",
            json!({ "level": "warn", "message": TRUNCATED }),
        );
        return;
    }
    b.log_lines += 1;
    b.log_bytes += message.len();
    let _ = session.call("script.log", json!({ "level": level, "message": message }));
}
