//! GIOŚ, Poland's Chief Inspectorate of Environmental Protection
//! (<https://powietrze.gios.gov.pl/pjp/content/api>), without a key. Terms
//! as read on 2026-11-16 are in the README.
//!
//! One origin, in the manifest's `network` permission, `GET` only. Four
//! services of its API version 1 are used: the station list, a station's
//! sensors, a station's index and a sensor's hourly values. The answers
//! are JSON-LD with Polish keys, and with the key `@context` twice, so they
//! are read as plain JSON values, never into a strict structure.

use deckpilot_plugin_sdk::prelude::*;
use serde_json::Value;

use crate::provider::{Failure, Fetched, Level, Provider, Reading, Sample, Sensors, Station};
use crate::texts::{Language, one_line};
use crate::time::{http_date, warsaw_to_unix};
use crate::{INVALID_RESPONSE, PROVIDER_ERROR, RESPONSE_TOO_LARGE};

pub const HOST: &str = "api.gios.gov.pl";
pub const API: &str = "https://api.gios.gov.pl/pjp-api/v1/rest";
/// The services answer `application/ld+json` and refuse a request that
/// accepts only `application/json` with HTTP 406 (seen on 2026-11-16).
pub const ACCEPT: &str = "application/ld+json, application/json";
/// The agent sends no User-Agent of its own for plugins.
const USER_AGENT: &str = concat!("DeckPilot-AirQuality/", env!("CARGO_PKG_VERSION"));

/// Stations asked for in one page of the list. The list had 288 stations
/// and 87 KiB in one such page on 2026-11-16; the default page has 20.
pub const PAGE_SIZE: usize = 500;
/// Pages of the list read. The list service allows two requests a minute,
/// so a third page could not be read in the same run anyway.
pub const MAX_PAGES: usize = 2;
/// The largest list page that is parsed. The agent refuses a body over
/// 512 KiB before the plugin sees it; a full page is about 150 KiB.
pub const MAX_LIST_BYTES: usize = 448 * 1024;
/// The largest of the other answers that is parsed; they have 3-7 KiB.
pub const MAX_BODY_BYTES: usize = 64 * 1024;
/// Hourly values asked for: the newest one with a value is used, and a
/// station that has been silent for a day has nothing to report.
pub const SAMPLES: usize = 24;
/// Waiting time assumed for an HTTP 429 without a usable `Retry-After`,
/// and the longest one honoured.
pub const DEFAULT_RETRY_SECONDS: u64 = 15 * 60;
pub const MAX_RETRY_SECONDS: u64 = 24 * 3_600;

/// The Polish air quality index: six categories, 0 is the best.
pub const LEVELS: [Level; 6] = [
    Level {
        name: "very-good",
        en: "Very good",
        pl: "Bardzo dobry",
    },
    Level {
        name: "good",
        en: "Good",
        pl: "Dobry",
    },
    Level {
        name: "moderate",
        en: "Moderate",
        pl: "Umiarkowany",
    },
    Level {
        name: "sufficient",
        en: "Sufficient",
        pl: "Dostateczny",
    },
    Level {
        name: "bad",
        en: "Bad",
        pl: "Zły",
    },
    Level {
        name: "very-bad",
        en: "Very bad",
        pl: "Bardzo zły",
    },
];

/// The words the terms ask for, in the form they prescribe.
pub const SOURCE: &str = "Źródło danych: GIOŚ - EKOINFONET.";

const STATIONS: &str = "Lista stacji pomiarowych";
const SENSORS: &str = "Lista stanowisk pomiarowych dla podanej stacji";
const VALUES: &str = "Lista danych pomiarowych";
const INDEX: &str = "AqIndex";
const INDEX_TIME: &str =
    "Data danych źródłowych, z których policzono wartość indeksu dla wskaźnika st";

#[derive(Debug, Clone, Copy, Default)]
pub struct Gios;

impl Provider for Gios {
    fn id(&self) -> &'static str {
        "gios"
    }

    fn name(&self) -> &'static str {
        "GIOŚ"
    }

    fn source(&self) -> &'static str {
        SOURCE
    }

    fn attribution(&self, language: Language) -> &'static str {
        language.pick(
            "Źródło danych: GIOŚ - EKOINFONET (data source: Poland's Chief Inspectorate of Environmental Protection, CC BY 4.0; an excerpt, values rounded, category names translated).",
            "Źródło danych: GIOŚ - EKOINFONET (CC BY 4.0; fragment danych, wartości zaokrąglone).",
        )
    }

    fn scale(&self, language: Language) -> &'static str {
        language.pick(
            "Polish air quality index, 0 best to 5 worst",
            "Polski indeks jakości powietrza, od 0 (najlepszy) do 5",
        )
    }

    fn level(&self, index: i64) -> Option<Level> {
        usize::try_from(index)
            .ok()
            .and_then(|i| LEVELS.get(i))
            .copied()
    }

    fn unix(&self, time: &str) -> Option<u64> {
        warsaw_to_unix(time)
    }

    fn stations(&self, ctx: &mut Context<'_>) -> Result<Fetched<Vec<Station>>, Failure> {
        let mut stations = Vec::new();
        let mut date = None;
        for page in 0..MAX_PAGES {
            let (body, answered) = get(ctx, stations_url(page), MAX_LIST_BYTES)?;
            date = answered.or(date);
            let (found, pages) = parse_stations(&body).map_err(|what| malformed(what).at(date))?;
            stations.extend(found);
            if pages <= page + 1 {
                break;
            }
            if page + 1 == MAX_PAGES {
                ctx.warn("the provider's station list has more pages than the plugin reads");
            }
        }
        if stations.is_empty() {
            return Err(malformed("the station list is empty").at(date));
        }
        Ok(Fetched {
            value: stations,
            date,
        })
    }

    fn sensors(&self, ctx: &mut Context<'_>, station: i64) -> Result<Fetched<Sensors>, Failure> {
        let (body, date) = get(ctx, sensors_url(station), MAX_BODY_BYTES)?;
        let sensors = parse_sensors(&body).map_err(|what| malformed(what).at(date))?;
        Ok(Fetched {
            value: sensors,
            date,
        })
    }

    fn reading(
        &self,
        ctx: &mut Context<'_>,
        station: i64,
        sensors: Sensors,
    ) -> Result<Fetched<Reading>, Failure> {
        let (body, mut date) = get(ctx, index_url(station), MAX_BODY_BYTES)?;
        let mut reading = parse_index(&body).map_err(|what| malformed(what).at(date))?;
        for (sensor, slot) in [
            (sensors.pm25, &mut reading.pm25),
            (sensors.pm10, &mut reading.pm10),
        ] {
            let Some(sensor) = sensor else { continue };
            let (body, answered) = get(ctx, data_url(sensor), MAX_BODY_BYTES)?;
            date = answered.or(date);
            *slot = parse_newest(&body).map_err(|what| malformed(what).at(date))?;
        }
        Ok(Fetched {
            value: reading,
            date,
        })
    }
}

pub fn stations_url(page: usize) -> String {
    format!("{API}/station/findAll?page={page}&size={PAGE_SIZE}")
}

pub fn sensors_url(station: i64) -> String {
    format!("{API}/station/sensors/{station}?size=100")
}

pub fn index_url(station: i64) -> String {
    format!("{API}/aqindex/getIndex/{station}")
}

pub fn data_url(sensor: i64) -> String {
    format!("{API}/data/getData/{sensor}?size={SAMPLES}")
}

fn malformed(what: &str) -> Failure {
    Failure::new(
        INVALID_RESPONSE,
        format!("GIOŚ's answer could not be read: {what}"),
    )
}

fn too_large() -> Failure {
    Failure::new(
        RESPONSE_TOO_LARGE,
        "GIOŚ's answer is larger than the plugin accepts and was not read",
    )
}

/// One `GET`: the parsed JSON body of a 2xx answer and the answer's `Date`.
/// The messages never contain the URL, whose path names the station.
fn get(ctx: &mut Context<'_>, url: String, limit: usize) -> Result<(Value, Option<u64>), Failure> {
    let request = Request::get(url)
        .header("Accept", ACCEPT)
        .header("User-Agent", USER_AGENT);
    let response = ctx.fetch(request).map_err(|e| fetch_failure(&e))?;
    let date = response.header("date").and_then(http_date);
    match response.status {
        200..=299 => {}
        429 => {
            let mut failure = Failure::new(
                codes::THROTTLED,
                "GIOŚ's request limit is reached (HTTP 429); try again later",
            )
            .retryable()
            .at(date);
            failure.retry_after = Some(retry_after(response.header("retry-after"), date));
            return Err(failure);
        }
        status => {
            let failure = Failure::new(
                PROVIDER_ERROR,
                format!("GIOŚ answered with an error (HTTP {status})"),
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
    if body.len() > limit {
        return Err(too_large().at(date));
    }
    let value: Value =
        serde_json::from_str(body).map_err(|_| malformed("it is not JSON").at(date))?;
    if !value.is_object() {
        return Err(malformed("it is not a JSON object").at(date));
    }
    // An error the service reports in a body with a 2xx status.
    if value.get("error_code").is_some() || value.get("error").is_some() {
        return Err(Failure::new(PROVIDER_ERROR, "GIOŚ reported an error").at(date));
    }
    Ok((value, date))
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
            "DeckPilot did not let the plugin reach GIOŚ: allow its network permission (api.gios.gov.pl) on the plugin's page",
        )
        .final_answer()
        .unreached();
    }
    // A request that failed before it was sent never reached GIOŚ; one
    // that timed out or failed otherwise may have, and counts.
    let reached = !matches!(
        e.network_kind(),
        Some(
            NetworkErrorKind::DnsFailed
                | NetworkErrorKind::ConnectionFailed
                | NetworkErrorKind::TlsFailed
                | NetworkErrorKind::BlockedAddress
        )
    );
    Failure {
        error: PluginError::from(e.clone()),
        date: None,
        retry_after: None,
        fallback: true,
        reached,
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

/// A list member of an answer: an array, or nothing where the service
/// writes `null` for "none".
fn list<'a>(body: &'a Value, key: &str) -> Result<&'a [Value], &'static str> {
    match body.get(key) {
        Some(Value::Array(items)) => Ok(items),
        Some(Value::Null) => Ok(&[]),
        _ => Err("the list it should have is missing"),
    }
}

fn text(item: &Value, key: &str) -> String {
    item.get(key)
        .and_then(Value::as_str)
        .map(one_line)
        .unwrap_or_default()
}

/// One page of the station list and the number of pages. An entry without
/// an id or a name is left out; coordinates and streets are not read.
pub fn parse_stations(body: &Value) -> Result<(Vec<Station>, usize), &'static str> {
    let stations = list(body, STATIONS)?
        .iter()
        .filter_map(|item| {
            let id = item.get("Identyfikator stacji")?.as_i64()?;
            let name = text(item, "Nazwa stacji");
            (id > 0 && !name.is_empty()).then(|| Station {
                id,
                name,
                city: text(item, "Nazwa miasta"),
                region: text(item, "Województwo").to_lowercase(),
            })
        })
        .collect();
    let pages = body
        .get("totalPages")
        .and_then(Value::as_u64)
        .and_then(|pages| usize::try_from(pages).ok())
        .unwrap_or(1);
    Ok((stations, pages))
}

/// The PM2.5 and PM10 sensors of a station; a station may have neither.
pub fn parse_sensors(body: &Value) -> Result<Sensors, &'static str> {
    let mut sensors = Sensors::default();
    for item in list(body, SENSORS)? {
        let Some(id) = item
            .get("Identyfikator stanowiska")
            .and_then(Value::as_i64)
            .filter(|id| *id > 0)
        else {
            continue;
        };
        let code = item.get("Wskaźnik - kod").and_then(Value::as_str);
        let slot = match code.map(str::trim) {
            Some("PM2.5") => &mut sensors.pm25,
            Some("PM10") => &mut sensors.pm10,
            _ => continue,
        };
        slot.get_or_insert(id);
    }
    Ok(sensors)
}

/// A concentration an instrument can report: rules out NaN, the infinities
/// and nonsense. Analysers do report small negative values near zero; those
/// are kept out as well, so the next older value is used.
fn plausible(value: f64) -> bool {
    value.is_finite() && (0.0..100_000.0).contains(&value)
}

/// The newest hourly value of a sensor that has a value. `None` when the
/// sensor reported nothing in the hours asked for.
pub fn parse_newest(body: &Value) -> Result<Option<Sample>, &'static str> {
    let newest = list(body, VALUES)?
        .iter()
        .filter_map(|item| {
            let value = item.get("Wartość")?.as_f64().filter(|v| plausible(*v))?;
            let time = item.get("Data")?.as_str()?;
            // Texts of one fixed format compare like the times they name.
            warsaw_to_unix(time).map(|_| Sample {
                value,
                time: time.to_owned(),
            })
        })
        .max_by(|a, b| a.time.cmp(&b.time));
    Ok(newest)
}

/// The index answer as a [`Reading`] without PM values. A station the
/// service does not know, or one without an index now, has `null` members:
/// that is "no index", not a malformed answer.
pub fn parse_index(body: &Value) -> Result<Reading, &'static str> {
    let index = body
        .get(INDEX)
        .filter(|index| index.is_object())
        .ok_or("the index it should have is missing")?;
    let level = |key: &str| {
        index
            .get(key)
            .and_then(Value::as_i64)
            .filter(|level| usize::try_from(*level).is_ok_and(|l| l < LEVELS.len()))
    };
    let overall =
        index.get("Status indeksu ogólnego dla stacji pomiarowej") != Some(&Value::Bool(false));
    let time = index
        .get(INDEX_TIME)
        .and_then(Value::as_str)
        .filter(|time| warsaw_to_unix(time).is_some());
    let value = level("Wartość indeksu").filter(|_| overall && time.is_some());
    Ok(Reading {
        index: value,
        index_time: value.and(time).map(str::to_owned),
        pm25: None,
        pm10: None,
        no2: level("Wartość indeksu dla wskaźnika NO2"),
        so2: level("Wartość indeksu dla wskaźnika SO2"),
        o3: level("Wartość indeksu dla wskaźnika O3"),
    })
}
