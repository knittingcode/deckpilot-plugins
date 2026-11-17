//! What a key bound to `current` shows (plugin API 1.2, key state): the
//! index category of the last reading on its colour, and the PM values,
//! never fetched by the key itself.
//!
//! A state call only reads, so it cannot ask the provider: it shows what
//! the last run of `current` stored. With the time (`time.now`) it applies
//! the plugin's rule for freshness to the present: a reading is current
//! only if it was fetched within its lifetime ([`READING_TTL_SECONDS`])
//! **and** measured within [`STALE_AFTER_SECONDS`]. Otherwise the values
//! stay, marked with a warning and the age of the measurement as a badge,
//! or `?` when it is unknown. Freshness is never implied.
//!
//! Key texts are drawn by DeckPilot's key font: `PM2.5 12`, and Polish
//! category names as they are (`Zły`, DeckPilot #307); a character the
//! font lacks is folded or dropped ([`key_text`]), never shown as `?`.

use deckpilot_plugin_sdk::prelude::*;

use crate::cache::{READING_TTL_SECONDS, ReadingEntry, age};
use crate::provider::{NO_LEVEL, Provider};
use crate::texts::Language;
use crate::{FUTURE_TOLERANCE_SECONDS, STALE_AFTER_SECONDS};

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

/// The colour of an index category, as close to the provider's scale as
/// DeckPilot's palette allows: green shades for good air, amber for
/// moderate and sufficient, red for bad, purple for very bad.
pub fn accent(name: &str) -> Option<Accent> {
    Some(match name {
        "very-good" => Accent::Teal,
        "good" => Accent::Green,
        "moderate" | "sufficient" => Accent::Amber,
        "bad" => Accent::Red,
        "very-bad" => Accent::Purple,
        _ => return None,
    })
}

/// The key state for the stored reading `entry`, seen at `now_unix`
/// (seconds). Without a reading with something in it: the key as the user
/// designed it.
pub fn current(
    entry: Option<&ReadingEntry>,
    provider: &dyn Provider,
    now_unix: u64,
    language: Language,
) -> KeyState {
    let Some(entry) = entry.filter(|e| !e.reading.is_empty()) else {
        return KeyState::new();
    };
    let reading = &entry.reading;
    let level = reading.index.and_then(|index| provider.level(index));
    let category = level.unwrap_or(NO_LEVEL);
    let values: Vec<String> = [("PM2.5", &reading.pm25), ("PM10", &reading.pm10)]
        .into_iter()
        .filter_map(|(name, sample)| {
            sample
                .as_ref()
                .map(|s| format!("{name} {}", s.value.round() as i64))
        })
        .collect();
    let mut state = KeyState::new().label(key_text(category.text(language), 32));
    if !values.is_empty() {
        state = state.detail(values.join(" "));
    }
    if let Some(accent) = level.and_then(|l| accent(l.name)) {
        state = state.accent(accent);
    }
    // The rule of the action, against the present.
    let oldest = reading.times().into_iter().min();
    let measured_age = oldest
        .and_then(|t| provider.unix(t))
        .filter(|measured| *measured <= now_unix.saturating_add(FUTURE_TOLERANCE_SECONDS))
        .map(|measured| now_unix.saturating_sub(measured));
    let fetched_fresh = age(entry.fetched_at, None, Some(now_unix), None)
        .is_some_and(|age| age < READING_TTL_SECONDS);
    let measured_fresh = measured_age.is_some_and(|age| age <= STALE_AFTER_SECONDS);
    if fetched_fresh && measured_fresh {
        return state;
    }
    let badge = measured_age.map_or_else(|| "?".to_owned(), age_badge);
    state.badge(badge).indicator(Indicator::Warning)
}

/// The state handler of `current`: settings, the chosen station and the
/// stored reading, and the time; nothing is fetched.
pub fn current_state(ctx: &mut Context<'_>, _args: &Args<'_>) -> Result<KeyState, PluginError> {
    let language = crate::language(ctx)?;
    let provider = crate::gios::Gios;
    let Some(chosen) = crate::cache::chosen(ctx, &provider)? else {
        return Ok(KeyState::new());
    };
    let entry = crate::cache::stored_reading(ctx, &provider, &chosen)?;
    let now = ctx.now()?;
    Ok(current(
        entry.as_ref(),
        &provider,
        now.unix_seconds(),
        language,
    ))
}
