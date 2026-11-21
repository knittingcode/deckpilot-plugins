//! Native unit tests: the module has no actions, as its manifest, which is
//! valid, declares only the `workflows` host view, its navigation entry and
//! the low-tier `hostViews` permission, translated into English and
//! Polish. The contract (id, view, navigation entry, permission, texts) is
//! the one of DeckPilot's `plugins/first-party/workflows` 0.1.0; the run
//! through the real agent is DeckPilot's
//! `apps/agent/tests/plugins_workflows.rs`.

use std::collections::BTreeSet;

use deckpilot_plugin_sdk::ActionOutcome;
use deckpilot_plugin_sdk::testing::{MockHost, manifest_mismatches};
use serde_json::{Value, json};

use super::*;

const MANIFEST: &str = include_str!("../manifest.json");
const EN: &str = include_str!("../locales/en.json");
const PL: &str = include_str!("../locales/pl.json");
const SCHEMA: &str = include_str!("../../../schemas/plugin-manifest.schema.json");

fn manifest() -> Value {
    serde_json::from_str(MANIFEST).unwrap()
}

#[test]
fn the_definition_matches_the_manifest_and_has_no_actions() {
    assert_eq!(
        manifest_mismatches(&plugin(), &manifest()),
        Vec::<String>::new()
    );
    plugin().validate().unwrap();
    assert!(plugin().actions().is_empty());
    assert!(manifest()["contributes"].get("actions").is_none());
    // Nothing to invoke.
    let mut host = MockHost::deny_all();
    assert!(matches!(
        host.invoke(&plugin(), "run", json!({})),
        ActionOutcome::Failed { .. }
    ));
    assert!(host.requests().is_empty());
}

#[test]
fn the_manifest_declares_only_the_workflows_screen() {
    let schema: Value = serde_json::from_str(SCHEMA).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let errors: Vec<String> = validator
        .iter_errors(&manifest())
        .map(|e| format!("{}: {e}", e.instance_path()))
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    let manifest = manifest();
    assert_eq!(manifest["engines"]["pluginApi"], "^1.9");
    assert_eq!(
        manifest["permissions"],
        json!([{"kind": "hostViews", "justification": "t:permissions.hostViews"}])
    );
    assert_eq!(
        manifest["contributes"]["views"],
        json!([{"id": "workflows", "kind": "workflows", "context": "profile"}])
    );
    let navigation = &manifest["contributes"]["navigation"];
    assert_eq!(navigation.as_array().unwrap().len(), 1);
    assert_eq!(navigation[0]["view"], "workflows");
    // No status items, dashboard cards, variables or settings.
    let keys: BTreeSet<&str> = manifest["contributes"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, BTreeSet::from(["navigation", "views"]));
}

/// What DeckPilot's app and agent rely on, exactly as DeckPilot's
/// `plugins/first-party/workflows/manifest.json` 0.1.0 declares it: the
/// plugin is found by its id, the screen by its view kind `workflows`, and
/// the navigation entry by its id. Only the version and the repository
/// differ since the move.
#[test]
fn the_contract_is_deckpilots() {
    let manifest = manifest();
    assert_eq!(manifest["id"], ID);
    assert_eq!(manifest["version"], VERSION);
    assert_eq!(manifest["publisher"]["id"], "deckpilot");
    assert_eq!(
        manifest["engines"],
        json!({"deckpilot": ">=0.10.0 <1.0.0", "pluginApi": "^1.9", "sdk": "0.1.0"})
    );
    assert_eq!(manifest["runtime"], json!({"kind": "wasm", "abi": 1}));
    assert_eq!(manifest["defaultLocale"], "en");
    assert_eq!(manifest["locales"], json!(["en", "pl"]));
    assert_eq!(manifest["icons"], json!({"128": "assets/icon-128.png"}));
    assert_eq!(
        manifest["contributes"],
        json!({
            "navigation": [{
                "id": "workflows",
                "label": "t:navigation.workflows",
                "icon": "assets/icon-128.png",
                "view": "workflows"
            }],
            "views": [{"id": "workflows", "kind": "workflows", "context": "profile"}]
        })
    );
    assert_eq!(
        manifest["repository"],
        json!({
            "type": "git",
            "url": "https://github.com/knittingcode/deckpilot-plugins",
            "directory": "official/workflows"
        })
    );
}

fn flat(text: &str) -> BTreeSet<String> {
    fn walk(prefix: &str, value: &Value, out: &mut BTreeSet<String>) {
        match value {
            Value::Object(map) => {
                for (k, v) in map {
                    let key = if prefix.is_empty() {
                        k.clone()
                    } else {
                        format!("{prefix}.{k}")
                    };
                    walk(&key, v, out);
                }
            }
            Value::String(_) => {
                out.insert(prefix.to_owned());
            }
            other => panic!("{prefix}: {other}"),
        }
    }
    let mut out = BTreeSet::new();
    walk("", &serde_json::from_str(text).unwrap(), &mut out);
    out
}

#[test]
fn every_text_is_translated_into_english_and_polish() {
    let (en, pl) = (flat(EN), flat(PL));
    assert_eq!(en, pl);
    let used: BTreeSet<String> = MANIFEST
        .split('"')
        .filter_map(|s| s.strip_prefix("t:"))
        .map(str::to_owned)
        .collect();
    assert_eq!(used, en);
}
