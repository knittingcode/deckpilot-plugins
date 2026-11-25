//! The manifest, the locales, the README and the module agree, and the
//! manifest keeps the Calls rules: the same actions and parameters, every
//! returned code declared, both locales with the same keys, the layout
//! templates DeckPilot's own (`deckpilot-calls::layouts::builtin_templates`),
//! no way out of the agent, and nothing that claims a Teams control.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use crate::host::{CONTROL_ACTIONS, codes};
use crate::{ID, VERSION, plugin};

const MANIFEST: &str = include_str!("../manifest.json");
const EN: &str = include_str!("../locales/en.json");
const PL: &str = include_str!("../locales/pl.json");
const README: &str = include_str!("../README.md");

fn manifest() -> Value {
    serde_json::from_str(MANIFEST).unwrap()
}

fn actions(m: &Value) -> &Vec<Value> {
    m["contributes"]["actions"].as_array().unwrap()
}

#[test]
fn ids_versions_and_the_api_level_agree() {
    let m = manifest();
    assert_eq!(m["id"], ID);
    assert_eq!(m["version"], VERSION);
    assert_eq!(m["engines"]["pluginApi"], "^1.16");
    assert_eq!(plugin().id(), ID);
    assert_eq!(plugin().version(), VERSION);
}

#[test]
fn actions_and_parameters_agree() {
    let m = manifest();
    let declared: Vec<(String, Vec<String>)> = actions(&m)
        .iter()
        .map(|a| {
            (
                a["id"].as_str().unwrap().to_owned(),
                a["parameters"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| p["key"].as_str().unwrap().to_owned())
                    .collect(),
            )
        })
        .collect();
    let built: Vec<(String, Vec<String>)> = plugin()
        .actions()
        .iter()
        .map(|a| {
            (
                a.id().to_owned(),
                a.params().iter().map(|p| p.key().to_owned()).collect(),
            )
        })
        .collect();
    assert_eq!(declared, built);
    // Every control action of DeckPilot's gate, and "Back to work".
    let mut ids: Vec<&str> = CONTROL_ACTIONS.iter().map(|(id, _)| *id).collect();
    ids.push("return-to-work");
    let declared_ids: Vec<&str> = actions(&m)
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    assert_eq!(declared_ids, ids);
}

/// The parameters of "Back to work" are those DeckPilot's driver reads
/// (`driver::settings_of`), with its defaults: mode C with 60 s, 30 s for
/// a timer, resume after a failure, stay after a cancellation.
#[test]
fn back_to_work_takes_the_drivers_parameters() {
    let m = manifest();
    let back = actions(&m)
        .iter()
        .find(|a| a["id"] == "return-to-work")
        .unwrap();
    let params: Vec<(String, Value, Value)> = back["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["key"].as_str().unwrap().to_owned(),
                p["kind"].clone(),
                p["default"]["value"].clone(),
            )
        })
        .collect();
    assert_eq!(
        params,
        vec![
            (
                "mode".to_owned(),
                json!({"type": "choice", "options": ["timer", "completion", "completionOrTimeout"]}),
                json!("completionOrTimeout")
            ),
            (
                "seconds".to_owned(),
                json!({"type": "integer", "min": 5, "max": 600}),
                json!(30)
            ),
            (
                "maxSeconds".to_owned(),
                json!({"type": "integer", "min": 10, "max": 1800}),
                json!(60)
            ),
            (
                "onFailure".to_owned(),
                json!({"type": "choice", "options": ["resume", "stay"]}),
                json!("resume")
            ),
            (
                "onCancel".to_owned(),
                json!({"type": "choice", "options": ["resume", "stay"]}),
                json!("stay")
            ),
            (
                "returnAllDevices".to_owned(),
                json!({"type": "boolean"}),
                json!(false)
            ),
            (
                "countNavigation".to_owned(),
                json!({"type": "boolean"}),
                json!(false)
            ),
        ]
    );
    assert_eq!(
        back["requiresPermissions"],
        Value::Null,
        "it controls no call"
    );
}

/// Every control asks for `calls:control` (optional: without it the plugin
/// still shows calls), has a key state, and runs from keys, gestures and
/// workflows.
#[test]
fn controls_need_the_control_consent() {
    let m = manifest();
    for (id, _) in CONTROL_ACTIONS {
        let action = actions(&m).iter().find(|a| a["id"] == id).unwrap();
        assert_eq!(
            action["requiresPermissions"],
            json!(["calls:control"]),
            "{id}"
        );
        assert_eq!(action["state"]["refresh"], "every", "{id}");
        assert_eq!(
            action["contexts"],
            json!(["key", "gesture", "workflow"]),
            "{id}"
        );
        assert_eq!(action["parameters"], json!([]), "{id}");
    }
}

/// `calls` with control and key state only: no network, credentials,
/// opening pages, tools, resources or connections, so what the plugin
/// learns of a call cannot leave the agent.
#[test]
fn no_way_out_of_the_agent() {
    let m = manifest();
    let kinds: Vec<&str> = m["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["calls", "device"]);
    assert_eq!(m["permissions"][0]["control"], true);
    assert_eq!(m["permissions"][1]["state"], true);
}

#[test]
fn every_returned_code_is_declared() {
    let m = manifest();
    let declared: BTreeSet<&str> = m["contributes"]["errorCodes"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(declared, codes::ALL.into_iter().collect());
}

/// The templates are DeckPilot's built-in ones
/// (`deckpilot-calls/src/layouts.rs`, `builtin_templates`), one per class
/// shape, and the overlay has the three pages.
#[test]
fn the_templates_are_deckpilots() {
    let m = manifest();
    assert_eq!(
        m["contributes"]["callOverlay"],
        json!({"pages": ["incoming", "outgoing", "active"]})
    );
    let key = |i: u8, a: &str| json!({"control": "key", "index": i, "action": a});
    let dial = |i: u8, a: &str| json!({"control": "dial", "index": i, "action": a});
    let incoming = |a: u8, r: u8| json!({"page": "incoming", "controls": [key(a, "answer"), key(r, "reject")]});
    let outgoing = |c: u8| json!({"page": "outgoing", "controls": [key(c, "cancel-outgoing")]});
    let active = |mic: u8, cam: u8, end: u8, back: u8| {
        json!({"page": "active", "controls": [
            key(mic, "mic-toggle"), key(cam, "camera-toggle"), key(end, "end"), key(back, "return-to-work")
        ]})
    };
    let expected = json!([
        {"shape": "grid-small", "version": 1, "pages": [incoming(0, 2), outgoing(2), active(0, 1, 2, 3)]},
        {"shape": "grid", "version": 1, "pages": [incoming(6, 8), outgoing(7), active(5, 6, 7, 10)]},
        {"shape": "grid-large", "version": 1, "pages": [incoming(10, 13), outgoing(12), active(10, 11, 12, 24)]},
        {"shape": "keys-dials", "version": 1, "pages": [incoming(0, 3), outgoing(3), {"page": "active", "controls": [
            dial(0, "mic-toggle"), dial(1, "camera-toggle"), key(3, "end"), key(4, "return-to-work")
        ]}]},
        {"shape": "no-display", "version": 1, "pages": [{"page": "active", "controls": [key(0, "mic-toggle")]}]},
    ]);
    assert_eq!(m["contributes"]["layoutTemplates"], expected);
    // Every placed action is one of the plugin's.
    let ids: BTreeSet<&str> = actions(&m)
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    for template in expected.as_array().unwrap() {
        for page in template["pages"].as_array().unwrap() {
            for control in page["controls"].as_array().unwrap() {
                assert!(ids.contains(control["action"].as_str().unwrap()));
            }
        }
    }
}

/// Every `t:` key of the manifest, from its strings.
fn t_keys(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::String(s) => {
            if let Some(key) = s.strip_prefix("t:") {
                out.insert(key.to_owned());
            }
        }
        Value::Array(items) => items.iter().for_each(|v| t_keys(v, out)),
        Value::Object(map) => map.values().for_each(|v| t_keys(v, out)),
        _ => {}
    }
}

/// The flattened keys of a locale file.
fn flat(prefix: &str, value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flat(&key, v, out);
            }
        }
        Value::String(text) => {
            assert!(!text.trim().is_empty(), "{prefix} is empty");
            out.insert(prefix.to_owned());
        }
        other => panic!("{prefix}: not a text: {other}"),
    }
}

#[test]
fn both_locales_have_every_text_and_nothing_else() {
    let mut used = BTreeSet::new();
    t_keys(&manifest(), &mut used);
    for (name, text) in [("en", EN), ("pl", PL)] {
        let mut keys = BTreeSet::new();
        flat("", &serde_json::from_str(text).unwrap(), &mut keys);
        assert_eq!(keys, used, "{name}");
    }
}

/// Nothing claims that Teams can be controlled: the texts say "where the
/// call app allows it" and that Teams can only be detected, and the README
/// lists every Teams control as unsupported, with the reason.
#[test]
fn nothing_claims_a_teams_control() {
    let en: Value = serde_json::from_str(EN).unwrap();
    let pl: Value = serde_json::from_str(PL).unwrap();
    assert!(
        en["plugin"]["description"]
            .as_str()
            .unwrap()
            .contains("Microsoft Teams can only be detected")
    );
    assert!(
        pl["plugin"]["description"]
            .as_str()
            .unwrap()
            .contains("Microsoft Teams można tylko wykryć")
    );
    for (id, _) in CONTROL_ACTIONS {
        let en = en["actions"][id]["description"].as_str().unwrap();
        let pl = pl["actions"][id]["description"].as_str().unwrap();
        assert!(en.contains("where the call app allows it"), "{id}: {en}");
        assert!(
            pl.contains("jeśli aplikacja do rozmów na to pozwala"),
            "{id}: {pl}"
        );
    }
    // The capability table: every control action's column (by the
    // header), and in the Teams row each one says unsupported.
    let cells = |line: &str| -> Vec<String> {
        line.trim()
            .trim_matches('|')
            .split('|')
            .map(|c| c.trim().to_owned())
            .collect()
    };
    let header = cells(
        README
            .lines()
            .find(|l| l.starts_with("| Call app"))
            .expect("the README's capability table"),
    );
    let teams = cells(
        README
            .lines()
            .find(|l| l.starts_with("| Microsoft Teams"))
            .expect("the README's Teams row"),
    );
    assert_eq!(header.len(), teams.len(), "{teams:?}");
    let column = |id: &str| match id {
        "answer" => "Answer",
        "reject" => "Reject",
        "cancel-outgoing" => "Cancel",
        "end" => "Leave",
        "mic-toggle" | "mic-mute" | "mic-unmute" => "Mute",
        "camera-toggle" | "camera-on" | "camera-off" => "Camera",
        other => panic!("no column for {other}"),
    };
    for (id, _) in CONTROL_ACTIONS {
        let at = header
            .iter()
            .position(|h| h == column(id))
            .unwrap_or_else(|| panic!("{id}: no `{}` column", column(id)));
        assert!(
            teams[at].starts_with("unsupported"),
            "{id}: the Teams cell is `{}`",
            teams[at]
        );
    }
    assert!(README.contains("2026-06-30"));
}
