//! Native unit tests: the definition agrees with `manifest.json`, and every
//! action behaves with the agent replaced by a scripted `MockHost` that
//! answers with canned Frankfurter responses. Nothing here reaches the
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
use crate::cache::{ALERT_PREFIX, MAX_PAIRS, PAIR_PREFIX, PairEntry, STATE_KEY};
use crate::provider::Point;

const MANIFEST: &str = include_str!("../manifest.json");

/// The newest rate alone, as `/v2/providers/ecb/rates?base=EUR&quotes=USD`
/// answered on 2026-11-16.
const LATEST: &str = r#"[{"date":"2026-11-16","base":"EUR","quote":"USD","rate":1.1014}]"#;

/// A window of rates, the shape of a real answer with `from`.
const WINDOW: &str = r#"[{"date":"2026-11-06","base":"EUR","quote":"USD","rate":1.1165},
{"date":"2026-11-09","base":"EUR","quote":"USD","rate":1.1193},
{"date":"2026-11-10","base":"EUR","quote":"USD","rate":1.1214},
{"date":"2026-11-11","base":"EUR","quote":"USD","rate":1.1127},
{"date":"2026-11-12","base":"EUR","quote":"USD","rate":1.1081},
{"date":"2026-11-13","base":"EUR","quote":"USD","rate":1.1052},
{"date":"2026-11-16","base":"EUR","quote":"USD","rate":1.1014}]"#;

/// The window up to Friday, as answered on a Sunday.
const WINDOW_TO_FRIDAY: &str = r#"[{"date":"2026-11-12","base":"EUR","quote":"USD","rate":1.1081},
{"date":"2026-11-13","base":"EUR","quote":"USD","rate":1.1052}]"#;

const CROSS: &str = r#"[{"date":"2026-11-13","base":"USD","quote":"PLN","rate":3.9914},
{"date":"2026-11-16","base":"USD","quote":"PLN","rate":3.9976}]"#;

/// What Frankfurter answers for an unknown currency (HTTP 422).
const INVALID_CURRENCY: &str = r#"{"status":422,"message":"invalid currency: ABC"}"#;

const DATE: &str = "Mon, 16 Nov 2026 19:03:29 GMT";
const SUNDAY: &str = "Sun, 15 Nov 2026 10:00:00 GMT";

/// The user's amount and threshold: they must never reach the log or a
/// request.
const AMOUNT: &str = "12345.67";
const THRESHOLD: &str = "1.0987";

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
    http_at(status, body, DATE)
}

fn http_at(status: u16, body: &str, date: &str) -> Reply {
    Reply::Http {
        status,
        headers: vec![
            ("date", date.into()),
            (
                "cache-control",
                "public, max-age=17768, stale-if-error=86400".into(),
            ),
        ],
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Notify {
    #[default]
    Shown,
    Nowhere,
    Throttled,
    NoSeverity,
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
    shown: Vec<(String, String)>,
    notify: Notify,
    cleared: usize,
    deny_network: bool,
}

type Shared = Rc<RefCell<Agent>>;

fn agent() -> Shared {
    Rc::new(RefCell::new(Agent {
        settings: json!({"language": "en", "desktop": true, "statusBar": true}),
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
            CapabilityRequest::Notify(n) => {
                assert!(n.title.chars().count() <= 64, "{}", n.title);
                assert!(n.body.chars().count() <= 300, "{}", n.body);
                match a.notify {
                    Notify::Shown => {
                        a.shown.push((n.title.clone(), n.body.clone()));
                        CallResult::Ok(json!({ "shown": true, "desktop": true, "statusBar": true }))
                    }
                    Notify::Nowhere => CallResult::Ok(json!({
                        "shown": false,
                        "reason": "desktop notifications are off"
                    })),
                    Notify::Throttled => {
                        CallResult::error(CallErrorCode::Throttled, "5 notifications per minute")
                    }
                    Notify::NoSeverity if n.severity.is_some() => {
                        CallResult::error(CallErrorCode::BadRequest, "unknown field `severity`")
                    }
                    Notify::NoSeverity => {
                        a.shown.push((n.title.clone(), n.body.clone()));
                        CallResult::Ok(Value::Null)
                    }
                }
            }
            CapabilityRequest::HttpFetch(f) => {
                assert_eq!(f.method, HttpMethod::Get);
                assert!(f.body.is_none() && f.auth.is_none());
                assert!(f.url.starts_with(frankfurter::RATES_API), "{}", f.url);
                assert!(
                    f.headers.iter().any(|h| h.name == "User-Agent"
                        && h.value == format!("DeckPilot-Markets/{VERSION}"))
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

fn urls(agent: &Shared) -> Vec<String> {
    agent.borrow().urls.clone()
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

/// Every output is declared (the agent drops the others), every output
/// declared as always present is there, and every one says when, from
/// where and how fresh.
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
    for key in ["rateDate", "source", "attribution", "delayed", "stale"] {
        assert!(data.contains_key(key), "{action}: {key}");
    }
    assert!(flag(data, "delayed"), "ECB rates are never live");
}

/// Seeds the cache with a window of EUR/USD, fetched at [`DATE`] by a run
/// at `at`.
fn seeded(at: Option<&str>) -> Shared {
    let agent = agent();
    queue(&agent, http(200, WINDOW));
    let params = match at {
        Some(at) => json!({"quote": "USD", "at": at}),
        None => json!({"quote": "USD"}),
    };
    // A stored state that knows the date: one request, with `from`.
    agent.borrow_mut().store.insert(
        STATE_KEY.into(),
        json!({"last_seen": time::http_date(DATE).unwrap()}),
    );
    ok(run(&agent, "rate", params));
    agent.borrow_mut().urls.clear();
    agent.borrow_mut().published.clear();
    agent
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

    let permissions = m["permissions"].as_array().unwrap();
    assert_eq!(permissions.len(), 2);
    assert_eq!(permissions[0]["kind"], "network");
    assert_eq!(permissions[0]["hosts"], json!([frankfurter::HOST]));
    assert_eq!(permissions[0]["methods"], json!(["GET"]));
    assert!(frankfurter::RATES_API.starts_with(&format!("https://{}/", frankfurter::HOST)));
    assert_eq!(permissions[1]["kind"], "notifications");
    assert_eq!(
        permissions[1]["routing"],
        json!({"desktop": "desktop", "statusBar": "statusBar"})
    );
    // No credential: Frankfurter needs no key, and nothing else is asked.
    assert!(m["contributes"].get("credentials").is_none());

    let codes: Vec<&str> = m["contributes"]["errorCodes"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let mut expected = PLUGIN_CODES.to_vec();
    expected.sort_unstable();
    let mut codes = codes;
    codes.sort_unstable();
    assert_eq!(codes, expected);

    let variables: Vec<&str> = m["contributes"]["variables"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["id"].as_str().unwrap())
        .collect();
    let all: Vec<&str> = RATE_VARIABLES
        .iter()
        .chain(ALERT_VARIABLES)
        .copied()
        .collect();
    assert_eq!(variables, all);

    let settings: Vec<&str> = m["contributes"]["settings"]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["key"].as_str().unwrap())
        .collect();
    assert_eq!(settings, ["language", "desktop", "statusBar"]);
}

#[test]
fn no_text_calls_a_rate_live_or_recommends_anything() {
    let lower = MANIFEST.to_lowercase();
    for word in [
        "live rate",
        "real-time",
        "real time",
        "buy",
        "sell",
        "invest",
    ] {
        assert!(!lower.contains(word), "manifest.json says {word}");
    }
    assert!(lower.contains("not live") || lower.contains("never live"));
    assert!(lower.contains("not financial advice"));
}

// ---- rate ------------------------------------------------------------------

#[test]
fn a_first_rate_asks_for_the_newest_rate_then_for_a_window() {
    let agent = agent();
    queue(&agent, http(200, LATEST));
    queue(&agent, http(200, WINDOW));
    let (message, data) = ok(run(&agent, "rate", json!({"quote": "usd"})));
    assert_eq!(
        urls(&agent),
        [
            format!("{}?base=EUR&quotes=USD", frankfurter::RATES_API),
            format!(
                "{}?base=EUR&quotes=USD&from=2026-10-16",
                frankfurter::RATES_API
            ),
        ]
    );
    assert_outputs("rate", &data);
    assert_eq!(text(&data, "pair"), "EUR/USD");
    assert_eq!(text(&data, "base"), "EUR");
    assert_eq!(text(&data, "quote"), "USD");
    assert_eq!(text(&data, "rate"), "1.1014");
    assert_eq!(text(&data, "rateDate"), "2026-11-16");
    assert_eq!(
        text(&data, "asOf"),
        "ECB reference rate of 2026-11-16, about 16:00 CET"
    );
    assert_eq!(text(&data, "previousRate"), "1.1052");
    assert_eq!(text(&data, "previousDate"), "2026-11-13");
    assert_eq!(text(&data, "change"), "-0.0038");
    assert_eq!(text(&data, "changePercent"), "-0.34");
    assert_eq!(text(&data, "direction"), "down");
    assert_eq!(text(&data, "min"), "1.1014");
    assert_eq!(text(&data, "max"), "1.1214");
    assert_eq!(text(&data, "trend"), "down");
    assert_eq!(int(&data, "historyCount"), 7);
    assert_eq!(text(&data, "historyFrom"), "2026-11-06");
    assert!(!flag(&data, "stale"));
    assert!(!flag(&data, "cross"));
    assert_eq!(text(&data, "source"), "ecb-frankfurter");
    assert_eq!(text(&data, "fetchedAt"), "2026-11-16T19:03:29Z");
    assert_eq!(int(&data, "ageMinutes"), 0);
    assert!(!data.contains_key("problem") && !data.contains_key("amount"));
    assert!(
        message.starts_with(
            "EUR/USD 1.1014: ECB reference rate of Mon 2026-11-16 (a daily rate, not live)."
        ),
        "{message}"
    );
    assert!(
        message.contains("Change since 2026-11-13: -0.0038 (-0.34%)."),
        "{message}"
    );
    assert!(
        message.contains("7 rates since 2026-11-06: 1.1014 to 1.1214, trend down."),
        "{message}"
    );
    assert!(message.ends_with(frankfurter::ATTRIBUTION), "{message}");

    assert_eq!(variable(&agent, "pair"), json!("EUR/USD"));
    assert_eq!(variable(&agent, "rate"), json!("1.1014"));
    assert_eq!(variable(&agent, "rateDate"), json!("2026-11-16"));
    assert_eq!(variable(&agent, "changePercent"), json!("-0.34"));
    assert_eq!(variable(&agent, "stale"), json!(false));
    assert_eq!(variable(&agent, "history").as_array().unwrap().len(), 7);
    assert_eq!(
        variable(&agent, "history")[6],
        json!({"date": "2026-11-16", "rate": "1.1014"})
    );
    assert!(
        !variable(&agent, "summary")
            .as_str()
            .unwrap()
            .starts_with("[stale]")
    );

    let entry: PairEntry =
        serde_json::from_value(agent.borrow().store[&format!("{PAIR_PREFIX}EUR-USD")].clone())
            .unwrap();
    assert_eq!(entry.points.len(), 7);
    assert_eq!(entry.ttl, 17_768);
}

#[test]
fn a_known_date_needs_one_request() {
    let agent = seeded(None);
    queue(&agent, http(200, WINDOW));
    ok(run(&agent, "rate", json!({"quote": "USD"})));
    assert_eq!(
        urls(&agent),
        [format!(
            "{}?base=EUR&quotes=USD&from=2026-10-16",
            frankfurter::RATES_API
        )]
    );
}

#[test]
fn a_scheduled_run_within_the_lifetime_sends_nothing() {
    let agent = seeded(Some("2026-11-16 20:00"));
    let (message, data) = ok(run(
        &agent,
        "rate",
        json!({"quote": "USD", "at": "2026-11-16 23:00"}),
    ));
    assert_eq!(requests(&agent), 0);
    assert_outputs("rate", &data);
    assert_eq!(text(&data, "source"), "ecb-frankfurter");
    assert_eq!(int(&data, "ageMinutes"), 180);
    assert!(!flag(&data, "stale"));
    assert!(message.contains("(saved 180 min ago)"), "{message}");

    // After the lifetime (17,768 s, about 4.9 h): asks again.
    queue(&agent, http(200, WINDOW));
    let (_, data) = ok(run(
        &agent,
        "rate",
        json!({"quote": "USD", "at": "2026-11-17 01:00"}),
    ));
    assert_eq!(requests(&agent), 1);
    assert_eq!(int(&data, "ageMinutes"), 0);
}

#[test]
fn an_amount_is_converted_and_never_logged_or_sent() {
    let agent = seeded(None);
    queue(&agent, http(200, WINDOW));
    let (message, data) = ok(run(
        &agent,
        "rate",
        json!({"quote": "USD", "amount": "12 345,67"}),
    ));
    assert_outputs("rate", &data);
    assert_eq!(text(&data, "amount"), "12345.67");
    assert_eq!(text(&data, "converted"), "13597.52");
    assert!(
        message.contains("12345.67 EUR = 13597.52 USD (converted by the plugin)."),
        "{message}"
    );
    let a = agent.borrow();
    for line in a.logs.iter().chain(a.urls.iter()) {
        assert!(!line.contains("12345") && !line.contains(AMOUNT), "{line}");
    }
}

#[test]
fn a_bad_amount_or_pair_is_refused_before_any_request() {
    let agent = agent();
    for (params, code) in [
        (json!({"quote": "USD", "amount": "-5"}), INVALID_AMOUNT),
        (json!({"quote": "USD", "amount": "lots"}), INVALID_AMOUNT),
        (json!({"quote": "US"}), "invalid-parameters"),
        (json!({"quote": "U$D"}), "invalid-parameters"),
        (json!({"base": "usd", "quote": "USD"}), "invalid-parameters"),
        (
            json!({"quote": "USD", "at": "yesterday"}),
            "invalid-parameters",
        ),
    ] {
        assert_eq!(
            failed(run(&agent, "rate", params.clone())).0,
            code,
            "{params}"
        );
    }
    assert_eq!(requests(&agent), 0);
}

#[test]
fn a_weekend_shows_fridays_rate_and_says_why() {
    let agent = agent();
    agent.borrow_mut().store.insert(
        STATE_KEY.into(),
        json!({"last_seen": time::http_date(SUNDAY).unwrap()}),
    );
    queue(&agent, http_at(200, WINDOW_TO_FRIDAY, SUNDAY));
    let (message, data) = ok(run(&agent, "rate", json!({"quote": "USD"})));
    assert_eq!(
        urls(&agent),
        [format!(
            "{}?base=EUR&quotes=USD&from=2026-10-15",
            frankfurter::RATES_API
        )]
    );
    assert_eq!(text(&data, "rateDate"), "2026-11-13");
    assert!(!flag(&data, "stale"), "a weekend is not stale");
    assert!(message.contains("of Fri 2026-11-13"), "{message}");
    assert!(
        message.contains("none on weekends and TARGET holidays"),
        "{message}"
    );
}

#[test]
fn a_rate_the_ecb_no_longer_publishes_is_stale() {
    let agent = agent();
    queue(
        &agent,
        http(
            200,
            r#"[{"date":"2022-03-01","base":"EUR","quote":"RUB","rate":117.3}]"#,
        ),
    );
    queue(
        &agent,
        http(
            200,
            r#"[{"date":"2022-03-01","base":"EUR","quote":"RUB","rate":117.3}]"#,
        ),
    );
    let (message, data) = ok(run(&agent, "rate", json!({"quote": "RUB"})));
    assert_outputs("rate", &data);
    assert!(flag(&data, "stale"));
    assert_eq!(text(&data, "problem"), cache::OLD_RATE);
    assert!(
        message.starts_with("Not current: the newest ECB rate for EUR/RUB is"),
        "{message}"
    );
    assert!(text(&data, "summary").starts_with("[stale] "));
    assert_eq!(variable(&agent, "stale"), json!(true));
}

#[test]
fn an_unknown_currency_is_a_clear_error() {
    let agent = agent();
    queue(&agent, http(422, INVALID_CURRENCY));
    let (code, message, retryable) = failed(run(&agent, "rate", json!({"quote": "ABC"})));
    assert_eq!(code, UNKNOWN_CURRENCY);
    assert!(message.contains("EUR or ABC"), "{message}");
    assert!(!retryable);
    // A code the ECB does not publish: an empty list.
    queue(&agent, http(200, "[]"));
    assert_eq!(
        failed(run(&agent, "rate", json!({"quote": "XAU"}))).0,
        UNKNOWN_CURRENCY
    );
    queue(&agent, http(404, r#"{"status":404,"message":"not found"}"#));
    assert_eq!(
        failed(run(&agent, "rate", json!({"quote": "XYZ"}))).0,
        UNKNOWN_CURRENCY
    );
    assert_eq!(variable(&agent, "stale"), json!(true));
}

#[test]
fn an_unknown_currency_is_not_hidden_by_saved_rates() {
    let agent = seeded(None);
    queue(&agent, http(422, INVALID_CURRENCY));
    assert_eq!(
        failed(run(&agent, "rate", json!({"quote": "USD"}))).0,
        UNKNOWN_CURRENCY
    );
}

#[test]
fn errors_without_saved_rates() {
    for (reply, code, retryable) in [
        (http(500, "oops"), PROVIDER_ERROR, true),
        (http(403, "no"), PROVIDER_ERROR, false),
        (http(400, "{}"), PROVIDER_ERROR, false),
        (http(200, "<html>"), INVALID_RESPONSE, false),
        (http(200, r#"{"rates":{}}"#), INVALID_RESPONSE, false),
        (
            http(
                200,
                r#"[{"date":"2026-11-16","base":"EUR","quote":"GBP","rate":0.87}]"#,
            ),
            INVALID_RESPONSE,
            false,
        ),
        (
            http(
                200,
                r#"[{"date":"16.11.2026","base":"EUR","quote":"USD","rate":1.1}]"#,
            ),
            INVALID_RESPONSE,
            false,
        ),
        (
            http(
                200,
                r#"[{"date":"2026-11-16","base":"EUR","quote":"USD","rate":-1}]"#,
            ),
            INVALID_RESPONSE,
            false,
        ),
        (
            http(
                200,
                &format!("[{}]", " ".repeat(frankfurter::MAX_BODY_BYTES)),
            ),
            RESPONSE_TOO_LARGE,
            false,
        ),
        (
            Reply::Error(
                CallErrorCode::Failed,
                "the answer is larger than 524288 bytes",
                None,
            ),
            RESPONSE_TOO_LARGE,
            false,
        ),
        (offline(), "connection-failed", true),
        (
            Reply::Error(
                CallErrorCode::Failed,
                "timed out",
                Some(NetworkErrorKind::TimedOut),
            ),
            TIMED_OUT,
            true,
        ),
    ] {
        let agent = agent();
        queue(&agent, reply);
        let (got, message, again) = failed(run(&agent, "rate", json!({"quote": "USD"})));
        assert_eq!((got.as_str(), again), (code, retryable), "{message}");
        assert_eq!(variable(&agent, "stale"), json!(true));
    }
}

#[test]
fn saved_rates_are_served_as_stale_when_the_provider_fails() {
    let agent = seeded(None);
    queue(&agent, offline());
    let (message, data) = ok(run(&agent, "rate", json!({"quote": "USD"})));
    assert_outputs("rate", &data);
    assert!(flag(&data, "stale"));
    assert_eq!(text(&data, "problem"), "connection-failed");
    assert_eq!(text(&data, "rate"), "1.1014");
    assert!(!data.contains_key("ageMinutes"));
    assert!(
        message.starts_with(
            "Not current: Frankfurter gave no new data (connection-failed). Saved rates fetched 2026-11-16T19:03:29Z; the plugin has no clock"
        ),
        "{message}"
    );
    assert_eq!(variable(&agent, "stale"), json!(true));

    // An HTTP 500 a day later: the answer's Date tells the age.
    queue(&agent, http_at(500, "", "Tue, 17 Nov 2026 19:03:29 GMT"));
    let (message, data) = ok(run(&agent, "rate", json!({"quote": "USD"})));
    assert!(flag(&data, "stale"));
    assert_eq!(int(&data, "ageMinutes"), 1_440);
    assert!(message.contains("about 1440 min ago"), "{message}");

    // Within the lifetime, a failed request is reported but not stale.
    queue(&agent, http_at(503, "", "Mon, 16 Nov 2026 20:03:29 GMT"));
    let (message, data) = ok(run(&agent, "rate", json!({"quote": "USD"})));
    assert!(!flag(&data, "stale"));
    assert_eq!(text(&data, "problem"), PROVIDER_ERROR);
    assert!(
        message.contains("(saved; Frankfurter gave no new data: provider-error)"),
        "{message}"
    );
}

#[test]
fn a_denied_network_is_never_hidden_by_saved_rates() {
    let agent = seeded(None);
    agent.borrow_mut().deny_network = true;
    let (code, message, _) = failed(run(&agent, "rate", json!({"quote": "USD"})));
    assert_eq!(code, "not-permitted");
    assert!(message.contains("api.frankfurter.dev"), "{message}");
}

#[test]
fn http_429_and_retry_after_are_honoured() {
    let agent = seeded(Some("2026-11-16 20:00"));
    let mut reply = http_at(429, "", "Tue, 17 Nov 2026 01:00:00 GMT");
    if let Reply::Http { headers, .. } = &mut reply {
        headers.push(("retry-after", "3600".into()));
    }
    queue(&agent, reply);
    let (_, data) = ok(run(
        &agent,
        "rate",
        json!({"quote": "USD", "at": "2026-11-17 02:00"}),
    ));
    assert_eq!(text(&data, "problem"), "throttled");
    assert!(flag(&data, "stale"));
    assert_eq!(requests(&agent), 1);
    // Within the hour: not asked.
    let (_, data) = ok(run(
        &agent,
        "rate",
        json!({"quote": "USD", "at": "2026-11-17 02:30"}),
    ));
    assert_eq!(text(&data, "problem"), "throttled");
    assert_eq!(requests(&agent), 1);
    // Without saved rates of that pair, the wait is an error.
    let (code, _, retryable) = failed(run(
        &agent,
        "rate",
        json!({"quote": "PLN", "at": "2026-11-17 02:30"}),
    ));
    assert_eq!((code.as_str(), retryable), ("throttled", true));
    assert_eq!(requests(&agent), 1);
    // After it: asked again.
    queue(&agent, http(200, WINDOW));
    let (_, data) = ok(run(
        &agent,
        "rate",
        json!({"quote": "USD", "at": "2026-11-17 03:01"}),
    ));
    assert!(!data.contains_key("problem"));
    assert_eq!(requests(&agent), 2);
}

#[test]
fn retry_after_and_max_age_parsing() {
    let now = time::http_date(DATE);
    assert_eq!(frankfurter::retry_after(Some("120"), now), 120);
    assert_eq!(frankfurter::retry_after(Some("1"), now), 60);
    assert_eq!(
        frankfurter::retry_after(None, now),
        frankfurter::DEFAULT_RETRY_SECONDS
    );
    assert_eq!(
        frankfurter::retry_after(Some("Mon, 16 Nov 2026 21:03:29 GMT"), now),
        7_200
    );
    assert_eq!(
        frankfurter::retry_after(Some("999999"), now),
        frankfurter::MAX_RETRY_SECONDS
    );
    assert_eq!(
        frankfurter::max_age(Some("public, max-age=17768, stale-if-error=86400")),
        Some(17_768)
    );
    assert_eq!(frankfurter::max_age(Some("no-store")), None);
    assert_eq!(frankfurter::max_age(None), None);
}

#[test]
fn the_lifetime_is_bounded() {
    for (header, ttl) in [
        ("max-age=60", cache::MIN_TTL_SECONDS),
        ("max-age=86400", cache::MAX_TTL_SECONDS),
        ("no-store", cache::DEFAULT_TTL_SECONDS),
    ] {
        let agent = agent();
        agent.borrow_mut().store.insert(
            STATE_KEY.into(),
            json!({"last_seen": time::http_date(DATE).unwrap()}),
        );
        queue(
            &agent,
            Reply::Http {
                status: 200,
                headers: vec![("date", DATE.into()), ("cache-control", header.into())],
                body: WINDOW.into(),
            },
        );
        ok(run(&agent, "rate", json!({"quote": "USD"})));
        let entry: PairEntry =
            serde_json::from_value(agent.borrow().store[&format!("{PAIR_PREFIX}EUR-USD")].clone())
                .unwrap();
        assert_eq!(entry.ttl, ttl, "{header}");
    }
}

#[test]
fn a_cross_rate_says_how_it_was_made() {
    let agent = agent();
    queue(&agent, http(200, CROSS));
    queue(&agent, http(200, CROSS));
    let (message, data) = ok(run(&agent, "rate", json!({"base": "USD", "quote": "PLN"})));
    assert_outputs("rate", &data);
    assert!(flag(&data, "cross"));
    assert_eq!(text(&data, "rate"), "3.9976");
    assert!(
        message.contains("Cross rate calculated by Frankfurter from the ECB's euro rates."),
        "{message}"
    );
}

#[test]
fn the_history_is_bounded_and_recent() {
    let old: Vec<Point> = (0..40)
        .map(|i| Point {
            d: time::date_text(time::day_number("2026-01-01").unwrap() + i),
            r: 1.0 + f64::from(u8::try_from(i).unwrap()) / 100.0,
        })
        .collect();
    let merged = cache::merge(&old, &[]);
    assert_eq!(merged.len(), cache::MAX_POINTS);
    assert_eq!(merged.last().unwrap().d, "2026-02-09");
    // A new rate months later drops what is older than 92 days before it.
    let new = [Point {
        d: "2026-11-16".into(),
        r: 1.1,
    }];
    assert_eq!(cache::merge(&old, &new), new.to_vec());
    // A date of the answer replaces the same saved date.
    let again = [Point {
        d: "2026-02-09".into(),
        r: 9.0,
    }];
    let merged = cache::merge(&old, &again);
    assert_eq!(merged.last().unwrap().r, 9.0);
    assert_eq!(merged.len(), cache::MAX_POINTS);
}

#[test]
fn pairs_beyond_the_limit_are_dropped_oldest_first() {
    let agent = agent();
    agent.borrow_mut().store.insert(
        STATE_KEY.into(),
        json!({"last_seen": time::http_date(DATE).unwrap()}),
    );
    // MAX_PAIRS + 2 pairs, each with a full answer.
    let codes: Vec<String> = (0..MAX_PAIRS + 2)
        .map(|i| {
            let a = u8::try_from(i / 26).unwrap();
            let b = u8::try_from(i % 26).unwrap();
            format!("Q{}{}", char::from(b'A' + a), char::from(b'A' + b))
        })
        .collect();
    for code in &codes {
        let body = format!(r#"[{{"date":"2026-11-16","base":"EUR","quote":"{code}","rate":2.5}}]"#);
        queue(&agent, http(200, &body));
        ok(run(&agent, "rate", json!({"quote": code})));
    }
    let a = agent.borrow();
    let stored: Vec<&String> = a
        .store
        .keys()
        .filter(|k| k.starts_with(PAIR_PREFIX))
        .collect();
    assert_eq!(stored.len(), MAX_PAIRS);
    assert!(
        !a.store
            .contains_key(&format!("{PAIR_PREFIX}EUR-{}", codes[0]))
    );
    assert!(
        a.store
            .contains_key(&format!("{PAIR_PREFIX}EUR-{}", codes[MAX_PAIRS + 1]))
    );
}

#[test]
fn polish_texts() {
    let agent = seeded(None);
    agent.borrow_mut().settings = json!({"language": "pl"});
    queue(&agent, http(200, WINDOW));
    let (message, data) = ok(run(
        &agent,
        "rate",
        json!({"quote": "USD", "amount": "100"}),
    ));
    assert!(
        message.starts_with(
            "EUR/USD 1.1014: kurs referencyjny EBC z pon. 2026-11-16 (kurs dzienny, nie bieżący)."
        ),
        "{message}"
    );
    assert!(
        message.contains("100.00 EUR = 110.14 USD (przeliczone przez wtyczkę)."),
        "{message}"
    );
    assert!(message.contains("Zmiana od 2026-11-13"), "{message}");
    assert_eq!(
        text(&data, "asOf"),
        "kurs referencyjny EBC z 2026-11-16, ok. 16:00 CET"
    );
}

#[test]
fn the_snapshot_is_what_a_key_would_show() {
    let served = cache::Served {
        points: crate::frankfurter::parse_series(
            &serde_json::from_str(WINDOW).unwrap(),
            &Pair {
                base: "EUR".into(),
                quote: "USD".into(),
            },
        )
        .unwrap(),
        source: cache::Source::Network,
        age: Some(0),
        fetched_at: None,
        lag_days: Some(0),
        stale: false,
        problem: None,
    };
    let pair = Pair {
        base: "EUR".into(),
        quote: "USD".into(),
    };
    let key = snapshot(&served, &pair).key();
    assert_eq!(key.label, "1.1014 -0.34%");
    assert_eq!(key.detail, "EUR/USD 11-16");
    assert_eq!(key.indicator, "ok");
    assert_eq!(key.series.len(), 7);
    assert_eq!(key.series[2], 100);
    assert_eq!(key.series[6], 0);
}

// ---- change ----------------------------------------------------------------

#[test]
fn change_over_working_days() {
    let agent = seeded(None);
    queue(&agent, http(200, WINDOW));
    let (message, data) = ok(run(&agent, "change", json!({"quote": "USD", "days": 4})));
    assert_outputs("change", &data);
    assert_eq!(text(&data, "fromDate"), "2026-11-10");
    assert_eq!(text(&data, "fromRate"), "1.1214");
    assert_eq!(int(&data, "days"), 4);
    assert_eq!(text(&data, "change"), "-0.02");
    assert_eq!(text(&data, "changePercent"), "-1.78");
    assert_eq!(text(&data, "direction"), "down");
    assert!(
        message.contains("over 4 ECB working day(s), from 1.1214 to 1.1014"),
        "{message}"
    );

    queue(&agent, http(200, WINDOW));
    let (code, message, _) = failed(run(&agent, "change", json!({"quote": "USD", "days": 7})));
    assert_eq!(code, NOT_ENOUGH_HISTORY);
    assert!(message.contains("at most 6"), "{message}");
}

// ---- check-alert -----------------------------------------------------------

fn alert(agent: &Shared, condition: &str, at: &str) -> (String, BTreeMap<String, ParamValue>) {
    ok(run(
        agent,
        "check-alert",
        json!({"quote": "USD", "condition": condition, "threshold": THRESHOLD, "at": at}),
    ))
}

#[test]
fn an_alert_notifies_once_per_crossing() {
    let agent = seeded(Some("2026-11-16 20:00"));
    // 1.1014 is above 1.0987: the first check notifies.
    let (message, data) = alert(&agent, "above", "2026-11-16 20:10");
    assert_outputs("check-alert", &data);
    assert!(flag(&data, "triggered") && flag(&data, "crossed") && flag(&data, "notified"));
    assert!(flag(&data, "evaluated"));
    assert_eq!(text(&data, "threshold"), THRESHOLD);
    assert_eq!(text(&data, "condition"), "above");
    assert!(message.contains("crossed it, notified"), "{message}");
    {
        let a = agent.borrow();
        assert_eq!(a.shown.len(), 1);
        assert_eq!(a.shown[0].0, format!("EUR/USD above {THRESHOLD}"));
        assert!(
            a.shown[0].1.contains("not a live price"),
            "{}",
            a.shown[0].1
        );
    }
    assert_eq!(variable(&agent, "alertTriggered"), json!(true));

    // Still above: no second notification.
    let (message, data) = alert(&agent, "above", "2026-11-16 20:20");
    assert!(flag(&data, "triggered") && !flag(&data, "crossed") && !flag(&data, "notified"));
    assert!(message.contains("already notified"), "{message}");
    assert_eq!(agent.borrow().shown.len(), 1);

    // The rate falls below: re-armed, nothing shown.
    let lower = WINDOW.replace("1.1014", "1.0900");
    queue(&agent, http(200, &lower));
    let (_, data) = alert(&agent, "above", "2026-11-17 03:00");
    assert!(!flag(&data, "triggered") && !flag(&data, "crossed"));
    assert_eq!(agent.borrow().shown.len(), 1);
    assert_eq!(variable(&agent, "alertTriggered"), json!(false));

    // And above again: a new crossing, a new notification.
    queue(&agent, http(200, WINDOW));
    let (_, data) = alert(&agent, "above", "2026-11-17 09:00");
    assert!(flag(&data, "crossed") && flag(&data, "notified"));
    assert_eq!(agent.borrow().shown.len(), 2);
    assert_eq!(requests(&agent), 2, "the other checks used the saved rates");

    // `below` with the same threshold is another alert.
    let (_, data) = alert(&agent, "below", "2026-11-17 09:05");
    assert!(!flag(&data, "triggered"));
    let keys: Vec<String> = agent
        .borrow()
        .store
        .keys()
        .filter(|k| k.starts_with(ALERT_PREFIX))
        .cloned()
        .collect();
    assert_eq!(
        keys,
        ["alert:EUR-USD:above:1.0987", "alert:EUR-USD:below:1.0987"]
    );
}

#[test]
fn a_stale_rate_neither_fires_nor_rearms_an_alert() {
    let agent = seeded(None);
    queue(&agent, offline());
    let (message, data) = ok(run(
        &agent,
        "check-alert",
        json!({"quote": "USD", "threshold": "1.05"}),
    ));
    assert_outputs("check-alert", &data);
    assert!(flag(&data, "stale"));
    assert!(!flag(&data, "evaluated") && !flag(&data, "notified") && !flag(&data, "crossed"));
    assert!(
        message.contains("not checked against your threshold"),
        "{message}"
    );
    assert!(agent.borrow().shown.is_empty());
    assert!(
        !agent
            .borrow()
            .store
            .keys()
            .any(|k| k.starts_with(ALERT_PREFIX))
    );
}

#[test]
fn a_refused_notification_is_tried_again() {
    let agent = seeded(Some("2026-11-16 20:00"));
    agent.borrow_mut().notify = Notify::Throttled;
    let (message, data) = alert(&agent, "above", "2026-11-16 20:01");
    assert!(flag(&data, "crossed") && !flag(&data, "notified"));
    assert!(
        message.contains("the notification was refused"),
        "{message}"
    );
    assert!(text(&data, "notification").contains("5 notifications per minute"));
    agent.borrow_mut().notify = Notify::NoSeverity;
    let (_, data) = alert(&agent, "above", "2026-11-16 20:02");
    assert!(flag(&data, "crossed") && flag(&data, "notified"));
    assert_eq!(agent.borrow().shown.len(), 1);

    // Shown nowhere by the user's settings: done, with the reason.
    let agent = seeded(Some("2026-11-16 20:00"));
    agent.borrow_mut().notify = Notify::Nowhere;
    let (_, data) = alert(&agent, "above", "2026-11-16 20:01");
    assert!(flag(&data, "notified"));
    assert_eq!(text(&data, "notification"), "desktop notifications are off");
    let (_, data) = alert(&agent, "above", "2026-11-16 20:02");
    assert!(!flag(&data, "crossed"));
}

#[test]
fn a_threshold_is_never_logged_or_sent() {
    let agent = seeded(Some("2026-11-16 20:00"));
    agent.borrow_mut().notify = Notify::Throttled;
    alert(&agent, "below", "2026-11-16 23:00");
    queue(&agent, http(200, WINDOW));
    alert(&agent, "above", "2026-11-17 06:00");
    let a = agent.borrow();
    assert!(!a.logs.is_empty(), "the refused notification was logged");
    for line in a.logs.iter().chain(a.urls.iter()) {
        assert!(
            !line.contains(THRESHOLD) && !line.contains("1.0987"),
            "{line}"
        );
    }
}

#[test]
fn a_bad_threshold_is_refused() {
    let agent = agent();
    for threshold in ["", "zero", "0", "-1.2"] {
        let (code, _, _) = failed(run(
            &agent,
            "check-alert",
            json!({"quote": "USD", "threshold": threshold}),
        ));
        assert_eq!(code, INVALID_THRESHOLD, "{threshold}");
    }
    assert_eq!(requests(&agent), 0);
}

#[test]
fn alerts_beyond_the_limit_are_dropped() {
    let agent = seeded(Some("2026-11-16 20:00"));
    for i in 0..cache::MAX_ALERTS + 3 {
        ok(run(
            &agent,
            "check-alert",
            json!({"quote": "USD", "condition": "below", "threshold": format!("0.{:03}", i + 1),
                   "at": "2026-11-16 20:05"}),
        ));
    }
    let a = agent.borrow();
    let alerts = a
        .store
        .keys()
        .filter(|k| k.starts_with(ALERT_PREFIX))
        .count();
    assert_eq!(alerts, cache::MAX_ALERTS);
    assert!(!a.store.contains_key("alert:EUR-USD:below:0.001"));
    assert!(a.store.contains_key("alert:EUR-USD:below:0.067"));
}

// ---- forget ----------------------------------------------------------------

#[test]
fn forget_deletes_everything_stored() {
    let agent = seeded(Some("2026-11-16 20:00"));
    alert(&agent, "above", "2026-11-16 20:01");
    assert!(agent.borrow().store.len() >= 3);
    let (message, _) = ok(run(&agent, "forget", json!({})));
    assert!(message.contains("deleted"));
    assert!(agent.borrow().store.is_empty());
    assert_eq!(agent.borrow().cleared, 1);
    assert_eq!(requests(&agent), 0);
}
