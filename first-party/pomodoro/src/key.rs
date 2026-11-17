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
//! Texts are drawn by DeckPilot's key font as they are (`·`, Polish
//! letters, DeckPilot #307); a character the font lacks is folded or
//! dropped ([`key_text`]), never shown as `?`.

use deckpilot_plugin_sdk::prelude::*;

use crate::{Phase, Snapshot, State, Timer};

/// A running countdown without a tick for this long is marked as late.
pub const LATE_TICK_MINUTES: i64 = 3;

/// Whether DeckPilot's key font draws `c` (DeckPilot #307): ASCII, the
/// Latin-1 Supplement, Latin Extended-A but `Ĳ ĳ ĸ Ŋ ŋ Œ œ ſ`, and `…`,
/// `€`, `•`, typographic quotes, primes and dashes. Anything else would be
/// drawn as `?`.
pub fn drawn(c: char) -> bool {
    match u32::from(c) {
        0x20..=0x7E | 0xA0..=0xFF => true,
        0x100..=0x17F => !matches!(c, 'Ĳ' | 'ĳ' | 'ĸ' | 'Ŋ' | 'ŋ' | 'Œ' | 'œ' | 'ſ'),
        _ => matches!(
            c,
            '…' | '€'
                | '•'
                | '‘'
                | '’'
                | '‚'
                | '′'
                | '“'
                | '”'
                | '„'
                | '″'
                | '‐'
                | '‑'
                | '‒'
                | '–'
                | '—'
                | '−'
        ),
    }
}

/// A key text: what [`drawn`] passes as it is, the letters it lacks folded
/// to ASCII (`Œ` as `OE`), anything else dropped, so nothing becomes `?`;
/// cut to `max` characters.
pub fn key_text(text: &str, max: usize) -> String {
    let mut out = String::new();
    for c in text.chars() {
        let folded: &str = match c {
            c if drawn(c) => {
                out.push(c);
                continue;
            }
            'Ĳ' => "IJ",
            'ĳ' => "ij",
            'ĸ' => "k",
            'Ŋ' => "N",
            'ŋ' => "n",
            'Œ' => "OE",
            'œ' => "oe",
            'ſ' => "s",
            _ => "",
        };
        out.push_str(folded);
    }
    out.chars().take(max).collect()
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
        .label(key_text(&face.label, 32))
        .detail(key_text(&face.detail, 32))
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
