//! The contract with DeckPilot: this plugin moved here from DeckPilot's
//! `plugins/first-party/<name>` and must stay a drop-in replacement of what
//! DeckPilot bundles. DeckPilot no longer has its source: it bundles the
//! signed release and keeps that release's files but the module as the
//! contract, `plugins/first-party/contracts/deckpilot.<name>/` (DeckPilot
//! #392). Our manifest is compared with that contract at the commit pinned
//! in `tools/deckpilot.env` (the checkout `tools/build-cli.sh` makes in
//! `.work/deckpilot`): the same id, the same action ids in the same order
//! with the same parameters, the same settings, credential slots, error
//! codes and permissions, so every stored `<id>.<action>` key, workflow node
//! and setting keeps working. Only `version` may differ (equal while this
//! is the release DeckPilot bundles, newer for the next one, never older),
//! and `repository` (the source lives here).
//!
//! A pinned DeckPilot without this contract fails the test: the comparison
//! is never skipped.

use std::path::{Path, PathBuf};

use serde_json::Value;

const MANIFEST: &str = include_str!("../manifest.json");
const ENV: &str = include_str!("../../../tools/deckpilot.env");

fn name() -> &'static str {
    env!("CARGO_PKG_NAME")
        .strip_prefix("deckpilot-")
        .expect("a deckpilot-<name> crate")
}

fn pinned_commit() -> &'static str {
    ENV.lines()
        .find_map(|line| line.strip_prefix("DECKPILOT_COMMIT="))
        .expect("tools/deckpilot.env has DECKPILOT_COMMIT")
        .trim()
}

/// The contract's directory: `plugins/first-party/contracts/<id>` of the
/// pinned checkout, checked to be the pinned commit.
fn contract_dir() -> PathBuf {
    let work = std::env::var_os("DECKPILOT_PLUGINS_WORK")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.work"));
    let checkout = work.join("deckpilot");
    let head = std::fs::read_to_string(checkout.join(".git/HEAD")).unwrap_or_else(|e| {
        panic!(
            "no pinned DeckPilot checkout at {} ({e}): run tools/build-cli.sh",
            checkout.display()
        )
    });
    assert_eq!(
        head.trim(),
        pinned_commit(),
        "{} is not DeckPilot at the pinned commit: run tools/build-cli.sh",
        checkout.display()
    );
    checkout.join(format!(
        "plugins/first-party/contracts/deckpilot.{}",
        name()
    ))
}

/// DeckPilot's contract manifest of this plugin at the pinned commit.
fn deckpilot_manifest() -> Value {
    let path = contract_dir().join("manifest.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{} ({e}): the pinned DeckPilot has no contract of this plugin \
             (plugins/first-party/contracts/deckpilot.<name>/)",
            path.display()
        )
    });
    serde_json::from_str(&text).unwrap()
}

fn ours() -> Value {
    serde_json::from_str(MANIFEST).unwrap()
}

fn list<'a>(manifest: &'a Value, pointer: &str) -> &'a [Value] {
    manifest
        .pointer(pointer)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn keys(items: &[Value], field: &str) -> Vec<String> {
    items
        .iter()
        .map(|item| item[field].as_str().unwrap_or_default().to_owned())
        .collect()
}

fn version(manifest: &Value) -> (u64, u64, u64) {
    let text = manifest["version"].as_str().unwrap();
    let mut parts = text.split('.').map(|p| p.parse::<u64>().unwrap());
    let v = (
        parts.next().unwrap(),
        parts.next().unwrap(),
        parts.next().unwrap(),
    );
    assert!(parts.next().is_none(), "{text} is not x.y.z");
    v
}

#[test]
fn same_id_as_deckpilot() {
    assert_eq!(ours()["id"], deckpilot_manifest()["id"]);
    assert_eq!(ours()["id"], format!("deckpilot.{}", name()));
}

#[test]
fn same_actions_and_parameters_as_deckpilot() {
    let (ours, theirs) = (ours(), deckpilot_manifest());
    let (a, b) = (
        list(&ours, "/contributes/actions"),
        list(&theirs, "/contributes/actions"),
    );
    assert_eq!(keys(a, "id"), keys(b, "id"), "action ids");
    for (a, b) in a.iter().zip(b) {
        let id = &a["id"];
        assert_eq!(
            keys(list(a, "/parameters"), "key"),
            keys(list(b, "/parameters"), "key"),
            "parameter keys of {id}"
        );
        assert_eq!(a["parameters"], b["parameters"], "parameters of {id}");
        assert_eq!(a, b, "action {id}");
    }
}

#[test]
fn same_settings_credentials_and_permissions_as_deckpilot() {
    let (ours, theirs) = (ours(), deckpilot_manifest());
    assert_eq!(
        keys(list(&ours, "/contributes/settings/fields"), "key"),
        keys(list(&theirs, "/contributes/settings/fields"), "key"),
        "setting keys"
    );
    assert_eq!(
        keys(list(&ours, "/contributes/credentials"), "key"),
        keys(list(&theirs, "/contributes/credentials"), "key"),
        "credential slots"
    );
    assert_eq!(ours["contributes"], theirs["contributes"], "contributes");
    assert_eq!(ours["permissions"], theirs["permissions"], "permissions");
}

#[test]
fn only_version_and_repository_differ_from_deckpilot() {
    let (mut ours, mut theirs) = (ours(), deckpilot_manifest());
    assert!(
        version(&ours) >= version(&theirs),
        "{} is older than DeckPilot's contract {}: the source here is the bundled release or a newer one",
        ours["version"],
        theirs["version"]
    );
    assert_eq!(
        ours["repository"]["url"],
        "https://github.com/knittingcode/deckpilot-plugins"
    );
    assert_eq!(
        ours["repository"]["directory"],
        format!("official/{}", name())
    );
    for manifest in [&mut ours, &mut theirs] {
        let object = manifest.as_object_mut().unwrap();
        object.remove("version");
        object.remove("repository");
    }
    assert_eq!(ours, theirs);
}

/// Besides the manifest and the module, what a bundle takes from a package
/// (the pins of DeckPilot's `build.sh`): the same files as the contract,
/// byte for byte.
#[test]
fn same_shipped_files_as_deckpilot() {
    fn shipped(base: &Path) -> Vec<(String, Vec<u8>)> {
        let mut files = vec![(
            "LICENSE".to_owned(),
            std::fs::read(base.join("LICENSE")).unwrap(),
        )];
        for dir in ["assets", "locales"] {
            let Ok(entries) = std::fs::read_dir(base.join(dir)) else {
                continue;
            };
            for entry in entries {
                let path = entry.unwrap().path();
                let rel = format!("{dir}/{}", path.file_name().unwrap().to_str().unwrap());
                files.push((rel, std::fs::read(&path).unwrap()));
            }
        }
        files.sort();
        files
    }
    deckpilot_manifest(); // the contract exists at the pinned commit
    let theirs = contract_dir();
    let (a, b) = (
        shipped(Path::new(env!("CARGO_MANIFEST_DIR"))),
        shipped(&theirs),
    );
    let names = |f: &[(String, Vec<u8>)]| f.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>();
    assert_eq!(names(&a), names(&b), "shipped files");
    for ((name, ours), (_, theirs)) in a.iter().zip(&b) {
        assert!(ours == theirs, "{name} differs from DeckPilot's");
    }
}
