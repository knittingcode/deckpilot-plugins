//! Native unit tests: the definition agrees with `manifest.json`, and every
//! action behaves with the agent replaced by a scripted `MockHost` that
//! answers with canned GIOŚ responses (the shapes of real answers of
//! 2026-11-16, shortened). Nothing here reaches the network.
//! `tools/first-party.sh` adds what a unit test cannot: the manifest
//! against the schema, the Polish texts, the wasm build and the package
//! checks.

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
use crate::cache::{CHUNK, LIST_KEY, LIST_PREFIX, READING_KEY, STATE_KEY, STATION_KEY};
use crate::provider::{Sample, Sensors};

const MANIFEST: &str = include_str!("../manifest.json");

/// Real answers have the key `@context` twice.
const CONTEXT: &str = r#""@context":{"meta":"https://schema.org/meta","links":"https://schema.org/links"},"@context":{"links":"https://schema.org/relatedLink","totalPages":"https://schema.org/numberOfPages"},"#;
const META: &str = r#""meta":{"dc:language":"pol (ISO 639-2 / ISO 639-3)","schema:provider":{"@context":"https://schema.org","@type":"Organization","email":"api@gios.gov.pl"},"sy:updatePeriod":"hourly"},"links":{"self":"https://api.gios.gov.pl/pjp-api/v1/rest/x?page=0&size=20"}"#;

/// Five stations as the list service writes them, coordinates and streets
/// included; one has no street.
const STATION_ROWS: &str = r#"
{"Identyfikator stacji":11,"Kod stacji":"DsCzerStraza","Nazwa stacji":"Czerniawa","WGS84 φ N":"50.912475","WGS84 λ E":"15.312190","Identyfikator miasta":142,"Nazwa miasta":"Czerniawa","Gmina":"Świeradów-Zdrój","Powiat":"lubański","Województwo":"DOLNOŚLĄSKIE","Ulica":null},
{"Identyfikator stacji":400,"Kod stacji":"MpKrakAlKras","Nazwa stacji":"Kraków, Aleja Krasińskiego","WGS84 φ N":"50.057678","WGS84 λ E":"19.926189","Identyfikator miasta":415,"Nazwa miasta":"Kraków","Gmina":"Kraków","Powiat":"Kraków","Województwo":"MAŁOPOLSKIE","Ulica":"al. Krasińskiego"},
{"Identyfikator stacji":530,"Kod stacji":"MzWarAlNiepo","Nazwa stacji":"Warszawa, Al. Niepodległości","WGS84 φ N":"52.219298","WGS84 λ E":"21.004724","Identyfikator miasta":1006,"Nazwa miasta":"Warszawa","Gmina":"Warszawa","Powiat":"Warszawa","Województwo":"MAZOWIECKIE","Ulica":"Al. Niepodległości 227/233"},
{"Identyfikator stacji":550,"Kod stacji":"MzWarWokalna","Nazwa stacji":"Warszawa, ul. Wokalna","WGS84 φ N":"52.160772","WGS84 λ E":"21.033819","Identyfikator miasta":1006,"Nazwa miasta":"Warszawa","Gmina":"Warszawa","Powiat":"Warszawa","Województwo":"MAZOWIECKIE","Ulica":"ul. Wokalna 1"},
{"Identyfikator stacji":900,"Kod stacji":"LdLodzCzerni","Nazwa stacji":"Łódź, ul. Czernika","WGS84 φ N":"51.758050","WGS84 λ E":"19.529786","Identyfikator miasta":513,"Nazwa miasta":"Łódź","Gmina":"Łódź","Powiat":"Łódź","Województwo":"ŁÓDZKIE","Ulica":"ul. Czernika 1/3"}"#;

/// Fragments of the list that must never be stored, logged or shown.
const NEVER_KEPT: &[&str] = &["52.21", "21.00", "50.05", "19.92", "227/233", "Wokalna 1"];

const SENSORS_530: &str = r#"
{"Identyfikator stanowiska":3580,"Identyfikator stacji":530,"Wskaźnik":"dwutlenek azotu","Wskaźnik - wzór":"NO2","Wskaźnik - kod":"NO2","Id wskaźnika":6},
{"Identyfikator stanowiska":3584,"Identyfikator stacji":530,"Wskaźnik":"pył zawieszony PM10","Wskaźnik - wzór":"PM10","Wskaźnik - kod":"PM10","Id wskaźnika":3},
{"Identyfikator stanowiska":3585,"Identyfikator stacji":530,"Wskaźnik":"pył zawieszony PM2.5","Wskaźnik - wzór":"PM2.5","Wskaźnik - kod":"PM2.5","Id wskaźnika":69}"#;
const SENSORS_OZONE: &str = r#"
{"Identyfikator stanowiska":70,"Identyfikator stacji":11,"Wskaźnik":"ozon","Wskaźnik - wzór":"O3","Wskaźnik - kod":"O3","Id wskaźnika":5}"#;

/// 19:42:09 in Poland (CET), 42 minutes after the newest hourly value.
const DATE: &str = "Mon, 16 Nov 2026 18:42:09 GMT";
const DATE_UNIX: u64 = 1_794_854_529;
const HOUR: &str = "2026-11-16 19:00:00";

fn manifest() -> Value {
    serde_json::from_str(MANIFEST).expect("manifest.json is JSON")
}

fn stations_body(rows: &str, pages: usize) -> String {
    format!(r#"{{{CONTEXT}"Lista stacji pomiarowych":[{rows}],{META},"totalPages":{pages}}}"#)
}

fn sensors_body(rows: &str) -> String {
    format!(
        r#"{{{CONTEXT}"Lista stanowisk pomiarowych dla podanej stacji":[{rows}],{META},"totalPages":1}}"#
    )
}

/// Hourly values, newest first as the service gives them.
fn data_body(values: &[(&str, Option<f64>)]) -> String {
    let rows: Vec<String> = values
        .iter()
        .map(|(time, value)| {
            json!({ "Kod stanowiska": "MzWarAlNiepo-PM2.5-1g", "Data": time, "Wartość": value })
                .to_string()
        })
        .collect();
    format!(
        r#"{{{CONTEXT}"Lista danych pomiarowych":[{}],{META},"totalPages":4}}"#,
        rows.join(",")
    )
}

fn pm25_body() -> String {
    data_body(&[
        ("2026-11-16 20:00:00", None),
        (HOUR, Some(15.4)),
        ("2026-11-16 18:00:00", Some(15.0)),
    ])
}

fn pm10_body() -> String {
    data_body(&[(HOUR, Some(22.6)), ("2026-11-16 18:00:00", Some(21.0))])
}

/// The index answer: the overall value and the values of single
/// pollutants (`null` where the station has none).
fn index_body(overall: Value, time: Value, status: Value, parts: [Value; 5]) -> String {
    let [so2, no2, pm10, pm25, o3] = parts;
    let index = json!({
        "Identyfikator stacji pomiarowej": 530,
        "Data wykonania obliczeń indeksu": "2026-11-16 19:35:20",
        "Wartość indeksu": overall,
        "Nazwa kategorii indeksu": "Dobry",
        "Data danych źródłowych, z których policzono wartość indeksu dla wskaźnika st": time,
        "Wartość indeksu dla wskaźnika SO2": so2,
        "Nazwa kategorii indeksu dla wskażnika SO2": null,
        "Wartość indeksu dla wskaźnika NO2": no2,
        "Nazwa kategorii indeksu dla wskażnika NO2": "Dobry",
        "Wartość indeksu dla wskaźnika PM10": pm10,
        "Wartość indeksu dla wskaźnika PM2.5": pm25,
        "Wartość indeksu dla wskaźnika O3": o3,
        "Status indeksu ogólnego dla stacji pomiarowej": status,
        "Kod zanieczyszczenia krytycznego": "PYL",
    });
    format!(r#"{{{CONTEXT}"AqIndex":{index},{META},"totalPages":1}}"#)
}

fn good_index() -> String {
    index_body(
        json!(1),
        json!(HOUR),
        json!(true),
        [json!(null), json!(1), json!(1), json!(1), json!(2)],
    )
}

/// What the service answers for a station it has no index for.
fn no_index() -> String {
    index_body(
        json!(null),
        json!(null),
        json!(null),
        [
            json!(null),
            json!(null),
            json!(null),
            json!(null),
            json!(null),
        ],
    )
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
    http_at(status, body, DATE)
}

fn http_at(status: u16, body: &str, date: &str) -> Reply {
    Reply::Http {
        status,
        headers: vec![("date", date.into())],
        body: body.into(),
    }
}

fn undated(body: &str) -> Reply {
    Reply::Http {
        status: 200,
        headers: Vec::new(),
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
    variables: BTreeMap<String, Value>,
    variable_calls: usize,
    cleared: usize,
    deny_network: bool,
    full_storage: bool,
    /// The agent's clock for `time.now` (Unix seconds); the default is
    /// the canned answers' `Date`.
    now: u64,
}

type Shared = Rc<RefCell<Agent>>;

fn agent(settings: Value) -> Shared {
    Rc::new(RefCell::new(Agent {
        settings,
        now: DATE_UNIX,
        ..Agent::default()
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
                if a.full_storage {
                    return CallResult::error(CallErrorCode::Failed, "the storage is full");
                }
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
                assert!(
                    VARIABLES.contains(&v.id.as_str()),
                    "{} is not declared",
                    v.id
                );
                a.variable_calls += 1;
                a.variables.insert(v.id.clone(), v.value.clone());
                CallResult::Ok(Value::Null)
            }
            CapabilityRequest::VariablesClear(c) => {
                a.variable_calls += 1;
                match &c.id {
                    Some(id) => drop(a.variables.remove(id)),
                    None => {
                        a.cleared += 1;
                        a.variables.clear();
                    }
                }
                CallResult::Ok(Value::Null)
            }
            CapabilityRequest::Log(l) => {
                a.logs.push(l.message.clone());
                CallResult::Ok(Value::Null)
            }
            CapabilityRequest::HttpFetch(f) => {
                assert_eq!(f.method, HttpMethod::Get);
                assert!(f.body.is_none() && f.auth.is_none());
                assert!(f.headers.iter().any(|h| h.name == "User-Agent"
                    && h.value == format!("DeckPilot-AirQuality/{VERSION}")));
                // GIOŚ answers 406 to a request that accepts only
                // application/json.
                assert!(
                    f.headers
                        .iter()
                        .any(|h| h.name == "Accept" && h.value == gios::ACCEPT)
                );
                assert!(gios::ACCEPT.starts_with("application/ld+json"));
                assert!(
                    f.url
                        .starts_with("https://api.gios.gov.pl/pjp-api/v1/rest/"),
                    "{}",
                    f.url
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
                    None => panic!("an HTTP request the test did not expect: {}", f.url),
                }
            }
            CapabilityRequest::TimeNow(_) => CallResult::Ok(json!({
                "unixMs": a.now * 1000, "utcOffsetMinutes": 60
            })),
            other => panic!("an unexpected capability call: {other:?}"),
        }
    })
}

fn queue(agent: &Shared, reply: Reply) {
    agent.borrow_mut().replies.push_back(reply);
}

/// Queues the three answers of one reading of station 530.
fn queue_reading(agent: &Shared) {
    queue(agent, http(200, &good_index()));
    queue(agent, http(200, &pm25_body()));
    queue(agent, http(200, &pm10_body()));
}

fn run(agent: &Shared, action: &str, params: Value) -> ActionOutcome {
    agent.borrow_mut().variable_calls = 0;
    let outcome = host(agent).invoke(&plugin(), action, params);
    // The agent allows ten variable calls a second.
    assert!(agent.borrow().variable_calls <= 10, "{action}");
    assert!(
        agent.borrow().replies.is_empty(),
        "{action} left a queued answer unasked"
    );
    outcome
}

fn ok(outcome: ActionOutcome) -> (String, BTreeMap<String, ParamValue>) {
    match outcome {
        ActionOutcome::Ok { message, data } => {
            let message = message.unwrap_or_default();
            assert!(message.chars().count() <= 1_024, "{message}");
            (message, data)
        }
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

fn variable(agent: &Shared, id: &str) -> Option<Value> {
    agent.borrow().variables.get(id).cloned()
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

/// An agent that has the station list and has chosen station 530; no
/// request is recorded.
fn chosen_530(settings: Value) -> Shared {
    let agent = agent(settings);
    queue(&agent, http(200, &stations_body(STATION_ROWS, 1)));
    queue(&agent, http(200, &sensors_body(SENSORS_530)));
    ok(run(&agent, "select-station", json!({ "station": 530 })));
    agent.borrow_mut().urls.clear();
    agent.borrow_mut().logs.clear();
    agent
}

fn current(agent: &Shared, params: Value) -> (String, BTreeMap<String, ParamValue>) {
    let (message, data) = ok(run(agent, "current", params));
    assert_outputs("current", &data);
    // The source line is in every text a user may see.
    assert!(
        message.contains(gios::SOURCE.trim_end_matches('.')),
        "{message}"
    );
    assert!(text(&data, "summary").ends_with(gios::SOURCE));
    assert!(text(&data, "attribution").starts_with("Źródło danych: GIOŚ - EKOINFONET"));
    (message, data)
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

    // The one GIOŚ origin the code asks, GET only; and key state for
    // Current air quality (plugin API 1.2).
    let permissions = m["permissions"].as_array().unwrap();
    assert_eq!(permissions.len(), 2);
    assert_eq!(permissions[1]["kind"], "device");
    assert_eq!(permissions[1]["state"], true);
    assert!(permissions[1].get("icons").is_none());
    assert_eq!(m["engines"]["pluginApi"], "^1.2");
    assert_eq!(plugin().api_level().to_string(), "1.2");
    assert_eq!(permissions[0]["kind"], "network");
    assert_eq!(permissions[0]["hosts"], json!([gios::HOST]));
    assert_eq!(permissions[0]["methods"], json!(["GET"]));
    assert!(permissions[0].get("userOrigins").is_none());
    assert!(permissions[0].get("privateNetwork").is_none());
    for url in [
        gios::stations_url(1),
        gios::sensors_url(530),
        gios::index_url(530),
        gios::data_url(3585),
    ] {
        assert!(
            url.starts_with(&format!("https://{}/", gios::HOST)),
            "{url}"
        );
    }
    // No key of anybody: no credential slot, no secret in the module.
    assert!(m["contributes"].get("credentials").is_none());

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
    assert_eq!(variables, VARIABLES);
    assert!(VARIABLES.len() <= 10);

    let fields = m["contributes"]["settings"]["fields"].as_array().unwrap();
    assert_eq!(fields[0]["key"], "provider");
    assert_eq!(fields[0]["kind"]["options"], json!([Gios.id()]));
    assert_eq!(fields[1]["kind"]["options"], json!(["en", "pl"]));
    // The description carries the source line: the Marketplace shows it.
    assert!(
        m["description"]
            .as_str()
            .unwrap()
            .contains("Źródło danych: GIOŚ - EKOINFONET")
    );
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
fn times_are_parsed_and_polish_time_follows_summer_time() {
    assert_eq!(time::http_date(DATE), Some(DATE_UNIX));
    assert_eq!(time::http_date("16 Nov 2026"), None);
    assert_eq!(time::utc_text(DATE_UNIX), "2026-11-16T18:42:09Z");
    assert_eq!(
        time::local_minutes("2026-11-16 19:42").unwrap()
            - time::local_minutes("2026-11-16 19:00").unwrap(),
        42
    );
    assert_eq!(time::local_minutes("2026-11-16T19:42"), None);

    // Winter: UTC+1. 19:00 in Poland is 18:00 UTC.
    assert_eq!(time::warsaw_to_unix(HOUR), Some(DATE_UNIX - 42 * 60 - 9));
    // Summer: UTC+2. 2026-07-01 12:00 in Poland is 10:00 UTC.
    assert_eq!(
        time::warsaw_to_unix("2026-07-01 12:00:00")
            .map(time::utc_text)
            .as_deref(),
        Some("2026-07-01T10:00:00Z")
    );
    // The changes of 2026: 29 March and 25 October, at 01:00 UTC.
    for (local, utc) in [
        ("2026-03-29 01:59:59", "2026-03-29T00:59:59Z"),
        ("2026-03-29 03:00:00", "2026-03-29T01:00:00Z"),
        ("2026-10-25 01:59:59", "2026-10-24T23:59:59Z"),
        // The hour that occurs twice: read as the first one, the older.
        ("2026-10-25 02:30:00", "2026-10-25T00:30:00Z"),
        ("2026-10-25 03:00:00", "2026-10-25T02:00:00Z"),
        ("2024-02-29 00:00:00", "2024-02-28T23:00:00Z"),
    ] {
        assert_eq!(
            time::warsaw_to_unix(local).map(time::utc_text).as_deref(),
            Some(utc),
            "{local}"
        );
    }
    for wrong in [
        "",
        "2026-11-16",
        "2026-11-16 19:00",
        "2026-11-16T19:00:00",
        "2026-13-01 00:00:00",
        "2026-02-30 00:00:00",
        "2026-11-16 24:00:00",
        "2026-11-16 19:00:00Z",
        "1969-12-31 23:00:00",
    ] {
        assert_eq!(time::warsaw_to_unix(wrong), None, "{wrong}");
    }
}

#[test]
fn every_index_value_has_a_category_in_both_languages() {
    let names: Vec<&str> = (0..6).map(|i| Gios.level(i).unwrap().name).collect();
    assert_eq!(
        names,
        [
            "very-good",
            "good",
            "moderate",
            "sufficient",
            "bad",
            "very-bad"
        ]
    );
    for index in 0..6 {
        let level = Gios.level(index).unwrap();
        assert!(!level.en.is_empty() && !level.pl.is_empty() && level.en != level.pl);
        assert_eq!(level.text(Language::Pl), level.pl);
    }
    assert_eq!(Gios.level(1).unwrap().pl, "Dobry");
    // Off the scale is no category, never a neighbour's.
    for index in [-1, 6, 100, i64::MAX, i64::MIN] {
        assert_eq!(Gios.level(index), None);
    }
    assert_eq!(NO_LEVEL.name, "none");
}

#[test]
fn folding_and_search() {
    assert_eq!(fold("  Łódź,  ul. Czernika "), "lodz ul czernika");
    assert_eq!(fold("ŻÓŁĆ-gęślą/JAŹŃ"), "zolc gesla jazn");
    assert_eq!(fold("?!"), "");

    let (stations, pages) =
        gios::parse_stations(&serde_json::from_str(&stations_body(STATION_ROWS, 1)).unwrap())
            .unwrap();
    assert_eq!((stations.len(), pages), (5, 1));
    let ids = |query: &str| -> Vec<i64> {
        search(&stations, &fold(query))
            .iter()
            .map(|s| s.id)
            .collect()
    };
    assert_eq!(ids("Warszawa"), [530, 550]);
    assert_eq!(ids("warszawa wokalna"), [550]);
    assert_eq!(ids("krakow"), [400]);
    assert_eq!(ids("KRAKÓW"), [400]);
    assert_eq!(ids("lodz"), [900]);
    assert_eq!(ids("mazowieckie"), [530, 550]);
    // A city that is the query comes before a name that only contains it.
    assert_eq!(ids("czerni"), [11, 900]);
    assert_eq!(ids("gdansk"), Vec::<i64>::new());
    assert_eq!(ids(""), Vec::<i64>::new());
    assert_eq!(
        stations[2].label(),
        "Warszawa, Al. Niepodległości (mazowieckie)"
    );
    assert_eq!(stations[0].label(), "Czerniawa (dolnośląskie)");
}

#[test]
fn the_station_list_is_read_without_coordinates_and_tolerates_bad_entries() {
    let rows = r#"{"Identyfikator stacji":1,"Nazwa stacji":"  A \n station ","Nazwa miasta":"A","Województwo":"X"},
        {"Identyfikator stacji":"2","Nazwa stacji":"text id"},
        {"Identyfikator stacji":3,"Nazwa stacji":""},
        {"Identyfikator stacji":-4,"Nazwa stacji":"negative"},
        "not an object",
        {"Identyfikator stacji":5,"Nazwa stacji":"Only a name"}"#;
    let body: Value = serde_json::from_str(&stations_body(rows, 3)).unwrap();
    let (stations, pages) = gios::parse_stations(&body).unwrap();
    assert_eq!(pages, 3);
    assert_eq!(
        stations,
        [
            Station {
                id: 1,
                name: "A station".into(),
                city: "A".into(),
                region: "x".into()
            },
            Station {
                id: 5,
                name: "Only a name".into(),
                city: String::new(),
                region: String::new()
            },
        ]
    );
    assert_eq!(stations[1].label(), "Only a name");
    // The stored form has no member for a coordinate or a street.
    let kept = serde_json::to_string(&stations).unwrap();
    assert!(!kept.contains("WGS") && !kept.contains("Ulica"));

    for wrong in [json!({}), json!({ "Lista stacji pomiarowych": "x" })] {
        assert!(gios::parse_stations(&wrong).is_err());
    }
    // A missing page count is one page.
    let one = json!({ "Lista stacji pomiarowych": [] });
    assert_eq!(gios::parse_stations(&one).unwrap(), (Vec::new(), 1));
}

#[test]
fn sensors_values_and_the_index_are_read_with_everything_that_can_be_missing() {
    let parse = |body: &str| serde_json::from_str::<Value>(body).unwrap();
    assert_eq!(
        gios::parse_sensors(&parse(&sensors_body(SENSORS_530))).unwrap(),
        Sensors {
            pm25: Some(3585),
            pm10: Some(3584)
        }
    );
    assert!(
        gios::parse_sensors(&parse(&sensors_body(SENSORS_OZONE)))
            .unwrap()
            .is_empty()
    );
    assert!(
        gios::parse_sensors(&parse(&sensors_body("")))
            .unwrap()
            .is_empty()
    );
    // The first sensor of a pollutant is the one used.
    let two = r#"{"Identyfikator stanowiska":1,"Wskaźnik - kod":"PM10"},
        {"Identyfikator stanowiska":2,"Wskaźnik - kod":"PM10"},
        {"Identyfikator stanowiska":"x","Wskaźnik - kod":"PM2.5"}"#;
    assert_eq!(
        gios::parse_sensors(&parse(&sensors_body(two))).unwrap(),
        Sensors {
            pm25: None,
            pm10: Some(1)
        }
    );
    assert!(gios::parse_sensors(&json!({})).is_err());

    // The newest hour with a value, whatever the order.
    assert_eq!(
        gios::parse_newest(&parse(&pm25_body())).unwrap(),
        Some(Sample {
            value: 15.4,
            time: HOUR.into()
        })
    );
    let unordered = data_body(&[
        ("2026-11-16 17:00:00", Some(1.0)),
        (HOUR, Some(3.0)),
        ("2026-11-16 18:00:00", Some(2.0)),
    ]);
    assert_eq!(
        gios::parse_newest(&parse(&unordered))
            .unwrap()
            .unwrap()
            .value,
        3.0
    );
    // No value, an impossible value, an unreadable time: not a sample.
    let unusable = data_body(&[
        (HOUR, None),
        ("2026-11-16 18:00:00", Some(-0.4)),
        ("2026-11-16 17:00:00", Some(1.0e9)),
        ("yesterday", Some(5.0)),
    ]);
    assert_eq!(gios::parse_newest(&parse(&unusable)).unwrap(), None);
    // An unknown sensor: the list is null.
    let null = format!(r#"{{{CONTEXT}"Lista danych pomiarowych":null,"totalPages":0}}"#);
    assert_eq!(gios::parse_newest(&parse(&null)).unwrap(), None);
    assert!(gios::parse_newest(&json!({ "Lista danych pomiarowych": 3 })).is_err());

    let reading = gios::parse_index(&parse(&good_index())).unwrap();
    assert_eq!(
        (reading.index, reading.index_time.as_deref()),
        (Some(1), Some(HOUR))
    );
    assert_eq!(
        (reading.no2, reading.o3, reading.so2),
        (Some(1), Some(2), None)
    );
    assert_eq!(reading.times(), [HOUR]);
    let none = gios::parse_index(&parse(&no_index())).unwrap();
    assert!(none.is_empty() && none.times().is_empty());
    // No overall index, an index off the scale, an index without a time.
    let nulls = || [json!(null), json!(1), json!(null), json!(null), json!(9)];
    for body in [
        index_body(json!(1), json!(HOUR), json!(false), nulls()),
        index_body(json!(-1), json!(HOUR), json!(true), nulls()),
        index_body(json!(6), json!(HOUR), json!(true), nulls()),
        index_body(json!("1"), json!(HOUR), json!(true), nulls()),
        index_body(json!(1), json!(null), json!(true), nulls()),
        index_body(json!(1), json!("soon"), json!(true), nulls()),
    ] {
        let reading = gios::parse_index(&parse(&body)).unwrap();
        assert_eq!(
            (reading.index, &reading.index_time),
            (None, &None),
            "{body}"
        );
        assert_eq!((reading.no2, reading.o3), (Some(1), None));
    }
    for wrong in [
        json!({}),
        json!({ "AqIndex": null }),
        json!({ "AqIndex": [] }),
    ] {
        assert!(gios::parse_index(&wrong).is_err());
    }
}

#[test]
fn ages_come_only_from_two_times_of_one_kind() {
    use cache::age;
    assert_eq!(age(Some(100), None, Some(160), None), Some(60));
    assert_eq!(age(None, Some(10), None, Some(12)), Some(120));
    assert_eq!(age(Some(100), None, None, Some(12)), None);
    assert_eq!(age(None, Some(10), Some(160), None), None);
    // From the future: unknown, and the other pair is tried.
    assert_eq!(age(Some(200), Some(10), Some(160), Some(11)), Some(60));
    assert_eq!(age(Some(200), None, Some(160), None), None);
    assert_eq!(age(None, None, None, None), None);
}

// ---- find-station ----------------------------------------------------------

#[test]
fn find_station_lists_matches_and_keeps_the_list_without_coordinates() {
    let agent = agent(json!({}));
    queue(&agent, http(200, &stations_body(STATION_ROWS, 1)));
    let (message, data) = ok(run(&agent, "find-station", json!({ "query": "Warszawa" })));
    assert_outputs("find-station", &data);
    assert_eq!(
        message,
        "Found 2: 530: Warszawa, Al. Niepodległości (mazowieckie); 550: Warszawa, ul. Wokalna (mazowieckie). \
Run Select station with the number in front of the station you want. Źródło danych: GIOŚ - EKOINFONET."
    );
    assert_eq!((int(&data, "count"), int(&data, "shown")), (2, 2));
    assert_eq!(int(&data, "firstId"), 530);
    assert_eq!(
        text(&data, "matches"),
        "530: Warszawa, Al. Niepodległości (mazowieckie)\n550: Warszawa, ul. Wokalna (mazowieckie)"
    );
    assert_eq!(text(&data, "source"), "network");
    assert_eq!(
        agent.borrow().urls,
        ["https://api.gios.gov.pl/pjp-api/v1/rest/station/findAll?page=0&size=500"]
    );
    {
        let a = agent.borrow();
        assert_eq!(a.store[LIST_KEY]["count"], 5);
        assert_eq!(a.store[LIST_KEY]["chunks"], 1);
        assert_eq!(a.store[LIST_KEY]["fetched_at"], DATE_UNIX);
        assert_eq!(
            a.store[&format!("{LIST_PREFIX}0")][2],
            json!([
                530,
                "Warszawa, Al. Niepodległości",
                "Warszawa",
                "mazowieckie"
            ])
        );
        let kept = serde_json::to_string(&a.store).unwrap();
        for part in NEVER_KEPT {
            assert!(!kept.contains(part), "{part} was stored");
        }
        // Searching chooses nothing.
        assert!(!a.store.contains_key(STATION_KEY));
    }

    // The next searches use the stored list: the service allows two
    // requests a minute.
    let (message, data) = ok(run(&agent, "find-station", json!({ "query": "  kraków " })));
    assert!(message.starts_with("Found 1: 400: Kraków, Aleja Krasińskiego (małopolskie)."));
    assert_eq!(text(&data, "source"), "cache");
    agent.borrow_mut().settings = json!({ "language": "pl" });
    let (message, _) = ok(run(&agent, "find-station", json!({ "query": "lodz" })));
    assert_eq!(
        message,
        "Znaleziono 1: 900: Łódź, ul. Czernika (łódzkie). Uruchom Wybierz stację z numerem sprzed nazwy wybranej stacji. \
Źródło danych: GIOŚ - EKOINFONET."
    );
    assert_eq!(requests(&agent), 1);

    let (code, message, _) = failed(run(&agent, "find-station", json!({ "query": "Gdańsk" })));
    assert_eq!(code, STATION_NOT_FOUND);
    assert!(!message.contains("Gdańsk"));
    for query in ["a", " ", "?", ",,,"] {
        let (code, _, _) = failed(run(&agent, "find-station", json!({ "query": query })));
        assert_eq!(code, codes::INVALID_PARAMETERS, "{query:?}");
    }
    let (code, _, _) = failed(run(&agent, "find-station", json!({})));
    assert_eq!(code, codes::INVALID_PARAMETERS);
    assert_eq!(requests(&agent), 1);
    assert!(agent.borrow().logs.is_empty());
}

/// `count` stations of one town with names of `width` characters.
fn many_rows(first: i64, count: i64, width: usize) -> String {
    (first..first + count)
        .map(|id| {
            let name = format!("Miasto, ul. {id} {}", "ż".repeat(width));
            json!({
                "Identyfikator stacji": id, "Nazwa stacji": name, "Nazwa miasta": "Miasto",
                "WGS84 φ N": "52.219298", "WGS84 λ E": "21.004724",
                "Województwo": "ZACHODNIOPOMORSKIE", "Ulica": "Al. Niepodległości 227/233",
            })
            .to_string()
        })
        .collect::<Vec<_>>()
        .join(",")
}

#[test]
fn a_long_station_list_is_read_in_two_pages_and_stored_in_chunks() {
    let agent = agent(json!({}));
    queue(&agent, http(200, &stations_body(&many_rows(1, 500, 5), 3)));
    queue(
        &agent,
        http(200, &stations_body(&many_rows(501, 130, 5), 3)),
    );
    let (message, data) = ok(run(&agent, "find-station", json!({ "query": "miasto" })));
    assert_outputs("find-station", &data);
    assert_eq!((int(&data, "count"), int(&data, "shown")), (630, 8));
    assert!(
        message
            .starts_with("Found 630, the first 8: 1: Miasto, ul. 1 żżżżż (zachodniopomorskie); ")
    );
    assert!(message.contains("Add a word to narrow the search"));
    assert_eq!(text(&data, "matches").lines().count(), MAX_MATCHES);
    {
        let a = agent.borrow();
        assert_eq!(
            a.urls,
            [
                "https://api.gios.gov.pl/pjp-api/v1/rest/station/findAll?page=0&size=500",
                "https://api.gios.gov.pl/pjp-api/v1/rest/station/findAll?page=1&size=500",
            ]
        );
        assert_eq!(a.store[LIST_KEY]["count"], 630);
        assert_eq!(a.store[LIST_KEY]["chunks"], 630_usize.div_ceil(CHUNK));
        let chunks = a
            .store
            .keys()
            .filter(|k| k.starts_with(LIST_PREFIX))
            .count();
        assert_eq!(chunks, 630_usize.div_ceil(CHUNK));
        // A third page exists and is not read: said in the log, without a
        // station.
        assert_eq!(
            a.logs,
            ["the provider's station list has more pages than the plugin reads"]
        );
    }
    let (_, data) = ok(run(
        &agent,
        "find-station",
        json!({ "query": "miasto 630" }),
    ));
    assert_eq!((int(&data, "count"), int(&data, "firstId")), (1, 630));
    assert_eq!(requests(&agent), 2);

    // A shorter list later: the surplus chunks go.
    agent.borrow_mut().store.remove(LIST_KEY);
    queue(&agent, http(200, &stations_body(STATION_ROWS, 1)));
    ok(run(&agent, "find-station", json!({ "query": "warszawa" })));
    let a = agent.borrow();
    let chunks: Vec<&String> = a
        .store
        .keys()
        .filter(|k| k.starts_with(LIST_PREFIX))
        .collect();
    assert_eq!(chunks, [&format!("{LIST_PREFIX}0")]);
}

#[test]
fn a_chunk_of_the_longest_names_fits_a_storage_value() {
    // 80 characters of four bytes in each text: the mock agent refuses a
    // value over 64 KiB, as the real one does.
    let rows: Vec<String> = (0..CHUNK as i64 * 2)
        .map(|id| {
            let long = "𝔸".repeat(200);
            json!({
                "Identyfikator stacji": 90_000_000 + id, "Nazwa stacji": long,
                "Nazwa miasta": long, "Województwo": long,
            })
            .to_string()
        })
        .collect();
    let agent = agent(json!({}));
    queue(&agent, http(200, &stations_body(&rows.join(","), 1)));
    let (message, data) = ok(run(&agent, "find-station", json!({ "query": "𝔸𝔸" })));
    assert_eq!(int(&data, "count"), CHUNK as i64 * 2);
    assert!(message.chars().count() <= 1_024);
    assert_eq!(agent.borrow().store[LIST_KEY]["chunks"], 2);
}

#[test]
fn a_storage_that_refuses_the_list_does_not_hide_the_answer() {
    let agent = agent(json!({}));
    agent.borrow_mut().full_storage = true;
    queue(&agent, http(200, &stations_body(STATION_ROWS, 1)));
    let (_, data) = ok(run(&agent, "find-station", json!({ "query": "krakow" })));
    assert_eq!(int(&data, "firstId"), 400);
    let a = agent.borrow();
    assert!(a.store.is_empty(), "half a list was kept");
    assert!(a.logs.iter().all(|l| l.contains("storage refused")));
}

#[test]
fn the_station_list_is_renewed_after_a_week_and_survives_a_failed_renewal() {
    let agent = agent(json!({}));
    queue(&agent, http(200, &stations_body(STATION_ROWS, 1)));
    ok(run(&agent, "find-station", json!({ "query": "warszawa" })));
    // Six days later by the last answer the plugin saw: still the list.
    agent.borrow_mut().store.insert(
        STATE_KEY.into(),
        json!({ "last_seen": DATE_UNIX + 6 * 86_400 }),
    );
    ok(run(&agent, "find-station", json!({ "query": "warszawa" })));
    assert_eq!(requests(&agent), 1);
    // Eight days: asked again; the service is down, the old list answers.
    agent.borrow_mut().store.insert(
        STATE_KEY.into(),
        json!({ "last_seen": DATE_UNIX + 8 * 86_400 }),
    );
    queue(&agent, http(503, ""));
    let (_, data) = ok(run(&agent, "find-station", json!({ "query": "warszawa" })));
    assert_eq!(
        (text(&data, "source"), int(&data, "count")),
        ("cache".into(), 2)
    );
    assert_eq!(requests(&agent), 2);
    // And renewed when it answers.
    let one = r#"{"Identyfikator stacji":530,"Nazwa stacji":"Warszawa, Al. Niepodległości","Nazwa miasta":"Warszawa","Województwo":"MAZOWIECKIE"}"#;
    queue(
        &agent,
        http_at(200, &stations_body(one, 1), "Wed, 25 Nov 2026 18:42:09 GMT"),
    );
    let (_, data) = ok(run(&agent, "find-station", json!({ "query": "warszawa" })));
    assert_eq!(
        (text(&data, "source"), int(&data, "count")),
        ("network".into(), 1)
    );
    assert_eq!(agent.borrow().store[LIST_KEY]["count"], 1);
}

#[test]
fn the_station_list_fails_with_the_provider() {
    for (reply, code, retryable) in [
        (http(429, ""), codes::THROTTLED, true),
        (http(500, "oops"), PROVIDER_ERROR, true),
        (http(404, "{}"), PROVIDER_ERROR, false),
        (http(200, "<html>"), INVALID_RESPONSE, false),
        (http(200, "[]"), INVALID_RESPONSE, false),
        (http(200, r#"{"Lista stacji":[]}"#), INVALID_RESPONSE, false),
        (http(200, &stations_body("", 1)), INVALID_RESPONSE, false),
        (
            http(
                200,
                r#"{"error_code":"API-ERR-100003","error_reason":"limit"}"#,
            ),
            PROVIDER_ERROR,
            false,
        ),
        (
            http(
                200,
                &format!("{{\"pad\":\"{}\"}}", "x".repeat(gios::MAX_LIST_BYTES)),
            ),
            RESPONSE_TOO_LARGE,
            false,
        ),
        (
            Reply::Error(
                CallErrorCode::Failed,
                "the response is larger than 524288 bytes",
                None,
            ),
            RESPONSE_TOO_LARGE,
            false,
        ),
        (offline(), "connection-failed", true),
        (
            Reply::Error(
                CallErrorCode::Failed,
                "slow",
                Some(NetworkErrorKind::TimedOut),
            ),
            TIMED_OUT,
            true,
        ),
    ] {
        let agent = agent(json!({}));
        queue(&agent, reply);
        let (got, message, retry) =
            failed(run(&agent, "find-station", json!({ "query": "warszawa" })));
        assert_eq!((got.as_str(), retry), (code, retryable), "{message}");
        assert!(agent.borrow().store.keys().all(|k| k == STATE_KEY));
    }
    assert!(codes::is_shared("connection-failed") && codes::is_shared(codes::THROTTLED));
}

// ---- select-station and forget-station --------------------------------------

#[test]
fn select_station_stores_the_station_and_its_pm_sensors() {
    let agent = agent(json!({}));
    queue(&agent, http(200, &stations_body(STATION_ROWS, 1)));
    queue(&agent, http(200, &sensors_body(SENSORS_530)));
    agent
        .borrow_mut()
        .variables
        .insert("pm25".into(), json!(99));
    let (message, data) = ok(run(&agent, "select-station", json!({ "station": 530 })));
    assert_outputs("select-station", &data);
    assert_eq!(
        message,
        "Station set to Warszawa, Al. Niepodległości (mazowieckie). It measures PM2.5 and PM10. \
Źródło danych: GIOŚ - EKOINFONET."
    );
    assert_eq!(
        text(&data, "selected"),
        "Warszawa, Al. Niepodległości (mazowieckie)"
    );
    assert_eq!(int(&data, "stationId"), 530);
    assert!(flag(&data, "hasPm25") && flag(&data, "hasPm10"));
    {
        let a = agent.borrow();
        assert_eq!(
            a.urls,
            [
                "https://api.gios.gov.pl/pjp-api/v1/rest/station/findAll?page=0&size=500",
                "https://api.gios.gov.pl/pjp-api/v1/rest/station/sensors/530?size=100",
            ]
        );
        assert_eq!(
            a.store[STATION_KEY],
            json!({
                "v": 1, "provider": "gios",
                "station": { "id": 530, "name": "Warszawa, Al. Niepodległości", "city": "Warszawa", "region": "mazowieckie" },
                "sensors": { "pm25": 3585, "pm10": 3584 },
            })
        );
        // The variables of the station chosen before are gone.
        assert!(a.variables.is_empty() && a.cleared == 1);
        // The choice is not written to the log.
        assert!(a.logs.is_empty());
    }

    // Another station, in Polish, with the list from storage; the reading
    // of the first one goes.
    agent
        .borrow_mut()
        .store
        .insert(READING_KEY.into(), json!({ "v": 1 }));
    agent.borrow_mut().settings = json!({ "language": "pl" });
    queue(&agent, http(200, &sensors_body(SENSORS_OZONE)));
    let (message, data) = ok(run(&agent, "select-station", json!({ "station": 11 })));
    assert_eq!(
        message,
        "Wybrano stację: Czerniawa (dolnośląskie). Nie mierzy ani PM2.5, ani PM10: Aktualna jakość powietrza poda tylko jej indeks. \
Źródło danych: GIOŚ - EKOINFONET."
    );
    assert!(!flag(&data, "hasPm25") && !flag(&data, "hasPm10"));
    assert_eq!(requests(&agent), 3);
    let a = agent.borrow();
    assert_eq!(a.store[STATION_KEY]["sensors"], json!({}));
    assert!(!a.store.contains_key(READING_KEY));
}

#[test]
fn select_station_refuses_what_it_cannot_store() {
    let agent = agent(json!({}));
    queue(&agent, http(200, &stations_body(STATION_ROWS, 1)));
    let (code, message, _) = failed(run(&agent, "select-station", json!({ "station": 531 })));
    assert_eq!(code, STATION_NOT_FOUND);
    assert!(!message.contains("531"));
    for params in [
        json!({}),
        json!({ "station": 0 }),
        json!({ "station": "530" }),
        json!({ "station": 100_000_000 }),
    ] {
        let (code, _, _) = failed(run(&agent, "select-station", params));
        assert_eq!(code, codes::INVALID_PARAMETERS);
    }
    // The sensors cannot be read: nothing is chosen.
    for (reply, code) in [
        (http(500, ""), PROVIDER_ERROR),
        (http(429, ""), codes::THROTTLED),
        (http(200, "{}"), INVALID_RESPONSE),
        (offline(), "connection-failed"),
    ] {
        queue(&agent, reply);
        let (got, _, _) = failed(run(&agent, "select-station", json!({ "station": 530 })));
        assert_eq!(got, code);
    }
    assert!(!agent.borrow().store.contains_key(STATION_KEY));
    assert!(agent.borrow().logs.is_empty());
}

#[test]
fn forget_station_deletes_the_choice_and_keeps_the_public_list() {
    let agent = chosen_530(json!({}));
    queue_reading(&agent);
    current(&agent, json!({}));
    assert!(agent.borrow().store.contains_key(READING_KEY));
    let (message, data) = ok(run(&agent, "forget-station", json!({})));
    assert!(message.starts_with("The chosen station and its saved values were deleted."));
    assert!(data.is_empty());
    {
        let a = agent.borrow();
        let keys: Vec<&String> = a.store.keys().collect();
        assert_eq!(keys, [LIST_KEY, &format!("{LIST_PREFIX}0")]);
        assert!(a.variables.is_empty());
    }
    let (code, _, _) = failed(run(&agent, "current", json!({})));
    assert_eq!(code, NO_STATION);
    agent.borrow_mut().settings = json!({ "language": "pl" });
    let (message, _) = ok(run(&agent, "forget-station", json!({})));
    assert!(message.starts_with("Usunięto wybraną stację"));
}

// ---- current ----------------------------------------------------------------

#[test]
fn current_without_a_station_asks_nobody() {
    let agent = agent(json!({}));
    let (code, message, retryable) = failed(run(&agent, "current", json!({})));
    assert_eq!((code.as_str(), retryable), (NO_STATION, false));
    assert!(message.contains("Find station") && message.contains("Select station"));
    assert_eq!(requests(&agent), 0);
    // A stored choice of another format or provider is no choice.
    for stored in [
        json!({ "v": 2, "provider": "gios", "station": { "id": 1, "name": "A", "city": "", "region": "" }, "sensors": {} }),
        json!({ "v": 1, "provider": "airly", "station": { "id": 1, "name": "A", "city": "", "region": "" }, "sensors": {} }),
        json!("damaged"),
    ] {
        agent.borrow_mut().store.insert(STATION_KEY.into(), stored);
        let (code, _, _) = failed(run(&agent, "current", json!({})));
        assert_eq!(code, NO_STATION);
    }
    assert_eq!(requests(&agent), 0);
}

#[test]
fn current_reports_the_index_the_pm_values_and_their_time() {
    let agent = chosen_530(json!({}));
    queue_reading(&agent);
    let (message, data) = current(&agent, json!({}));
    assert_eq!(
        message,
        "Warszawa, Al. Niepodległości (mazowieckie): air quality Good (index 1; Polish air quality index, 0 best to 5 worst). \
PM2.5 15 µg/m³, PM10 23 µg/m³. Index by pollutant: NO2: Good, O3: Moderate. Measured 2026-11-16 19:00 (Polish time). \
Źródło danych: GIOŚ - EKOINFONET (data source: Poland's Chief Inspectorate of Environmental Protection, CC BY 4.0; \
an excerpt, values rounded, category names translated)."
    );
    assert_eq!(
        agent.borrow().urls,
        [
            "https://api.gios.gov.pl/pjp-api/v1/rest/aqindex/getIndex/530",
            "https://api.gios.gov.pl/pjp-api/v1/rest/data/getData/3585?size=24",
            "https://api.gios.gov.pl/pjp-api/v1/rest/data/getData/3584?size=24",
        ]
    );
    assert_eq!(
        text(&data, "summary"),
        "Warszawa, Al. Niepodległości (mazowieckie): air quality Good (index 1; Polish air quality index, 0 best to 5 worst). \
PM2.5 15 µg/m³, PM10 23 µg/m³. Index by pollutant: NO2: Good, O3: Moderate. Measured 2026-11-16 19:00 (Polish time). \
Źródło danych: GIOŚ - EKOINFONET."
    );
    assert_eq!(
        (int(&data, "index"), int(&data, "pm25"), int(&data, "pm10")),
        (1, 15, 23)
    );
    assert_eq!(
        (text(&data, "category"), text(&data, "categoryName")),
        ("Good".into(), "good".into())
    );
    assert_eq!(text(&data, "unit"), "µg/m³");
    assert_eq!(
        text(&data, "station"),
        "Warszawa, Al. Niepodległości (mazowieckie)"
    );
    assert_eq!(text(&data, "provider"), "gios");
    for key in ["measuredAt", "indexTime", "pm25Time", "pm10Time"] {
        assert_eq!(text(&data, key), "2026-11-16 19:00", "{key}");
    }
    assert_eq!(
        (text(&data, "no2Category"), text(&data, "o3Category")),
        ("good".into(), "moderate".into())
    );
    assert!(!data.contains_key("so2Category"));
    assert!(!flag(&data, "stale"));
    assert_eq!(int(&data, "measurementAgeMinutes"), 42);
    assert_eq!(text(&data, "source"), "network");
    assert_eq!(text(&data, "fetchedAt"), "2026-11-16T18:42:09Z");
    assert_eq!(int(&data, "ageMinutes"), 0);
    assert!(!data.contains_key("problem"));

    assert_eq!(variable(&agent, "pm25"), Some(json!(15)));
    assert_eq!(variable(&agent, "pm10"), Some(json!(23)));
    assert_eq!(variable(&agent, "index"), Some(json!(1)));
    assert_eq!(variable(&agent, "category"), Some(json!("Good")));
    assert_eq!(variable(&agent, "categoryName"), Some(json!("good")));
    assert_eq!(
        variable(&agent, "measuredAt"),
        Some(json!("2026-11-16 19:00"))
    );
    assert_eq!(variable(&agent, "stale"), Some(json!(false)));
    assert_eq!(
        variable(&agent, "station"),
        Some(json!("Warszawa, Al. Niepodległości (mazowieckie)"))
    );
    assert_eq!(
        variable(&agent, "summary"),
        Some(json!(text(&data, "summary")))
    );
    assert_eq!(agent.borrow().variables.len(), VARIABLES.len());

    let a = agent.borrow();
    assert_eq!(a.store[READING_KEY]["fetched_at"], DATE_UNIX);
    assert_eq!(a.store[READING_KEY]["station"], 530);
    assert_eq!(a.store[STATE_KEY]["last_seen"], DATE_UNIX);
    // Neither the station nor a value is written to the log.
    assert!(a.logs.is_empty(), "{:?}", a.logs);
}

#[test]
fn current_in_polish() {
    let agent = chosen_530(json!({ "language": "pl", "provider": "gios" }));
    queue_reading(&agent);
    let (message, data) = current(&agent, json!({}));
    assert_eq!(
        message,
        "Warszawa, Al. Niepodległości (mazowieckie): jakość powietrza: Dobry (indeks 1; Polski indeks jakości powietrza, od 0 (najlepszy) do 5). \
PM2.5 15 µg/m³, PM10 23 µg/m³. Indeks zanieczyszczeń: NO2: Dobry, O3: Umiarkowany. Pomiar: 2026-11-16 19:00 (czas polski). \
Źródło danych: GIOŚ - EKOINFONET (CC BY 4.0; fragment danych, wartości zaokrąglone)."
    );
    assert_eq!(
        (text(&data, "category"), text(&data, "categoryName")),
        ("Dobry".into(), "good".into())
    );
    assert_eq!(variable(&agent, "category"), Some(json!("Dobry")));
}

#[test]
fn a_scheduled_run_uses_the_cache_for_half_an_hour() {
    let agent = chosen_530(json!({}));
    queue_reading(&agent);
    let (_, data) = current(&agent, json!({ "at": "2026-11-16 19:42" }));
    assert_eq!(text(&data, "source"), "network");
    assert_eq!(requests(&agent), 3);

    // 29 minutes later: the stored reading, no request, its age counted on.
    let (message, data) = current(&agent, json!({ "at": "2026-11-16 20:11" }));
    assert_eq!(requests(&agent), 3);
    assert_eq!(
        (text(&data, "source"), int(&data, "ageMinutes")),
        ("cache".into(), 29)
    );
    assert_eq!(int(&data, "measurementAgeMinutes"), 42 + 29);
    assert!(!flag(&data, "stale"));
    assert!(message.contains(
        "Measured 2026-11-16 19:00 (Polish time). (from the cache, fetched 29 min ago) Źródło"
    ));
    assert_eq!(variable(&agent, "stale"), Some(json!(false)));

    // 30 minutes: asked again.
    queue_reading(&agent);
    let (_, data) = current(&agent, json!({ "at": "2026-11-16 20:12" }));
    assert_eq!(
        (text(&data, "source"), requests(&agent)),
        ("network".into(), 6)
    );

    // A key press has no time: it cannot know the age, and asks.
    queue_reading(&agent);
    let (_, data) = current(&agent, json!({}));
    assert_eq!(
        (text(&data, "source"), requests(&agent)),
        ("network".into(), 9)
    );
    // A run "before" the stored one (the clock was set back): asks.
    queue_reading(&agent);
    current(&agent, json!({ "at": "2026-11-16 19:00" }));
    assert_eq!(requests(&agent), 12);

    for at in [
        "yesterday",
        "2026-11-16T19:42",
        "2026-02-30 10:00",
        "2026-11-16 24:00",
    ] {
        let (code, _, _) = failed(run(&agent, "current", json!({ "at": at })));
        assert_eq!(code, codes::INVALID_PARAMETERS, "{at}");
    }
    assert_eq!(requests(&agent), 12);
}

#[test]
fn old_measurements_are_never_presented_as_current() {
    // The provider answers now, but the station's newest values are five
    // hours old.
    let agent = chosen_530(json!({}));
    queue(
        &agent,
        http(
            200,
            &index_body(
                json!(3),
                json!("2026-11-16 15:00:00"),
                json!(true),
                [json!(null), json!(null), json!(3), json!(2), json!(null)],
            ),
        ),
    );
    queue(
        &agent,
        http(
            200,
            &data_body(&[(HOUR, None), ("2026-11-16 14:00:00", Some(40.2))]),
        ),
    );
    queue(
        &agent,
        http(200, &data_body(&[("2026-11-16 15:00:00", Some(61.5))])),
    );
    let (message, data) = current(&agent, json!({}));
    assert!(
        message.starts_with(
            "Not current: the oldest of these values is about 5 h 42 min old. Warszawa, Al. Niepodległości (mazowieckie): air quality Sufficient (index 3;"
        ),
        "{message}"
    );
    assert!(message.contains("Measured 2026-11-16 14:00 to 2026-11-16 15:00 (Polish time)."));
    assert!(flag(&data, "stale"));
    assert_eq!(text(&data, "source"), "network");
    assert_eq!(int(&data, "measurementAgeMinutes"), 5 * 60 + 42);
    assert_eq!(text(&data, "measuredAt"), "2026-11-16 14:00");
    assert_eq!(
        (text(&data, "pm25Time"), text(&data, "pm10Time")),
        ("2026-11-16 14:00".into(), "2026-11-16 15:00".into())
    );
    assert!(text(&data, "summary").starts_with("[stale] Warszawa"));
    assert_eq!(variable(&agent, "stale"), Some(json!(true)));

    // Exactly three hours is still current; a second more is not.
    for (date, stale) in [
        ("Mon, 16 Nov 2026 21:00:00 GMT", false),
        ("Mon, 16 Nov 2026 21:00:01 GMT", true),
    ] {
        let agent = chosen_530(json!({}));
        queue(&agent, http_at(200, &good_index(), date));
        queue(&agent, http_at(200, &pm25_body(), date));
        queue(&agent, http_at(200, &pm10_body(), date));
        let (_, data) = current(&agent, json!({}));
        assert_eq!(flag(&data, "stale"), stale, "{date}");
    }

    // A reading that was current when it was stored grows old in the
    // cache: fetched 2 h 50 min after the measurement, served again 15
    // minutes later. The cache is young; the measurement is not.
    let agent = chosen_530(json!({}));
    let date = "Mon, 16 Nov 2026 20:50:00 GMT";
    queue(&agent, http_at(200, &good_index(), date));
    queue(&agent, http_at(200, &pm25_body(), date));
    queue(&agent, http_at(200, &pm10_body(), date));
    let (_, data) = current(&agent, json!({ "at": "2026-11-16 21:50" }));
    assert!(!flag(&data, "stale"));
    let (message, data) = current(&agent, json!({ "at": "2026-11-16 22:05" }));
    assert_eq!(
        (text(&data, "source"), int(&data, "ageMinutes")),
        ("cache".into(), 15)
    );
    assert!(flag(&data, "stale"));
    assert_eq!(int(&data, "measurementAgeMinutes"), 185);
    assert!(
        message.starts_with("Not current: the oldest of these values is about 3 h 5 min old. ")
    );
    assert!(!message.contains("from the cache"));
}

#[test]
fn without_a_time_source_nothing_is_called_current() {
    // No Date header: the values may be right, but the plugin cannot show
    // that they are recent.
    let agent = chosen_530(json!({ "language": "pl" }));
    queue(&agent, undated(&good_index()));
    queue(&agent, undated(&pm25_body()));
    queue(&agent, undated(&pm10_body()));
    let (message, data) = current(&agent, json!({}));
    assert!(message.starts_with(
        "Być może nieaktualne: wtyczka nie ma zegara i nie mogła sprawdzić wieku tych wartości. Warszawa"
    ));
    assert!(flag(&data, "stale"));
    assert!(!data.contains_key("measurementAgeMinutes") && !data.contains_key("fetchedAt"));
    assert!(text(&data, "summary").starts_with("[nieaktualne] "));

    // A measurement from the future is not believed either.
    let agent = chosen_530(json!({}));
    queue(&agent, http(200, &no_index()));
    queue(
        &agent,
        http(200, &data_body(&[("2026-11-17 19:00:00", Some(5.0))])),
    );
    queue(&agent, http(200, &data_body(&[])));
    let (message, data) = current(&agent, json!({}));
    assert!(
        message.starts_with(
            "Possibly not current: the plugin has no clock and could not check the age"
        )
    );
    assert!(flag(&data, "stale") && !data.contains_key("measurementAgeMinutes"));
    // Within the hour after "now" it is the hour that just ended.
    let agent = chosen_530(json!({}));
    queue(&agent, http(200, &no_index()));
    queue(
        &agent,
        http(200, &data_body(&[("2026-11-16 20:00:00", Some(5.0))])),
    );
    queue(&agent, http(200, &data_body(&[])));
    let (_, data) = current(&agent, json!({}));
    assert_eq!(
        (flag(&data, "stale"), int(&data, "measurementAgeMinutes")),
        (false, 0)
    );
}

#[test]
fn missing_values_are_said_and_their_variables_cleared() {
    // No PM2.5 value in the hours asked for, and no index.
    let agent = chosen_530(json!({}));
    queue_reading(&agent);
    current(&agent, json!({}));
    queue(&agent, http(200, &no_index()));
    queue(&agent, http(200, &data_body(&[(HOUR, None)])));
    queue(&agent, http(200, &pm10_body()));
    let (message, data) = current(&agent, json!({}));
    assert!(
        message.starts_with(
            "Warszawa, Al. Niepodległości (mazowieckie): no air quality index now. PM2.5: no recent value, PM10 23 µg/m³. Measured 2026-11-16 19:00 (Polish time)."
        ),
        "{message}"
    );
    for key in ["pm25", "pm25Time", "index", "indexTime", "no2Category"] {
        assert!(!data.contains_key(key), "{key}");
    }
    assert_eq!(
        (text(&data, "category"), text(&data, "categoryName")),
        ("No index".into(), "none".into())
    );
    assert_eq!(int(&data, "pm10"), 23);
    assert!(!flag(&data, "stale"));
    // The values of the run before do not linger.
    assert_eq!(
        (variable(&agent, "pm25"), variable(&agent, "index")),
        (None, None)
    );
    assert_eq!(variable(&agent, "pm10"), Some(json!(23)));
    assert_eq!(variable(&agent, "categoryName"), Some(json!("none")));

    // A station without PM sensors: one request, the index only.
    let agent = agent_with_station(json!({ "language": "pl" }), SENSORS_OZONE);
    queue(&agent, http(200, &good_index()));
    let (message, data) = current(&agent, json!({}));
    assert!(
        message.contains("PM2.5: nie jest tu mierzony, PM10: nie jest tu mierzony."),
        "{message}"
    );
    assert_eq!((int(&data, "index"), requests(&agent)), (1, 1));
    assert!(!data.contains_key("pm25") && !data.contains_key("pm10"));

    // Nothing at all: an error, not an empty success.
    let agent = chosen_530(json!({}));
    queue(&agent, http(200, &no_index()));
    queue(&agent, http(200, &data_body(&[])));
    queue(&agent, http(200, &data_body(&[(HOUR, None)])));
    let (code, _, retryable) = failed(run(&agent, "current", json!({})));
    assert_eq!((code.as_str(), retryable), (NO_DATA, false));
    assert_eq!(variable(&agent, "stale"), Some(json!(true)));
}

/// An agent whose chosen station has the given sensors.
fn agent_with_station(settings: Value, sensors: &str) -> Shared {
    let agent = agent(settings);
    queue(&agent, http(200, &stations_body(STATION_ROWS, 1)));
    queue(&agent, http(200, &sensors_body(sensors)));
    ok(run(&agent, "select-station", json!({ "station": 530 })));
    agent.borrow_mut().urls.clear();
    agent
}

#[test]
fn a_failed_request_serves_the_saved_reading_as_stale() {
    let agent = chosen_530(json!({}));
    queue_reading(&agent);
    current(&agent, json!({}));

    // Offline: no answer, so no time. Saved data, age unknown.
    queue(&agent, offline());
    let (message, data) = current(&agent, json!({}));
    assert!(
        message.starts_with(
            "Not current: GIOŚ gave no new data (connection-failed). Saved data fetched 2026-11-16T18:42:09Z; \
the plugin has no clock and cannot tell how old that is. Warszawa"
        ),
        "{message}"
    );
    assert!(flag(&data, "stale"));
    assert_eq!(
        (text(&data, "source"), text(&data, "problem")),
        ("cache".into(), "connection-failed".into())
    );
    assert!(!data.contains_key("ageMinutes") && !data.contains_key("measurementAgeMinutes"));
    assert_eq!(int(&data, "pm25"), 15);
    assert_eq!(variable(&agent, "stale"), Some(json!(true)));
    assert!(text(&data, "summary").starts_with("[stale] "));

    // The provider answers with an error an hour later: its Date tells the
    // age.
    queue(&agent, http_at(503, "", "Mon, 16 Nov 2026 19:42:09 GMT"));
    let (message, data) = current(&agent, json!({}));
    assert!(message.starts_with(
        "Not current: GIOŚ gave no new data (provider-error). Saved data fetched 2026-11-16T18:42:09Z, about 1 h ago. "
    ));
    assert_eq!(
        (
            int(&data, "ageMinutes"),
            int(&data, "measurementAgeMinutes")
        ),
        (60, 102)
    );
    assert!(flag(&data, "stale"));

    // An error ten minutes after the fetch: the saved reading is still
    // within its lifetime, and the failure is told all the same.
    queue(&agent, http_at(500, "", "Mon, 16 Nov 2026 18:52:09 GMT"));
    let (message, data) = current(&agent, json!({}));
    assert!(!flag(&data, "stale"));
    assert!(
        message.contains(
            "(from the cache, fetched 10 min ago; GIOŚ gave no new data: provider-error)"
        )
    );
    assert_eq!(text(&data, "problem"), "provider-error");

    // The second of three answers fails: nothing half-new is shown.
    queue(
        &agent,
        http(
            200,
            &index_body(
                json!(5),
                json!(HOUR),
                json!(true),
                [json!(null), json!(null), json!(5), json!(5), json!(null)],
            ),
        ),
    );
    queue(&agent, http(200, "not json"));
    let (_, data) = current(&agent, json!({}));
    assert_eq!(
        (int(&data, "index"), text(&data, "problem")),
        (1, INVALID_RESPONSE.into())
    );

    // In Polish.
    agent.borrow_mut().settings = json!({ "language": "pl" });
    queue(&agent, offline());
    let (message, _) = current(&agent, json!({}));
    assert!(message.starts_with(
        "Nieaktualne: GIOŚ nie podał nowych danych (connection-failed). Zapisane dane pobrane 2026-11-16T18:42:09Z; \
wtyczka nie ma zegara i nie wie, jak dawno to było. "
    ));
}

#[test]
fn every_failure_without_saved_data_is_an_error() {
    let big = format!("{{\"pad\":\"{}\"}}", "x".repeat(gios::MAX_BODY_BYTES));
    for (reply, code, retryable) in [
        (http(429, ""), codes::THROTTLED, true),
        (http(500, ""), PROVIDER_ERROR, true),
        (
            http(400, r#"{"status":400,"error":"Bad Request"}"#),
            PROVIDER_ERROR,
            false,
        ),
        (
            http(
                200,
                r#"{"timestamp":"x","status":400,"error":"Bad Request"}"#,
            ),
            PROVIDER_ERROR,
            false,
        ),
        (http(200, ""), INVALID_RESPONSE, false),
        (http(200, "<html>busy</html>"), INVALID_RESPONSE, false),
        (http(200, "[1]"), INVALID_RESPONSE, false),
        (http(200, "{}"), INVALID_RESPONSE, false),
        (http(200, &big), RESPONSE_TOO_LARGE, false),
        (offline(), "connection-failed", true),
        (
            Reply::Error(
                CallErrorCode::Failed,
                "slow",
                Some(NetworkErrorKind::TimedOut),
            ),
            TIMED_OUT,
            true,
        ),
    ] {
        let agent = chosen_530(json!({}));
        queue(&agent, reply);
        let (got, message, retry) = failed(run(&agent, "current", json!({})));
        assert_eq!((got.as_str(), retry), (code, retryable), "{message}");
        // The message names neither the station nor the request's path.
        assert!(
            !message.contains("530")
                && !message.contains("Warszawa")
                && !message.contains("pjp-api"),
            "{message}"
        );
        assert_eq!(variable(&agent, "stale"), Some(json!(true)));
        assert!(!agent.borrow().store.contains_key(READING_KEY));
        assert!(agent.borrow().logs.is_empty());
    }
    // A malformed value list after a good index.
    let agent = chosen_530(json!({}));
    queue(&agent, http(200, &good_index()));
    queue(&agent, http(200, r#"{"Lista danych pomiarowych":{"x":1}}"#));
    let (code, _, _) = failed(run(&agent, "current", json!({})));
    assert_eq!(code, INVALID_RESPONSE);
}

#[test]
fn http_429_is_honoured_by_scheduled_runs() {
    let agent = chosen_530(json!({}));
    queue_reading(&agent);
    current(&agent, json!({ "at": "2026-11-16 19:42" }));
    queue(
        &agent,
        Reply::Http {
            status: 429,
            headers: vec![
                ("date", "Mon, 16 Nov 2026 19:42:09 GMT".into()),
                ("retry-after", "600".into()),
            ],
            body: String::new(),
        },
    );
    let (_, data) = current(&agent, json!({ "at": "2026-11-16 20:42" }));
    assert_eq!(
        (text(&data, "problem"), int(&data, "ageMinutes")),
        ("throttled".into(), 60)
    );
    assert!(flag(&data, "stale"));
    assert_eq!(requests(&agent), 4);
    // Within the ten minutes: nothing is sent.
    let (message, data) = current(&agent, json!({ "at": "2026-11-16 20:51" }));
    assert_eq!(
        (requests(&agent), text(&data, "problem")),
        (4, "throttled".into())
    );
    assert!(message.starts_with("Not current: GIOŚ gave no new data (throttled)."));
    // After them: asked again, and the wait is over.
    queue_reading(&agent);
    let (_, data) = current(&agent, json!({ "at": "2026-11-16 20:52" }));
    assert_eq!(
        (text(&data, "source"), requests(&agent)),
        ("network".into(), 7)
    );
    assert!(agent.borrow().store[STATE_KEY]["wait_until_local"].is_null());

    // Without Retry-After: 15 minutes. Without saved data: an error.
    let agent = chosen_530(json!({}));
    queue(&agent, http(429, ""));
    let (code, _, retryable) = failed(run(&agent, "current", json!({ "at": "2026-11-16 19:42" })));
    assert_eq!((code.as_str(), retryable), (codes::THROTTLED, true));
    let (code, message, _) = failed(run(&agent, "current", json!({ "at": "2026-11-16 19:56" })));
    assert_eq!(code, codes::THROTTLED);
    assert!(message.contains("asked to wait"));
    assert_eq!(requests(&agent), 1);
    queue_reading(&agent);
    current(&agent, json!({ "at": "2026-11-16 19:57" }));
    assert_eq!(requests(&agent), 4);
}

#[test]
fn a_denied_network_permission_is_said_and_saved_data_is_not_shown_instead() {
    let agent = chosen_530(json!({}));
    queue_reading(&agent);
    current(&agent, json!({}));
    agent.borrow_mut().deny_network = true;
    let (code, message, retryable) = failed(run(&agent, "current", json!({})));
    assert_eq!((code.as_str(), retryable), (codes::NOT_PERMITTED, false));
    assert!(message.contains("api.gios.gov.pl") && !message.contains("530"));
    assert_eq!(variable(&agent, "stale"), Some(json!(true)));

    let fresh = agent_denied();
    for (action, params) in [
        ("find-station", json!({ "query": "warszawa" })),
        ("select-station", json!({ "station": 530 })),
    ] {
        let (code, _, _) = failed(run(&fresh, action, params));
        assert_eq!(code, codes::NOT_PERMITTED, "{action}");
    }
    assert!(fresh.borrow().store.is_empty());
}

fn agent_denied() -> Shared {
    let agent = agent(json!({}));
    agent.borrow_mut().deny_network = true;
    agent
}

#[test]
fn a_reading_of_another_station_is_never_served() {
    let agent = chosen_530(json!({}));
    queue_reading(&agent);
    current(&agent, json!({ "at": "2026-11-16 19:42" }));
    // The choice changes behind the cache's back (an older version, a
    // damaged store): the stored reading is of station 530.
    agent.borrow_mut().store.get_mut(STATION_KEY).unwrap()["station"]["id"] = json!(550);
    queue(&agent, offline());
    let (code, _, _) = failed(run(&agent, "current", json!({ "at": "2026-11-16 19:50" })));
    assert_eq!(code, "connection-failed");
    // A stored reading of another format is not read.
    let agent = chosen_530(json!({}));
    queue_reading(&agent);
    current(&agent, json!({ "at": "2026-11-16 19:42" }));
    agent.borrow_mut().store.get_mut(READING_KEY).unwrap()["v"] = json!(7);
    queue_reading(&agent);
    let (_, data) = current(&agent, json!({ "at": "2026-11-16 19:50" }));
    assert_eq!(text(&data, "source"), "network");
}

#[test]
fn storage_and_log_never_hold_coordinates_and_the_log_never_names_the_station() {
    let agent = agent(json!({}));
    queue(&agent, http(200, &stations_body(STATION_ROWS, 1)));
    ok(run(&agent, "find-station", json!({ "query": "warszawa" })));
    queue(&agent, http(200, &sensors_body(SENSORS_530)));
    ok(run(&agent, "select-station", json!({ "station": 530 })));
    queue_reading(&agent);
    let (message, data) = current(&agent, json!({}));
    queue(&agent, offline());
    current(&agent, json!({}));
    // A storage that refuses writes is the only thing logged, in general
    // words.
    agent.borrow_mut().full_storage = true;
    queue_reading(&agent);
    current(&agent, json!({}));
    ok(run(&agent, "forget-station", json!({})));

    let a = agent.borrow();
    let everything = format!(
        "{} {} {message} {data:?} {:?}",
        serde_json::to_string(&a.store).unwrap(),
        serde_json::to_string(&a.variables).unwrap(),
        a.logs
    );
    for part in NEVER_KEPT {
        assert!(!everything.contains(part), "{part}");
    }
    assert!(!a.logs.is_empty());
    for line in &a.logs {
        assert!(line.contains("storage refused"), "{line}");
        for word in ["Warszawa", "530", "3585", "mazowieckie", "Niepodleg"] {
            assert!(!line.contains(word), "{line}");
        }
    }
    // Requests go to the one declared origin, and name only the station
    // and its sensors.
    for url in &a.urls {
        let path = url
            .strip_prefix("https://api.gios.gov.pl/pjp-api/v1/rest/")
            .unwrap();
        assert!(
            [
                "station/findAll?page=0&size=500",
                "station/sensors/530?size=100",
                "aqindex/getIndex/530",
                "data/getData/3585?size=24",
                "data/getData/3584?size=24"
            ]
            .contains(&path),
            "{url}"
        );
    }
}

#[test]
fn a_very_long_station_name_still_gives_a_message_within_the_limit() {
    let long = "Ż".repeat(300);
    let row = json!({
        "Identyfikator stacji": 530, "Nazwa stacji": long, "Nazwa miasta": long, "Województwo": long,
    })
    .to_string();
    let agent = agent(json!({ "language": "pl" }));
    queue(&agent, http(200, &stations_body(&row, 1)));
    queue(&agent, http(200, &sensors_body(SENSORS_530)));
    ok(run(&agent, "select-station", json!({ "station": 530 })));
    agent.borrow_mut().urls.clear();
    queue_reading(&agent);
    current(&agent, json!({}));
    queue(&agent, offline());
    let (message, data) = current(&agent, json!({}));
    assert!(message.chars().count() <= 1_024);
    assert!(text(&data, "station").chars().count() <= 80);
}

// ---- Key state ------------------------------------------------------------

/// What a key bound to Current air quality shows, through the state
/// handler.
fn key(agent: &Shared) -> KeyState {
    match host(agent).key_state(&plugin(), "current", json!({})) {
        deckpilot_plugin_sdk::StateOutcome::Ok(state) => state,
        other => panic!("{other:?}"),
    }
}

/// 2026-11-16 19:00 in Warsaw, the time of the canned values.
const MEASURED: u64 = 1_794_852_000;

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
        assert_eq!(plugin().find(id).unwrap().has_state(), declared, "{id}");
    }
    let a = chosen_530(json!({}));
    queue_reading(&a);
    current(&a, json!({}));
    let (urls, store) = (requests(&a), a.borrow().store.clone());
    key(&a);
    assert_eq!(requests(&a), urls);
    assert_eq!(a.borrow().store, store);
}

#[test]
fn the_key_shows_the_category_on_its_colour_and_marks_old_readings() {
    // No station, then a station without a reading: the user's title.
    assert_eq!(key(&agent(json!({}))), KeyState::new());
    let a = chosen_530(json!({}));
    assert_eq!(key(&a), KeyState::new());
    queue_reading(&a);
    current(&a, json!({}));
    let fresh = key(&a);
    assert_eq!(fresh.label.as_deref(), Some("Good"));
    assert_eq!(fresh.detail.as_deref(), Some("PM2.5 15 PM10 23"));
    assert_eq!(fresh.accent, Some(Accent::Green));
    assert_eq!((fresh.badge.as_deref(), fresh.indicator), (None, None));
    fresh.validate().expect("within the bounds of key state");
    // Fetched 29 minutes ago, measured 71 minutes ago: still current.
    a.borrow_mut().now = DATE_UNIX + 29 * 60;
    assert_eq!(key(&a), fresh);
    // Fetched 30 minutes ago: newer data may exist; marked with the age of
    // the measurement, values kept.
    for (now, badge) in [
        (DATE_UNIX + 30 * 60, "1h"),
        (MEASURED + 26 * 3_600, "26h"),
        (MEASURED + 3 * 86_400, "3d"),
    ] {
        a.borrow_mut().now = now;
        let old = key(&a);
        assert_eq!(
            (old.label.clone(), old.detail.clone()),
            (fresh.label.clone(), fresh.detail.clone())
        );
        assert_eq!(old.badge.as_deref(), Some(badge));
        assert_eq!(old.indicator, Some(Indicator::Warning));
    }
    // A clock far behind the measurement: its age is unknown, and said so.
    a.borrow_mut().now = MEASURED - 2 * 3_600;
    let unknown = key(&a);
    assert_eq!(
        (unknown.badge.as_deref(), unknown.indicator),
        (Some("?"), Some(Indicator::Warning))
    );
    // Polish, without Polish letters on the key.
    a.borrow_mut().now = DATE_UNIX;
    a.borrow_mut().settings = json!({ "language": "pl" });
    assert_eq!(key(&a).label.as_deref(), Some("Dobry"));
}

#[test]
fn freshly_fetched_but_long_measured_is_not_current() {
    use crate::cache::ReadingEntry;
    use crate::provider::{Reading, Sample};
    let entry = |index: Option<i64>, pm25_time: &str| ReadingEntry {
        v: 1,
        provider: "gios".into(),
        station: 530,
        fetched_at: Some(DATE_UNIX),
        fetched_local: None,
        reading: Reading {
            index,
            index_time: index.map(|_| pm25_time.to_owned()),
            pm25: Some(Sample {
                value: 41.6,
                time: pm25_time.into(),
            }),
            ..Reading::default()
        },
    };
    let show = |e: &ReadingEntry, language| key::current(Some(e), &gios::Gios, DATE_UNIX, language);
    // Fetched now, measured five hours before: not current.
    let old = show(&entry(Some(4), "2026-11-16 14:42:00"), texts::Language::En);
    assert_eq!(old.label.as_deref(), Some("Bad"));
    assert_eq!(old.detail.as_deref(), Some("PM2.5 42"));
    assert_eq!(old.accent, Some(Accent::Red));
    assert_eq!(
        (old.badge.as_deref(), old.indicator),
        (Some("5h"), Some(Indicator::Warning))
    );
    // The categories: colours and ASCII texts in both languages.
    for (index, en, pl, accent) in [
        (0, "Very good", "Bardzo dobry", Accent::Teal),
        (1, "Good", "Dobry", Accent::Green),
        (2, "Moderate", "Umiarkowany", Accent::Amber),
        (3, "Sufficient", "Dostateczny", Accent::Amber),
        (4, "Bad", "Zly", Accent::Red),
        (5, "Very bad", "Bardzo zly", Accent::Purple),
    ] {
        let e = entry(Some(index), HOUR);
        let shown = show(&e, texts::Language::En);
        assert_eq!(
            (shown.label.as_deref(), shown.accent),
            (Some(en), Some(accent)),
            "{index}"
        );
        assert_eq!(
            show(&e, texts::Language::Pl).label.as_deref(),
            Some(pl),
            "{index}"
        );
    }
    // No index: said so, without a colour; nothing at all: the title.
    let none = show(&entry(None, HOUR), texts::Language::En);
    assert_eq!(
        (none.label.as_deref(), none.accent),
        (Some("No index"), None)
    );
    let mut empty = entry(None, HOUR);
    empty.reading.pm25 = None;
    assert_eq!(show(&empty, texts::Language::En), KeyState::new());
}
