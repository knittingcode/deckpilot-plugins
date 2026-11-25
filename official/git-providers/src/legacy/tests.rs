//! The legacy actions against their sources: the contract (every legacy
//! action is its old action, exactly, plus the injected `connection`),
//! the parity coverage (every old unit test runs through the legacy
//! actions, byte for byte, or is replaced for a stated reason), and
//! read-only access without credentials.
//!
//! The sources are the old plugins' manifests in this repository
//! (`official/github/manifest.json`, `official/gitlab/manifest.json`),
//! which `tools/check-official.py --deckpilot` proves equal to the
//! contracts DeckPilot bundles at the pin
//! (`plugins/first-party/contracts/deckpilot.{github,gitlab}/manifest.json`,
//! `version` and `repository` aside).

use std::collections::BTreeSet;

use deckpilot_plugin_sdk::testing::manifest_mismatches;
use serde_json::{Value, json};

use super::{ACTIONS, CODES};

const MANIFEST: &str = include_str!("../../manifest.json");
const GITHUB: &str = include_str!("../../../github/manifest.json");
const GITLAB: &str = include_str!("../../../gitlab/manifest.json");
const GITHUB_TESTS: &str = include_str!("../../../github/src/tests.rs");
const GITLAB_TESTS: &str = include_str!("../../../gitlab/src/tests.rs");
const PARITY_GITHUB: &str = include_str!("parity_github.rs");
const PARITY_GITLAB: &str = include_str!("parity_gitlab.rs");

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn source(plugin: &str) -> Value {
    match plugin {
        "deckpilot.github" => json(GITHUB),
        "deckpilot.gitlab" => json(GITLAB),
        other => panic!("{other}"),
    }
}

fn action<'a>(manifest: &'a Value, id: &str) -> &'a Value {
    manifest["contributes"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == id)
        .unwrap_or_else(|| panic!("no action {id}"))
}

/// What a legacy action must be: its source action, with the legacy id,
/// the injected `connection` first and `deprecated: true`. Nothing else
/// differs: names, descriptions, category, every parameter (key, label,
/// description, kind, bounds, options, default, required), every output
/// (key, label, kind, `always`, `sensitive`) and the node (version, icon,
/// time limit, retry-safety, idempotence, cancellation, effect).
fn expected(id: &str, plugin: &str, old: &str) -> Value {
    let source = source(plugin);
    let mut want = action(&source, old).clone();
    let kind = plugin.strip_prefix("deckpilot.").unwrap();
    want["id"] = json!(id);
    let mut parameters = vec![json!({
        "key": "connection",
        "label": "t:params.connection",
        "kind": {"type": "connection", "types": [kind]},
        "required": true
    })];
    parameters.extend(want["parameters"].as_array().unwrap().iter().cloned());
    want["parameters"] = Value::Array(parameters);
    want["deprecated"] = json!(true);
    want
}

#[test]
fn every_legacy_action_is_its_source_action() {
    let m = json(MANIFEST);
    for (id, plugin, old) in ACTIONS {
        assert_eq!(*action(&m, id), expected(id, plugin, old), "{id}");
        let a = action(&m, id);
        assert_eq!(a.get("internal"), None, "{id}: routed, so not internal");
    }
    // Every source action has one, and nothing else is deprecated.
    for plugin in ["deckpilot.github", "deckpilot.gitlab"] {
        let olds: BTreeSet<String> = source(plugin)["contributes"]["actions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["id"].as_str().unwrap().to_owned())
            .collect();
        let wrapped: BTreeSet<String> = ACTIONS
            .iter()
            .filter(|(_, p, _)| *p == plugin)
            .map(|(_, _, old)| (*old).to_owned())
            .collect();
        assert_eq!(olds, wrapped, "{plugin}");
    }
    let legacy: BTreeSet<&str> = ACTIONS.iter().map(|(id, _, _)| *id).collect();
    for a in m["contributes"]["actions"].as_array().unwrap() {
        let id = a["id"].as_str().unwrap();
        assert_eq!(
            a.get("deprecated") == Some(&json!(true)),
            legacy.contains(id),
            "{id}"
        );
        assert_eq!(id.starts_with("legacy-"), legacy.contains(id), "{id}");
    }
}

/// The old `definition_is_valid_and_matches_the_manifest`: the module's
/// definition of each legacy action agrees with the manifest (the SDK's
/// own comparison), as the old plugin's did with its manifest.
#[test]
fn the_definition_of_every_legacy_action_matches_the_manifest() {
    crate::plugin().validate().unwrap();
    let mismatches = manifest_mismatches(&crate::plugin(), &json(MANIFEST));
    // The `connection` kind is plugin API 1.14's, which the SDK pinned
    // here predates (it reads it as text, as the module declares it).
    let legacy: Vec<&String> = mismatches
        .iter()
        .filter(|m| {
            ACTIONS
                .iter()
                .any(|(id, _, _)| m.starts_with(&format!("{id}.")))
        })
        .filter(|m| !m.contains(".connection: kind."))
        .collect();
    assert!(
        mismatches
            .iter()
            .any(|m| m.starts_with("legacy-github-open.connection: kind.type")),
        "the comparison ran"
    );
    assert!(legacy.is_empty(), "{legacy:#?}");
}

/// The old `codes_slots_settings_and_permissions_are_declared`, for what
/// is left of it on a connection: every code an old action returns is
/// declared, with the same meaning. There is no token slot, setting or
/// network permission of its own: the connection is all of them.
#[test]
fn every_old_code_is_declared() {
    let declared: BTreeSet<String> = json(MANIFEST)["contributes"]["errorCodes"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    for (plugin, codes) in [
        ("deckpilot.github", super::github::api::codes::ALL),
        ("deckpilot.gitlab", super::gitlab::api::codes::ALL),
    ] {
        let old: BTreeSet<String> = source(plugin)["contributes"]["errorCodes"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        let ours: BTreeSet<String> = codes.iter().map(|c| (*c).to_owned()).collect();
        assert_eq!(old, ours, "{plugin}: the module's codes are the old ones");
        assert!(old.is_subset(&declared), "{plugin}: {old:?}");
    }
    for code in CODES {
        assert!(declared.contains(code), "{code}");
    }
}

/// `(old test, what replaces it, why)`: the only old unit tests not run
/// unchanged by the parity files.
pub const REPLACED: [(&str, &str, &str); 7] = [
    (
        "definition_is_valid_and_matches_the_manifest",
        "the_definition_of_every_legacy_action_matches_the_manifest",
        "the definition is Git Providers'; compared action by action with its manifest, and \
         every legacy action with its source (every_legacy_action_is_its_source_action)",
    ),
    (
        "manifest_is_valid_against_the_schema",
        "",
        "the manifest is Git Providers' (plugin API 1.14), which the schema pinned here \
         predates; it validates against knittingcode/deckpilot#451's schema",
    ),
    (
        "codes_slots_settings_and_permissions_are_declared",
        "every_old_code_is_declared",
        "no token slot, settings or network permission of its own: the connection is all of \
         them",
    ),
    (
        "a_missing_token_or_an_invalid_api_is_refused_before_any_request",
        "an_invalid_api_is_refused_before_any_request",
        "a connection has its own token, never the plugin's (the host refuses a run without a \
         connection); the message names the connection, not the settings",
    ),
    (
        "the_time_limit_is_counted_with_githubs_date",
        "the_time_limit_is_the_old_default_counted_with_githubs_date",
        "no timeoutSeconds setting: the old default, 20 s",
    ),
    (
        "a_missing_token_or_an_address_that_is_no_api_is_refused_before_any_request",
        "an_address_that_is_no_api_is_refused_before_any_request",
        "a connection has its own token, never the plugin's (the host refuses a run without a \
         connection); the message names the connection, not the settings",
    ),
    (
        "the_time_limit_is_counted_with_gitlabs_date",
        "the_time_limit_is_the_old_default_counted_with_gitlabs_date",
        "no timeoutSeconds setting: the old default, 20 s",
    ),
];

/// `#[test] fn name` and its text (doc comments above it included), in
/// file order.
fn test_fns(src: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let marker = "\n#[test]\nfn ";
    let mut from = 0;
    while let Some(at) = src[from..].find(marker) {
        let start = from + at + marker.len();
        let name = src[start..src[start..].find('(').unwrap() + start].to_owned();
        let end = src[start..].find("\n}\n").unwrap() + start + 3;
        out.push((name, src[from + at + 1..end].to_owned()));
        from = end - 1;
    }
    out
}

#[test]
fn parity_covers_every_old_test() {
    for (old, parity) in [(GITHUB_TESTS, PARITY_GITHUB), (GITLAB_TESTS, PARITY_GITLAB)] {
        let old = test_fns(old);
        let parity = test_fns(parity);
        let replaced = |name: &str| REPLACED.iter().find(|(o, _, _)| *o == name);
        let mut ran = 0;
        for (name, text) in &old {
            match replaced(name) {
                Some((_, by, why)) => {
                    assert!(!why.is_empty());
                    if !by.is_empty() {
                        assert!(
                            parity.iter().any(|(n, _)| n == by)
                                || test_fns(include_str!("tests.rs"))
                                    .iter()
                                    .any(|(n, _)| n == by),
                            "{name} is replaced by {by}, which does not exist"
                        );
                    }
                    assert!(
                        parity.iter().all(|(n, _)| n != name),
                        "{name} is both run and replaced"
                    );
                }
                None => {
                    let ours = parity
                        .iter()
                        .find(|(n, _)| n == name)
                        .unwrap_or_else(|| panic!("the old test {name} is not run"));
                    // Byte for byte: the same scenario, the same
                    // expectations.
                    assert_eq!(&ours.1, text, "{name} differs from the old test");
                    ran += 1;
                }
            }
        }
        // No other test in the parity files but the replacements.
        for (name, _) in &parity {
            assert!(
                old.iter().any(|(n, _)| n == name) || REPLACED.iter().any(|(_, by, _)| by == name),
                "{name} is neither an old test nor a replacement"
            );
        }
        assert!(ran >= 15, "{ran}");
    }
}

/// A transport whose requests stay readable after the action ran.
struct Watched(std::rc::Rc<std::cell::RefCell<crate::transport::mock::MockTransport>>);

impl crate::transport::Transport for Watched {
    fn send(
        &mut self,
        request: &crate::transport::HttpRequest,
    ) -> Result<crate::transport::HttpReply, crate::transport::TransportError> {
        self.0.borrow_mut().send(request)
    }

    fn report(&mut self, status: crate::provider::ConnectionStatus, detail: Option<&str>) {
        self.0.borrow_mut().report(status, detail);
    }

    fn sleep(&mut self, ms: u32) -> Result<(), crate::transport::TransportError> {
        self.0.borrow_mut().sleep(ms)
    }

    fn now_ms(&mut self) -> Option<u64> {
        self.0.borrow_mut().now_ms()
    }
}

/// Every legacy action, on a connection that may make changes, with any
/// answer: `GET` only, no credential header, and no capability of the
/// host but opening a page (no credential, setting or secret is ever
/// asked for).
#[test]
fn legacy_actions_only_read_and_never_touch_a_credential() {
    use deckpilot_plugin_sdk::testing::MockHost;
    use deckpilot_plugin_sdk::{CallResult, CapabilityRequest};

    use crate::provider::{ConnectionInfo, ConnectionStatus, Deployment};
    use crate::transport::{HttpReply, Method, mock::MockTransport};

    let id = "cn_01JD7Q0000000000000000000W";
    for (action, plugin, _) in ACTIONS {
        let (type_id, api, web) = if plugin == "deckpilot.github" {
            ("github", "https://api.github.com", "https://github.com")
        } else {
            ("gitlab", "https://gitlab.com/api/v4", "https://gitlab.com")
        };
        for body in [
            "{}",
            "[]",
            r#"{"id":1,"login":"a","username":"a","full_name":"a/b","path_with_namespace":"a/b","total_count":1,"items":[],"check_runs":[],"statuses":[],"head":{"sha":"abc"},"head_pipeline":{"id":2,"status":"running"}}"#,
        ] {
            let mut t = MockTransport::new();
            t.fallback = Some(HttpReply {
                status: 200,
                headers: Vec::new(),
                body: body.into(),
            });
            let t = std::rc::Rc::new(std::cell::RefCell::new(t));
            crate::host::test_binding::set(
                ConnectionInfo {
                    id: id.into(),
                    type_id: type_id.into(),
                    deployment: Deployment::Cloud,
                    api_base: api.into(),
                    web_base: web.into(),
                    methods: vec![Method::Get, Method::Post, Method::Put],
                    status: ConnectionStatus::Ok,
                    capabilities: None,
                },
                Box::new(Watched(t.clone())),
            );
            let mut host = MockHost::new(|request| match request {
                CapabilityRequest::OpenUrl(_) => CallResult::Ok(Value::Null),
                other => panic!("a legacy action asked the host for {other:?}"),
            });
            let params = json!({
                "connection": id, "repository": "a/b", "project": "a/b", "ref": "main",
                "filter": "author-me",
            });
            let mut params = params.as_object().unwrap().clone();
            // Only the parameters the action declares.
            let declared: Vec<String> = action_params(action);
            params.retain(|k, _| declared.contains(k));
            let _ = host.invoke(&crate::plugin(), action, Value::Object(params));
            // Every action but `open` reads something: the checks below
            // are never vacuous.
            if !action.ends_with("-open") {
                assert!(!t.borrow().requests.is_empty(), "{action} sent nothing");
            }
            for request in &t.borrow().requests {
                assert_eq!(request.method, Method::Get, "{action}: {}", request.url);
                for (name, _) in &request.headers {
                    let name = name.to_ascii_lowercase();
                    assert!(
                        !["authorization", "private-token", "cookie"].contains(&name.as_str()),
                        "{action}: {name}"
                    );
                }
                assert!(
                    request.url.starts_with(&format!("{api}/")),
                    "{action}: {} is off the connection",
                    request.url
                );
            }
        }
    }
}

fn action_params(id: &str) -> Vec<String> {
    action(&json(MANIFEST), id)["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["key"].as_str().unwrap().to_owned())
        .collect()
}

/// The legacy GitLab actions keep paging projects named by path: a next
/// link with the project's `group%2Fapp` is followed (the main clients
/// refuse a `%2f` the request did not have; this one had it).
#[test]
fn a_legacy_gitlab_list_follows_a_next_link_with_the_encoded_project_path() {
    use deckpilot_plugin_sdk::testing::MockHost;
    use deckpilot_plugin_sdk::{ActionOutcome, ParamValue};

    use crate::provider::{ConnectionInfo, ConnectionStatus, Deployment};
    use crate::transport::{Method, mock::MockTransport};

    let api = "https://gitlab.com/api/v4";
    let first = format!(
        "{api}/projects/group%2Fapp/merge_requests?state=opened&order_by=updated_at&sort=desc&per_page=50"
    );
    let second = format!(
        "{api}/projects/group%2Fapp/merge_requests?order_by=updated_at&page=2&per_page=50&sort=desc&state=opened"
    );
    let items = |from: usize, count: usize| {
        Value::Array(
            (from..from + count)
                .map(|n| json!({"iid": n, "title": format!("MR {n}"), "author": {"username": "dev"},
                                "references": {"full": format!("group/app!{n}")},
                                "web_url": format!("https://gitlab.com/group/app/-/merge_requests/{n}")}))
                .collect(),
        )
        .to_string()
    };
    let t = MockTransport::new()
        .on_with(
            Method::Get,
            &first,
            200,
            &[
                ("link", &format!("<{second}>; rel=\"next\"")),
                ("x-total", "70"),
            ],
            &items(1, 50),
        )
        .on_with(
            Method::Get,
            &second,
            200,
            &[("x-total", "70")],
            &items(51, 20),
        );
    let t = std::rc::Rc::new(std::cell::RefCell::new(t));
    let id = "cn_01JD7Q0000000000000000000P";
    crate::host::test_binding::set(
        ConnectionInfo {
            id: id.into(),
            type_id: "gitlab".into(),
            deployment: Deployment::Cloud,
            api_base: api.into(),
            web_base: "https://gitlab.com".into(),
            methods: vec![Method::Get],
            status: ConnectionStatus::Ok,
            capabilities: None,
        },
        Box::new(Watched(t.clone())),
    );
    let mut host = MockHost::new(|request| panic!("unexpected request {request:?}"));
    let out = host.invoke(
        &crate::plugin(),
        "legacy-gitlab-merge-requests",
        json!({"connection": id, "project": "group/app", "limit": 70}),
    );
    let ActionOutcome::Ok { data, .. } = &out else {
        panic!("{out:?}")
    };
    assert_eq!(data["listed"], ParamValue::Integer(70));
    assert_eq!(data["count"], ParamValue::Integer(70));
    assert_eq!(t.borrow().urls(), [first, second]);
}
