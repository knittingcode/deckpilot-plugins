//! DeckPilot's weather plugin: the current conditions and a forecast of a
//! few days for one place, from Open-Meteo.
//!
//! Four actions. `current` and `forecast` report the weather of the
//! configured place as a message, as outputs for workflows and as the
//! plugin's own variables. `find-location` searches a place by name and
//! stores the one the user picks; `forget-location` deletes everything
//! stored. The place is the latitude and longitude of the settings when
//! both are set, otherwise the stored one.
//!
//! What plugin API 1.0 does not have, and this plugin therefore does not
//! do: it cannot draw on a key (no temperature, no icon), it cannot refresh
//! by itself (no timers), and it has no clock ([`cache`] explains what that
//! means for the age of cached data). It cannot write settings either,
//! which is why a place found by name lives in the plugin's storage.
//!
//! Coordinates are personal data. They go to the provider (rounded to two
//! decimals) and into the plugin's storage, and nowhere else: not into the
//! log, a message, an output or a variable.
#![deny(unsafe_code)]

use deckpilot_plugin_sdk::prelude::*;
use serde::Serialize;

pub mod cache;
pub mod conditions;
pub mod open_meteo;
pub mod provider;
pub mod time;
pub mod units;

use cache::{Served, Source};
use conditions::{Language, condition};
use open_meteo::OpenMeteo;
use provider::{Current, Day, Place, Provider};
use units::{Units, compass};

/// Must equal `manifest.json` (`id`, `version`).
pub const ID: &str = "deckpilot.weather";
pub const VERSION: &str = "0.1.0";

/// Error codes of this plugin, each declared in `contributes.errorCodes`.
pub const NO_LOCATION: &str = "no-location";
pub const INVALID_LOCATION: &str = "invalid-location";
pub const LOCATION_NOT_FOUND: &str = "location-not-found";
pub const INVALID_SELECTION: &str = "invalid-selection";
pub const PROVIDER_ERROR: &str = "provider-error";
pub const INVALID_RESPONSE: &str = "invalid-response";
pub const RESPONSE_TOO_LARGE: &str = "response-too-large";
/// Reported by the SDK for a request that timed out; not a shared code.
pub const TIMED_OUT: &str = "timed-out";
pub const PLUGIN_CODES: &[&str] = &[
    NO_LOCATION,
    INVALID_LOCATION,
    LOCATION_NOT_FOUND,
    INVALID_SELECTION,
    PROVIDER_ERROR,
    INVALID_RESPONSE,
    RESPONSE_TOO_LARGE,
    TIMED_OUT,
];

/// The plugin's own variables (`contributes.variables`), read as
/// `{{ plugins.deckpilot.weather.<id> }}`.
pub const CURRENT_VARIABLES: &[&str] = &[
    "temperature",
    "feelsLike",
    "condition",
    "conditionName",
    "summary",
    "stale",
];
pub const FORECAST_VARIABLES: &[&str] = &["forecast", "forecastSummary", "forecastStale"];

/// Days `forecast` reports as outputs of their own (`day1Date`, ...); all
/// requested days are in `summary` and `json`.
pub const OUTPUT_DAYS: usize = 3;

/// The plugin definition. The action ids, their order and their parameters
/// must equal `manifest.json`; a unit test compares them.
pub fn plugin() -> Plugin {
    let at = || Param::text("at").max_length(16);
    Plugin::new(ID, VERSION)
        .action(Action::new("current", current).param(at()))
        .action(
            Action::new("forecast", forecast)
                .param(Param::integer("days").min(1.0).max(7.0).default(3))
                .param(at()),
        )
        .action(
            Action::new("find-location", find_location)
                .param(Param::text("query").required().max_length(80))
                .param(Param::integer("select").min(0.0).max(5.0).default(0)),
        )
        .action(Action::new("forget-location", forget_location))
}

deckpilot_plugin_sdk::plugin!(plugin);

/// The settings an action needs.
struct Config {
    units: Units,
    language: Language,
    /// The place of the settings: none, one, or a mistake to report.
    manual: Result<Option<Place>, PluginError>,
}

fn config(ctx: &mut Context<'_>) -> Result<Config, PluginError> {
    let settings = ctx.settings()?;
    Ok(Config {
        units: Units::from_setting(settings.text("units")),
        language: Language::from_setting(settings.text("language")),
        manual: manual_place(&settings),
    })
}

/// The place typed into the settings. Latitude and longitude are texts,
/// because a setting has no kind for a decimal number; a decimal comma is
/// accepted.
fn manual_place(settings: &Settings) -> Result<Option<Place>, PluginError> {
    let text = |key: &str| settings.text(key).unwrap_or_default().trim().to_owned();
    let (latitude, longitude) = (text("latitude"), text("longitude"));
    if latitude.is_empty() && longitude.is_empty() {
        return Ok(None);
    }
    let degrees = |text: &str, limit: f64| {
        text.replace(',', ".")
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite() && v.abs() <= limit)
    };
    let (Some(latitude), Some(longitude)) = (degrees(&latitude, 90.0), degrees(&longitude, 180.0))
    else {
        return Err(PluginError::new(
            INVALID_LOCATION,
            "Set both Latitude (-90 to 90) and Longitude (-180 to 180) in the settings as decimal degrees, or clear both and use Find location",
        ));
    };
    let timezone = text("timezone");
    if !timezone.is_empty() && !open_meteo::is_timezone(&timezone) {
        return Err(PluginError::new(
            INVALID_LOCATION,
            "The time zone in the settings is not a tz database name such as Europe/Warsaw; leave it empty to use the place's own",
        ));
    }
    Ok(Some(Place {
        name: cache::normalize_name(&text("locationName")),
        latitude,
        longitude,
        timezone: (!timezone.is_empty()).then_some(timezone),
        region: None,
        country: None,
    }))
}

/// The place to report: the settings' one, else the stored one.
fn place(ctx: &mut Context<'_>, config: &Config) -> Result<Place, PluginError> {
    if let Some(place) = config.manual.clone()? {
        return Ok(place);
    }
    cache::location(ctx)?.ok_or_else(|| {
        PluginError::new(
            NO_LOCATION,
            "No location is set: run Find location with a place name, or enter a latitude and longitude in the plugin's settings",
        )
    })
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

fn place_name(place: &Place, language: Language) -> String {
    match (place.name.is_empty(), language) {
        (false, _) => place.name.clone(),
        (true, Language::En) => "Your location".into(),
        (true, Language::Pl) => "Twoja lokalizacja".into(),
    }
}

/// Publishing a variable is best effort: the action's result does not
/// depend on it.
fn publish(ctx: &mut Context<'_>, id: &str, value: impl Serialize) {
    if ctx.publish_variable(id, value).is_err() {
        ctx.warn(format!("the variable {id} could not be published"));
    }
}

/// `[stale] ` in front of a text that is not known to be current.
fn stale_mark(stale: bool, language: Language) -> &'static str {
    match (stale, language) {
        (false, _) => "",
        (true, Language::En) => "[stale] ",
        (true, Language::Pl) => "[nieaktualne] ",
    }
}

/// The message of a weather action: the sentence, with what the user must
/// know about its freshness, and the attribution the licence asks for.
fn message(sentence: &str, served: &Served, provider: &dyn Provider, language: Language) -> String {
    let minutes = served.age.map(|seconds| seconds / 60);
    let mut text = String::new();
    if served.stale {
        let problem = served.problem.as_deref().unwrap_or("unknown");
        let when = match (served.fetched_at.map(time::utc_text), minutes, language) {
            (Some(at), Some(m), Language::En) => format!("{at}, about {m} min ago"),
            (Some(at), Some(m), Language::Pl) => format!("{at}, około {m} min temu"),
            (Some(at), None, Language::En) => {
                format!("{at}; the plugin has no clock and cannot tell how old that is")
            }
            (Some(at), None, Language::Pl) => {
                format!("{at}; wtyczka nie ma zegara i nie wie, jak dawno to było")
            }
            (None, Some(m), Language::En) => format!("about {m} min ago"),
            (None, Some(m), Language::Pl) => format!("około {m} min temu"),
            (None, None, Language::En) => "an unknown time".into(),
            (None, None, Language::Pl) => "nieznanego czasu".into(),
        };
        text.push_str(&match language {
            Language::En => format!(
                "Not current: {} gave no new data ({problem}). Saved data from {when}. ",
                provider.name()
            ),
            Language::Pl => format!(
                "Nieaktualne: {} nie podał nowych danych ({problem}). Zapisane dane z {when}. ",
                provider.name()
            ),
        });
    }
    text.push_str(sentence);
    if !served.stale
        && served.source == Source::Cache
        && let Some(m) = minutes
    {
        // Still within its lifetime; a failed request is told all the same.
        let problem = served.problem.as_deref().map(|problem| match language {
            Language::En => format!("; {} gave no new data: {problem}", provider.name()),
            Language::Pl => format!("; {} nie podał nowych danych: {problem}", provider.name()),
        });
        let problem = problem.unwrap_or_default();
        text.push_str(&match language {
            Language::En => format!(" (from the cache, {m} min old{problem})"),
            Language::Pl => format!(" (z pamięci podręcznej, sprzed {m} min{problem})"),
        });
    }
    text.push(' ');
    text.push_str(provider.attribution());
    text
}

/// The outputs every weather action has about where its data is from.
fn with_origin(mut out: Output, served: &Served, provider: &dyn Provider) -> Output {
    out = out
        .with("stale", served.stale)
        .with("source", served.source.id())
        .with("attribution", provider.attribution());
    if let Some(at) = served.fetched_at {
        out = out.with("fetchedAt", time::utc_text(at));
    }
    if let Some(age) = served.age {
        out = out.with("ageMinutes", i64::try_from(age / 60).unwrap_or(i64::MAX));
    }
    if let Some(problem) = &served.problem {
        out = out.with("problem", problem.as_str());
    }
    out
}

fn current_sentence(name: &str, c: &Current, units: Units, language: Language) -> String {
    let unit = units.temperature_unit();
    let (feels, wind, humidity) = match language {
        Language::En => ("feels like", "Wind", "humidity"),
        Language::Pl => ("odczuwalna", "Wiatr", "wilgotność"),
    };
    format!(
        "{name}: {}, {} {unit} ({feels} {} {unit}). {wind} {} {} {}, {humidity} {}%.",
        condition(c.code).text(language),
        units.temperature(c.temperature),
        units.temperature(c.feels_like),
        units.wind(c.wind_speed),
        units.wind_unit(),
        compass(c.wind_direction),
        c.humidity,
    )
}

fn current(ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let at = run_time(args)?;
    let config = config(ctx)?;
    let place = place(ctx, &config)?;
    let provider = OpenMeteo;
    let served = match cache::weather(ctx, &provider, &place, at, cache::CURRENT_TTL_SECONDS) {
        Ok(served) => served,
        Err(error) => {
            // Variables keep their last values: say that they are old.
            publish(ctx, "stale", true);
            return Err(error);
        }
    };
    let (units, language) = (config.units, config.language);
    let c = &served.report.current;
    let name = place_name(&place, language);
    let sentence = current_sentence(&name, c, units, language);
    let summary = format!("{}{sentence}", stale_mark(served.stale, language));
    let kind = condition(c.code);

    publish(ctx, "temperature", units.temperature(c.temperature));
    publish(ctx, "feelsLike", units.temperature(c.feels_like));
    publish(ctx, "condition", kind.text(language));
    publish(ctx, "conditionName", kind.name);
    publish(ctx, "summary", &summary);
    publish(ctx, "stale", served.stale);

    let out = Output::message(message(&sentence, &served, &provider, language))
        .with("summary", summary)
        .with("condition", kind.text(language))
        .with("conditionName", kind.name)
        .with("weatherCode", c.code)
        .with("isDay", c.is_day)
        .with("temperature", units.temperature(c.temperature))
        .with("feelsLike", units.temperature(c.feels_like))
        .with("temperatureUnit", units.temperature_unit())
        .with("windSpeed", units.wind(c.wind_speed))
        .with("windUnit", units.wind_unit())
        .with("windDirection", c.wind_direction)
        .with("windCompass", compass(c.wind_direction))
        .with("humidity", c.humidity)
        .with("units", units.id())
        .with("location", name)
        .with("observedAt", c.time.as_str());
    Ok(with_origin(out, &served, &provider))
}

fn day_line(day: &Day, units: Units, language: Language) -> String {
    let unit = units.temperature_unit();
    let (min, max) = (units.temperature(day.min), units.temperature(day.max));
    let text = condition(day.code).text(language);
    let mut line = match language {
        Language::En => format!("{}: {text}, {min} to {max} {unit}", day.date),
        Language::Pl => format!("{}: {text}, od {min} do {max} {unit}", day.date),
    };
    if let Some(p) = day.precipitation_probability {
        line.push_str(&match language {
            Language::En => format!(", precipitation {p}%"),
            Language::Pl => format!(", opady {p}%"),
        });
    }
    line
}

fn forecast(ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let wanted: i64 = args.get("days")?;
    let at = run_time(args)?;
    let config = config(ctx)?;
    let place = place(ctx, &config)?;
    let provider = OpenMeteo;
    let served = match cache::weather(ctx, &provider, &place, at, cache::FORECAST_TTL_SECONDS) {
        Ok(served) => served,
        Err(error) => {
            publish(ctx, "forecastStale", true);
            return Err(error);
        }
    };
    let (units, language) = (config.units, config.language);
    let wanted = usize::try_from(wanted).unwrap_or(1).max(1);
    let days: Vec<&Day> = served.report.days.iter().take(wanted).collect();
    let name = place_name(&place, language);
    let lines: Vec<String> = days.iter().map(|d| day_line(d, units, language)).collect();
    let mark = stale_mark(served.stale, language);
    let summary = format!("{mark}{}", lines.join("\n"));
    let json = serde_json::Value::Array(
        days.iter()
            .map(|d| {
                let kind = condition(d.code);
                serde_json::json!({
                    "date": d.date,
                    "min": units.temperature(d.min),
                    "max": units.temperature(d.max),
                    "weatherCode": d.code,
                    "condition": kind.text(language),
                    "conditionName": kind.name,
                    "precipitationProbability": d.precipitation_probability,
                })
            })
            .collect(),
    );

    publish(ctx, "forecast", &json);
    publish(ctx, "forecastSummary", &summary);
    publish(ctx, "forecastStale", served.stale);

    let sentence = format!("{name}: {}.", lines.join(" | "));
    let mut out = Output::message(message(&sentence, &served, &provider, language))
        .with("summary", summary)
        .with("json", json.to_string())
        .with("days", i64::try_from(days.len()).unwrap_or(0))
        .with("temperatureUnit", units.temperature_unit())
        .with("units", units.id())
        .with("location", name);
    for (i, day) in days.iter().take(OUTPUT_DAYS).enumerate() {
        let n = i + 1;
        let kind = condition(day.code);
        out = out
            .with(format!("day{n}Date"), day.date.as_str())
            .with(format!("day{n}Min"), units.temperature(day.min))
            .with(format!("day{n}Max"), units.temperature(day.max))
            .with(format!("day{n}Condition"), kind.text(language))
            .with(format!("day{n}ConditionName"), kind.name);
        if let Some(p) = day.precipitation_probability {
            out = out.with(format!("day{n}Precipitation"), p);
        }
    }
    Ok(with_origin(out, &served, &provider))
}

fn find_location(ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let query: String = args.get("query")?;
    let select: i64 = args.get("select")?;
    let query = cache::normalize_query(&query);
    if query.chars().count() < 2 {
        return Err(PluginError::new(
            codes::INVALID_PARAMETERS,
            "query: type at least two characters of the place's name",
        ));
    }
    let config = config(ctx)?;
    let language = config.language;
    let provider = OpenMeteo;
    let (places, source) = cache::places(ctx, &provider, &query, language)?;
    if places.is_empty() {
        return Err(PluginError::new(
            LOCATION_NOT_FOUND,
            "No place with that name was found; try another spelling, or add the country after a comma (Springfield, US)",
        ));
    }
    let lines: Vec<String> = places
        .iter()
        .enumerate()
        .map(|(i, p)| match &p.timezone {
            Some(tz) => format!("{}. {} ({tz})", i + 1, p.label()),
            None => format!("{}. {}", i + 1, p.label()),
        })
        .collect();
    let count = i64::try_from(places.len()).unwrap_or(0);
    let out = |text: String| {
        Output::message(format!("{text} {}", provider.places_attribution()))
            .with("count", count)
            .with("matches", lines.join("\n"))
            .with("source", source.id())
            .with("attribution", provider.places_attribution())
    };
    if select == 0 {
        let found = lines.join("; ");
        return Ok(out(match language {
            Language::En => format!(
                "Found {count}: {found}. Run again with Select set to the number of the place to use."
            ),
            Language::Pl => format!(
                "Znaleziono {count}: {found}. Uruchom ponownie z numerem miejsca w polu Wybór, aby go użyć."
            ),
        }));
    }
    let Some(chosen) = usize::try_from(select - 1).ok().and_then(|i| places.get(i)) else {
        return Err(PluginError::new(
            INVALID_SELECTION,
            format!("Select is {select}, but the search found only {count}"),
        ));
    };
    cache::set_location(ctx, chosen)?;
    let label = chosen.label();
    let mut text = match language {
        Language::En => format!("Location set to {label}."),
        Language::Pl => format!("Ustawiono lokalizację: {label}."),
    };
    // The settings win over a stored place; say so instead of leaving the
    // user with weather for another place.
    if !matches!(config.manual, Ok(None)) {
        text.push_str(match language {
            Language::En => {
                " The latitude and longitude in the settings take precedence: clear them to use this place."
            }
            Language::Pl => {
                " Szerokość i długość z ustawień mają pierwszeństwo: wyczyść je, aby użyć tego miejsca."
            }
        });
    }
    let mut result = out(text).with("selected", label);
    if let Some(tz) = &chosen.timezone {
        result = result.with("timezone", tz.as_str());
    }
    Ok(result)
}

fn forget_location(ctx: &mut Context<'_>, _args: &Args<'_>) -> ActionResult {
    let language = config(ctx)?.language;
    cache::forget(ctx)?;
    if ctx.clear_variables().is_err() {
        ctx.warn("the variables could not be cleared");
    }
    Ok(Output::message(match language {
        Language::En => {
            "The stored location, the cached weather and the place searches were deleted. A latitude and longitude in the settings stay."
        }
        Language::Pl => {
            "Usunięto zapisaną lokalizację, zapisaną pogodę i wyszukiwania miejsc. Szerokość i długość w ustawieniach pozostają."
        }
    }))
}

#[cfg(test)]
mod tests;
