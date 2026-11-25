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

/// Runs the operation behind a read-only or opening action, with
/// placeholder arguments. Every such action of the manifest must be here.
fn run_read_only(
    id: &str,
    p: &dyn crate::provider::Provider,
    t: &mut crate::transport::mock::MockTransport,
    r: &crate::provider::RepoRef,
) {
    use crate::ops;
    use crate::provider::{CrFilter, IssueQuery, PipelineSelector, RepoPage};
    let _ = match id {
        "probe" => ops::probe(p, t).map(drop),
        "list-repositories" => ops::list_repositories(p, t, Some("a".into()), 1).map(drop),
        "get-repository" => ops::get_repository(p, t, r).map(drop),
        "list-branches" => ops::list_branches(p, t, r, None).map(drop),
        "list-commits" => ops::list_commits(p, t, r, Some("main".into()), 5).map(drop),
        "list-change-requests" => {
            for filter in [CrFilter::Open, CrFilter::Mine, CrFilter::Review] {
                let _ = ops::list_change_requests(p, t, Some(r), filter, 5);
                let _ = ops::list_change_requests(p, t, None, filter, 5);
            }
            Ok(())
        }
        "list-pipelines" => ops::list_pipelines(p, t, r, None, 5).map(drop),
        "list-workflows" => ops::list_workflows(p, t, r, Some("ci")).map(drop),
        "list-issues" => ops::list_issues(p, t, r, &IssueQuery::default()).map(drop),
        "get-issue" => ops::get_issue(p, t, r, 1).map(drop),
        "resolve-remote" => {
            ops::resolve_remote(p, t, "gitlab.com", "", "git@gitlab.com:a/b.git").map(drop)
        }
        "extension" => {
            for ext in EXTENSIONS {
                let _ = ops::extension(
                    p,
                    t,
                    ext,
                    r,
                    &serde_json::json!({"ref": "main", "changeRequest": 1}),
                );
            }
            // The one that changes something is refused here.
            let _ = ops::extension(
                p,
                t,
                "github.workflow-dispatch",
                r,
                &serde_json::json!({"workflow": "ci.yml", "ref": "main"}),
            );
            Ok(())
        }
        "open-repository" | "open-change-requests" => {
            ops::repository_page(p, t, r, RepoPage::Changes).map(drop)
        }
        "open-issue" => ops::issue_page(p, t, r, 1).map(drop),
        "pipeline-status" => {
            let _ = ops::pipeline_status(p, t, r, &PipelineSelector::ChangeRequest(1));
            ops::pipeline_status(p, t, r, &PipelineSelector::Ref("main".into())).map(drop)
        }
        "wait-pipeline" => ops::wait_pipeline(p, t, r, "1", 10).map(drop),
        // The legacy actions: GET only, by their own test
        // (`legacy::tests::legacy_actions_only_read_and_never_touch_a_credential`).
        legacy
            if crate::legacy::ACTIONS
                .iter()
                .any(|(id, _, _)| *id == legacy) =>
        {
            Ok(())
        }
        other => panic!("{other}: a read-only or opening action the contract test does not run"),
    };
}

#[test]
fn no_read_only_or_opening_action_sends_anything_but_get() {
    use crate::provider::{ConnectionInfo, ConnectionStatus, Deployment, ProviderKind, RepoRef};
    use crate::transport::mock::MockTransport;
    use crate::transport::{HttpReply, Method};
    let id = "cn_01JD7Q0000000000000000000A";
    let connection = |type_id: &str, api: &str, web: &str| ConnectionInfo {
        id: id.into(),
        type_id: type_id.into(),
        deployment: Deployment::Cloud,
        api_base: api.into(),
        web_base: web.into(),
        // Even a connection that may make changes.
        methods: vec![Method::Get, Method::Post, Method::Put],
        status: ConnectionStatus::Ok,
        capabilities: None,
    };
    let providers: Vec<(Box<dyn crate::provider::Provider>, RepoRef)> = vec![
        (
            crate::adapters::for_connection(&connection(
                "gitlab",
                "https://gitlab.com/api/v4",
                "https://gitlab.com",
            ))
            .unwrap(),
            RepoRef::parse("gl:1", ProviderKind::Gitlab, id).unwrap(),
        ),
        (
            crate::adapters::for_connection(&connection(
                "github",
                "https://api.github.com",
                "https://github.com",
            ))
            .unwrap(),
            RepoRef::parse("gh:1", ProviderKind::Github, id).unwrap(),
        ),
    ];
    let m = manifest();
    let mut ran = 0;
    for action in m["contributes"]["actions"].as_array().unwrap() {
        let effect = action["node"]["effect"].as_str().unwrap();
        if effect == "modifies" {
            continue;
        }
        let name = action["id"].as_str().unwrap();
        for (p, r) in &providers {
            // Answers everything (objects, arrays, empty): only what is
            // sent matters here.
            for body in [
                "{}",
                "[]",
                r#"{"id":1,"full_name":"a/b","path_with_namespace":"a/b","status":"running","workflow_runs":[],"items":[],"check_runs":[]}"#,
            ] {
                let mut t = MockTransport::new();
                t.fallback = Some(HttpReply {
                    status: 200,
                    headers: Vec::new(),
                    body: body.into(),
                });
                run_read_only(name, p.as_ref(), &mut t, r);
                assert!(
                    t.requests.iter().all(|q| q.method == Method::Get),
                    "{name} sent {:?}",
                    t.requests
                        .iter()
                        .map(|q| (q.method, q.url.clone()))
                        .collect::<Vec<_>>()
                );
                ran += 1;
            }
        }
    }
    assert!(ran >= 15 * 2 * 3, "{ran}");
}

/// The rules of DeckPilot's 1.14 validator for `parameters[].options`
/// (`connections_check.rs` `parameter_options`, knittingcode/deckpilot#451),
/// so a picker that the host would refuse fails here first: a
/// single-line text parameter, fed by another action of the plugin that
/// is internal and readOnly with a `records` output whose schema has the
/// `value` and `label` fields; every `dependsOn` parameter on both actions
/// with the same kind; `search` only when the source has a text `search`.
#[test]
fn every_picker_is_fed_by_an_internal_read_only_list() {
    let m = manifest();
    let actions = m["contributes"]["actions"].as_array().unwrap();
    let schemas = m["contributes"]["recordSchemas"].as_array().unwrap();
    let mut pickers = 0;
    for action in actions {
        let id = action["id"].as_str().unwrap();
        for parameter in action["parameters"].as_array().unwrap() {
            let Some(options) = parameter.get("options") else {
                continue;
            };
            pickers += 1;
            let key = parameter["key"].as_str().unwrap();
            assert_eq!(parameter["kind"]["type"], "text", "{id}.{key}");
            let from = options["from"].as_str().unwrap();
            assert_ne!(from, id, "{id}.{key}");
            let source = actions
                .iter()
                .find(|a| a["id"] == from)
                .unwrap_or_else(|| panic!("{id}.{key}: no action {from}"));
            assert_eq!(source["internal"], true, "{from}");
            assert_eq!(source["node"]["effect"], "readOnly", "{from}");
            let schema_id = source["node"]["outputs"]
                .as_array()
                .unwrap()
                .iter()
                .find(|o| o["kind"] == "records")
                .and_then(|o| o["schema"].as_str())
                .unwrap_or_else(|| panic!("{from}: no records output"));
            let fields: BTreeSet<&str> =
                schemas.iter().find(|s| s["id"] == schema_id).unwrap()["fields"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|f| f["key"].as_str().unwrap())
                    .collect();
            for field in ["value", "label"] {
                if let Some(name) = options[field].as_str() {
                    assert!(fields.contains(name), "{id}.{key}: {from} has no {name}");
                }
            }
            for dep in options["dependsOn"].as_array().into_iter().flatten() {
                let find = |a: &Value| {
                    a["parameters"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|p| p["key"] == *dep)
                        .cloned()
                        .unwrap_or_else(|| panic!("{id}.{key}: {dep} missing in {}", a["id"]))
                };
                assert_eq!(
                    find(action)["kind"],
                    find(source)["kind"],
                    "{id}.{key}: {dep}"
                );
            }
            if options["search"] == true {
                assert!(
                    source["parameters"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|p| p["key"] == "search" && p["kind"]["type"] == "text"),
                    "{from} has no search"
                );
            }
        }
    }
    // trigger-pipeline's workflow is one of them (list-workflows).
    let workflow = &actions
        .iter()
        .find(|a| a["id"] == "trigger-pipeline")
        .unwrap()["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["key"] == "workflow")
        .unwrap()["options"];
    assert_eq!(workflow["from"], "list-workflows");
    assert_eq!(workflow["value"], "file");
    assert!(pickers >= 10, "{pickers}");
}

/// Internal actions are never on a key (the 1.14 validator): no key state,
/// no title; `pipeline-status` (not internal) has its key state, refreshed
/// at the plugin minimum of 5 s, with `device` `state: true` declared.
#[test]
fn key_state_only_where_a_key_can_be() {
    let m = manifest();
    for action in m["contributes"]["actions"].as_array().unwrap() {
        let id = action["id"].as_str().unwrap();
        if action["internal"] == true {
            for member in [
                "state",
                "title",
                "editorNavigation",
                "replacedBy",
                "contexts",
            ] {
                assert!(action.get(member).is_none(), "{id}.{member}");
            }
        }
        assert_eq!(
            action.get("state").is_some(),
            id == "pipeline-status",
            "{id}"
        );
    }
    let status = m["contributes"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "pipeline-status")
        .unwrap();
    assert_eq!(status["state"]["refresh"], "every");
    assert!(status["state"]["intervalSeconds"].as_u64().unwrap() >= 5);
    assert!(
        m["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["kind"] == "device" && p["state"] == true)
    );
}
