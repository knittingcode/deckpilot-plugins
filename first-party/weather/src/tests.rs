//! Native unit tests: the definition agrees with `manifest.json`, and every
//! action behaves with the agent replaced by a scripted `MockHost` that
//! answers with canned Open-Meteo responses. Nothing here reaches the
//! network. `tools/first-party.sh` adds what a unit test cannot: the
//! manifest against the schema, the Polish texts, the wasm build and the
//! package checks.

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

use deckpilot_plugin_sdk::testing::{MockHost, manifest_mismatches};
use deckpilot_plugin_sdk::{
    ActionOutcome, CallErrorCode, CallResult, CapabilityRequest, HttpMethod, NetworkErrorKind,
    ParamValue,
};
use serde_json::{Value, json};

use super::*;
use crate::cache::{GEO_PREFIX, LOCATION_KEY, MAX_GEO_ENTRIES, STATE_KEY, WEATHER_KEY};

const MANIFEST: &str = include_str!("../manifest.json");

/// The shape of a real answer (2026-11-16, Warsaw), with metric values.
const FORECAST: &str = r#"{"latitude":52.23009,"longitude":21.017075,"generationtime_ms":0.37,
"utc_offset_seconds":3600,"timezone":"Europe/Warsaw","timezone_abbreviation":"GMT+1","elevation":113.0,
"current_units":{"time":"iso8601","interval":"seconds","temperature_2m":"°C","apparent_temperature":"°C",
"relative_humidity_2m":"%","wind_speed_10m":"km/h","wind_direction_10m":"°","weather_code":"wmo code","is_day":""},
"current":{"time":"2026-11-16T19:15","interval":900,"temperature_2m":6.5,"apparent_temperature":3.2,
"relative_humidity_2m":92,"wind_speed_10m":14.8,"wind_direction_10m":191,"weather_code":51,"is_day":0},
"daily_units":{"time":"iso8601","weather_code":"wmo code","temperature_2m_max":"°C","temperature_2m_min":"°C",
"precipitation_probability_max":"%"},
"daily":{"time":["2026-11-16","2026-11-17","2026-11-18","2026-11-19","2026-11-20","2026-11-21","2026-11-22"],
"weather_code":[51,53,3,61,2,85,71],
"temperature_2m_max":[7.4,6.8,10.4,10.6,8.3,3.7,3.1],
"temperature_2m_min":[0.6,3.6,4.1,7.2,0.8,-0.3,-0.8],
"precipitation_probability_max":[41,78,88,49,null,22,31]}}"#;

/// The shape of a real geocoding answer with several matches.
const PLACES: &str = r#"{"results":[
{"id":4409896,"name":"Springfield","latitude":37.21533,"longitude":-93.29824,"elevation":396.0,
"feature_code":"PPLA2","country_code":"US","timezone":"America/Chicago","population":170188,
"postcodes":["65801","65802"],"country_id":6252001,"country":"United States","admin1":"Missouri",
"admin2":"Greene County"},
{"id":4250542,"name":"Springfield","latitude":39.80172,"longitude":-89.64371,"elevation":182.0,
"feature_code":"PPLA","country_code":"US","timezone":"America/Chicago","population":114394,
"country_id":6252001,"country":"United States","admin1":"Illinois"},
{"id":4951788,"name":"Springfield","latitude":42.10148,"longitude":-72.58981,"elevation":25.0,
"feature_code":"PPL","country_code":"US","timezone":"America/New_York","population":154341,
"country_id":6252001,"country":"United States","admin1":"Massachusetts"}],
"generationtime_ms":0.16}"#;

/// What the geocoding API answers when nothing matches.
const NO_PLACES: &str = r#"{"generationtime_ms":0.17440319}"#;

const DATE: &str = "Mon, 16 Nov 2026 18:16:34 GMT";
const DATE_UNIX: u64 = 1_794_852_994;

/// Fragments of the test coordinates that must never leave the plugin
/// except in a request to the provider.
const SECRET_PARTS: &[&str] = &["52.2", "21.0", "37.2", "93.2", "52,2", "21,0"];

fn manifest() -> Value {
    serde_json::from_str(MANIFEST).expect("manifest.json is JSON")
}

#[derive(Debug)]
enum Reply {
    Http {
        status: u16,
        headers: Vec<(&'static str, String)>,
        body: String,
    },
    Error(CallErrorCode, &'static str, Option<NetworkErrorKind>),
}

fn http(status: u16, body: &str) -> Reply {
    Reply::Http {
        status,
        headers: vec![("date", DATE.into())],
        body: body.into(),
    }
}

fn http_at(status: u16, body: &str, date: &str) -> Reply {
    Reply::Http {
        status,
        headers: vec![("date", date.into())],
        body: body.into(),
    }
}

fn offline() -> Reply {
    Reply::Error(
        CallErrorCode::Failed,
        "no route to host",
        Some(NetworkErrorKind::ConnectionFailed),
    )
}

/// The agent's side: settings, the plugin's storage, queued HTTP answers,
/// and a record of everything the plugin did.
#[derive(Debug, Default)]
struct Agent {
    settings: Value,
    store: BTreeMap<String, Value>,
    replies: VecDeque<Reply>,
    urls: Vec<String>,
    logs: Vec<String>,
    published: Vec<(String, Value)>,
    cleared: usize,
    deny_network: bool,
    /// The agent's clock for `time.now` (Unix seconds); the default is
    /// the canned answer's `Date`.
    now: u64,
}

type Shared = Rc<RefCell<Agent>>;

/// `2026-11-16T18:16:34Z`, the `Date` of the canned answers.
const FETCHED: u64 = 1_794_852_994;

fn agent(settings: Value) -> Shared {
    Rc::new(RefCell::new(Agent {
        settings,
        now: FETCHED,
        ..Agent::default()
    }))
}

/// An agent whose settings name Warsaw by coordinates.
fn warsaw() -> Shared {
    agent(json!({
        "latitude": "52.2297", "longitude": "21.0122",
        "locationName": "Warsaw", "timezone": "Europe/Warsaw",
    }))
}

fn host(agent: &Shared) -> MockHost {
    let agent = Rc::clone(agent);
    MockHost::new(move |request| {
        let mut a = agent.borrow_mut();
        match request {
            CapabilityRequest::SettingsGet(_) => CallResult::Ok(a.settings.clone()),
            CapabilityRequest::StorageGet(k) => {
                CallResult::Ok(a.store.get(&k.key).cloned().unwrap_or(Value::Null))
            }
            CapabilityRequest::StorageSet(s) => {
                let size = s.key.len() + s.value.to_string().len();
                assert!(size <= 64 * 1024, "{} is {size} bytes", s.key);
                assert!(s.key.chars().count() <= 128, "{}", s.key);
                a.store.insert(s.key.clone(), s.value.clone());
                CallResult::Ok(Value::Null)
            }
            CapabilityRequest::StorageDelete(k) => {
                a.store.remove(&k.key);
                CallResult::Ok(Value::Null)
            }
            CapabilityRequest::StorageKeys(p) => {
                let prefix = p.prefix.clone().unwrap_or_default();
                CallResult::Ok(json!(
                    a.store
                        .keys()
                        .filter(|k| k.starts_with(&prefix))
                        .collect::<Vec<_>>()
                ))
            }
            CapabilityRequest::VariablesPublish(v) => {
                a.published.push((v.id.clone(), v.value.clone()));
                CallResult::Ok(Value::Null)
            }
            CapabilityRequest::VariablesClear(_) => {
                a.cleared += 1;
                CallResult::Ok(Value::Null)
            }
            CapabilityRequest::Log(l) => {
                a.logs.push(l.message.clone());
                CallResult::Ok(Value::Null)
            }
            CapabilityRequest::TimeNow(_) => CallResult::Ok(json!({
                "unixMs": a.now * 1000, "utcOffsetMinutes": 60
            })),
            CapabilityRequest::HttpFetch(f) => {
                assert_eq!(f.method, HttpMethod::Get);
                assert!(f.body.is_none() && f.auth.is_none());
                assert!(
                    f.headers.iter().any(|h| h.name == "User-Agent"
                        && h.value == format!("DeckPilot-Weather/{VERSION}"))
                );
                a.urls.push(f.url.clone());
                if a.deny_network {
                    return CallResult::error(CallErrorCode::NotPermitted, "not granted");
                }
                match a.replies.pop_front() {
                    Some(Reply::Http {
                        status,
                        headers,
                        body,
                    }) => CallResult::Ok(json!({
                        "status": status,
                        "headers": headers.iter()
                            .map(|(name, value)| json!({ "name": name, "value": value }))
                            .collect::<Vec<_>>(),
                        "body": body,
                    })),
                    Some(Reply::Error(code, message, kind)) => {
                        CallResult::error_with_kind(code, message, kind)
                    }
                    None => panic!("an HTTP request the test did not expect"),
                }
            }
            other => panic!("an unexpected capability call: {other:?}"),
        }
    })
}

fn queue(agent: &Shared, reply: Reply) {
    agent.borrow_mut().replies.push_back(reply);
}

fn run(agent: &Shared, action: &str, params: Value) -> ActionOutcome {
    host(agent).invoke(&plugin(), action, params)
}

fn ok(outcome: ActionOutcome) -> (String, BTreeMap<String, ParamValue>) {
    match outcome {
        ActionOutcome::Ok { message, data } => (message.unwrap_or_default(), data),
        other => panic!("expected a success: {other:?}"),
    }
}

fn failed(outcome: ActionOutcome) -> (String, String, bool) {
    match outcome {
        ActionOutcome::Failed {
            code,
            message,
            retryable,
        } => (code, message.unwrap_or_default(), retryable),
        other => panic!("expected a failure: {other:?}"),
    }
}

fn text(data: &BTreeMap<String, ParamValue>, key: &str) -> String {
    match data.get(key) {
        Some(ParamValue::String(s)) => s.clone(),
        other => panic!("{key} is {other:?}"),
    }
}

fn int(data: &BTreeMap<String, ParamValue>, key: &str) -> i64 {
    match data.get(key) {
        Some(ParamValue::Integer(i)) => *i,
        other => panic!("{key} is {other:?}"),
    }
}

fn flag(data: &BTreeMap<String, ParamValue>, key: &str) -> bool {
    match data.get(key) {
        Some(ParamValue::Bool(b)) => *b,
        other => panic!("{key} is {other:?}"),
    }
}

fn variable(agent: &Shared, id: &str) -> Value {
    let a = agent.borrow();
    a.published
        .iter()
        .rev()
        .find(|(name, _)| name == id)
        .map(|(_, value)| value.clone())
        .unwrap_or_else(|| panic!("{id} was not published"))
}

fn requests(agent: &Shared) -> usize {
    agent.borrow().urls.len()
}

/// The declared outputs of an action: `(key, always)`.
fn declared_outputs(action: &str) -> Vec<(String, bool)> {
    let m = manifest();
    let actions = m["contributes"]["actions"].as_array().unwrap().clone();
    let entry = actions.iter().find(|a| a["id"] == action).unwrap();
    entry["node"]["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            (
                o["key"].as_str().unwrap().to_owned(),
                o["always"] == json!(true),
            )
        })
        .collect()
}

/// Every output is declared (the agent drops the others), and every output
/// declared as always present is there.
fn assert_outputs(action: &str, data: &BTreeMap<String, ParamValue>) {
    let declared = declared_outputs(action);
    for key in data.keys() {
        assert!(
            declared.iter().any(|(k, _)| k == key),
            "{action}: {key} is not declared"
        );
    }
    for (key, always) in &declared {
        assert!(
            !always || data.contains_key(key),
            "{action}: {key} is missing"
        );
    }
}

// ---- The definition and the manifest -------------------------------------

#[test]
fn definition_is_valid_and_matches_the_manifest() {
    plugin().validate().expect("the definition is valid");
    assert_eq!(
        manifest_mismatches(&plugin(), &manifest()),
        Vec::<String>::new()
    );
}

#[test]
fn the_manifest_declares_what_the_code_uses_and_nothing_more() {
    let m = manifest();
    assert_eq!(m["id"], ID);
    assert_eq!(m["version"], VERSION);
    assert_eq!(m["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(m["publisher"]["id"], "deckpilot");

    // Two permissions: the two Open-Meteo origins the code asks, GET only;
    // and key state for Current weather (plugin API 1.2).
    let permissions = m["permissions"].as_array().unwrap();
    assert_eq!(permissions.len(), 2);
    assert_eq!(permissions[1]["kind"], "device");
    assert_eq!(permissions[1]["state"], true);
    assert!(permissions[1].get("icons").is_none());
    assert_eq!(m["engines"]["pluginApi"], "^1.2");
    assert_eq!(plugin().api_level().to_string(), "1.2");
    assert_eq!(permissions[0]["kind"], "network");
    assert_eq!(
        permissions[0]["hosts"],
        json!(["api.open-meteo.com", "geocoding-api.open-meteo.com"])
    );
    assert_eq!(permissions[0]["methods"], json!(["GET"]));
    assert!(permissions[0].get("userOrigins").is_none());
    assert!(permissions[0].get("privateNetwork").is_none());
    for (url, host) in [
        (open_meteo::FORECAST_API, "api.open-meteo.com"),
        (open_meteo::GEOCODING_API, "geocoding-api.open-meteo.com"),
    ] {
        assert!(url.starts_with(&format!("https://{host}/")), "{url}");
    }
    assert!(m["contributes"].get("credentials").is_none(), "no key");

    let declared = m["contributes"]["errorCodes"].as_object().unwrap();
    for code in PLUGIN_CODES {
        assert!(declared.contains_key(*code), "{code} is not in errorCodes");
    }
    for code in declared.keys() {
        assert!(!codes::is_shared(code), "{code} redeclares a shared code");
        assert!(PLUGIN_CODES.contains(&code.as_str()), "{code} is unused");
    }

    let variables: Vec<&str> = m["contributes"]["variables"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["id"].as_str().unwrap())
        .collect();
    let used: Vec<&str> = CURRENT_VARIABLES
        .iter()
        .chain(FORECAST_VARIABLES)
        .copied()
        .collect();
    assert_eq!(variables, used);
    // The bucket for publishing is 10: both actions in a row must fit.
    assert!(used.len() <= 10);
}

#[test]
fn icon_is_a_128px_png() {
    let png = include_bytes!("../assets/icon-128.png");
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let side = |at: usize| u32::from_be_bytes([png[at], png[at + 1], png[at + 2], png[at + 3]]);
    assert_eq!((side(16), side(20)), (128, 128));
    assert!(png.len() <= 256 * 1024);
}

// ---- Pure parts ------------------------------------------------------------

#[test]
fn every_wmo_code_has_a_text_in_both_languages_and_a_known_name() {
    for &(code, name, en, pl) in conditions::CODES {
        let c = condition(code);
        assert_eq!((c.name, c.en, c.pl), (name, en, pl));
        assert!(conditions::NAMES.contains(&name), "{name}");
        assert!(!en.is_empty() && !pl.is_empty() && en != pl, "{code}");
        assert!(
            en.chars().count() <= 32 && pl.chars().count() <= 40,
            "{code}"
        );
    }
    let codes: Vec<i64> = conditions::CODES.iter().map(|c| c.0).collect();
    assert!(codes.windows(2).all(|w| w[0] < w[1]), "sorted and unique");
    // Open-Meteo's documented codes are all there.
    for code in [
        0, 1, 2, 3, 45, 48, 51, 53, 55, 56, 57, 61, 63, 65, 66, 67, 71, 73, 75, 77, 80, 81, 82, 85,
        86, 95, 96, 99,
    ] {
        assert_ne!(condition(code).name, "unknown", "{code}");
    }
    assert_eq!(condition(0).text(Language::En), "Clear sky");
    assert_eq!(condition(0).text(Language::Pl), "Bezchmurnie");
    assert_eq!(condition(63).name, "rain");
    assert_eq!(condition(96).name, "thunderstorm-hail");
    // A code without a meaning is unknown, never a neighbour's text.
    for code in [-1, 4, 44, 50, 98, 100] {
        let c = condition(code);
        assert_eq!(
            (c.name, c.en, c.pl),
            ("unknown", "Unknown conditions", "Nieznane warunki")
        );
    }
}

#[test]
fn units_convert_and_round() {
    let (m, i) = (Units::Metric, Units::Imperial);
    assert_eq!((m.temperature(6.5), m.temperature(-0.4)), (7, 0));
    assert_eq!((m.temperature(-0.5), m.temperature(-12.6)), (-1, -13));
    assert_eq!((i.temperature(0.0), i.temperature(100.0)), (32, 212));
    assert_eq!((i.temperature(-40.0), i.temperature(6.5)), (-40, 44));
    assert_eq!((m.wind(14.8), i.wind(14.8)), (15, 9));
    assert_eq!((i.wind(160.9344), i.wind(0.0)), (100, 0));
    assert_eq!((m.temperature_unit(), i.temperature_unit()), ("°C", "°F"));
    assert_eq!((m.wind_unit(), i.wind_unit()), ("km/h", "mph"));
    assert_eq!(Units::from_setting(Some("imperial")), i);
    assert_eq!(Units::from_setting(Some("kelvin")), m);
    assert_eq!(Units::from_setting(None), m);
    for (degrees, point) in [
        (0, "N"),
        (22, "N"),
        (23, "NE"),
        (90, "E"),
        (135, "SE"),
        (191, "S"),
        (225, "SW"),
        (270, "W"),
        (315, "NW"),
        (338, "N"),
        (359, "N"),
        (-90, "W"),
        (720, "N"),
    ] {
        assert_eq!(compass(degrees), point, "{degrees}");
    }
}

#[test]
fn times_are_read_from_the_date_header_and_the_trigger() {
    assert_eq!(time::http_date(DATE), Some(DATE_UNIX));
    assert_eq!(time::http_date("Thu, 01 Jan 1970 00:00:00 GMT"), Some(0));
    assert_eq!(
        time::http_date("Thu, 29 Feb 2024 23:59:59 GMT"),
        Some(1_709_251_199)
    );
    for bad in [
        "",
        "yesterday",
        "Mon, 16 Nov 2026 18:16:34",
        "Mon, 16 Nov 2026 18:16:34 CET",
        "Mon, 31 Nov 2026 18:16:34 GMT",
        "Mon, 29 Feb 2026 18:16:34 GMT",
        "Mon, 16 Nov 2026 24:00:00 GMT",
        "Mon, 16 Nov 2026 18:16 GMT",
        "Mon, 16 Foo 2026 18:16:34 GMT",
        "Mon, 16 Nov 1969 18:16:34 GMT",
        "Mon, +6 Nov 2026 18:16:34 GMT",
    ] {
        assert_eq!(time::http_date(bad), None, "{bad}");
    }
    assert_eq!(time::utc_text(DATE_UNIX), "2026-11-16T18:16:34Z");
    assert_eq!(time::utc_text(0), "1970-01-01T00:00:00Z");
    assert_eq!(time::utc_text(1_709_251_199), "2024-02-29T23:59:59Z");

    let at = |text| time::local_minutes(text);
    assert_eq!(at("1970-01-01 00:00"), Some(0));
    assert_eq!(at("1970-01-02 01:01"), Some(1_501));
    assert_eq!(
        at("2026-11-16 19:30").unwrap() - at("2026-11-16 19:16").unwrap(),
        14
    );
    assert_eq!(
        at("2027-01-01 00:05").unwrap() - at("2026-12-31 23:55").unwrap(),
        10
    );
    assert_eq!(
        at("2024-03-01 00:00").unwrap() - at("2024-02-28 00:00").unwrap(),
        2 * 1_440
    );
    for bad in [
        "",
        "now",
        "2026-11-16",
        "2026-11-16T19:30",
        "2026-11-16 19:30:00",
        "2026-13-01 00:00",
        "2026-02-30 00:00",
        "2026-11-16 24:00",
        "2026-11-16 19:60",
        "26-11-16 19:30",
        "2026-11-16 1:30",
    ] {
        assert_eq!(at(bad), None, "{bad}");
    }
}

#[test]
fn the_age_needs_two_times_of_the_same_kind() {
    use cache::age;
    assert_eq!(age(Some(100), None, Some(160), None), Some(60));
    assert_eq!(age(None, Some(10), None, Some(25)), Some(900));
    // UTC wins when both pairs are known.
    assert_eq!(age(Some(100), Some(10), Some(160), Some(25)), Some(60));
    // A UTC time and a local time cannot be subtracted.
    assert_eq!(age(Some(100), None, None, Some(25)), None);
    assert_eq!(age(None, Some(10), Some(160), None), None);
    assert_eq!(age(None, None, None, None), None);
    // Data from the future (a clock set back, the end of summer time) has
    // no known age.
    assert_eq!(age(Some(200), None, Some(160), None), None);
    assert_eq!(age(None, Some(30), None, Some(25)), None);
    assert_eq!(age(Some(200), Some(10), Some(160), Some(25)), Some(900));
}

#[test]
fn a_forecast_answer_is_read_and_checked() {
    let body: Value = serde_json::from_str(FORECAST).unwrap();
    let report = open_meteo::parse_report(&body).unwrap();
    assert_eq!(report.current.time, "2026-11-16T19:15");
    assert_eq!(report.current.code, 51);
    assert_eq!(report.current.humidity, 92);
    assert!(!report.current.is_day);
    assert_eq!(report.days.len(), 7);
    assert_eq!(report.days[0].precipitation_probability, Some(41));
    assert_eq!(report.days[4].precipitation_probability, None);
    assert_eq!((report.days[5].min, report.days[5].max), (-0.3, 3.7));

    let broken = |change: fn(&mut Value)| {
        let mut body: Value = serde_json::from_str(FORECAST).unwrap();
        change(&mut body);
        open_meteo::parse_report(&body).expect_err("refused")
    };
    broken(|b| b["current"] = Value::Null);
    broken(|b| {
        b.as_object_mut().unwrap().remove("daily");
    });
    broken(|b| b["current"]["temperature_2m"] = json!("6.5"));
    broken(|b| b["current"]["temperature_2m"] = json!(1.0e9));
    broken(|b| b["current"]["weather_code"] = json!(51.5));
    broken(|b| b["current"]["weather_code"] = json!(300));
    broken(|b| b["current"]["time"] = json!("<script>alert(1)</script>"));
    broken(|b| b["daily"]["temperature_2m_max"] = json!([1.0]));
    broken(|b| b["daily"]["weather_code"][0] = Value::Null);
    broken(|b| b["daily"]["time"][0] = json!("today"));

    // Days end at the first one without values; no probability is fine.
    let mut body: Value = serde_json::from_str(FORECAST).unwrap();
    body["daily"]["temperature_2m_min"][2] = Value::Null;
    body["daily"]
        .as_object_mut()
        .unwrap()
        .remove("precipitation_probability_max");
    let report = open_meteo::parse_report(&body).unwrap();
    assert_eq!(report.days.len(), 2);
    assert_eq!(report.days[0].precipitation_probability, None);
}

#[test]
fn request_urls_hold_only_what_the_provider_needs() {
    let place = Place {
        name: "Home".into(),
        latitude: 52.229_676,
        longitude: -21.012_229,
        timezone: Some("America/Argentina/Buenos_Aires".into()),
        region: None,
        country: None,
    };
    assert_eq!(
        open_meteo::forecast_url(&place),
        "https://api.open-meteo.com/v1/forecast?latitude=52.23&longitude=-21.01\
&current=temperature_2m,apparent_temperature,relative_humidity_2m,wind_speed_10m,\
wind_direction_10m,weather_code,is_day\
&daily=weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max\
&forecast_days=7&timezone=America%2FArgentina%2FBuenos_Aires\
&temperature_unit=celsius&wind_speed_unit=kmh"
    );
    let auto = Place {
        timezone: None,
        ..place.clone()
    };
    assert!(open_meteo::forecast_url(&auto).contains("&timezone=auto&"));
    // A time zone that is not one cannot add parameters to the request.
    let odd = Place {
        timezone: Some("UTC&latitude=0".into()),
        ..place.clone()
    };
    assert!(open_meteo::forecast_url(&odd).contains("&timezone=auto&"));
    assert_eq!(
        open_meteo::search_url("łódź & co, pl", Language::Pl),
        "https://geocoding-api.open-meteo.com/v1/search?name=%C5%82%C3%B3d%C5%BA%20%26%20co%2C%20pl\
&count=5&language=pl&format=json"
    );
    // The name, never the coordinates, in a debug print.
    assert_eq!(format!("{place:?}"), "Place { name: \"Home\", .. }");
    assert_eq!(place.grid(), "52.23,-21.01");
}

// ---- current ---------------------------------------------------------------

#[test]
fn current_reports_the_weather_and_stores_it() {
    let a = warsaw();
    queue(&a, http(200, FORECAST));
    let (message, data) = ok(run(&a, "current", json!({})));
    assert_eq!(
        message,
        "Warsaw: Light drizzle, 7 °C (feels like 3 °C). Wind 15 km/h S, humidity 92%. \
Weather data by Open-Meteo.com (https://open-meteo.com/), CC BY 4.0"
    );
    assert_outputs("current", &data);
    assert_eq!(text(&data, "condition"), "Light drizzle");
    assert_eq!(text(&data, "conditionName"), "drizzle");
    assert_eq!(int(&data, "weatherCode"), 51);
    assert!(!flag(&data, "isDay"));
    assert_eq!((int(&data, "temperature"), int(&data, "feelsLike")), (7, 3));
    assert_eq!(text(&data, "temperatureUnit"), "°C");
    assert_eq!(
        (int(&data, "windSpeed"), text(&data, "windUnit")),
        (15, "km/h".into())
    );
    assert_eq!(
        (int(&data, "windDirection"), text(&data, "windCompass")),
        (191, "S".into())
    );
    assert_eq!(int(&data, "humidity"), 92);
    assert_eq!(text(&data, "units"), "metric");
    assert_eq!(text(&data, "location"), "Warsaw");
    assert_eq!(text(&data, "observedAt"), "2026-11-16T19:15");
    assert!(!flag(&data, "stale"));
    assert_eq!(text(&data, "source"), "network");
    assert_eq!(text(&data, "fetchedAt"), "2026-11-16T18:16:34Z");
    assert_eq!(int(&data, "ageMinutes"), 0);
    assert!(!data.contains_key("problem"));
    assert!(text(&data, "attribution").contains("Open-Meteo.com"));
    assert_eq!(
        text(&data, "summary"),
        "Warsaw: Light drizzle, 7 °C (feels like 3 °C). Wind 15 km/h S, humidity 92%."
    );

    assert_eq!(variable(&a, "temperature"), json!(7));
    assert_eq!(variable(&a, "feelsLike"), json!(3));
    assert_eq!(variable(&a, "condition"), json!("Light drizzle"));
    assert_eq!(variable(&a, "conditionName"), json!("drizzle"));
    assert_eq!(variable(&a, "summary"), json!(text(&data, "summary")));
    assert_eq!(variable(&a, "stale"), json!(false));
    let published: Vec<String> = a.borrow().published.iter().map(|p| p.0.clone()).collect();
    assert_eq!(published, CURRENT_VARIABLES);

    let a = a.borrow();
    assert_eq!(
        a.urls,
        [
            "https://api.open-meteo.com/v1/forecast?latitude=52.23&longitude=21.01\
&current=temperature_2m,apparent_temperature,relative_humidity_2m,wind_speed_10m,\
wind_direction_10m,weather_code,is_day\
&daily=weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max\
&forecast_days=7&timezone=Europe%2FWarsaw&temperature_unit=celsius&wind_speed_unit=kmh"
        ]
    );
    let entry = &a.store[WEATHER_KEY];
    assert_eq!(entry["grid"], "52.23,21.01");
    assert_eq!(entry["fetched_at"], DATE_UNIX);
    assert_eq!(entry["fetched_local"], Value::Null);
    assert_eq!(a.store[STATE_KEY]["last_seen"], DATE_UNIX);
    assert!(a.logs.is_empty(), "{:?}", a.logs);
}

#[test]
fn units_and_language_come_from_the_settings() {
    let a = warsaw();
    a.borrow_mut().settings["units"] = json!("imperial");
    a.borrow_mut().settings["language"] = json!("pl");
    a.borrow_mut().settings["locationName"] = json!("Warszawa");
    queue(&a, http(200, FORECAST));
    let (message, data) = ok(run(&a, "current", json!({})));
    assert_eq!(
        message,
        "Warszawa: Słaba mżawka, 44 °F (odczuwalna 38 °F). Wiatr 9 mph S, wilgotność 92%. \
Weather data by Open-Meteo.com (https://open-meteo.com/), CC BY 4.0"
    );
    assert_eq!(
        (int(&data, "temperature"), int(&data, "feelsLike")),
        (44, 38)
    );
    assert_eq!(
        (int(&data, "windSpeed"), text(&data, "windUnit")),
        (9, "mph".into())
    );
    assert_eq!(text(&data, "temperatureUnit"), "°F");
    assert_eq!(text(&data, "units"), "imperial");
    assert_eq!(text(&data, "condition"), "Słaba mżawka");
    // The name is the same in every language.
    assert_eq!(text(&data, "conditionName"), "drizzle");
    // The request and the cache stay metric: changing the unit needs no
    // new request.
    assert!(a.borrow().urls[0].contains("temperature_unit=celsius&wind_speed_unit=kmh"));
    a.borrow_mut().settings["units"] = json!("metric");
    queue(&a, offline());
    let (_, data) = ok(run(&a, "current", json!({})));
    assert_eq!(int(&data, "temperature"), 7);
}

#[test]
fn a_run_with_a_time_uses_the_cache_for_fifteen_minutes() {
    let a = warsaw();
    queue(&a, http(200, FORECAST));
    let (_, data) = ok(run(&a, "current", json!({ "at": "2026-11-16 19:16" })));
    assert_eq!(text(&data, "source"), "network");
    assert_eq!(
        a.borrow().store[WEATHER_KEY]["fetched_local"],
        json!(29_914_276)
    );

    // 14 minutes later: the cache, no request.
    let (message, data) = ok(run(&a, "current", json!({ "at": "2026-11-16 19:30" })));
    assert_eq!(requests(&a), 1);
    assert_outputs("current", &data);
    assert_eq!(text(&data, "source"), "cache");
    assert!(!flag(&data, "stale"));
    assert_eq!(int(&data, "ageMinutes"), 14);
    assert_eq!(text(&data, "fetchedAt"), "2026-11-16T18:16:34Z");
    assert!(
        message.contains("(from the cache, 14 min old)"),
        "{message}"
    );
    assert!(!text(&data, "summary").starts_with("[stale]"));
    assert_eq!(variable(&a, "stale"), json!(false));

    // 15 minutes: a new request.
    queue(&a, http_at(200, FORECAST, "Mon, 16 Nov 2026 18:31:40 GMT"));
    let (_, data) = ok(run(&a, "current", json!({ "at": "2026-11-16 19:31" })));
    assert_eq!(requests(&a), 2);
    assert_eq!(text(&data, "source"), "network");
    assert_eq!(text(&data, "fetchedAt"), "2026-11-16T18:31:40Z");

    // A time before the stored one (the clock went back): the age is
    // unknown, so the provider is asked.
    queue(&a, http(200, FORECAST));
    ok(run(&a, "current", json!({ "at": "2026-11-16 18:40" })));
    assert_eq!(requests(&a), 3);
}

#[test]
fn a_run_without_a_time_cannot_know_the_age_and_asks() {
    let a = warsaw();
    queue(&a, http(200, FORECAST));
    ok(run(&a, "current", json!({ "at": "2026-11-16 19:16" })));
    // A key press a moment later has no clock: one request.
    queue(&a, http(200, FORECAST));
    let (_, data) = ok(run(&a, "current", json!({})));
    assert_eq!(requests(&a), 2);
    assert_eq!(text(&data, "source"), "network");
    // An empty `at` (an unset workflow input) is no time.
    queue(&a, http(200, FORECAST));
    ok(run(&a, "current", json!({ "at": "  " })));
    assert_eq!(requests(&a), 3);
    // What a key press stored has no local time, so a timed run asks once.
    queue(&a, http(200, FORECAST));
    ok(run(&a, "current", json!({ "at": "2026-11-16 19:17" })));
    assert_eq!(requests(&a), 4);
    ok(run(&a, "current", json!({ "at": "2026-11-16 19:18" })));
    assert_eq!(requests(&a), 4);
}

#[test]
fn a_time_that_is_not_one_is_refused_before_anything_else() {
    let a = warsaw();
    for at in ["tomorrow", "2026-11-16", "2026-11-16T19:16"] {
        let (code, message, _) = failed(run(&a, "current", json!({ "at": at })));
        assert_eq!(code, codes::INVALID_PARAMETERS);
        assert!(message.contains("YYYY-MM-DD HH:MM"), "{message}");
        let (code, ..) = failed(run(&a, "forecast", json!({ "at": at })));
        assert_eq!(code, codes::INVALID_PARAMETERS);
    }
    let (code, ..) = failed(run(
        &a,
        "current",
        json!({ "at": "2026-11-16 19:16:00.000" }),
    ));
    assert_eq!(code, codes::INVALID_PARAMETERS);
    assert_eq!(requests(&a), 0);
}

#[test]
fn offline_serves_saved_data_as_stale_and_never_as_fresh() {
    let a = warsaw();
    queue(&a, http(200, FORECAST));
    ok(run(&a, "current", json!({})));

    // No answer at all and no time: stale, with the fetch time, no age.
    queue(&a, offline());
    let (message, data) = ok(run(&a, "current", json!({})));
    assert_outputs("current", &data);
    assert!(flag(&data, "stale"));
    assert_eq!(text(&data, "source"), "cache");
    assert_eq!(text(&data, "problem"), "connection-failed");
    assert_eq!(text(&data, "fetchedAt"), "2026-11-16T18:16:34Z");
    assert!(!data.contains_key("ageMinutes"));
    assert!(
        message.starts_with(
            "Not current: Open-Meteo gave no new data (connection-failed). Saved data from \
2026-11-16T18:16:34Z; the plugin has no clock and cannot tell how old that is. Warsaw: "
        ),
        "{message}"
    );
    assert!(text(&data, "summary").starts_with("[stale] Warsaw: "));
    assert_eq!(variable(&a, "stale"), json!(true));
    assert!(
        variable(&a, "summary")
            .as_str()
            .unwrap()
            .starts_with("[stale] ")
    );
    assert_eq!(int(&data, "temperature"), 7);

    // The provider answers with an error 42 minutes later: its Date gives
    // the age.
    queue(&a, http_at(503, "busy", "Mon, 16 Nov 2026 18:58:40 GMT"));
    let (message, data) = ok(run(&a, "current", json!({})));
    assert!(flag(&data, "stale"));
    assert_eq!(text(&data, "problem"), "provider-error");
    assert_eq!(int(&data, "ageMinutes"), 42);
    assert!(
        message.contains("Saved data from 2026-11-16T18:16:34Z, about 42 min ago. "),
        "{message}"
    );

    // The same in Polish.
    a.borrow_mut().settings["language"] = json!("pl");
    queue(&a, http_at(503, "busy", "Mon, 16 Nov 2026 18:58:40 GMT"));
    let (message, data) = ok(run(&a, "current", json!({})));
    assert!(
        message.starts_with(
            "Nieaktualne: Open-Meteo nie podał nowych danych (provider-error). Zapisane dane z \
2026-11-16T18:16:34Z, około 42 min temu. Warsaw: Słaba mżawka"
        ),
        "{message}"
    );
    assert!(text(&data, "summary").starts_with("[nieaktualne] "));
    queue(&a, offline());
    let (message, _) = ok(run(&a, "current", json!({})));
    assert!(
        message.contains("wtyczka nie ma zegara i nie wie, jak dawno to było"),
        "{message}"
    );
    a.borrow_mut().settings["language"] = json!("en");

    // An error five minutes after the fetch: the saved data is still
    // within its lifetime and is not called stale, but the problem is told.
    queue(&a, http_at(500, "", "Mon, 16 Nov 2026 18:21:34 GMT"));
    let (message, data) = ok(run(&a, "current", json!({})));
    assert!(!flag(&data, "stale"));
    assert_eq!(text(&data, "source"), "cache");
    assert_eq!(int(&data, "ageMinutes"), 5);
    assert_eq!(text(&data, "problem"), "provider-error");
    assert!(
        message
            .contains("(from the cache, 5 min old; Open-Meteo gave no new data: provider-error)"),
        "{message}"
    );

    // Offline with a run time: the age comes from the two run times only
    // when the saved data has one; this one was fetched by a key press.
    queue(&a, offline());
    let (_, data) = ok(run(&a, "current", json!({ "at": "2026-11-16 21:00" })));
    assert!(flag(&data, "stale"));
    assert!(!data.contains_key("ageMinutes"));
}

#[test]
fn offline_with_run_times_tells_the_age() {
    let a = warsaw();
    queue(&a, http(200, FORECAST));
    ok(run(&a, "current", json!({ "at": "2026-11-16 19:16" })));
    queue(&a, offline());
    let (message, data) = ok(run(&a, "current", json!({ "at": "2026-11-16 21:16" })));
    assert!(flag(&data, "stale"));
    assert_eq!(int(&data, "ageMinutes"), 120);
    assert!(message.contains("about 120 min ago"), "{message}");
}

#[test]
fn every_failure_without_saved_data_has_its_code() {
    let big = format!("{{\"pad\":\"{}\"}}", "x".repeat(open_meteo::MAX_BODY_BYTES));
    let cases: Vec<(Reply, &str, bool)> = vec![
        (http(500, "oops"), PROVIDER_ERROR, true),
        (http(503, ""), PROVIDER_ERROR, true),
        (http(404, "{}"), PROVIDER_ERROR, false),
        (
            http(
                400,
                r#"{"error":true,"reason":"Latitude must be in range of -90 to 90°. Given: 52.23."}"#,
            ),
            PROVIDER_ERROR,
            false,
        ),
        (http(429, "Too many requests"), codes::THROTTLED, true),
        (
            http(200, "<html>maintenance</html>"),
            INVALID_RESPONSE,
            false,
        ),
        (http(200, ""), INVALID_RESPONSE, false),
        (http(200, "[1,2,3]"), INVALID_RESPONSE, false),
        (http(200, "{}"), INVALID_RESPONSE, false),
        (
            http(200, r#"{"current":{"time":"2026-11-16T19:15"}}"#),
            INVALID_RESPONSE,
            false,
        ),
        (
            http(200, &FORECAST[..FORECAST.len() / 2]),
            INVALID_RESPONSE,
            false,
        ),
        (
            http(200, r#"{"error":true,"reason":"52.23 21.01"}"#),
            PROVIDER_ERROR,
            false,
        ),
        (http(200, &big), RESPONSE_TOO_LARGE, false),
        (
            Reply::Error(
                CallErrorCode::Failed,
                "the response is larger than 524288 bytes",
                None,
            ),
            RESPONSE_TOO_LARGE,
            false,
        ),
        (
            Reply::Error(
                CallErrorCode::Failed,
                "timed out",
                Some(NetworkErrorKind::TimedOut),
            ),
            TIMED_OUT,
            true,
        ),
        (
            Reply::Error(
                CallErrorCode::Failed,
                "no address",
                Some(NetworkErrorKind::DnsFailed),
            ),
            "dns-failed",
            true,
        ),
        (offline(), "connection-failed", true),
        (
            Reply::Error(
                CallErrorCode::Failed,
                "bad certificate",
                Some(NetworkErrorKind::TlsFailed),
            ),
            "tls-failed",
            false,
        ),
        (
            Reply::Error(CallErrorCode::Throttled, "60 requests per minute", None),
            codes::THROTTLED,
            true,
        ),
        (
            Reply::Error(CallErrorCode::Unavailable, "interrupted", None),
            codes::REQUEST_FAILED,
            true,
        ),
    ];
    for (reply, expected, retryable) in cases {
        let label = format!("{reply:?}");
        let label: String = label.chars().take(120).collect();
        for action in ["current", "forecast"] {
            let a = warsaw();
            queue(
                &a,
                match &reply {
                    Reply::Http {
                        status,
                        headers,
                        body,
                    } => Reply::Http {
                        status: *status,
                        headers: headers.clone(),
                        body: body.clone(),
                    },
                    Reply::Error(code, message, kind) => Reply::Error(*code, message, *kind),
                },
            );
            let (code, message, retry) = failed(run(&a, action, json!({})));
            assert_eq!(code, expected, "{action}: {label}");
            assert_eq!(retry, retryable, "{action}: {label}");
            assert!(
                !message.is_empty() && message.chars().count() <= 300,
                "{message}"
            );
            for part in SECRET_PARTS {
                assert!(!message.contains(part), "{action}: {message}");
            }
            assert!(codes::is_shared(&code) || PLUGIN_CODES.contains(&code.as_str()));
            // Nothing is stored, and the variables say the values are old.
            let a = a.borrow();
            assert!(!a.store.contains_key(WEATHER_KEY), "{label}");
            let stale = if action == "current" {
                "stale"
            } else {
                "forecastStale"
            };
            assert_eq!(a.published, [(stale.to_owned(), json!(true))], "{label}");
        }
    }
}

#[test]
fn saved_data_stands_in_for_every_failure_of_the_provider_but_not_for_a_refusal() {
    let fallbacks = [
        http(500, "oops"),
        http(429, ""),
        http(200, "not json"),
        http(200, "{}"),
        Reply::Error(
            CallErrorCode::Failed,
            "the response is larger than 524288 bytes",
            None,
        ),
        Reply::Error(
            CallErrorCode::Failed,
            "timed out",
            Some(NetworkErrorKind::TimedOut),
        ),
        offline(),
    ];
    let problems = [
        PROVIDER_ERROR,
        codes::THROTTLED,
        INVALID_RESPONSE,
        INVALID_RESPONSE,
        RESPONSE_TOO_LARGE,
        TIMED_OUT,
        "connection-failed",
    ];
    for (reply, problem) in fallbacks.into_iter().zip(problems) {
        let a = warsaw();
        queue(&a, http(200, FORECAST));
        ok(run(&a, "current", json!({})));
        queue(&a, reply);
        let (_, data) = ok(run(&a, "forecast", json!({})));
        assert_outputs("forecast", &data);
        assert_eq!(text(&data, "problem"), problem);
        assert_eq!(text(&data, "source"), "cache");
        // The saved answer is kept for the next run.
        assert_eq!(a.borrow().store[WEATHER_KEY]["fetched_at"], DATE_UNIX);
    }

    // HTTP 400: the request is wrong, so old data would be for something
    // else. A denied network: the user said no.
    let a = warsaw();
    queue(&a, http(200, FORECAST));
    ok(run(&a, "current", json!({})));
    queue(
        &a,
        http(400, r#"{"error":true,"reason":"Invalid timezone"}"#),
    );
    let (code, message, _) = failed(run(&a, "current", json!({})));
    assert_eq!(code, PROVIDER_ERROR);
    assert!(message.contains("HTTP 400"), "{message}");
    a.borrow_mut().deny_network = true;
    let (code, message, retryable) = failed(run(&a, "current", json!({})));
    assert_eq!(code, codes::NOT_PERMITTED);
    assert!(!retryable);
    assert!(
        message.contains("allow its network permission"),
        "{message}"
    );
    assert!(!message.contains("latitude"), "{message}");
}

#[test]
fn a_denied_plugin_fails_cleanly() {
    let mut denied = MockHost::deny_all();
    for (action, params) in [
        ("current", json!({})),
        ("forecast", json!({})),
        ("find-location", json!({ "query": "Warsaw" })),
        ("forget-location", json!({})),
    ] {
        let (code, ..) = failed(denied.invoke(&plugin(), action, params));
        assert_eq!(code, codes::NOT_PERMITTED, "{action}");
    }
}

#[test]
fn after_http_429_a_timed_run_waits_as_long_as_the_provider_asked() {
    let a = warsaw();
    queue(&a, http(200, FORECAST));
    ok(run(&a, "current", json!({ "at": "2026-11-16 19:00" })));
    queue(
        &a,
        Reply::Http {
            status: 429,
            headers: vec![
                ("date", "Mon, 16 Nov 2026 18:36:34 GMT".into()),
                ("retry-after", "1800".into()),
            ],
            body: String::new(),
        },
    );
    let (_, data) = ok(run(&a, "current", json!({ "at": "2026-11-16 19:20" })));
    assert_eq!(text(&data, "problem"), "throttled");
    assert!(flag(&data, "stale"));
    assert_eq!(int(&data, "ageMinutes"), 20);
    assert_eq!(requests(&a), 2);

    // Within the 30 minutes: no request at all, the saved data as stale.
    for at in ["2026-11-16 19:21", "2026-11-16 19:49"] {
        let (_, data) = ok(run(&a, "current", json!({ "at": at })));
        assert_eq!(text(&data, "problem"), "throttled");
        assert!(flag(&data, "stale"));
        assert_eq!(text(&data, "source"), "cache");
    }
    let (_, data) = ok(run(&a, "forecast", json!({ "at": "2026-11-16 19:49" })));
    // 49 minutes old: within the forecast's hour, so nothing is wrong.
    assert!(!flag(&data, "stale"));
    assert!(!data.contains_key("problem"));
    assert_eq!(requests(&a), 2);

    // After it: asked again, and the wait is over.
    queue(&a, http(200, FORECAST));
    let (_, data) = ok(run(&a, "current", json!({ "at": "2026-11-16 19:50" })));
    assert_eq!(text(&data, "source"), "network");
    assert_eq!(requests(&a), 3);
    assert_eq!(a.borrow().store[STATE_KEY]["wait_until_local"], Value::Null);

    // Without saved data the wait is a failure, still without a request.
    let b = warsaw();
    queue(&b, http(429, ""));
    let (code, ..) = failed(run(&b, "current", json!({ "at": "2026-11-16 19:00" })));
    assert_eq!(code, codes::THROTTLED);
    // No Retry-After: 15 minutes.
    let (code, message, retryable) =
        failed(run(&b, "current", json!({ "at": "2026-11-16 19:14" })));
    assert_eq!((code.as_str(), retryable), (codes::THROTTLED, true));
    assert!(message.contains("asked to wait"), "{message}");
    assert_eq!(requests(&b), 1);
    queue(&b, http(200, FORECAST));
    ok(run(&b, "current", json!({ "at": "2026-11-16 19:15" })));
    assert_eq!(requests(&b), 2);
}

// ---- forecast --------------------------------------------------------------

#[test]
fn forecast_reports_the_next_days() {
    let a = warsaw();
    queue(&a, http(200, FORECAST));
    let (message, data) = ok(run(&a, "forecast", json!({})));
    assert_outputs("forecast", &data);
    assert_eq!(
        message,
        "Warsaw: 2026-11-16: Light drizzle, 1 to 7 °C, precipitation 41% | \
2026-11-17: Moderate drizzle, 4 to 7 °C, precipitation 78% | \
2026-11-18: Overcast, 4 to 10 °C, precipitation 88%. \
Weather data by Open-Meteo.com (https://open-meteo.com/), CC BY 4.0"
    );
    assert_eq!(int(&data, "days"), 3);
    assert_eq!(text(&data, "summary").lines().count(), 3);
    assert_eq!(text(&data, "day1Date"), "2026-11-16");
    assert_eq!((int(&data, "day1Min"), int(&data, "day1Max")), (1, 7));
    assert_eq!(text(&data, "day1Condition"), "Light drizzle");
    assert_eq!(text(&data, "day1ConditionName"), "drizzle");
    assert_eq!(int(&data, "day1Precipitation"), 41);
    assert_eq!(text(&data, "day3ConditionName"), "overcast");
    assert_eq!(int(&data, "day3Max"), 10);
    assert!(!flag(&data, "stale"));
    let days: Value = serde_json::from_str(&text(&data, "json")).unwrap();
    assert_eq!(
        days[1],
        json!({
            "date": "2026-11-17", "min": 4, "max": 7, "weatherCode": 53,
            "condition": "Moderate drizzle", "conditionName": "drizzle",
            "precipitationProbability": 78,
        })
    );
    assert_eq!(variable(&a, "forecast"), days);
    assert_eq!(
        variable(&a, "forecastSummary"),
        json!(text(&data, "summary"))
    );
    assert_eq!(variable(&a, "forecastStale"), json!(false));
    let published: Vec<String> = a.borrow().published.iter().map(|p| p.0.clone()).collect();
    assert_eq!(published, FORECAST_VARIABLES);

    // All seven days, in Polish and imperial units, from the same answer.
    a.borrow_mut().settings["language"] = json!("pl");
    a.borrow_mut().settings["units"] = json!("imperial");
    queue(&a, http(200, FORECAST));
    let (message, data) = ok(run(&a, "forecast", json!({ "days": 7 })));
    assert_outputs("forecast", &data);
    assert_eq!(int(&data, "days"), 7);
    let summary = text(&data, "summary");
    let lines: Vec<&str> = summary.lines().collect();
    assert_eq!(
        lines[0],
        "2026-11-16: Słaba mżawka, od 33 do 45 °F, opady 41%"
    );
    // A day without a probability says nothing about precipitation.
    assert_eq!(
        lines[4],
        "2026-11-20: Częściowe zachmurzenie, od 33 do 47 °F"
    );
    assert_eq!(
        lines[6],
        "2026-11-22: Słabe opady śniegu, od 31 do 38 °F, opady 31%"
    );
    assert!(!data.contains_key("day4Date"));
    assert!(
        message.chars().count() <= 1024,
        "{}",
        message.chars().count()
    );
    let days: Value = serde_json::from_str(&text(&data, "json")).unwrap();
    assert_eq!(days[4]["precipitationProbability"], Value::Null);
    assert_eq!(days.as_array().unwrap().len(), 7);

    // One day.
    queue(&a, http(200, FORECAST));
    let (_, data) = ok(run(&a, "forecast", json!({ "days": 1 })));
    assert_outputs("forecast", &data);
    assert_eq!(int(&data, "days"), 1);
    assert!(!data.contains_key("day2Date"));

    for days in [0, 8, -1] {
        let (code, ..) = failed(run(&a, "forecast", json!({ "days": days })));
        assert_eq!(code, codes::INVALID_PARAMETERS);
    }
}

#[test]
fn the_forecast_is_cached_for_an_hour_and_shares_the_answer_with_current() {
    let a = warsaw();
    queue(&a, http(200, FORECAST));
    ok(run(&a, "current", json!({ "at": "2026-11-16 19:00" })));
    // 59 minutes later the forecast still comes from that one answer;
    // the current conditions would be asked for again.
    let (message, data) = ok(run(&a, "forecast", json!({ "at": "2026-11-16 19:59" })));
    assert_eq!(requests(&a), 1);
    assert_eq!(text(&data, "source"), "cache");
    assert!(!flag(&data, "stale"));
    assert_eq!(int(&data, "ageMinutes"), 59);
    assert!(
        message.contains("(from the cache, 59 min old)"),
        "{message}"
    );
    queue(&a, http(200, FORECAST));
    ok(run(&a, "forecast", json!({ "at": "2026-11-16 20:00" })));
    assert_eq!(requests(&a), 2);

    // Offline: the saved days, marked.
    queue(&a, offline());
    let (message, data) = ok(run(&a, "forecast", json!({ "days": 2 })));
    assert!(flag(&data, "stale"));
    assert!(message.starts_with("Not current: "), "{message}");
    assert!(text(&data, "summary").starts_with("[stale] 2026-11-16: "));
    assert_eq!(variable(&a, "forecastStale"), json!(true));
    assert!(
        variable(&a, "forecastSummary")
            .as_str()
            .unwrap()
            .starts_with("[stale] ")
    );
}

// ---- the place -------------------------------------------------------------

#[test]
fn without_a_place_the_actions_say_how_to_set_one() {
    let a = agent(json!({}));
    for action in ["current", "forecast"] {
        let (code, message, retryable) = failed(run(&a, action, json!({})));
        assert_eq!(code, NO_LOCATION);
        assert!(!retryable);
        assert!(message.contains("Find location"), "{message}");
    }
    assert_eq!(requests(&a), 0);
}

#[test]
fn coordinates_in_the_settings_are_checked() {
    for settings in [
        json!({ "latitude": "52.23" }),
        json!({ "longitude": "21.01" }),
        json!({ "latitude": "north", "longitude": "21.01" }),
        json!({ "latitude": "90.01", "longitude": "21.01" }),
        json!({ "latitude": "52.23", "longitude": "-180.5" }),
        json!({ "latitude": "NaN", "longitude": "21.01" }),
        json!({ "latitude": "inf", "longitude": "21.01" }),
        json!({ "latitude": "52.23", "longitude": "21.01", "timezone": "Warsaw time" }),
        json!({ "latitude": "52.23", "longitude": "21.01", "timezone": "UTC&x=1" }),
    ] {
        let a = agent(settings.clone());
        let (code, message, _) = failed(run(&a, "current", json!({})));
        assert_eq!(code, INVALID_LOCATION, "{settings}");
        for part in SECRET_PARTS {
            assert!(!message.contains(part), "{message}");
        }
        assert_eq!(requests(&a), 0);
    }
    // A decimal comma, spaces, no name and no time zone are fine.
    let a = agent(json!({ "latitude": " 52,2297 ", "longitude": "21,0122" }));
    queue(&a, http(200, FORECAST));
    let (message, data) = ok(run(&a, "current", json!({})));
    assert!(message.starts_with("Your location: "), "{message}");
    assert_eq!(text(&data, "location"), "Your location");
    assert!(a.borrow().urls[0].contains("latitude=52.23&longitude=21.01&"));
    assert!(a.borrow().urls[0].contains("&timezone=auto&"));
    a.borrow_mut().settings["language"] = json!("pl");
    queue(&a, http(200, FORECAST));
    let (message, _) = ok(run(&a, "current", json!({})));
    assert!(message.starts_with("Twoja lokalizacja: "), "{message}");
}

#[test]
fn find_location_lists_matches_and_stores_the_chosen_one() {
    let a = agent(json!({}));
    queue(&a, http(200, PLACES));
    let (message, data) = ok(run(
        &a,
        "find-location",
        json!({ "query": "  Springfield " }),
    ));
    assert_outputs("find-location", &data);
    assert_eq!(int(&data, "count"), 3);
    assert_eq!(
        text(&data, "matches"),
        "1. Springfield, Missouri, United States (America/Chicago)\n\
2. Springfield, Illinois, United States (America/Chicago)\n\
3. Springfield, Massachusetts, United States (America/New_York)"
    );
    assert!(
        message.starts_with("Found 3: 1. Springfield, Missouri"),
        "{message}"
    );
    assert!(message.contains("Run again with Select"), "{message}");
    assert!(message.ends_with(
        "Location data based on GeoNames (https://www.geonames.org/), via Open-Meteo.com"
    ));
    assert_eq!(text(&data, "source"), "network");
    assert!(!data.contains_key("selected"));
    assert!(
        !a.borrow().store.contains_key(LOCATION_KEY),
        "nothing chosen"
    );
    assert_eq!(
        a.borrow().urls,
        [
            "https://geocoding-api.open-meteo.com/v1/search?name=springfield&count=5&language=en&format=json"
        ]
    );

    // The second run picks one: from the cache, without a request.
    let (message, data) = ok(run(
        &a,
        "find-location",
        json!({ "query": "SPRINGFIELD", "select": 3 }),
    ));
    assert_outputs("find-location", &data);
    assert_eq!(requests(&a), 1);
    assert_eq!(text(&data, "source"), "cache");
    assert_eq!(
        text(&data, "selected"),
        "Springfield, Massachusetts, United States"
    );
    assert_eq!(text(&data, "timezone"), "America/New_York");
    assert!(
        message.starts_with(
            "Location set to Springfield, Massachusetts, United States. Location data"
        ),
        "{message}"
    );
    assert_eq!(a.borrow().store[LOCATION_KEY]["latitude"], json!(42.10148));

    // The weather is now for that place.
    queue(&a, http(200, FORECAST));
    let (message, data) = ok(run(&a, "current", json!({})));
    assert!(message.starts_with("Springfield: "), "{message}");
    assert_eq!(text(&data, "location"), "Springfield");
    assert!(a.borrow().urls[1].contains("latitude=42.10&longitude=-72.59&"));
    assert!(a.borrow().urls[1].contains("&timezone=America%2FNew_York&"));

    // Another choice drops the weather of the old place.
    ok(run(
        &a,
        "find-location",
        json!({ "query": "springfield", "select": 1 }),
    ));
    assert!(!a.borrow().store.contains_key(WEATHER_KEY));
    queue(&a, offline());
    let (code, ..) = failed(run(&a, "current", json!({})));
    assert_eq!(code, "connection-failed");

    let (code, message, _) = failed(run(
        &a,
        "find-location",
        json!({ "query": "springfield", "select": 4 }),
    ));
    assert_eq!(code, INVALID_SELECTION);
    assert_eq!(message, "Select is 4, but the search found only 3");
    for select in [-1, 6] {
        let (code, ..) = failed(run(
            &a,
            "find-location",
            json!({ "query": "springfield", "select": select }),
        ));
        assert_eq!(code, codes::INVALID_PARAMETERS);
    }
}

#[test]
fn find_location_without_a_match_and_with_a_bad_query() {
    let a = agent(json!({ "language": "pl" }));
    queue(&a, http(200, NO_PLACES));
    let (code, message, retryable) =
        failed(run(&a, "find-location", json!({ "query": "Zzqxqvj" })));
    assert_eq!(code, LOCATION_NOT_FOUND);
    assert!(!retryable);
    assert!(!message.contains("Zzqxqvj"), "{message}");
    assert!(a.borrow().urls[0].contains("language=pl"));
    // A search without a match is not kept: the next one asks again.
    assert!(!a.borrow().store.keys().any(|k| k.starts_with(GEO_PREFIX)));
    queue(&a, http(200, r#"{"results":[]}"#));
    let (code, ..) = failed(run(
        &a,
        "find-location",
        json!({ "query": "Zzqxqvj", "select": 1 }),
    ));
    assert_eq!(code, LOCATION_NOT_FOUND);
    assert_eq!(requests(&a), 2);

    for query in ["", " ", "a", " \n b\t"] {
        let (code, ..) = failed(run(&a, "find-location", json!({ "query": query })));
        assert_eq!(code, codes::INVALID_PARAMETERS, "{query:?}");
    }
    let (code, ..) = failed(run(&a, "find-location", json!({})));
    assert_eq!(code, codes::INVALID_PARAMETERS);
    let (code, ..) = failed(run(&a, "find-location", json!({ "query": "x".repeat(81) })));
    assert_eq!(code, codes::INVALID_PARAMETERS);
    assert_eq!(requests(&a), 2);

    // Failures of the geocoding service.
    for (reply, expected) in [
        (http(500, ""), PROVIDER_ERROR),
        (http(429, ""), codes::THROTTLED),
        (
            http(
                400,
                r#"{"error":true,"reason":"Parameter count must be between 1 and 100."}"#,
            ),
            PROVIDER_ERROR,
        ),
        (http(200, "nope"), INVALID_RESPONSE),
        (http(200, r#"{"results":"many"}"#), INVALID_RESPONSE),
        (
            http(
                200,
                r#"{"results":[{"name":"X","latitude":91.0,"longitude":0.0}]}"#,
            ),
            INVALID_RESPONSE,
        ),
        (
            http(
                200,
                r#"{"results":[{"name":" ","latitude":1.0,"longitude":0.0}]}"#,
            ),
            INVALID_RESPONSE,
        ),
        (offline(), "connection-failed"),
    ] {
        queue(&a, reply);
        let (code, ..) = failed(run(&a, "find-location", json!({ "query": "Kraków" })));
        assert_eq!(code, expected);
    }
    assert!(!a.borrow().store.contains_key(LOCATION_KEY));
}

#[test]
fn names_from_the_provider_are_cleaned() {
    let a = agent(json!({}));
    let long = "N".repeat(200);
    queue(
        &a,
        http(
            200,
            &json!({ "results": [
                { "name": "Nowy\n  Targ\u{7}", "latitude": 49.48, "longitude": 20.03,
                  "timezone": "Europe/Warsaw; DROP", "admin1": "", "country": "Polska" },
                { "name": long, "latitude": 1.0, "longitude": 2.0 },
                { "name": "Singapore", "latitude": 1.29, "longitude": 103.85, "country": "Singapore" },
            ]})
            .to_string(),
        ),
    );
    let (_, data) = ok(run(&a, "find-location", json!({ "query": "nowy targ" })));
    let matches = text(&data, "matches");
    let lines: Vec<&str> = matches.lines().collect();
    assert_eq!(lines[0], "1. Nowy Targ, Polska");
    assert_eq!(lines[1], format!("2. {}", "N".repeat(80)));
    // A country with the place's own name is not repeated.
    assert_eq!(lines[2], "3. Singapore");
}

#[test]
fn settings_coordinates_win_over_the_stored_place_and_the_user_is_told() {
    let a = warsaw();
    queue(&a, http(200, PLACES));
    let (message, _) = ok(run(
        &a,
        "find-location",
        json!({ "query": "Springfield", "select": 1 }),
    ));
    assert!(
        message.contains("The latitude and longitude in the settings take precedence"),
        "{message}"
    );
    queue(&a, http(200, FORECAST));
    let (message, _) = ok(run(&a, "current", json!({})));
    assert!(message.starts_with("Warsaw: "), "{message}");
    assert!(a.borrow().urls[1].contains("latitude=52.23&longitude=21.01&"));

    // Clearing them switches to the stored place, and the weather saved
    // for Warsaw is not shown for it.
    a.borrow_mut().settings = json!({});
    queue(&a, offline());
    let (code, ..) = failed(run(&a, "current", json!({})));
    assert_eq!(code, "connection-failed");
    queue(&a, http(200, FORECAST));
    let (message, _) = ok(run(&a, "current", json!({})));
    assert!(message.starts_with("Springfield: "), "{message}");
    assert!(a.borrow().urls[3].contains("latitude=37.22&longitude=-93.30&"));
}

#[test]
fn place_searches_are_bounded_and_expire_by_the_latest_known_time() {
    let a = agent(json!({}));
    for i in 0..MAX_GEO_ENTRIES + 3 {
        queue(&a, http(200, PLACES));
        ok(run(
            &a,
            "find-location",
            json!({ "query": format!("place {i}") }),
        ));
    }
    {
        let a = a.borrow();
        let keys: Vec<&String> = a
            .store
            .keys()
            .filter(|k| k.starts_with(GEO_PREFIX))
            .collect();
        assert_eq!(keys.len(), MAX_GEO_ENTRIES);
        assert!(!a.store.contains_key("geo:en:place 0"));
        assert!(!a.store.contains_key("geo:en:place 2"));
        assert!(a.store.contains_key("geo:en:place 3"));
        assert_eq!(
            a.store[STATE_KEY]["geo_keys"].as_array().unwrap().len(),
            MAX_GEO_ENTRIES
        );
    }
    // Another language is another search.
    let before = requests(&a);
    a.borrow_mut().settings = json!({ "language": "pl" });
    queue(&a, http(200, PLACES));
    ok(run(&a, "find-location", json!({ "query": "place 22" })));
    assert_eq!(requests(&a), before + 1);
    a.borrow_mut().settings = json!({});

    // Still served 29 days later by the latest known time ...
    let day = 86_400;
    a.borrow_mut().store.get_mut(STATE_KEY).unwrap()["last_seen"] = json!(DATE_UNIX + 29 * day);
    ok(run(&a, "find-location", json!({ "query": "place 22" })));
    assert_eq!(requests(&a), before + 1);
    // ... and asked again once an answer from 31 days later was seen.
    a.borrow_mut().store.get_mut(STATE_KEY).unwrap()["last_seen"] = json!(DATE_UNIX + 31 * day);
    queue(&a, http_at(200, PLACES, "Thu, 17 Dec 2026 18:16:34 GMT"));
    let (_, data) = ok(run(&a, "find-location", json!({ "query": "place 22" })));
    assert_eq!(text(&data, "source"), "network");
    assert_eq!(requests(&a), before + 2);
}

#[test]
fn forget_location_deletes_everything_stored() {
    let a = agent(json!({ "language": "pl" }));
    queue(&a, http(200, PLACES));
    ok(run(
        &a,
        "find-location",
        json!({ "query": "Springfield", "select": 2 }),
    ));
    queue(&a, http(200, FORECAST));
    ok(run(&a, "current", json!({})));
    assert_eq!(a.borrow().store.len(), 4, "{:?}", a.borrow().store.keys());

    let (message, data) = ok(run(&a, "forget-location", json!({})));
    assert!(
        message.starts_with("Usunięto zapisaną lokalizację"),
        "{message}"
    );
    assert!(data.is_empty());
    assert!(a.borrow().store.is_empty(), "{:?}", a.borrow().store.keys());
    assert_eq!(a.borrow().cleared, 1);
    let (code, ..) = failed(run(&a, "current", json!({})));
    assert_eq!(code, NO_LOCATION);
    // Again, with nothing stored.
    ok(run(&a, "forget-location", json!({})));
}

// ---- storage and privacy ---------------------------------------------------

#[test]
fn damaged_or_foreign_storage_counts_as_empty() {
    let a = warsaw();
    {
        let mut a = a.borrow_mut();
        a.store.insert(WEATHER_KEY.into(), json!("rubbish"));
        a.store.insert(STATE_KEY.into(), json!([1, 2, 3]));
        a.store.insert(LOCATION_KEY.into(), json!({ "name": 7 }));
        a.store
            .insert("geo:en:warsaw".into(), json!({ "v": 99, "places": [] }));
    }
    queue(&a, http(200, FORECAST));
    ok(run(&a, "current", json!({})));
    queue(&a, http(200, PLACES));
    ok(run(&a, "find-location", json!({ "query": "Warsaw" })));
    // An entry of another format version is not used.
    a.borrow_mut().store.get_mut(WEATHER_KEY).unwrap()["v"] = json!(2);
    queue(&a, offline());
    let (code, ..) = failed(run(&a, "current", json!({})));
    assert_eq!(code, "connection-failed");
    // A damaged stored place is no place.
    let b = agent(json!({}));
    b.borrow_mut()
        .store
        .insert(LOCATION_KEY.into(), json!({ "name": 7 }));
    let (code, ..) = failed(run(&b, "current", json!({})));
    assert_eq!(code, NO_LOCATION);
}

#[test]
fn a_refused_write_does_not_hide_the_weather() {
    let a = warsaw();
    let shared = Rc::clone(&a);
    let mut host = MockHost::new(move |request| match request {
        CapabilityRequest::SettingsGet(_) => CallResult::Ok(shared.borrow().settings.clone()),
        CapabilityRequest::StorageGet(_) => CallResult::Ok(Value::Null),
        CapabilityRequest::StorageSet(_) => {
            CallResult::error(CallErrorCode::Failed, "storage-quota: 256 KiB")
        }
        CapabilityRequest::HttpFetch(_) => {
            CallResult::Ok(json!({ "status": 200, "body": FORECAST }))
        }
        CapabilityRequest::Log(l) => {
            shared.borrow_mut().logs.push(l.message.clone());
            CallResult::Ok(Value::Null)
        }
        // Publishing fails too: the action still succeeds.
        CapabilityRequest::VariablesPublish(_) => {
            CallResult::error(CallErrorCode::Throttled, "10 per second")
        }
        other => panic!("{other:?}"),
    });
    let (message, data) = ok(host.invoke(&plugin(), "current", json!({})));
    assert!(message.starts_with("Warsaw: Light drizzle"), "{message}");
    // No Date header: the answer is fresh, its time unknown.
    assert!(!flag(&data, "stale"));
    assert!(!data.contains_key("fetchedAt"));
    let logs = a.borrow().logs.clone();
    assert!(
        logs.iter().any(|l| l.contains("storage refused")),
        "{logs:?}"
    );
    assert!(
        logs.iter()
            .any(|l| l.contains("temperature could not be published"))
    );
}

#[test]
fn stored_values_are_far_below_the_storage_limit() {
    let a = agent(json!({}));
    // The longest names the plugin keeps, in the widest characters.
    let name = "\u{10348}".repeat(200);
    let results: Vec<Value> = (0..20)
        .map(|i| {
            json!({ "name": name, "latitude": -89.123_456, "longitude": -179.123_456 + f64::from(i),
                    "timezone": "America/Argentina/ComodRivadavia", "admin1": name, "country": name })
        })
        .collect();
    queue(&a, http(200, &json!({ "results": results }).to_string()));
    ok(run(
        &a,
        "find-location",
        json!({ "query": "ż".repeat(80), "select": 5 }),
    ));
    queue(&a, http(200, FORECAST));
    ok(run(
        &a,
        "forecast",
        json!({ "days": 7, "at": "2026-11-16 19:16" }),
    ));
    let a = a.borrow();
    assert_eq!(a.store.len(), 4);
    for (key, value) in &a.store {
        let size = key.len() + value.to_string().len();
        assert!(size < 8 * 1024, "{key}: {size} bytes");
    }
    // Everything the plugin may ever hold: the place, the weather, the
    // state and 20 searches fit the default quota of 256 KiB many times.
    let geo = a
        .store
        .iter()
        .find(|(k, _)| k.starts_with(GEO_PREFIX))
        .unwrap();
    let total: usize = a
        .store
        .iter()
        .map(|(k, v)| k.len() + v.to_string().len())
        .sum::<usize>()
        + (MAX_GEO_ENTRIES - 1) * (geo.0.len() + geo.1.to_string().len());
    assert!(total < 128 * 1024, "{total} bytes");
}

#[test]
fn coordinates_reach_the_provider_and_nothing_else() {
    let a = warsaw();
    let mut seen: Vec<String> = Vec::new();
    let mut record = |outcome: ActionOutcome| match outcome {
        ActionOutcome::Ok { message, data } => {
            seen.push(message.unwrap_or_default());
            seen.push(format!("{data:?}"));
        }
        ActionOutcome::Failed { message, .. } => seen.push(message.unwrap_or_default()),
        ActionOutcome::Skipped { message } => seen.push(message.unwrap_or_default()),
    };
    queue(&a, http(200, FORECAST));
    record(run(&a, "current", json!({})));
    queue(&a, http(200, FORECAST));
    record(run(&a, "forecast", json!({ "days": 7 })));
    queue(&a, offline());
    record(run(&a, "current", json!({})));
    queue(&a, http(400, r#"{"error":true,"reason":"Given: 52.23."}"#));
    record(run(&a, "current", json!({})));
    queue(&a, http(200, "{\"latitude\":52.23"));
    record(run(&a, "forecast", json!({})));
    a.borrow_mut().deny_network = true;
    record(run(&a, "current", json!({})));
    a.borrow_mut().deny_network = false;
    queue(&a, http(200, PLACES));
    record(run(&a, "find-location", json!({ "query": "Springfield" })));
    record(run(
        &a,
        "find-location",
        json!({ "query": "Springfield", "select": 1 }),
    ));
    a.borrow_mut().settings = json!({});
    queue(&a, http(500, ""));
    record(run(&a, "current", json!({})));
    queue(&a, http(200, FORECAST));
    record(run(&a, "current", json!({})));

    let a = a.borrow();
    seen.extend(a.logs.iter().cloned());
    seen.extend(
        a.published
            .iter()
            .map(|(id, value)| format!("{id}={value}")),
    );
    assert!(seen.len() > 20);
    for text in &seen {
        for part in SECRET_PARTS {
            assert!(!text.contains(part), "{part} in {text}");
        }
    }
    // Requests go to the two declared origins only, with coordinates of
    // two decimals.
    assert_eq!(a.urls.len(), 9);
    for url in &a.urls {
        assert!(
            url.starts_with("https://api.open-meteo.com/v1/forecast?")
                || url.starts_with("https://geocoding-api.open-meteo.com/v1/search?"),
            "{url}"
        );
        assert!(!url.contains("52.229") && !url.contains("21.012") && !url.contains("37.215"));
    }
}

// ------------------------------------------------------------- key state

/// What a key bound to Current weather shows, through the state handler.
fn key(agent: &Shared) -> KeyState {
    match host(agent).key_state(&plugin(), "current", json!({})) {
        deckpilot_plugin_sdk::StateOutcome::Ok(state) => state,
        other => panic!("{other:?}"),
    }
}

#[test]
fn only_current_shows_key_state_and_it_fetches_nothing() {
    let m = manifest();
    for action in m["contributes"]["actions"].as_array().unwrap() {
        let id = action["id"].as_str().unwrap();
        let declared = action.get("state").is_some();
        assert_eq!(declared, id == "current", "{id}");
        if declared {
            assert_eq!(action["node"]["effect"], "readOnly");
            assert_eq!(
                action["state"],
                json!({"refresh": "every", "intervalSeconds": 60})
            );
        }
        let shows = plugin().find(id).unwrap().has_state();
        assert_eq!(shows, declared, "{id}");
    }
    // A state call with a report stored: no request, no write.
    let a = warsaw();
    queue(&a, http(200, FORECAST));
    ok(run(&a, "current", json!({})));
    let (urls, store) = (requests(&a), a.borrow().store.clone());
    key(&a);
    assert_eq!(requests(&a), urls);
    assert_eq!(a.borrow().store, store);
}

#[test]
fn the_key_shows_the_last_report_and_its_age_once_old() {
    let a = warsaw();
    // Nothing fetched yet: the key as the user designed it.
    assert_eq!(key(&a), KeyState::new());
    queue(&a, http(200, FORECAST));
    ok(run(&a, "current", json!({})));
    // Fresh: the temperature in ASCII, the condition, a colour for rain.
    let fresh = key(&a);
    assert_eq!(fresh.label.as_deref(), Some("7 C"));
    assert_eq!(fresh.detail.as_deref(), Some("Light drizzle"));
    assert_eq!(fresh.accent, Some(Accent::Blue));
    assert_eq!((fresh.badge.as_deref(), fresh.indicator), (None, None));
    // Still within the lifetime of 15 minutes.
    a.borrow_mut().now = FETCHED + 14 * 60;
    assert_eq!(key(&a), fresh);
    // Older: the same values, marked with the age; never as current.
    for (age, badge) in [(15 * 60, "15m"), (3 * 3_600, "3h"), (3 * 86_400, "3d")] {
        a.borrow_mut().now = FETCHED + age;
        let old = key(&a);
        assert_eq!(old.label, fresh.label);
        assert_eq!(old.badge.as_deref(), Some(badge));
        assert_eq!(old.indicator, Some(Indicator::Warning));
    }
    // A clock behind the answer's date: the age is unknown, and said so.
    a.borrow_mut().now = FETCHED - 60;
    let unknown = key(&a);
    assert_eq!(
        (unknown.badge.as_deref(), unknown.indicator),
        (Some("?"), Some(Indicator::Warning))
    );
    for state in [fresh, unknown] {
        state.validate().expect("within the bounds of key state");
    }
}

#[test]
fn the_key_follows_units_language_and_place() {
    let a = warsaw();
    queue(&a, http(200, FORECAST));
    ok(run(&a, "current", json!({})));
    // Imperial and Polish: no degree sign, no Polish letters on a key.
    a.borrow_mut().settings["units"] = json!("imperial");
    a.borrow_mut().settings["language"] = json!("pl");
    let state = key(&a);
    assert_eq!(state.label.as_deref(), Some("44 F"));
    assert_eq!(state.detail.as_deref(), Some("Slaba mzawka"));
    // Another place than the stored report's: nothing shown for it.
    a.borrow_mut().settings["latitude"] = json!("50.06");
    a.borrow_mut().settings["longitude"] = json!("19.94");
    assert_eq!(key(&a), KeyState::new());
    // A mistake in the settings: left to the action to report.
    a.borrow_mut().settings["latitude"] = json!("north");
    assert_eq!(key(&a), KeyState::new());
}

#[test]
fn key_texts_are_ascii_within_their_bounds() {
    use crate::key::{age_badge, ascii};
    assert_eq!(ascii("Mgła osadzająca szadź", 32), "Mgla osadzajaca szadz");
    assert_eq!(ascii("ZAŻÓŁĆ GĘŚLĄ JAŹŃ", 32), "ZAZOLC GESLA JAZN");
    assert_eq!(ascii("7 °C\n", 32), "7 C");
    assert_eq!(ascii(&"x".repeat(40), 32).len(), 32);
    for (code, _, en, pl) in conditions::CODES {
        for text in [en, pl] {
            let shown = ascii(text, 32);
            assert!(shown.is_ascii() && !shown.is_empty(), "{code}: {text}");
        }
    }
    for (seconds, badge) in [
        (0, "0m"),
        (59 * 60, "59m"),
        (60 * 60, "1h"),
        (47 * 3_600, "47h"),
        (48 * 3_600, "2d"),
        (u64::MAX, "999d"),
    ] {
        assert_eq!(age_badge(seconds), badge, "{seconds}");
        assert!(badge.chars().count() <= 4);
    }
}
