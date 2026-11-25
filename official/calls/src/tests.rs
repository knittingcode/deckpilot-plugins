//! The actions and their key states against a scripted host: the exact
//! requests, the confirmed outcome, every refusal with its code, and faces
//! that show states only.

use deckpilot_plugin_sdk::testing::MockHost;
use deckpilot_plugin_sdk::{ActionOutcome, Glyph, Indicator, KeyState, StateOutcome};
use serde_json::{Value, json};

use crate::host::{CONTROL_ACTIONS, codes, test_host};
use crate::plugin;

fn ok(value: Value) -> Option<Value> {
    Some(json!({"ok": value}))
}

fn error(code: &str, message: &str) -> Option<Value> {
    Some(json!({"error": {"code": code, "message": message}}))
}

fn invoke(action: &str, params: Value) -> ActionOutcome {
    // The SDK's own host answers nothing the plugin asks: every call goes
    // through the raw host call (`host::test_host`).
    let mut host = MockHost::deny_all();
    host.invoke(&plugin(), action, params)
}

fn state_of(action: &str) -> StateOutcome {
    let mut host = MockHost::deny_all();
    host.key_state(&plugin(), action, json!({}))
}

fn failure(outcome: &ActionOutcome) -> (String, String) {
    let value = serde_json::to_value(outcome).unwrap();
    assert_eq!(value["status"], "failed", "{value}");
    (
        value["code"].as_str().unwrap_or_default().to_owned(),
        value["message"].as_str().unwrap_or_default().to_owned(),
    )
}

#[test]
fn every_control_asks_for_exactly_its_action() {
    let expected = [
        json!({"kind": "answer"}),
        json!({"kind": "reject"}),
        json!({"kind": "cancelOutgoing"}),
        json!({"kind": "end"}),
        json!({"kind": "micToggle"}),
        json!({"kind": "micSet", "muted": true}),
        json!({"kind": "micSet", "muted": false}),
        json!({"kind": "cameraToggle"}),
        json!({"kind": "cameraSet", "on": true}),
        json!({"kind": "cameraSet", "on": false}),
    ];
    for ((id, _), action) in CONTROL_ACTIONS.iter().zip(expected) {
        test_host::set(|_| ok(json!("confirmed")));
        let outcome = invoke(id, json!({}));
        let value = serde_json::to_value(&outcome).unwrap();
        assert_eq!(value["data"]["outcome"], "confirmed", "{id}: {value}");
        assert_eq!(
            test_host::sent(),
            vec![json!({"op": "call.control", "args": {"action": action}})],
            "{id}"
        );
    }
}

#[test]
fn a_set_of_the_state_the_call_has_says_so() {
    test_host::set(|_| ok(json!("alreadyThere")));
    let value = serde_json::to_value(invoke("mic-mute", json!({}))).unwrap();
    assert_eq!(value["data"]["outcome"], "alreadyThere");
}

/// Detection-only Teams: every control is refused with the host's code
/// and reason; the plugin never answers success on its own.
#[test]
fn a_refusal_keeps_its_code_and_reason() {
    for (code, message, want) in [
        (
            "failed",
            "calls-unsupported: provider-has-no-api",
            codes::UNSUPPORTED,
        ),
        (
            "failed",
            "calls-not-in-call: no call is in a state this action applies to",
            codes::NOT_IN_CALL,
        ),
        (
            "failed",
            "calls-unconfirmed: the call app did not confirm it in time",
            codes::UNCONFIRMED,
        ),
        (
            "not-permitted",
            "calls-consent: the user has not consented to controlling calls",
            codes::CONSENT,
        ),
        (
            "unavailable",
            "calls-locked: DeckPilot is locked",
            codes::LOCKED,
        ),
        (
            "unavailable",
            "calls-provider-offline: Calls is not running",
            codes::PROVIDER_OFFLINE,
        ),
        // The broker's own refusals carry no Calls code.
        (
            "not-permitted",
            "this runs only in an action the user started",
            codes::CONSENT,
        ),
        // An agent without plugin API 1.16.
        (
            "unsupported-op",
            "unknown operation",
            codes::PROVIDER_OFFLINE,
        ),
    ] {
        test_host::set(move |_| error(code, message));
        let (got, text) = failure(&invoke("end", json!({})));
        assert_eq!(got, want, "{message}");
        assert_eq!(text, message);
    }
    // Throttled by the broker: worth trying again.
    test_host::set(|_| error("throttled", "rate limited"));
    let value = serde_json::to_value(invoke("end", json!({}))).unwrap();
    assert_eq!(value["code"], codes::UNCONFIRMED);
    assert_eq!(value["retryable"], true);
}

#[test]
fn off_the_host_or_on_a_strange_reply_nothing_succeeds() {
    test_host::set(|_| None);
    assert_eq!(
        failure(&invoke("end", json!({}))).0,
        codes::PROVIDER_OFFLINE
    );
    test_host::set(|_| ok(json!("done")));
    assert_eq!(
        failure(&invoke("end", json!({}))).0,
        codes::PROVIDER_OFFLINE
    );
}

#[test]
fn back_to_work_works_on_the_calls_pages_only() {
    test_host::set(|_| ok(json!("confirmed")));
    let (code, _) = failure(&invoke("return-to-work", json!({})));
    assert_eq!(code, codes::PAGES_ONLY);
    assert!(test_host::sent().is_empty(), "it asks the host nothing");
    // Its parameters are checked like any action's.
    let refused =
        serde_json::to_value(invoke("return-to-work", json!({"mode": "forever"}))).unwrap();
    assert_eq!(refused["status"], "failed");
    assert_ne!(refused["code"], codes::PAGES_ONLY);
}

fn face(action: &str, state: Value) -> KeyState {
    test_host::set(move |request| {
        assert_eq!(request, &json!({"op": "call.state", "args": {}}));
        ok(state.clone())
    });
    match state_of(action) {
        StateOutcome::Ok(face) => face,
        other => panic!("{other:?}"),
    }
}

fn all_supported() -> Value {
    CONTROL_ACTIONS
        .iter()
        .map(|(id, _)| ((*id).to_owned(), json!({"supported": true})))
        .collect::<serde_json::Map<_, _>>()
        .into()
}

#[test]
fn no_call_shows_nothing() {
    assert_eq!(
        face("mic-toggle", json!({"state": "idle"})),
        KeyState::new()
    );
    assert_eq!(face("end", json!({"state": "ended"})), KeyState::new());
}

/// Teams: a call, the microphone held (mute unknown) and every control
/// refused: the keys show a cross, never a microphone state.
#[test]
fn an_unsupported_control_shows_a_cross_and_no_state() {
    let teams = json!({
        "state": "active",
        "provider": "os-signals",
        "connected": true,
        "controls": {
            "mic-toggle": {"supported": false, "reason": "provider-has-no-api"},
            "end": {"supported": false, "reason": "provider-has-no-api"},
        },
        "title": "never shown",
    });
    for id in ["mic-toggle", "end", "camera-off"] {
        let shown = face(id, teams.clone());
        assert_eq!(shown.glyph, Some(Glyph::Cross), "{id}");
        assert_eq!(shown.toggle, None, "{id}");
        assert_eq!(shown.label, None, "{id}: no text of the call");
        assert_eq!(shown.detail, None, "{id}");
    }
}

#[test]
fn the_microphone_and_camera_show_only_what_the_app_reported() {
    let mut state =
        json!({"state": "active", "connected": true, "micMuted": true, "cameraOn": false});
    state["controls"] = all_supported();
    let mic = face("mic-toggle", state.clone());
    assert_eq!((mic.toggle, mic.glyph), (Some(false), None));
    assert_eq!(face("camera-toggle", state.clone()).toggle, Some(false));
    state["micMuted"] = Value::Null;
    assert_eq!(
        face("mic-mute", state.clone()).toggle,
        None,
        "unknown stays unknown"
    );
    state["pending"] = json!(true);
    assert_eq!(
        face("mic-toggle", state.clone()).indicator,
        Some(Indicator::Running)
    );
    state["pending"] = json!(false);
    state["unconfirmed"] = json!(true);
    assert_eq!(
        face("mic-toggle", state).indicator,
        Some(Indicator::Warning)
    );
}

#[test]
fn a_state_the_host_refuses_is_a_failed_state() {
    test_host::set(|_| error("not-permitted", "calls-consent: no"));
    assert!(
        matches!(state_of("end"), StateOutcome::Error(_)),
        "{:?}",
        state_of("end")
    );
}
