//! What a key bound to `current` shows (plugin API 1.2, key state): the
//! temperature and the conditions of the last report, never fetched by
//! the key itself.
//!
//! A state call only reads, so it cannot ask Open-Meteo: it shows what the
//! last run of `current` stored (a key press, or a scheduled workflow).
//! With the time (`time.now`) it knows how old that is: older than the
//! report's lifetime, or of an unknown age, it keeps the values but marks
//! them, with the age as a badge and a warning mark. It never shows old
//! data as current.
//!
//! Key texts are drawn by DeckPilot's key font: `12 °C`, and Polish
//! condition names as they are (DeckPilot #307); a character the font
//! lacks is folded or dropped ([`key_text`]), never shown as `?`.

use deckpilot_plugin_sdk::prelude::*;

use crate::cache::{CURRENT_TTL_SECONDS, WeatherEntry, age};
use crate::conditions::{Language, condition};
use crate::units::Units;

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

/// The accent of a condition: sun and moon, clouds and fog, rain, snow,
/// storms.
fn accent(name: &str, is_day: bool) -> Option<Accent> {
    Some(match name {
        "clear" | "mostly-clear" if is_day => Accent::Amber,
        "clear" | "mostly-clear" => Accent::Purple,
        "partly-cloudy" | "overcast" | "fog" => Accent::Grey,
        "drizzle" | "freezing-drizzle" | "rain" | "freezing-rain" | "rain-showers" => Accent::Blue,
        "snow" | "snow-showers" => Accent::Teal,
        "thunderstorm" | "thunderstorm-hail" => Accent::Red,
        _ => return None,
    })
}

/// The key state for the stored report `entry`, seen at `now_unix`
/// (seconds), in `units` and `language`. Without a report for the place:
/// the key as the user designed it.
pub fn current(
    entry: Option<&WeatherEntry>,
    now_unix: u64,
    units: Units,
    language: Language,
) -> KeyState {
    let Some(entry) = entry else {
        return KeyState::new();
    };
    let c = &entry.report.current;
    let kind = condition(c.code);
    let label = format!(
        "{} {}",
        units.temperature(c.temperature),
        units.temperature_unit()
    );
    let mut state = KeyState::new()
        .label(key_text(&label, 32))
        .detail(key_text(kind.text(language), 32));
    if let Some(accent) = accent(kind.name, c.is_day) {
        state = state.accent(accent);
    }
    match age(entry.fetched_at, None, Some(now_unix), None) {
        Some(age) if age < CURRENT_TTL_SECONDS => state,
        // Older than its lifetime: the values stay, marked with their age.
        Some(age) => state.badge(age_badge(age)).indicator(Indicator::Warning),
        // No date, or one in the future: the age is unknown.
        None => state.badge("?").indicator(Indicator::Warning),
    }
}

/// The state handler of `current`: settings, the place and the stored
/// report, and the time; nothing is fetched.
pub fn current_state(ctx: &mut Context<'_>, _args: &Args<'_>) -> Result<KeyState, PluginError> {
    let settings = ctx.settings()?;
    let units = Units::from_setting(settings.text("units"));
    let language = Language::from_setting(settings.text("language"));
    let place = match crate::manual_place(&settings) {
        Ok(Some(place)) => place,
        // A mistake in the settings is reported by `current` itself.
        Err(_) => return Ok(KeyState::new()),
        Ok(None) => match crate::cache::location(ctx)? {
            Some(place) => place,
            None => return Ok(KeyState::new()),
        },
    };
    let entry = crate::cache::stored_weather(ctx, &place)?;
    let now = ctx.now()?;
    Ok(current(entry.as_ref(), now.unix_seconds(), units, language))
}
