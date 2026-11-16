//! DeckPilot's air quality plugin: PM2.5, PM10 and the air quality index
//! of one measuring station, from GIOŚ (Poland's Chief Inspectorate of
//! Environmental Protection).
//!
//! Four actions. `current` reports the chosen station as a message, as
//! outputs for workflows and as the plugin's own variables. `find-station`
//! searches the provider's station list by city or name, `select-station`
//! stores one of them, `forget-station` deletes the choice.
//!
//! What plugin API 1.0 does not have, and this plugin therefore does not
//! do: it cannot draw on a key (no value, no colour), it cannot refresh by
//! itself (no timers), and it has no clock ([`cache`] explains what that
//! means for the age of what it shows).
//!
//! Freshness is the point of an air quality reading, so it is never
//! implied. Every value carries the time the provider measured it, and a
//! reading is marked `stale` unless the plugin can show that it is recent:
//! fetched within its lifetime **and** measured within
//! [`STALE_AFTER_SECONDS`] of a known present time.
//!
//! The station a user chooses says where they are. It goes to the provider
//! (in the request) and into the plugin's storage, a message, an output and
//! a variable, and never into the log. Coordinates are not read at all.
#![deny(unsafe_code)]

use deckpilot_plugin_sdk::prelude::*;
use serde::Serialize;

pub mod cache;
pub mod gios;
pub mod provider;
pub mod texts;
pub mod time;

use cache::{Chosen, Served, Source};
use gios::Gios;
use provider::{NO_LEVEL, Provider, Reading, Station};
use texts::{Language, UNIT, fold, one_line};

/// Must equal `manifest.json` (`id`, `version`).
pub const ID: &str = "deckpilot.air-quality";
pub const VERSION: &str = "0.1.0";

/// Error codes of this plugin, each declared in `contributes.errorCodes`.
pub const NO_STATION: &str = "no-station";
pub const STATION_NOT_FOUND: &str = "station-not-found";
pub const NO_DATA: &str = "no-data";
pub const PROVIDER_ERROR: &str = "provider-error";
pub const INVALID_RESPONSE: &str = "invalid-response";
pub const RESPONSE_TOO_LARGE: &str = "response-too-large";
/// Reported by the SDK for a request that timed out; not a shared code.
pub const TIMED_OUT: &str = "timed-out";
pub const PLUGIN_CODES: &[&str] = &[
    NO_STATION,
    STATION_NOT_FOUND,
    NO_DATA,
    PROVIDER_ERROR,
    INVALID_RESPONSE,
    RESPONSE_TOO_LARGE,
    TIMED_OUT,
];

/// The plugin's own variables (`contributes.variables`), read as
/// `{{ plugins.deckpilot.airQuality.<id> }}`. One run publishes or clears
/// each once, which fits the agent's ten a second.
pub const VARIABLES: &[&str] = &[
    "pm25",
    "pm10",
    "index",
    "category",
    "categoryName",
    "summary",
    "measuredAt",
    "stale",
    "station",
];

/// A measurement older than this is not current. GIOŚ publishes an hour's
/// value during the hour after it (seen on 2026-11-16: the values of 19:00
/// at 19:42), so a station that works stays below two hours; three allow
/// one missed publication.
pub const STALE_AFTER_SECONDS: u64 = 3 * 3_600;
/// A measurement time this far after the present is not believed.
const FUTURE_TOLERANCE_SECONDS: u64 = 3_600;
/// Stations `find-station` lists.
pub const MAX_MATCHES: usize = 8;
/// The agent's limit for a message.
const MAX_MESSAGE_CHARS: usize = 1_024;

/// The plugin definition. The action ids, their order and their parameters
/// must equal `manifest.json`; a unit test compares them.
pub fn plugin() -> Plugin {
    Plugin::new(ID, VERSION)
        .action(Action::new("current", current).param(Param::text("at").max_length(16)))
        .action(
            Action::new("find-station", find_station)
                .param(Param::text("query").required().max_length(80)),
        )
        .action(
            Action::new("select-station", select_station).param(
                Param::integer("station")
                    .required()
                    .min(1.0)
                    .max(99_999_999.0),
            ),
        )
        .action(Action::new("forget-station", forget_station))
}

deckpilot_plugin_sdk::plugin!(plugin);

/// The settings an action needs. The `provider` setting has one value in
/// this version, so there is nothing to read from it yet.
fn language(ctx: &mut Context<'_>) -> Result<Language, PluginError> {
    let settings = ctx.settings()?;
    Ok(Language::from_setting(settings.text("language")))
}

/// The run's local time from the optional `at` parameter.
fn run_time(args: &Args<'_>) -> Result<Option<i64>, PluginError> {
    let at: Option<String> = args.get("at")?;
    match at.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        None => Ok(None),
        Some(text) => time::local_minutes(text).map(Some).ok_or_else(|| {
            PluginError::new(
                codes::INVALID_PARAMETERS,
                "at: expected a schedule trigger's firing time, YYYY-MM-DD HH:MM",
            )
        }),
    }
}

/// Publishing a variable is best effort: the action's result does not
/// depend on it. `None` clears it, so a value that is gone does not linger.
fn publish<T: Serialize>(ctx: &mut Context<'_>, id: &str, value: Option<T>) {
    let done = match value {
        Some(value) => ctx.publish_variable(id, value).is_ok(),
        None => ctx.clear_variable(id).is_ok(),
    };
    if !done {
        ctx.warn(format!("the variable {id} could not be published"));
    }
}

/// A message within the agent's limit.
fn clip(text: String) -> String {
    if text.chars().count() <= MAX_MESSAGE_CHARS {
        return text;
    }
    let mut clipped: String = text.chars().take(MAX_MESSAGE_CHARS - 1).collect();
    clipped.push('…');
    clipped
}

/// `2026-11-16 19:00:00` as `2026-11-16 19:00`.
fn minute(time: &str) -> &str {
    time.strip_suffix(":00").unwrap_or(time)
}

/// A concentration as a whole number of µg/m³.
fn rounded(value: f64) -> i64 {
    value.round() as i64
}

/// `3 h 20 min`.
fn duration(seconds: u64) -> String {
    let minutes = seconds / 60;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// How fresh a served reading is.
struct Freshness {
    /// The oldest and the newest time of the values reported.
    oldest: String,
    newest: String,
    /// Seconds from the oldest value to the present, when both are known.
    measurement_age: Option<u64>,
    /// True unless the reading is known to be recent.
    stale: bool,
}

fn freshness(served: &Served, provider: &dyn Provider) -> Freshness {
    let times = served.reading.times();
    let oldest = times.iter().min().copied().unwrap_or_default();
    let newest = times.iter().max().copied().unwrap_or_default();
    let measurement_age = match (served.now, provider.unix(oldest)) {
        (Some(now), Some(measured)) if measured <= now.saturating_add(FUTURE_TOLERANCE_SECONDS) => {
            Some(now.saturating_sub(measured))
        }
        _ => None,
    };
    Freshness {
        oldest: minute(oldest).to_owned(),
        newest: minute(newest).to_owned(),
        measurement_age,
        stale: served.expired || !measurement_age.is_some_and(|age| age <= STALE_AFTER_SECONDS),
    }
}

/// `[stale] ` in front of a text that is not known to be current.
fn stale_mark(stale: bool, language: Language) -> &'static str {
    if stale {
        language.pick("[stale] ", "[nieaktualne] ")
    } else {
        ""
    }
}

/// What a station reports, as one sentence group: the index with its
/// scale, the PM values, the index of other pollutants, the time.
fn sentence(
    chosen: &Chosen,
    reading: &Reading,
    fresh: &Freshness,
    provider: &dyn Provider,
    language: Language,
) -> String {
    let pl = language == Language::Pl;
    let mut text = format!("{}: ", one_line(&chosen.station.label()));
    match reading.index.and_then(|i| Some((i, provider.level(i)?))) {
        Some((index, level)) => text.push_str(&if pl {
            format!(
                "jakość powietrza: {} (indeks {index}; {}).",
                level.pl,
                provider.scale(language)
            )
        } else {
            format!(
                "air quality {} (index {index}; {}).",
                level.en,
                provider.scale(language)
            )
        }),
        None => text.push_str(language.pick(
            "no air quality index now.",
            "brak indeksu jakości powietrza.",
        )),
    }
    let pm = [
        ("PM2.5", &reading.pm25, chosen.sensors.pm25.is_some()),
        ("PM10", &reading.pm10, chosen.sensors.pm10.is_some()),
    ]
    .map(|(name, sample, measured)| match (sample, measured) {
        (Some(sample), _) => format!("{name} {} {UNIT}", rounded(sample.value)),
        (None, true) => format!(
            "{name}: {}",
            language.pick("no recent value", "brak aktualnej wartości")
        ),
        (None, false) => format!(
            "{name}: {}",
            language.pick("not measured here", "nie jest tu mierzony")
        ),
    });
    text.push_str(&format!(" {}, {}.", pm[0], pm[1]));
    let others: Vec<String> = [
        ("NO2", reading.no2),
        ("O3", reading.o3),
        ("SO2", reading.so2),
    ]
    .into_iter()
    .filter_map(|(name, index)| {
        Some(format!(
            "{name}: {}",
            provider.level(index?)?.text(language)
        ))
    })
    .collect();
    if !others.is_empty() {
        text.push_str(&format!(
            " {} {}.",
            language.pick("Index by pollutant:", "Indeks zanieczyszczeń:"),
            others.join(", ")
        ));
    }
    let when = if fresh.oldest == fresh.newest {
        fresh.newest.clone()
    } else {
        format!(
            "{} {} {}",
            fresh.oldest,
            language.pick("to", "do"),
            fresh.newest
        )
    };
    text.push_str(&if pl {
        format!(" Pomiar: {when} (czas polski).")
    } else {
        format!(" Measured {when} (Polish time).")
    });
    text
}

/// What the user must know about the freshness of a reading before they
/// read it; empty for a reading known to be current.
fn warning(
    served: &Served,
    fresh: &Freshness,
    provider: &dyn Provider,
    language: Language,
) -> String {
    let pl = language == Language::Pl;
    if !fresh.stale {
        return String::new();
    }
    if let (true, Some(problem)) = (served.expired, served.problem.as_deref()) {
        let when = match (
            served.fetched_at.map(time::utc_text),
            served.age.map(duration),
        ) {
            (Some(at), Some(age)) if pl => format!("{at}, około {age} temu"),
            (Some(at), Some(age)) => format!("{at}, about {age} ago"),
            (Some(at), None) if pl => {
                format!("{at}; wtyczka nie ma zegara i nie wie, jak dawno to było")
            }
            (Some(at), None) => {
                format!("{at}; the plugin has no clock and cannot tell how old that is")
            }
            (None, Some(age)) if pl => format!("około {age} temu"),
            (None, Some(age)) => format!("about {age} ago"),
            (None, None) if pl => "nieznanego czasu".into(),
            (None, None) => "an unknown time".into(),
        };
        return if pl {
            format!(
                "Nieaktualne: {} nie podał nowych danych ({problem}). Zapisane dane pobrane {when}. ",
                provider.name()
            )
        } else {
            format!(
                "Not current: {} gave no new data ({problem}). Saved data fetched {when}. ",
                provider.name()
            )
        };
    }
    match fresh.measurement_age {
        Some(age) if pl => format!(
            "Nieaktualne: najstarsza z tych wartości ma około {}. ",
            duration(age)
        ),
        Some(age) => format!(
            "Not current: the oldest of these values is about {} old. ",
            duration(age)
        ),
        None if pl => {
            "Być może nieaktualne: wtyczka nie ma zegara i nie mogła sprawdzić wieku tych wartości. "
                .into()
        }
        None => {
            "Possibly not current: the plugin has no clock and could not check the age of these values. "
                .into()
        }
    }
}

/// A note for a reading that is current but came from the cache.
fn cache_note(
    served: &Served,
    fresh: &Freshness,
    provider: &dyn Provider,
    language: Language,
) -> String {
    let pl = language == Language::Pl;
    let (false, Source::Cache, Some(age)) = (fresh.stale, served.source, served.age) else {
        return String::new();
    };
    // Still within its lifetime; a failed request is told all the same.
    let problem = match served.problem.as_deref() {
        Some(problem) if pl => format!("; {} nie podał nowych danych: {problem}", provider.name()),
        Some(problem) => format!("; {} gave no new data: {problem}", provider.name()),
        None => String::new(),
    };
    if pl {
        format!(
            " (z pamięci podręcznej, pobrano {} temu{problem})",
            duration(age)
        )
    } else {
        format!(" (from the cache, fetched {} ago{problem})", duration(age))
    }
}

fn current(ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let at = run_time(args)?;
    let language = language(ctx)?;
    let provider = Gios;
    let chosen = cache::chosen(ctx, &provider)?.ok_or_else(|| {
        PluginError::new(
            NO_STATION,
            "No station is chosen: run Find station with a city, then Select station with the number of the station you want",
        )
    })?;
    let served = match cache::reading(ctx, &provider, &chosen, at) {
        Ok(served) if !served.reading.is_empty() => served,
        outcome => {
            // Variables keep their last values: say that they are old.
            publish(ctx, "stale", Some(true));
            return Err(outcome.err().unwrap_or_else(|| {
                PluginError::new(
                    NO_DATA,
                    "The station reports no PM2.5 value, no PM10 value and no air quality index now; try later, or choose another station",
                )
            }));
        }
    };
    let reading = &served.reading;
    let fresh = freshness(&served, &provider);
    let level = reading.index.and_then(|index| provider.level(index));
    let category = level.unwrap_or(NO_LEVEL);
    let station = one_line(&chosen.station.label());
    let sentence = sentence(&chosen, reading, &fresh, &provider, language);
    let summary = format!(
        "{}{sentence} {}",
        stale_mark(fresh.stale, language),
        provider.source()
    );
    let (pm25, pm10) = (
        reading.pm25.as_ref().map(|s| rounded(s.value)),
        reading.pm10.as_ref().map(|s| rounded(s.value)),
    );
    let index = reading.index.filter(|_| level.is_some());

    publish(ctx, "pm25", pm25);
    publish(ctx, "pm10", pm10);
    publish(ctx, "index", index);
    publish(ctx, "category", Some(category.text(language)));
    publish(ctx, "categoryName", Some(category.name));
    publish(ctx, "summary", Some(&summary));
    publish(ctx, "measuredAt", Some(&fresh.oldest));
    publish(ctx, "stale", Some(fresh.stale));
    publish(ctx, "station", Some(&station));

    let message = format!(
        "{}{sentence}{} {}",
        warning(&served, &fresh, &provider, language),
        cache_note(&served, &fresh, &provider, language),
        provider.attribution(language)
    );
    let mut out = Output::message(clip(message))
        .with("summary", summary)
        .with("category", category.text(language))
        .with("categoryName", category.name)
        .with("unit", UNIT)
        .with("station", station)
        .with("provider", provider.id())
        .with("measuredAt", fresh.oldest.as_str())
        .with("stale", fresh.stale)
        .with("source", served.source.id())
        .with("attribution", provider.attribution(language));
    if let Some(index) = index {
        out = out.with("index", index);
    }
    if let (Some(_), Some(time)) = (index, &reading.index_time) {
        out = out.with("indexTime", minute(time));
    }
    for (key, value, sample) in [("pm25", pm25, &reading.pm25), ("pm10", pm10, &reading.pm10)] {
        if let (Some(value), Some(sample)) = (value, sample) {
            out = out
                .with(key, value)
                .with(format!("{key}Time"), minute(&sample.time));
        }
    }
    for (key, index) in [
        ("no2Category", reading.no2),
        ("o3Category", reading.o3),
        ("so2Category", reading.so2),
    ] {
        if let Some(level) = index.and_then(|index| provider.level(index)) {
            out = out.with(key, level.name);
        }
    }
    if let Some(age) = fresh.measurement_age {
        out = out.with(
            "measurementAgeMinutes",
            i64::try_from(age / 60).unwrap_or(i64::MAX),
        );
    }
    if let Some(at) = served.fetched_at {
        out = out.with("fetchedAt", time::utc_text(at));
    }
    if let Some(age) = served.age {
        out = out.with("ageMinutes", i64::try_from(age / 60).unwrap_or(i64::MAX));
    }
    if let Some(problem) = &served.problem {
        out = out.with("problem", problem.as_str());
    }
    Ok(out)
}

/// The stations a folded query matches: every word of the query must be in
/// the station's name, city or voivodeship. A station whose city is the
/// query comes first, then one whose city starts with it, then the rest;
/// the provider's order within each group.
pub fn search<'a>(stations: &'a [Station], query: &str) -> Vec<&'a Station> {
    let words: Vec<&str> = query.split(' ').filter(|w| !w.is_empty()).collect();
    let mut found: Vec<(u8, &Station)> = stations
        .iter()
        .filter_map(|station| {
            let city = fold(&station.city);
            let all = format!("{} {city} {}", fold(&station.name), fold(&station.region));
            let rank = if city == query {
                0
            } else if city.starts_with(query) {
                1
            } else {
                2
            };
            (!words.is_empty() && words.iter().all(|word| all.contains(word)))
                .then_some((rank, station))
        })
        .collect();
    found.sort_by_key(|(rank, _)| *rank);
    found.into_iter().map(|(_, station)| station).collect()
}

fn find_station(ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let query: String = args.get("query")?;
    let query = fold(&query);
    if query.chars().count() < 2 {
        return Err(PluginError::new(
            codes::INVALID_PARAMETERS,
            "query: type at least two letters of a city or a station name",
        ));
    }
    let language = language(ctx)?;
    let provider = Gios;
    let (stations, source) = cache::stations(ctx, &provider)?;
    let found = search(&stations, &query);
    let Some(first) = found.first() else {
        return Err(PluginError::new(
            STATION_NOT_FOUND,
            "No station matches; try the city alone, or a voivodeship (GIOŚ has stations in about 200 Polish towns)",
        ));
    };
    let lines: Vec<String> = found
        .iter()
        .take(MAX_MATCHES)
        .map(|station| format!("{}: {}", station.id, one_line(&station.label())))
        .collect();
    let (count, shown) = (found.len(), lines.len());
    let list = lines.join("; ");
    let message = match (language, count > shown) {
        (Language::En, false) => format!(
            "Found {count}: {list}. Run Select station with the number in front of the station you want. {}",
            provider.source()
        ),
        (Language::En, true) => format!(
            "Found {count}, the first {shown}: {list}. Add a word to narrow the search, or run Select station with the number in front of the station you want. {}",
            provider.source()
        ),
        (Language::Pl, false) => format!(
            "Znaleziono {count}: {list}. Uruchom Wybierz stację z numerem sprzed nazwy wybranej stacji. {}",
            provider.source()
        ),
        (Language::Pl, true) => format!(
            "Znaleziono {count}, pierwsze {shown}: {list}. Dodaj słowo, aby zawęzić wyszukiwanie, albo uruchom Wybierz stację z numerem sprzed nazwy wybranej stacji. {}",
            provider.source()
        ),
    };
    Ok(Output::message(clip(message))
        .with("count", i64::try_from(count).unwrap_or(i64::MAX))
        .with("shown", i64::try_from(shown).unwrap_or(i64::MAX))
        .with("matches", lines.join("\n"))
        .with("firstId", first.id)
        .with("source", source.id())
        .with("attribution", provider.attribution(language)))
}

fn select_station(ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let id: i64 = args.get("station")?;
    let language = language(ctx)?;
    let provider = Gios;
    let (stations, _) = cache::stations(ctx, &provider)?;
    let Some(station) = stations.iter().find(|station| station.id == id) else {
        return Err(PluginError::new(
            STATION_NOT_FOUND,
            "No station has this number; run Find station to list stations with their numbers",
        ));
    };
    let sensors = provider
        .sensors(ctx, station.id)
        .map_err(|failure| failure.error)?
        .value;
    cache::choose(ctx, &provider, station, sensors)?;
    // The variables describe the station chosen before.
    if ctx.clear_variables().is_err() {
        ctx.warn("the variables could not be cleared");
    }
    let label = one_line(&station.label());
    let pl = language == Language::Pl;
    let measures = match (sensors.pm25.is_some(), sensors.pm10.is_some(), pl) {
        (true, true, false) => "It measures PM2.5 and PM10.",
        (true, false, false) => "It measures PM2.5, not PM10.",
        (false, true, false) => "It measures PM10, not PM2.5.",
        (false, false, false) => {
            "It measures neither PM2.5 nor PM10: Current air quality will report its index only."
        }
        (true, true, true) => "Mierzy PM2.5 i PM10.",
        (true, false, true) => "Mierzy PM2.5, nie mierzy PM10.",
        (false, true, true) => "Mierzy PM10, nie mierzy PM2.5.",
        (false, false, true) => {
            "Nie mierzy ani PM2.5, ani PM10: Aktualna jakość powietrza poda tylko jej indeks."
        }
    };
    let message = if pl {
        format!("Wybrano stację: {label}. {measures} {}", provider.source())
    } else {
        format!("Station set to {label}. {measures} {}", provider.source())
    };
    Ok(Output::message(clip(message))
        .with("selected", label)
        .with("stationId", station.id)
        .with("hasPm25", sensors.pm25.is_some())
        .with("hasPm10", sensors.pm10.is_some())
        .with("attribution", provider.attribution(language)))
}

fn forget_station(ctx: &mut Context<'_>, _args: &Args<'_>) -> ActionResult {
    let language = language(ctx)?;
    cache::forget(ctx)?;
    if ctx.clear_variables().is_err() {
        ctx.warn("the variables could not be cleared");
    }
    Ok(Output::message(language.pick(
        "The chosen station and its saved values were deleted. The list of GIOŚ stations stays for the next search.",
        "Usunięto wybraną stację i jej zapisane wartości. Lista stacji GIOŚ pozostaje na potrzeby następnego wyszukiwania.",
    )))
}

#[cfg(test)]
mod tests;
