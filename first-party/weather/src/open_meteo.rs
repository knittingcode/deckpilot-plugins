//! Open-Meteo (<https://open-meteo.com>), without a key: the free API for
//! non-commercial use. Terms as read on 2026-11-16 are in the README.
//!
//! Two origins, both in the manifest's `network` permission, `GET` only:
//! the forecast API and the geocoding API. One forecast request brings the
//! current conditions and the daily values together.

use deckpilot_plugin_sdk::prelude::*;
use serde::Deserialize;
use serde_json::Value;

use crate::cache::normalize_name;
use crate::conditions::Language;
use crate::provider::{
    Current, Day, Failure, Fetched, MAX_DAYS, MAX_MATCHES, Place, Provider, Report,
};
use crate::time::http_date;
use crate::{INVALID_RESPONSE, PROVIDER_ERROR, RESPONSE_TOO_LARGE};

pub const FORECAST_API: &str = "https://api.open-meteo.com/v1/forecast";
pub const GEOCODING_API: &str = "https://geocoding-api.open-meteo.com/v1/search";
/// The agent sends no User-Agent of its own for plugins.
const USER_AGENT: &str = concat!("DeckPilot-Weather/", env!("CARGO_PKG_VERSION"));
/// The answers used here have a few kilobytes. Anything much larger is not
/// the answer that was asked for and is not parsed. (The agent itself
/// refuses a body over 512 KiB.)
pub const MAX_BODY_BYTES: usize = 64 * 1024;
/// Waiting time assumed for an HTTP 429 without a usable `Retry-After`,
/// and the longest one honoured.
pub const DEFAULT_RETRY_SECONDS: u64 = 15 * 60;
pub const MAX_RETRY_SECONDS: u64 = 24 * 3_600;

const CURRENT_FIELDS: &str = "temperature_2m,apparent_temperature,relative_humidity_2m,\
wind_speed_10m,wind_direction_10m,weather_code,is_day";
const DAILY_FIELDS: &str =
    "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max";

#[derive(Debug, Clone, Copy, Default)]
pub struct OpenMeteo;

impl Provider for OpenMeteo {
    fn name(&self) -> &'static str {
        "Open-Meteo"
    }

    fn attribution(&self) -> &'static str {
        "Weather data by Open-Meteo.com (https://open-meteo.com/), CC BY 4.0"
    }

    fn places_attribution(&self) -> &'static str {
        "Location data based on GeoNames (https://www.geonames.org/), via Open-Meteo.com"
    }

    fn weather(&self, ctx: &mut Context<'_>, place: &Place) -> Result<Fetched<Report>, Failure> {
        let (body, date) = get(ctx, forecast_url(place))?;
        let report = parse_report(&body).map_err(|what| malformed(what).at(date))?;
        Ok(Fetched {
            value: report,
            date,
        })
    }

    fn search(
        &self,
        ctx: &mut Context<'_>,
        query: &str,
        language: Language,
    ) -> Result<Fetched<Vec<Place>>, Failure> {
        let (body, date) = get(ctx, search_url(query, language))?;
        let places = parse_places(&body).map_err(|what| malformed(what).at(date))?;
        Ok(Fetched {
            value: places,
            date,
        })
    }
}

/// The forecast request for a place: metric units, the place's own time
/// zone, coordinates of two decimals ([`Place::grid`]).
pub fn forecast_url(place: &Place) -> String {
    let timezone = place
        .timezone
        .as_deref()
        .filter(|tz| is_timezone(tz))
        .unwrap_or("auto");
    format!(
        "{FORECAST_API}?latitude={:.2}&longitude={:.2}&current={CURRENT_FIELDS}\
&daily={DAILY_FIELDS}&forecast_days={MAX_DAYS}&timezone={}\
&temperature_unit=celsius&wind_speed_unit=kmh",
        place.latitude,
        place.longitude,
        encode(timezone)
    )
}

pub fn search_url(query: &str, language: Language) -> String {
    format!(
        "{GEOCODING_API}?name={}&count={MAX_MATCHES}&language={}&format=json",
        encode(query),
        language.code()
    )
}

/// Whether a text has the shape of a tz database name (`Europe/Warsaw`,
/// `Etc/GMT+1`, `UTC`).
pub fn is_timezone(text: &str) -> bool {
    (1..=64).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'_' | b'-' | b'+'))
}

/// Percent-encoding of one query value (RFC 3986 unreserved characters
/// stay).
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn malformed(what: &str) -> Failure {
    Failure::new(
        INVALID_RESPONSE,
        format!("Open-Meteo's answer could not be read: {what}"),
    )
}

/// One `GET`: the parsed JSON body of a 2xx answer and the answer's `Date`.
/// The messages never contain the URL, whose query holds the coordinates.
fn get(ctx: &mut Context<'_>, url: String) -> Result<(Value, Option<u64>), Failure> {
    let request = Request::get(url)
        .header("Accept", "application/json")
        .header("User-Agent", USER_AGENT);
    let response = ctx.fetch(request).map_err(|e| fetch_failure(&e))?;
    let date = response.header("date").and_then(http_date);
    match response.status {
        200..=299 => {}
        429 => {
            let mut failure = Failure::new(
                codes::THROTTLED,
                "Open-Meteo's rate limit is reached (HTTP 429); try again later",
            )
            .retryable()
            .at(date);
            failure.retry_after = Some(retry_after(response.header("retry-after"), date));
            return Err(failure);
        }
        // The request itself is wrong (coordinates out of range, an unknown
        // time zone). The provider's reason may quote the coordinates, so
        // it is not repeated.
        400 => {
            return Err(Failure::new(
                PROVIDER_ERROR,
                "Open-Meteo refused the request (HTTP 400); check the location and the time zone in the settings",
            )
            .at(date)
            .final_answer());
        }
        status => {
            let failure = Failure::new(
                PROVIDER_ERROR,
                format!("Open-Meteo answered with an error (HTTP {status})"),
            )
            .at(date);
            return Err(if status >= 500 {
                failure.retryable()
            } else {
                failure
            });
        }
    }
    let body = response.text();
    if body.len() > MAX_BODY_BYTES {
        return Err(too_large().at(date));
    }
    let value: Value =
        serde_json::from_str(body).map_err(|_| malformed("it is not JSON").at(date))?;
    if !value.is_object() {
        return Err(malformed("it is not a JSON object").at(date));
    }
    if value.get("error").and_then(Value::as_bool) == Some(true) {
        return Err(Failure::new(PROVIDER_ERROR, "Open-Meteo reported an error").at(date));
    }
    Ok((value, date))
}

fn too_large() -> Failure {
    Failure::new(
        RESPONSE_TOO_LARGE,
        format!(
            "Open-Meteo's answer is larger than expected (over {} KiB) and was not read",
            MAX_BODY_BYTES / 1024
        ),
    )
}

/// A request without an answer: denied, refused by the agent, or failed on
/// the network. The SDK's message names the operation and the origin only.
fn fetch_failure(e: &CapabilityError) -> Failure {
    if e.kind() == CapabilityErrorKind::Failed && e.detail().contains("larger than") {
        return too_large();
    }
    if e.is_denied() {
        // The SDK's text is for the plugin's author; this one is for the user.
        return Failure::new(
            codes::NOT_PERMITTED,
            "DeckPilot did not let the plugin reach Open-Meteo: allow its network permission (api.open-meteo.com and geocoding-api.open-meteo.com) on the plugin's page",
        )
        .final_answer();
    }
    Failure {
        error: PluginError::from(e.clone()),
        date: None,
        retry_after: None,
        fallback: true,
    }
}

/// Seconds to wait after an HTTP 429: `Retry-After` as seconds or as an
/// HTTP date counted from the answer's `Date`.
fn retry_after(header: Option<&str>, now: Option<u64>) -> u64 {
    let header = header.map(str::trim);
    let seconds = header
        .and_then(|h| h.parse::<u64>().ok())
        .or_else(|| Some(header.and_then(http_date)?.saturating_sub(now?)))
        .unwrap_or(DEFAULT_RETRY_SECONDS);
    seconds.clamp(60, MAX_RETRY_SECONDS)
}

#[derive(Deserialize)]
struct ForecastBody {
    current: Option<CurrentBody>,
    daily: Option<DailyBody>,
}

#[derive(Deserialize)]
struct CurrentBody {
    time: String,
    temperature_2m: f64,
    apparent_temperature: f64,
    relative_humidity_2m: f64,
    wind_speed_10m: f64,
    wind_direction_10m: f64,
    weather_code: f64,
    is_day: f64,
}

#[derive(Deserialize)]
struct DailyBody {
    time: Vec<String>,
    weather_code: Vec<Option<f64>>,
    temperature_2m_max: Vec<Option<f64>>,
    temperature_2m_min: Vec<Option<f64>>,
    #[serde(default)]
    precipitation_probability_max: Vec<Option<f64>>,
}

/// A value a thermometer or an anemometer can show; rules out NaN, the
/// infinities and nonsense, so later arithmetic cannot overflow.
fn plausible(value: f64) -> bool {
    value.is_finite() && value.abs() < 1_000.0
}

/// A WMO code: a whole number from 0 to 99.
fn wmo(value: f64) -> Option<i64> {
    (value.fract() == 0.0 && (0.0..=99.0).contains(&value)).then_some(value as i64)
}

/// A date or a date and time as Open-Meteo writes them: digits and `-`,
/// `:`, `T`, at most 25 characters.
fn is_time(text: &str) -> bool {
    (10..=25).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'-' | b':' | b'T'))
}

/// The forecast answer as a [`Report`]; the error says what is wrong
/// without quoting the answer.
pub fn parse_report(body: &Value) -> Result<Report, &'static str> {
    let body: ForecastBody =
        ForecastBody::deserialize(body).map_err(|_| "a value is missing or has another type")?;
    let (Some(c), Some(d)) = (body.current, body.daily) else {
        return Err("the current conditions or the daily values are missing");
    };
    let values = [
        c.temperature_2m,
        c.apparent_temperature,
        c.relative_humidity_2m,
        c.wind_speed_10m,
        c.wind_direction_10m,
    ];
    if !values.into_iter().all(plausible) || !is_time(&c.time) {
        return Err("the current conditions are not plausible");
    }
    let current = Current {
        time: c.time,
        temperature: c.temperature_2m,
        feels_like: c.apparent_temperature,
        humidity: (c.relative_humidity_2m.round() as i64).clamp(0, 100),
        wind_speed: c.wind_speed_10m.max(0.0),
        wind_direction: (c.wind_direction_10m.round() as i64).rem_euclid(360),
        code: wmo(c.weather_code).ok_or("the weather code is not a WMO code")?,
        is_day: c.is_day != 0.0,
    };
    let count = d.time.len();
    if d.weather_code.len() != count
        || d.temperature_2m_max.len() != count
        || d.temperature_2m_min.len() != count
    {
        return Err("the daily values have different lengths");
    }
    let mut days = Vec::new();
    for (i, date) in d.time.into_iter().enumerate().take(MAX_DAYS) {
        // A day the model has no values for ends the list: a gap would
        // shift the days after it.
        let (Some(code), Some(max), Some(min)) = (
            d.weather_code[i],
            d.temperature_2m_max[i],
            d.temperature_2m_min[i],
        ) else {
            break;
        };
        if !plausible(max) || !plausible(min) || !is_time(&date) {
            return Err("a daily value is not plausible");
        }
        days.push(Day {
            date,
            min,
            max,
            code: wmo(code).ok_or("a daily weather code is not a WMO code")?,
            precipitation_probability: d
                .precipitation_probability_max
                .get(i)
                .copied()
                .flatten()
                .filter(|p| p.is_finite())
                .map(|p| (p.round() as i64).clamp(0, 100)),
        });
    }
    if days.is_empty() {
        return Err("there is no day with values");
    }
    Ok(Report { current, days })
}

#[derive(Deserialize)]
struct SearchBody {
    /// Absent when nothing matches.
    #[serde(default)]
    results: Vec<SearchResult>,
}

#[derive(Deserialize)]
struct SearchResult {
    name: String,
    latitude: f64,
    longitude: f64,
    timezone: Option<String>,
    admin1: Option<String>,
    country: Option<String>,
}

/// The geocoding answer as places; none is an empty list, not an error.
pub fn parse_places(body: &Value) -> Result<Vec<Place>, &'static str> {
    let body: SearchBody =
        SearchBody::deserialize(body).map_err(|_| "a value is missing or has another type")?;
    let mut places = Vec::new();
    for r in body.results.into_iter().take(MAX_MATCHES) {
        let name = normalize_name(&r.name);
        if name.is_empty()
            || !(-90.0..=90.0).contains(&r.latitude)
            || !(-180.0..=180.0).contains(&r.longitude)
        {
            return Err("a place has no name or impossible coordinates");
        }
        let part =
            |text: Option<String>| text.map(|t| normalize_name(&t)).filter(|t| !t.is_empty());
        places.push(Place {
            name,
            latitude: r.latitude,
            longitude: r.longitude,
            timezone: r.timezone.filter(|tz| is_timezone(tz)),
            region: part(r.admin1),
            country: part(r.country),
        });
    }
    Ok(places)
}
