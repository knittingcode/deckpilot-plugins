//! What a key bound to `rate` shows (plugin API 1.2, key state): the
//! pair's newest reference rate and its change since the previous working
//! day, the pair and the rate's date, and a sparkline of the stored rates
//! ([`crate::history::Snapshot::key`]). Never fetched by the key itself.
//!
//! **Never shown as live.** These are the ECB's daily reference rates. The
//! key always names the rate's date, and shows no "ok" mark even when the
//! rates are current. When they are not known to be current (fetched
//! longer ago than the answer's lifetime, or a newest rate more than
//! [`MAX_RATE_LAG_DAYS`] days old, both against the present from
//! `time.now`), the key keeps them but adds a warning and a badge: the
//! rate's lag in days when that is the reason, otherwise the age of the
//! fetch, or `?` when unknown.
//!
//! The colour follows the change since the previous rate: green up, red
//! down, none when flat or unknown. Texts are ASCII (DeckPilot's key font).

use deckpilot_plugin_sdk::prelude::*;

use crate::cache::{MAX_RATE_LAG_DAYS, PairEntry, age};
use crate::history::{self, Direction, Snapshot};
use crate::provider::Pair;
use crate::time::{day_number, utc_day};

/// An age as a badge of at most 4 characters: `40m`, `3h`, `2d`.
pub fn age_badge(seconds: u64) -> String {
    let minutes = seconds / 60;
    if minutes < 60 {
        format!("{minutes}m")
    } else if minutes < 48 * 60 {
        format!("{}h", minutes / 60)
    } else {
        format!("{}d", (minutes / (24 * 60)).min(999))
    }
}

/// The key state for the stored rates `entry` of `pair`, seen at
/// `now_unix` (seconds). Without rates: the key as the user designed it.
pub fn rate(entry: Option<&PairEntry>, pair: &Pair, now_unix: u64) -> KeyState {
    let Some(entry) = entry.filter(|e| !e.points.is_empty()) else {
        return KeyState::new();
    };
    let latest = entry.points.last().expect("not empty");
    let change = history::change(&entry.points, 1);
    let face = Snapshot {
        pair: pair.label(),
        rate: crate::numbers::rate(latest.r),
        date: latest.d.clone(),
        // Decided below, against the present.
        stale: false,
        change_percent: change.as_ref().map(|c| crate::numbers::percent(c.percent)),
        series: history::series(&entry.points),
    }
    .key();
    let ascii = |text: &str| -> String {
        text.chars()
            .filter(|c| c.is_ascii() && !c.is_ascii_control())
            .take(32)
            .collect()
    };
    let mut state = KeyState::new()
        .label(ascii(&face.label))
        .detail(ascii(&face.detail));
    if face.series.len() >= 2 {
        state = state.series(face.series);
    }
    match change.map(|c| c.direction) {
        Some(Direction::Up) => state = state.accent(Accent::Green),
        Some(Direction::Down) => state = state.accent(Accent::Red),
        Some(Direction::Flat) | None => {}
    }
    let lag = day_number(&latest.d).map(|day| utc_day(now_unix) - day);
    let fetched = age(entry.fetched_at, None, Some(now_unix), None);
    if lag.is_some_and(|days| days > MAX_RATE_LAG_DAYS) {
        let days = lag.unwrap_or(0).min(999);
        return state
            .badge(format!("{days}d"))
            .indicator(Indicator::Warning);
    }
    match fetched {
        Some(age) if age < entry.ttl => state,
        Some(age) => state.badge(age_badge(age)).indicator(Indicator::Warning),
        None => state.badge("?").indicator(Indicator::Warning),
    }
}

/// The state handler of `rate`: the key's pair, its stored rates and the
/// time; nothing is fetched. A pair that is not one is the action's error.
pub fn rate_state(ctx: &mut Context<'_>, args: &Args<'_>) -> Result<KeyState, PluginError> {
    let pair = crate::pair(args)?;
    let entry = crate::cache::stored_rates(ctx, &pair)?;
    let now = ctx.now()?;
    Ok(rate(entry.as_ref(), &pair, now.unix_seconds()))
}
