//! Native unit tests: the definition agrees with `manifest.json`, and both
//! actions behave with the agent replaced by a scripted `MockHost`.
//! `tools/first-party.sh` adds what a unit test cannot: the manifest against
//! the schema, the Polish texts, the wasm build and the package checks.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use deckpilot_plugin_sdk::testing::{MockHost, manifest_mismatches};
use deckpilot_plugin_sdk::{ActionOutcome, CallResult, CapabilityRequest, ParamValue, codes};
use serde_json::{Value, json};

use super::*;

const MANIFEST: &str = include_str!("../manifest.json");

fn manifest() -> Value {
    serde_json::from_str(MANIFEST).expect("manifest.json is JSON")
}

type Store = Rc<RefCell<BTreeMap<String, Value>>>;

/// An agent with the given settings and an in-memory storage.
fn host(settings: Value, store: &Store) -> MockHost {
    let store = Rc::clone(store);
    MockHost::new(move |request| match request {
        CapabilityRequest::SettingsGet(_) => CallResult::Ok(settings.clone()),
        CapabilityRequest::StorageGet(k) => {
            CallResult::Ok(store.borrow().get(&k.key).cloned().unwrap_or(Value::Null))
        }
        CapabilityRequest::StorageSet(s) => {
            store.borrow_mut().insert(s.key.clone(), s.value.clone());
            CallResult::Ok(Value::Null)
        }
        CapabilityRequest::StorageDelete(k) => {
            store.borrow_mut().remove(&k.key);
            CallResult::Ok(Value::Null)
        }
        _ => CallResult::Ok(Value::Null),
    })
}

#[test]
fn definition_is_valid_and_matches_the_manifest() {
    plugin().validate().expect("the definition is valid");
    assert_eq!(
        manifest_mismatches(&plugin(), &manifest()),
        Vec::<String>::new()
    );
}

#[test]
fn the_manifest_declares_what_the_code_uses() {
    let m = manifest();
    assert_eq!(m["id"], ID);
    assert_eq!(m["version"], VERSION);
    assert_eq!(m["publisher"]["id"], "deckpilot");
    assert_eq!(
        m["permissions"],
        json!([]),
        "the template needs no permission"
    );
    let declared = m["contributes"]["errorCodes"]
        .as_object()
        .expect("errorCodes");
    for code in PLUGIN_CODES {
        assert!(declared.contains_key(*code), "{code} is not in errorCodes");
    }
    for code in declared.keys() {
        assert!(!codes::is_shared(code), "{code} redeclares a shared code");
    }
    assert_eq!(m["contributes"]["variables"][0]["id"], COUNT_VARIABLE);
}

#[test]
fn controls_need_api_1_3_and_reset_stays_on_keys() {
    let m = manifest();
    assert_eq!(m["engines"]["pluginApi"], format!("^{API}"));
    let actions = m["contributes"]["actions"].as_array().expect("actions");
    assert_eq!(
        actions[0]["controls"],
        json!({ "key": ["press"], "encoder": ["rotate-clockwise"] })
    );
    // No declaration: keys only, the plugin API default.
    assert!(actions[1].get("controls").is_none());
    let reset = plugin();
    let reset = reset.actions().iter().find(|a| a.id() == "reset");
    assert_eq!(reset.and_then(|a| a.declared_controls()), None);
    // The same definition below plugin API 1.3 is refused.
    let below = plugin().requires_api(ApiLevel { major: 1, minor: 2 });
    assert!(below.validate().is_err());
}

#[test]
fn navigation_opens_a_declared_view_with_a_bundled_svg_icon() {
    let m = manifest();
    let entry = &m["contributes"]["navigation"][0];
    assert_eq!(entry["icon"], "assets/nav-counter.svg");
    assert_eq!(
        m["contributes"]["views"],
        json!([{ "id": entry["view"], "kind": "variables" }])
    );
    let svg = include_str!("../assets/nav-counter.svg");
    assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
    for refused in [
        "<script",
        "foreignObject",
        " on",
        "href",
        "url(",
        "<!DOCTYPE",
    ] {
        assert!(!svg.contains(refused), "the icon contains {refused}");
    }
}

#[test]
fn icon_is_a_128px_png() {
    let png = include_bytes!("../assets/icon-128.png");
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let side = |at: usize| u32::from_be_bytes([png[at], png[at + 1], png[at + 2], png[at + 3]]);
    assert_eq!((side(16), side(20)), (128, 128));
}

#[test]
fn count_adds_keeps_and_publishes() {
    let store = Store::default();
    let mut host = host(json!({ "label": "Presses" }), &store);
    let out = host.invoke(&plugin(), "count", json!({}));
    let ActionOutcome::Ok { message, data } = out else {
        panic!("{out:?}")
    };
    assert_eq!(message.as_deref(), Some("Presses: 1"));
    assert_eq!(data["count"], ParamValue::Integer(1));

    let out = host.invoke(&plugin(), "count", json!({ "step": 5 }));
    assert!(
        matches!(&out, ActionOutcome::Ok { message, .. } if message.as_deref() == Some("Presses: 6"))
    );
    assert_eq!(store.borrow()["count"], json!(6));
    let published: Vec<&Value> = host
        .requests()
        .iter()
        .filter_map(|r| match r {
            CapabilityRequest::VariablesPublish(v) if v.id == COUNT_VARIABLE => Some(&v.value),
            _ => None,
        })
        .collect();
    assert_eq!(published, [&json!(1), &json!(6)]);
}

#[test]
fn reset_clears_the_counter() {
    let store = Store::default();
    store.borrow_mut().insert("count".into(), json!(41));
    let mut host = host(json!({}), &store);
    let out = host.invoke(&plugin(), "reset", json!({}));
    assert!(matches!(out, ActionOutcome::Ok { .. }), "{out:?}");
    assert!(store.borrow().is_empty());
    let out = host.invoke(&plugin(), "count", json!({}));
    assert!(
        matches!(&out, ActionOutcome::Ok { message, .. } if message.as_deref() == Some("Count: 1"))
    );
}

#[test]
fn a_full_counter_is_a_typed_error_and_changes_nothing() {
    let store = Store::default();
    store.borrow_mut().insert("count".into(), json!(i64::MAX));
    let mut host = host(json!({}), &store);
    let out = host.invoke(&plugin(), "count", json!({}));
    assert!(
        matches!(&out, ActionOutcome::Failed { code, .. } if code == COUNTER_FULL),
        "{out:?}"
    );
    assert_eq!(store.borrow()["count"], json!(i64::MAX));
}

#[test]
fn bad_parameters_and_a_denied_host_fail_cleanly() {
    let store = Store::default();
    let mut ok = host(json!({}), &store);
    for params in [
        json!({ "step": 0 }),
        json!({ "step": 101 }),
        json!({ "step": "2" }),
    ] {
        let out = ok.invoke(&plugin(), "count", params);
        assert!(
            matches!(&out, ActionOutcome::Failed { code, .. } if code == codes::INVALID_PARAMETERS),
            "{out:?}"
        );
    }
    assert!(store.borrow().is_empty());
    // A disabled plugin is denied everything: no panic, a shared code.
    let mut denied = MockHost::deny_all();
    let out = denied.invoke(&plugin(), "count", json!({}));
    assert!(
        matches!(&out, ActionOutcome::Failed { code, .. } if code == codes::NOT_PERMITTED),
        "{out:?}"
    );
}
