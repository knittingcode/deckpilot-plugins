//! What a key bound to `status` shows (plugin API 1.2, key state): the
//! minutes left and the phase ([`crate::Snapshot::key`]), a bar of the
//! phase's progress, and whether the countdown runs.
//!
//! The timer moves only when the every-minute workflow runs `tick`, so a
//! key shows the minutes as of the last tick. When the countdown says it
//! runs but no tick has come for [`LATE_TICK_MINUTES`] minutes, or none
//! ever came, the key does not pretend to count: it adds a warning and the
//! time since the last tick as a badge (`?` before the first). With the
//! time (`time.now`) it can tell; the timer itself is not changed.
//!
//! Texts are ASCII (DeckPilot's key font): `·` becomes `-`, Polish words
//! lose their diacritics.

use deckpilot_plugin_sdk::prelude::*;

use crate::{Phase, Snapshot, State, Timer};

/// A running countdown without a tick for this long is marked as late.
pub const LATE_TICK_MINUTES: i64 = 3;

/// The text with `·` as `-`, Polish letters folded to ASCII and anything
/// else outside ASCII dropped, cut to `max` characters.
pub fn ascii(text: &str, max: usize) -> String {
    text.chars()
        .filter_map(|c| match c {
            '·' => Some('-'),
            'ą' => Some('a'),
            'ć' => Some('c'),
            'ę' => Some('e'),
            'ł' => Some('l'),
            'ń' => Some('n'),
            'ó' => Some('o'),
            'ś' => Some('s'),
            'ź' | 'ż' => Some('z'),
            'Ą' => Some('A'),
            'Ć' => Some('C'),
            'Ę' => Some('E'),
            'Ł' => Some('L'),
            'Ń' => Some('N'),
            'Ó' => Some('O'),
            'Ś' => Some('S'),
            'Ź' | 'Ż' => Some('Z'),
            c if c.is_ascii() && !c.is_ascii_control() => Some(c),
            _ => None,
        })
        .take(max)
        .collect()
}

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

/// The key state of `snapshot` (of `state`), at the local minute `now`.
pub fn status(snapshot: &Snapshot, state: &State, now_local_minute: i64) -> KeyState {
    let face = snapshot.key();
    let mut key = KeyState::new()
        .label(ascii(&face.label, 32))
        .detail(ascii(&face.detail, 32))
        .progress(face.progress)
        .toggle(face.toggle)
        .accent(match snapshot.phase {
            Phase::Focus => Accent::Red,
            Phase::ShortBreak => Accent::Green,
            Phase::LongBreak => Accent::Teal,
        })
        .indicator(match snapshot.timer {
            Timer::Running => Indicator::Running,
            Timer::Paused => Indicator::Warning,
            Timer::Idle => Indicator::Unknown,
        });
    if snapshot.timer == Timer::Running {
        match state.last_tick {
            None => key = key.badge("?").indicator(Indicator::Warning),
            Some(last) => {
                let since = now_local_minute - last;
                if since >= LATE_TICK_MINUTES {
                    key = key
                        .badge(minutes_badge(since))
                        .indicator(Indicator::Warning);
                }
            }
        }
    }
    key
}

/// The state handler of `status`: the settings and the stored timer, read
/// as `status` reads them, and the time; nothing is written.
pub fn status_state(ctx: &mut Context<'_>, _args: &Args<'_>) -> Result<KeyState, PluginError> {
    let (config, state) = crate::load(ctx)?;
    let snapshot = Snapshot::of(&state, &config);
    let now = ctx.now()?;
    Ok(status(
        &snapshot,
        &state,
        now.local_seconds().div_euclid(60),
    ))
}
