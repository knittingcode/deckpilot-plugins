//! The manifest and the module agree: the same actions and parameter
//! keys, the internal actions marked, every error code the module returns
//! declared, both locales with the same keys, and no token-shaped text
//! anywhere in the package's own files.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::errors::codes;
use crate::{EXTENSIONS, ID, INTERNAL, VERSION, plugin};

const MANIFEST: &str = include_str!("../manifest.json");
const EN: &str = include_str!("../locales/en.json");
const PL: &str = include_str!("../locales/pl.json");

fn manifest() -> Value {
    serde_json::from_str(MANIFEST).unwrap()
}

#[test]
fn ids_and_versions_agree() {
    let m = manifest();
    assert_eq!(m["id"], ID);
    assert_eq!(m["version"], VERSION);
    assert_eq!(m["engines"]["pluginApi"], "^1.14");
    assert_eq!(plugin().id(), ID);
}

#[test]
fn actions_and_parameters_agree() {
    let m = manifest();
    let declared: Vec<(String, Vec<String>)> = m["contributes"]["actions"]
        .as_array()
        .unwrap()
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
    // Every action takes the connection first.
    assert!(declared.iter().all(|(_, params)| params[0] == "connection"));
    let internal: BTreeSet<&str> = m["contributes"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["internal"] == true)
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    assert_eq!(internal, INTERNAL.into_iter().collect());
    let declared: BTreeSet<&str> = m["contributes"]["connectionTypes"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|t| t["extensions"].as_array().unwrap())
        .map(|e| e.as_str().unwrap())
        .collect();
    // The `extension` action offers exactly the read-only extensions, each
    // one a connection type declares.
    let offered: BTreeSet<&str> = m["contributes"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "extension")
        .unwrap()["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["key"] == "id")
        .unwrap()["kind"]["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o.as_str().unwrap())
        .collect();
    assert_eq!(offered, EXTENSIONS.into_iter().collect());
    assert!(offered.is_subset(&declared));
    assert!(
        crate::adapters::gitlab::EXTENSIONS
            .iter()
            .all(|e| offered.contains(e))
    );
}

#[test]
fn modifying_actions_are_marked_and_ask_first() {
    let m = manifest();
    for action in m["contributes"]["actions"].as_array().unwrap() {
        let id = action["id"].as_str().unwrap();
        let effect = action["node"]["effect"].as_str().unwrap();
        let modifies = matches!(id, "create-change-request" | "trigger-pipeline");
        assert_eq!(effect == "modifies", modifies, "{id}");
        if modifies {
            assert_eq!(action["node"]["retrySafe"], Value::Null, "{id}");
            let confirm = action["parameters"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["key"] == "confirm")
                .unwrap();
            assert_eq!(confirm["default"]["value"], true, "{id}");
        }
        if action["internal"] == true {
            assert_eq!(effect, "readOnly", "{id}");
        }
    }
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
    let own: BTreeSet<&str> = codes::DECLARED.iter().copied().collect();
    assert_eq!(declared, own);
}

fn keys(value: &Value, prefix: &str, out: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                let path = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                keys(v, &path, out);
            }
        }
        _ => {
            out.insert(prefix.to_owned());
        }
    }
}

#[test]
fn both_locales_have_every_key_the_manifest_names() {
    let mut named = BTreeSet::new();
    fn walk(value: &Value, out: &mut BTreeSet<String>) {
        match value {
            Value::String(s) => {
                if let Some(key) = s.strip_prefix("t:") {
                    out.insert(key.to_owned());
                }
            }
            Value::Array(items) => items.iter().for_each(|v| walk(v, out)),
            Value::Object(map) => map.values().for_each(|v| walk(v, out)),
            _ => {}
        }
    }
    walk(&manifest(), &mut named);
    for (name, text) in [("en", EN), ("pl", PL)] {
        let mut have = BTreeSet::new();
        keys(&serde_json::from_str(text).unwrap(), "", &mut have);
        assert_eq!(have, named, "{name}");
    }
}

#[test]
fn no_token_shaped_text_in_the_package() {
    for (name, text) in [("manifest", MANIFEST), ("en", EN), ("pl", PL)] {
        for marker in [
            "ghp_",
            "github_pat_",
            "glpat-",
            "gloas-",
            "glrt-",
            "Bearer ",
        ] {
            assert!(!text.contains(marker), "{name} contains {marker}");
        }
    }
}
