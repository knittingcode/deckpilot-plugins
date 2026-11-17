//! What a key bound to `status` shows (plugin API 1.2, key state): the
//! countdowns to the next break and the next glass of water, or that the
//! reminders are paused, in quiet hours, or waiting for the first tick.
//!
//! The countdowns move only when the every-minute workflow runs `tick`, so
//! a key shows the minutes as of the last tick. When the reminders should
//! be counting but no tick has come for [`LATE_TICK_MINUTES`] minutes, the
//! key does not pretend to count: it adds a warning and the time since the
//! last tick as a badge. With the time (`time.now`) it can tell; nothing
//! stored is changed.
//!
//! Texts keep their Polish letters, which DeckPilot's key font draws
//! (DeckPilot #307).

use deckpilot_plugin_sdk::prelude::*;

use crate::{Config, Language, Phase, Snapshot, State};

/// Reminders that count without a tick for this long are marked as late.
pub const LATE_TICK_MINUTES: i64 = 3;

/// Minutes as a badge of at most 4 characters: `5m`, `3h`, `2d`.
pub fn minutes_badge(minutes: i64) -> String {
    let minutes = minutes.max(0);
    if minutes < 60 {
        format!("{minutes}m")
    } else if minutes < 48 * 60 {
        format!("{}h", minutes / 60)
    } else {
        format!("{}d", (minutes / (24 * 60)).min(999))
    }
}

/// `Break 12 min` / `Przerwa 12 min`, or due now.
fn countdown(break_: bool, left: u32, language: Language) -> String {
    let word = match (break_, language) {
        (true, Language::English) => "Break",
        (true, Language::Polish) => "Przerwa",
        (false, Language::English) => "Water",
        (false, Language::Polish) => "Woda",
    };
    match (left, language) {
        (0, Language::English) => format!("{word} now"),
        (0, Language::Polish) => format!("{word} teraz"),
        (left, _) => format!("{word} {left} min"),
    }
}

/// How much of the countdown to the sooner reminder is over, 0-100.
fn progress(state: &State, snapshot: &Snapshot) -> Option<u8> {
    let timers = [
        (snapshot.break_left, &state.break_),
        (snapshot.hydration_left, &state.hydration),
    ];
    let (left, timer) = timers
        .into_iter()
        .filter_map(|(left, timer)| left.map(|left| (left, timer)))
        .min_by_key(|(left, _)| *left)?;
    if timer.interval == 0 {
        return None;
    }
    let done = timer.interval.saturating_sub(left.min(timer.interval));
    u8::try_from(u64::from(done) * 100 / u64::from(timer.interval)).ok()
}

/// The key state of `snapshot` (of `state` under `config`), at the local
/// minute `now`.
pub fn status(snapshot: &Snapshot, state: &State, config: &Config, now: i64) -> KeyState {
    let language = config.language;
    let pl = language == Language::Polish;
    let mut lines: Vec<String> = Vec::new();
    if let Some(left) = snapshot.break_left {
        lines.push(countdown(true, left, language));
    }
    if let Some(left) = snapshot.hydration_left {
        lines.push(countdown(false, left, language));
    }
    if lines.is_empty() {
        lines.push(if pl { "Wyłączone" } else { "Off" }.to_owned());
    }
    let mut key = match snapshot.phase {
        Phase::Waiting => KeyState::new()
            .label(if pl { "Czeka na takt" } else { "Waiting" })
            .detail(lines.join(" "))
            .badge("?")
            .indicator(Indicator::Warning),
        Phase::Paused => {
            let detail = match (snapshot.pause_left, pl) {
                (Some(left), false) => format!("{left} min left"),
                (Some(left), true) => format!("jeszcze {left} min"),
                (None, false) => "until resumed".to_owned(),
                (None, true) => "do wznowienia".to_owned(),
            };
            KeyState::new()
                .label(if pl { "Pauza" } else { "Paused" })
                .detail(detail)
                .accent(Accent::Grey)
                .indicator(Indicator::Warning)
                .toggle(false)
        }
        Phase::Quiet => KeyState::new()
            .label(if pl { "Godziny ciszy" } else { "Quiet hours" })
            .detail(lines.join(" "))
            .accent(Accent::Purple)
            .indicator(Indicator::Unknown),
        Phase::Running => {
            let due = [snapshot.break_left, snapshot.hydration_left]
                .into_iter()
                .flatten()
                .any(|left| left == 0);
            let mut key = KeyState::new()
                .label(lines[0].clone())
                .accent(if due { Accent::Amber } else { Accent::Teal })
                .indicator(Indicator::Running)
                .toggle(true);
            if let Some(second) = lines.get(1) {
                key = key.detail(second.clone());
            }
            key
        }
    };
    if let Some(progress) = progress(state, snapshot).filter(|_| snapshot.phase == Phase::Running) {
        key = key.progress(progress);
    }
    // Ticks count in running and quiet hours alike; a late one is said.
    if matches!(snapshot.phase, Phase::Running | Phase::Quiet)
        && let Some(last) = state.last_tick
    {
        let since = now - last;
        if since >= LATE_TICK_MINUTES {
            key = key
                .badge(minutes_badge(since))
                .indicator(Indicator::Warning);
        }
    }
    key
}

/// The state handler of `status`: the settings and the stored state, read
/// as `status` reads them, and the time; nothing is written.
pub fn status_state(ctx: &mut Context<'_>, _args: &Args<'_>) -> Result<KeyState, PluginError> {
    let (config, state) = crate::load(ctx)?;
    let snapshot = Snapshot::of(&state, &config);
    let now = ctx.now()?;
    Ok(status(
        &snapshot,
        &state,
        &config,
        now.local_seconds().div_euclid(60),
    ))
}
