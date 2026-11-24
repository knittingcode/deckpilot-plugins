//! The GitLab adapter against scripted answers only ([`MockTransport`]):
//! never a real GitLab.com or company server. Self-managed servers are
//! fictitious hosts with a custom port and a path prefix.

use serde_json::{Value, json};

use super::{Gitlab, check_variables, pipeline_state};
use crate::errors::codes;
use crate::ops;
use crate::provider::{
    ConnectionInfo, ConnectionStatus, CrCreate, CrFilter, CrQuery, Deployment, PipelineSelector,
    PipelineTrigger, Provider, RepoPage, RepoQuery, RepoRef, WebTarget,
};
use crate::records::PipelineState;
use crate::transport::mock::MockTransport;
use crate::transport::{Method, TransportError};

const ID: &str = "cn_01JD7Q0000000000000000000A";
const SELF_API: &str = "https://code.example.com:8443/gitlab/api/v4";

fn connection(api: &str, web: &str, version: Option<&str>, write: bool) -> ConnectionInfo {
    ConnectionInfo {
        id: ID.into(),
        type_id: "gitlab".into(),
        deployment: if api.starts_with("https://gitlab.com") {
            Deployment::Cloud
        } else {
            Deployment::SelfHosted
        },
        api_base: api.into(),
        web_base: web.into(),
        methods: if write {
            vec![Method::Get, Method::Post, Method::Put]
        } else {
            vec![Method::Get]
        },
        status: ConnectionStatus::Ok,
        capabilities: version.map(|v| json!({ "apiVersion": format!("GitLab {v}") })),
    }
}

fn self_managed(version: Option<&str>) -> (ConnectionInfo, Gitlab) {
    let c = connection(
        SELF_API,
        "https://code.example.com:8443/gitlab",
        version,
        true,
    );
    let g = Gitlab::new(&c).unwrap();
    (c, g)
}

fn cloud() -> (ConnectionInfo, Gitlab) {
    let c = connection(
        "https://gitlab.com/api/v4",
        "https://gitlab.com",
        None,
        false,
    );
    let g = Gitlab::new(&c).unwrap();
    (c, g)
}

fn repo() -> RepoRef {
    RepoRef::parse("gl:4711", crate::provider::ProviderKind::Gitlab, ID).unwrap()
}

fn project(id: u64, path: &str, web: &str) -> Value {
    json!({
        "id": id,
        "name": path.rsplit('/').next().unwrap(),
        "path_with_namespace": path,
        "default_branch": "main",
        "visibility": "internal",
        "archived": false,
        "web_url": format!("{web}/{path}"),
        "http_url_to_repo": format!("{web}/{path}.git"),
        "ssh_url_to_repo": format!("ssh://git@code.example.com:2222/{path}.git"),
        "last_activity_at": "2026-11-24T21:05:00.000Z"
    })
}

fn items(out: &deckpilot_plugin_sdk::Output) -> Vec<Value> {
    match out.data().get("items") {
        Some(deckpilot_plugin_sdk::ParamValue::List(list)) => list
            .iter()
            .map(|v| match v {
                deckpilot_plugin_sdk::ParamValue::String(s) => serde_json::from_str(s).unwrap(),
                other => panic!("{other:?}"),
            })
            .collect(),
        other => panic!("{other:?}"),
    }
}

fn data<'a>(
    out: &'a deckpilot_plugin_sdk::Output,
    key: &str,
) -> &'a deckpilot_plugin_sdk::ParamValue {
    out.data().get(key).unwrap_or_else(|| panic!("no {key}"))
}

#[test]
fn a_self_managed_gitlab_on_a_port_and_a_path_prefix() {
    let (_, g) = self_managed(Some("16.11.2"));
    let web = "https://code.example.com:8443/gitlab";
    let mut t = MockTransport::new().on_with(
        Method::Get,
        &format!(
            "{SELF_API}/projects?membership=true&order_by=last_activity_at&sort=desc&page=1&per_page=50&search=app"
        ),
        200,
        &[(
            "link",
            &format!("<{SELF_API}/projects?page=2>; rel=\"next\""),
        )],
        &json!([project(4711, "team/app", web)]).to_string(),
    );
    let out = ops::list_repositories(&g, &mut t, Some("app".into()), 1).unwrap();
    let records = items(&out);
    assert_eq!(records[0]["id"], "gl:4711");
    assert_eq!(records[0]["path"], "team/app");
    assert_eq!(records[0]["webUrl"], format!("{web}/team/app"));
    assert_eq!(records[0]["updatedAt"], 1_795_554_300_000_u64);
    assert_eq!(
        data(&out, "next"),
        &deckpilot_plugin_sdk::ParamValue::Integer(2)
    );
    // Pages are opened on the same server, under its path prefix.
    assert_eq!(
        g.web_url(&WebTarget::Repository {
            path: "team/app".into(),
            page: RepoPage::Pipelines
        })
        .unwrap(),
        format!("{web}/team/app/-/pipelines")
    );
    assert_eq!(
        g.web_url(&WebTarget::ChangeRequest {
            path: "team/app".into(),
            number: 7
        })
        .unwrap(),
        format!("{web}/team/app/-/merge_requests/7")
    );
    for bad in ["../evil", "team/../../x", "team", "team/a b"] {
        assert_eq!(
            g.web_url(&WebTarget::Repository {
                path: bad.into(),
                page: RepoPage::Home
            })
            .unwrap_err()
            .code,
            codes::INVALID_REPOSITORY,
            "{bad}"
        );
    }
}

#[test]
fn links_and_urls_off_the_server_are_refused() {
    let (_, g) = self_managed(Some("16.0.0"));
    // A next page outside the API (another host, or a sibling path).
    for next in [
        "https://evil.example.com/api/v4/projects?page=2",
        "https://code.example.com:8443/other/api/v4/projects?page=2",
        "http://code.example.com:8443/gitlab/api/v4/projects?page=2",
    ] {
        let mut t = MockTransport::new().on_with(
            Method::Get,
            &format!("{SELF_API}/projects/4711/merge_requests?state=opened&order_by=updated_at&sort=desc&per_page=20"),
            200,
            &[("link", &format!("<{next}>; rel=\"next\""))],
            "[]",
        );
        let error = g
            .list_change_requests(
                &mut t,
                Some(&repo()),
                &CrQuery {
                    filter: CrFilter::Open,
                    limit: 20,
                },
            )
            .unwrap_err();
        assert_eq!(error.code, codes::INVALID_RESPONSE, "{next}");
    }
    // A web_url on another site is never kept.
    let mut hostile = project(4711, "team/app", "https://code.example.com:8443/gitlab");
    hostile["web_url"] = json!("https://evil.example.com/phish");
    let mut t = MockTransport::new().on(
        Method::Get,
        &format!("{SELF_API}/projects/4711"),
        200,
        &hostile.to_string(),
    );
    let record = g.get_repository(&mut t, &repo()).unwrap();
    assert_eq!(
        record.web_url,
        "https://code.example.com:8443/gitlab/team/app"
    );
}

#[test]
fn repositories_are_named_by_id_never_by_path() {
    let kind = crate::provider::ProviderKind::Gitlab;
    assert_eq!(RepoRef::parse("gl:4711", kind, ID).unwrap().id, "4711");
    assert_eq!(
        RepoRef::parse(&format!("{ID}:gl:4711"), kind, ID)
            .unwrap()
            .qualified(),
        "gl:4711"
    );
    for bad in [
        "group/proj",
        "gh:4711",
        "gl:",
        "gl:12a",
        "4711",
        "cn_01JD7Q0000000000000000000B:gl:4711",
    ] {
        assert_eq!(
            RepoRef::parse(bad, kind, ID).unwrap_err().code,
            codes::INVALID_REPOSITORY,
            "{bad}"
        );
    }
}

#[test]
fn probing_reads_the_version_the_token_and_its_scopes() {
    let (_, g) = self_managed(None);
    let mut t = MockTransport::new()
        .on(
            Method::Get,
            &format!("{SELF_API}/version"),
            200,
            r#"{"version":"16.11.2-ee"}"#,
        )
        .on(
            Method::Get,
            &format!("{SELF_API}/user"),
            200,
            r#"{"id":42,"username":"ada"}"#,
        )
        .on(
            Method::Get,
            &format!("{SELF_API}/personal_access_tokens/self"),
            200,
            r#"{"scopes":["read_api","sudo"],"expires_at":"2026-12-01"}"#,
        );
    let probe = g.probe(&mut t).unwrap();
    assert_eq!(probe.account, "ada");
    assert_eq!(probe.version.as_deref(), Some("GitLab 16.11.2"));
    assert_eq!(
        probe.scopes.as_deref(),
        Some(&["read_api".to_owned(), "sudo".to_owned()][..])
    );
    assert_eq!(probe.excess_scopes, ["sudo"]);
    assert!(
        !probe.capabilities.pipeline_trigger,
        "read_api cannot start pipelines"
    );
    assert!(probe.expires_at.is_some());
    assert_eq!(t.reports, [(ConnectionStatus::Ok, None)]);

    // An old GitLab: no token endpoint asked; scopes unknown.
    let mut t = MockTransport::new()
        .on(
            Method::Get,
            &format!("{SELF_API}/version"),
            200,
            r#"{"version":"15.4.6"}"#,
        )
        .on(
            Method::Get,
            &format!("{SELF_API}/user"),
            200,
            r#"{"id":42,"username":"ada"}"#,
        );
    let probe = g.probe(&mut t).unwrap();
    assert_eq!(probe.scopes, None);
    assert!(
        !t.urls()
            .iter()
            .any(|u| u.contains("personal_access_tokens"))
    );

    // A token endpoint that is not there (404): scopes unknown, not a failure.
    let mut t = MockTransport::new()
        .on(Method::Get, &format!("{SELF_API}/version"), 404, "")
        .on(
            Method::Get,
            &format!("{SELF_API}/user"),
            200,
            r#"{"id":42,"username":"ada"}"#,
        )
        .on(
            Method::Get,
            &format!("{SELF_API}/personal_access_tokens/self"),
            404,
            "{}",
        );
    let probe = g.probe(&mut t).unwrap();
    assert_eq!((probe.version, probe.scopes), (None, None));
}

#[test]
fn failures_map_to_codes_and_connection_statuses() {
    let (_, g) = cloud();
    let url = "https://gitlab.com/api/v4/projects/4711";
    let run = |t: &mut MockTransport| g.get_repository(t, &repo()).unwrap_err();

    let mut t = MockTransport::new().on(Method::Get, url, 401, r#"{"message":"401 Unauthorized"}"#);
    assert_eq!(run(&mut t).code, codes::UNAUTHORIZED);
    assert_eq!(t.reports[0].0, ConnectionStatus::Unauthorized);

    let mut t =
        MockTransport::new().on(Method::Get, url, 403, r#"{"message":"insufficient_scope"}"#);
    let e = run(&mut t);
    assert_eq!(e.code, codes::INSUFFICIENT_SCOPE);
    assert!(e.message.contains("insufficient_scope"));
    assert_eq!(t.reports[0].0, ConnectionStatus::InsufficientScope);

    let mut t = MockTransport::new().on_with(Method::Get, url, 429, &[("retry-after", "30")], "");
    let e = run(&mut t);
    assert_eq!((e.code, e.retryable), (codes::RATE_LIMITED, true));
    assert!(e.message.contains("30 s"), "{}", e.message);

    let mut t = MockTransport::new().on(Method::Get, url, 404, "{}");
    assert_eq!(run(&mut t).code, codes::NOT_FOUND);
    let mut t = MockTransport::new().on(Method::Get, url, 502, "");
    let e = run(&mut t);
    assert_eq!((e.code, e.retryable), (codes::PROVIDER_ERROR, true));
    let mut t = MockTransport::new().on(Method::Get, url, 200, "<html>");
    assert_eq!(run(&mut t).code, codes::INVALID_RESPONSE);

    // Transport failures: closed codes, and the connection's status.
    for (error, code, status) in [
        (
            TransportError::Network("tls-failed".into()),
            codes::TLS_FAILED,
            Some(ConnectionStatus::TlsFailed),
        ),
        (
            TransportError::Network("dns-failed".into()),
            codes::UNREACHABLE,
            Some(ConnectionStatus::Unreachable),
        ),
        (
            TransportError::Network("connection-failed".into()),
            codes::UNREACHABLE,
            Some(ConnectionStatus::Unreachable),
        ),
        (
            TransportError::RedirectRefused,
            codes::REDIRECT_REFUSED,
            None,
        ),
        (
            TransportError::ConnectionMissing,
            codes::CONNECTION_MISSING,
            None,
        ),
        (
            TransportError::ConnectionDisabled,
            codes::CONNECTION_DISABLED,
            None,
        ),
        (TransportError::Unsupported, codes::UNSUPPORTED, None),
    ] {
        let mut t = MockTransport::new().reply(Method::Get, url, Err(error.clone()));
        assert_eq!(run(&mut t).code, code, "{error:?}");
        assert_eq!(t.reports.first().map(|r| r.0), status, "{error:?}");
    }
    let mut t = MockTransport::new().reply(
        Method::Get,
        url,
        Err(TransportError::Network("timed-out".into())),
    );
    assert!(run(&mut t).retryable);
}

#[test]
fn review_requests_follow_the_version_gate() {
    let me = (
        Method::Get,
        format!("{SELF_API}/user"),
        r#"{"id":42,"username":"ada"}"#,
    );
    // 16.x: the server filters by reviewer.
    let (_, g) = self_managed(Some("16.11.2"));
    let mut t = MockTransport::new()
        .on(me.0, &me.1, 200, me.2)
        .on(
            Method::Get,
            &format!("{SELF_API}/projects/4711/merge_requests?state=opened&order_by=updated_at&sort=desc&per_page=20&reviewer_username=ada"),
            200,
            r#"[{"iid":3,"title":"Fix","state":"opened","source_branch":"fix","target_branch":"main","draft":true,"web_url":"https://code.example.com:8443/gitlab/team/app/-/merge_requests/3"}]"#,
        );
    let list = g
        .list_change_requests(
            &mut t,
            Some(&repo()),
            &CrQuery {
                filter: CrFilter::Review,
                limit: 20,
            },
        )
        .unwrap();
    assert_eq!(
        (list[0].number, list[0].state.as_str(), list[0].draft),
        (3, "open", true)
    );

    // 13.7: every open one, filtered by its reviewers here; and
    // `work_in_progress` is a draft.
    let (_, g) = self_managed(Some("13.7.9"));
    let mut t = MockTransport::new()
        .on(me.0, &me.1, 200, me.2)
        .on(
            Method::Get,
            &format!("{SELF_API}/projects/4711/merge_requests?state=opened&order_by=updated_at&sort=desc&per_page=100"),
            200,
            r#"[{"iid":1,"title":"A","state":"opened","work_in_progress":true,"reviewers":[{"username":"ada"}]},
                {"iid":2,"title":"B","state":"opened","reviewers":[{"username":"bob"}]}]"#,
        );
    let list = g
        .list_change_requests(
            &mut t,
            Some(&repo()),
            &CrQuery {
                filter: CrFilter::Review,
                limit: 20,
            },
        )
        .unwrap();
    assert_eq!(list.iter().map(|c| c.number).collect::<Vec<_>>(), [1]);
    assert!(list[0].draft);
}

#[test]
fn creating_a_merge_request_is_idempotent_and_needs_changes_allowed() {
    let (c, g) = self_managed(Some("16.0.0"));
    let lookup = format!(
        "{SELF_API}/projects/4711/merge_requests?state=opened&source_branch=feature%2Fx&target_branch=main&per_page=1"
    );
    let create = CrCreate {
        source: "feature/x".into(),
        target: "main".into(),
        title: "Add x".into(),
        description: Some("Why".into()),
        draft: true,
    };
    // One is open already: nothing is created.
    let mut t = MockTransport::new().on(
        Method::Get,
        &lookup,
        200,
        r#"[{"iid":9,"title":"Add x","state":"opened","source_branch":"feature/x","target_branch":"main","web_url":"https://code.example.com:8443/gitlab/team/app/-/merge_requests/9"}]"#,
    );
    let out = ops::create_change_request(&g, &mut t, &c, &repo(), &create).unwrap();
    assert_eq!(
        data(&out, "existing"),
        &deckpilot_plugin_sdk::ParamValue::Bool(true)
    );
    assert!(t.requests.iter().all(|r| r.method == Method::Get));

    // None open: one POST, a draft by its title on every GitLab.
    let mut t = MockTransport::new()
        .on(Method::Get, &lookup, 200, "[]")
        .on(
            Method::Post,
            &format!("{SELF_API}/projects/4711/merge_requests"),
            201,
            r#"{"iid":10,"title":"Draft: Add x","state":"opened","source_branch":"feature/x","target_branch":"main","draft":true,"web_url":"https://code.example.com:8443/gitlab/team/app/-/merge_requests/10"}"#,
        );
    let out = ops::create_change_request(&g, &mut t, &c, &repo(), &create).unwrap();
    assert_eq!(
        data(&out, "number"),
        &deckpilot_plugin_sdk::ParamValue::Integer(10)
    );
    let body: Value = serde_json::from_str(t.requests[1].body.as_deref().unwrap()).unwrap();
    assert_eq!(body["title"], "Draft: Add x");
    assert_eq!(body["source_branch"], "feature/x");
    assert!(
        t.requests[1]
            .headers
            .iter()
            .all(|(n, _)| !n.eq_ignore_ascii_case("authorization")
                && !n.eq_ignore_ascii_case("private-token"))
    );

    // A read-only connection: refused before any request.
    let read_only = connection(
        SELF_API,
        "https://code.example.com:8443/gitlab",
        None,
        false,
    );
    let mut t = MockTransport::new();
    let e = ops::create_change_request(&g, &mut t, &read_only, &repo(), &create).unwrap_err();
    assert_eq!(e.code, codes::READ_ONLY_CONNECTION);
    assert!(t.requests.is_empty());

    // Bad branch names never reach the server.
    let mut t = MockTransport::new();
    let bad = CrCreate {
        source: "a..b".into(),
        ..create
    };
    assert_eq!(
        g.create_change_request(&mut t, &repo(), &bad)
            .unwrap_err()
            .code,
        codes::INVALID_REF
    );
    assert!(t.requests.is_empty());
}

#[test]
fn triggering_a_pipeline_checks_its_variables() {
    let (c, g) = self_managed(Some("16.0.0"));
    let mut t = MockTransport::new().on(
        Method::Post,
        &format!("{SELF_API}/projects/4711/pipeline"),
        201,
        r#"{"id":555,"status":"created","ref":"main","sha":"abc","web_url":"https://code.example.com:8443/gitlab/team/app/-/pipelines/555"}"#,
    );
    let trigger = PipelineTrigger {
        reference: "main".into(),
        variables: ops::parse_variables("DEPLOY_ENV=staging\nDRY_RUN=1").unwrap(),
        workflow: None,
    };
    let out = ops::trigger_pipeline(&g, &mut t, &c, &repo(), &trigger).unwrap();
    assert_eq!(
        data(&out, "state"),
        &deckpilot_plugin_sdk::ParamValue::String("pending".into())
    );
    assert_eq!(
        data(&out, "pipelineId"),
        &deckpilot_plugin_sdk::ParamValue::String("555".into())
    );
    let body: Value = serde_json::from_str(t.requests[0].body.as_deref().unwrap()).unwrap();
    assert_eq!(
        body,
        json!({"ref": "main", "variables": [
            {"key": "DEPLOY_ENV", "value": "staging", "variable_type": "env_var"},
            {"key": "DRY_RUN", "value": "1", "variable_type": "env_var"}]})
    );
    for bad in [
        vec![("deploy".into(), "x".into())],
        vec![("1ABC".into(), "x".into())],
        vec![("A".into(), "x".into()), ("A".into(), "y".into())],
        vec![("A".into(), "line\nbreak".into())],
        vec![("A".into(), "x".repeat(1025))],
    ] {
        assert_eq!(
            check_variables(&bad).unwrap_err().code,
            codes::INVALID_VARIABLES,
            "{bad:?}"
        );
    }
    assert!(ops::parse_variables("NOVALUE").is_err());
    let mut t = MockTransport::new();
    let workflow = PipelineTrigger {
        workflow: Some("ci.yml".into()),
        ..trigger
    };
    assert_eq!(
        g.trigger_pipeline(&mut t, &repo(), &workflow)
            .unwrap_err()
            .code,
        codes::CAPABILITY_UNSUPPORTED
    );
}

#[test]
fn pipeline_states_are_a_closed_set() {
    for (status, state) in [
        ("created", PipelineState::Pending),
        ("waiting_for_resource", PipelineState::Pending),
        ("preparing", PipelineState::Pending),
        ("scheduled", PipelineState::Pending),
        ("running", PipelineState::Running),
        ("success", PipelineState::Success),
        ("failed", PipelineState::Failed),
        ("canceled", PipelineState::Cancelled),
        ("skipped", PipelineState::Skipped),
        ("manual", PipelineState::Manual),
        ("something-new", PipelineState::Unknown),
    ] {
        assert_eq!(pipeline_state(status), state, "{status}");
    }
}

#[test]
fn the_pipeline_of_a_branch_a_commit_and_a_merge_request() {
    let (_, g) = cloud();
    let base = "https://gitlab.com/api/v4/projects/4711";
    let mut t = MockTransport::new()
        .on(Method::Get, &format!("{base}/pipelines?ref=main&order_by=id&sort=desc&per_page=1"), 200,
            r#"[{"id":1,"status":"success","ref":"main","web_url":"https://gitlab.com/team/app/-/pipelines/1"}]"#)
        .on(Method::Get, &format!("{base}/pipelines?sha=0123456789abcdef0123456789abcdef01234567&order_by=id&sort=desc&per_page=1"), 200, "[]")
        .on(Method::Get, &format!("{base}/merge_requests/7"), 200, r#"{"iid":7,"head_pipeline":{"id":2,"status":"running"}}"#);
    let one = g
        .pipeline_status(&mut t, &repo(), &PipelineSelector::Ref("main".into()))
        .unwrap()
        .unwrap();
    assert_eq!((one.id.as_str(), one.state), ("1", PipelineState::Success));
    assert_eq!(
        g.pipeline_status(
            &mut t,
            &repo(),
            &PipelineSelector::Ref("0123456789abcdef0123456789abcdef01234567".into())
        )
        .unwrap(),
        None
    );
    let mr = g
        .pipeline_status(&mut t, &repo(), &PipelineSelector::ChangeRequest(7))
        .unwrap()
        .unwrap();
    assert_eq!(mr.state, PipelineState::Running);
    let out = ops::pipeline_status(
        &g,
        &mut t,
        &repo(),
        &PipelineSelector::Ref("0123456789abcdef0123456789abcdef01234567".into()),
    )
    .unwrap();
    assert_eq!(
        data(&out, "state"),
        &deckpilot_plugin_sdk::ParamValue::String("none".into())
    );
}

#[test]
fn waiting_for_a_pipeline_sleeps_and_stops_when_cancelled() {
    let (_, g) = cloud();
    let url = "https://gitlab.com/api/v4/projects/4711/pipelines/555";
    let mut t = MockTransport::new()
        .on(Method::Get, url, 200, r#"{"id":555,"status":"running"}"#)
        .on(Method::Get, url, 200, r#"{"id":555,"status":"running"}"#)
        .on(
            Method::Get,
            url,
            200,
            r#"{"id":555,"status":"success","duration":42}"#,
        );
    let out = ops::wait_pipeline(&g, &mut t, &repo(), "555", 45).unwrap();
    assert_eq!(
        data(&out, "state"),
        &deckpilot_plugin_sdk::ParamValue::String("success".into())
    );
    // 45 s in steps of at most 30 s, twice.
    assert_eq!(t.slept, [30_000, 15_000, 30_000, 15_000]);

    // Cancelled during the first wait: ends at once.
    let mut t = MockTransport::new().on(Method::Get, url, 200, r#"{"id":555,"status":"running"}"#);
    t.cancel_sleep_after = Some(0);
    assert_eq!(
        ops::wait_pipeline(&g, &mut t, &repo(), "555", 15)
            .unwrap_err()
            .code,
        codes::CANCELLED
    );
    assert_eq!(t.requests.len(), 1);

    // Still running at the end of the budget: timed out, never a busy loop.
    let mut t = MockTransport::new().on(Method::Get, url, 200, r#"{"id":555,"status":"pending"}"#);
    let e = ops::wait_pipeline(&g, &mut t, &repo(), "555", 120).unwrap_err();
    assert_eq!(e.code, codes::TIMED_OUT);
    assert!(t.slept.iter().sum::<u32>() <= ops::WAIT_BUDGET_SECONDS * 1_000);
    // Bounds: the poll interval is clamped.
    let mut t = MockTransport::new().on(Method::Get, url, 200, r#"{"id":555,"status":"failed"}"#);
    ops::wait_pipeline(&g, &mut t, &repo(), "555", 1).unwrap();
    assert!(t.slept.is_empty());
}

#[test]
fn a_git_remote_is_resolved_only_when_a_clone_url_matches() {
    let (_, g) = self_managed(Some("16.0.0"));
    let web = "https://code.example.com:8443/gitlab";
    let mut t = MockTransport::new().on(
        Method::Get,
        &format!("{SELF_API}/projects/team%2Fapp"),
        200,
        &project(4711, "team/app", web).to_string(),
    );
    let out = ops::resolve_remote(
        &g,
        &mut t,
        "code.example.com:8443",
        "/gitlab",
        "git@code.example.com:team/app.git",
    )
    .unwrap();
    assert_eq!(items(&out)[0]["id"], "gl:4711");
    // The remote's password never travels.
    let mut t = MockTransport::new().on(
        Method::Get,
        &format!("{SELF_API}/projects/team%2Fapp"),
        200,
        &project(4711, "team/app", web).to_string(),
    );
    ops::resolve_remote(
        &g,
        &mut t,
        "code.example.com:8443",
        "/gitlab",
        "https://ada:fake-password@code.example.com/team/app.git",
    )
    .unwrap();
    assert!(
        t.urls()
            .iter()
            .all(|u| !u.contains("fake-password") && !u.contains("ada"))
    );
    // Another host: no request, no record.
    let mut t = MockTransport::new();
    let out = ops::resolve_remote(
        &g,
        &mut t,
        "code.example.com:8443",
        "/gitlab",
        "git@github.com:team/app.git",
    )
    .unwrap();
    assert!(items(&out).is_empty());
    assert!(t.requests.is_empty());
    // A project whose clone URLs differ: no record.
    let mut other = project(4711, "team/app", web);
    other["http_url_to_repo"] = json!("https://code.example.com/elsewhere/app.git");
    other["ssh_url_to_repo"] = json!(null);
    let mut t = MockTransport::new().on(
        Method::Get,
        &format!("{SELF_API}/projects/team%2Fapp"),
        200,
        &other.to_string(),
    );
    let out = ops::resolve_remote(
        &g,
        &mut t,
        "code.example.com:8443",
        "/gitlab",
        "git@code.example.com:team/app.git",
    )
    .unwrap();
    assert!(items(&out).is_empty());
}

#[test]
fn records_carry_no_secret_and_list_repositories_pages() {
    let (_, g) = cloud();
    let mut t = MockTransport::new().on(
        Method::Get,
        "https://gitlab.com/api/v4/projects?membership=true&order_by=last_activity_at&sort=desc&page=2&per_page=30",
        200,
        &json!([project(1, "a/b", "https://gitlab.com")]).to_string(),
    );
    let page = g
        .list_repositories(
            &mut t,
            &RepoQuery {
                search: None,
                page: crate::provider::PageQuery {
                    page: 2,
                    per_page: 30,
                },
            },
        )
        .unwrap();
    assert_eq!(page.next, None);
    let text = serde_json::to_string(&page.items).unwrap();
    for marker in ["token", "PRIVATE-TOKEN", "Authorization"] {
        assert!(!text.contains(marker), "{marker}");
    }
}

#[test]
fn review_fixes_scopes_forbidden_urls_variables_prefix_and_budget() {
    // Unknown scopes: read, never a claimed write.
    let (_, g) = self_managed(None);
    let mut t = MockTransport::new()
        .on(
            Method::Get,
            &format!("{SELF_API}/version"),
            200,
            r#"{"version":"15.4.6"}"#,
        )
        .on(
            Method::Get,
            &format!("{SELF_API}/user"),
            200,
            r#"{"id":42,"username":"ada"}"#,
        );
    let probe = g.probe(&mut t).unwrap();
    assert!(!probe.capabilities.pipeline_trigger);
    assert_eq!(
        probe.capabilities.change_requests,
        crate::provider::Access::Read
    );
    assert!(
        !probe
            .capabilities
            .extensions
            .contains("gitlab.pipeline-variables")
    );

    // A 403 that is the project's permission leaves the connection alone;
    // one with Retry-After is a rate limit.
    let (_, g) = cloud();
    let url = "https://gitlab.com/api/v4/projects/4711";
    let mut t = MockTransport::new().on(Method::Get, url, 403, r#"{"message":"403 Forbidden"}"#);
    assert_eq!(
        g.get_repository(&mut t, &repo()).unwrap_err().code,
        codes::FORBIDDEN
    );
    assert!(t.reports.is_empty());
    let mut t = MockTransport::new().on_with(Method::Get, url, 403, &[("retry-after", "5")], "{}");
    assert_eq!(
        g.get_repository(&mut t, &repo()).unwrap_err().code,
        codes::RATE_LIMITED
    );

    // Dot segments in a next link, even encoded.
    let (_, g) = self_managed(Some("16.0.0"));
    let mut t = MockTransport::new().on_with(
        Method::Get,
        &format!("{SELF_API}/projects/4711/merge_requests?state=opened&order_by=updated_at&sort=desc&per_page=20"),
        200,
        &[("link", &format!("<{SELF_API}/projects/%2e%2e/x?page=2>; rel=\"next\""))],
        "[]",
    );
    assert_eq!(
        g.list_change_requests(
            &mut t,
            Some(&repo()),
            &CrQuery {
                filter: CrFilter::Open,
                limit: 20
            }
        )
        .unwrap_err()
        .code,
        codes::INVALID_RESPONSE
    );

    // A merge request without web_url: its page from references.full.
    let mut t = MockTransport::new().on(
        Method::Get,
        &format!("{SELF_API}/projects/4711/merge_requests?state=opened&order_by=updated_at&sort=desc&per_page=20"),
        200,
        r#"[{"iid":3,"title":"Fix","state":"opened","references":{"full":"team/app!3"}}]"#,
    );
    let list = g
        .list_change_requests(
            &mut t,
            Some(&repo()),
            &CrQuery {
                filter: CrFilter::Open,
                limit: 20,
            },
        )
        .unwrap();
    assert_eq!(
        list[0].web_url,
        "https://code.example.com:8443/gitlab/team/app/-/merge_requests/3"
    );

    // GitLab's own variables are refused.
    for name in ["CI_COMMIT_REF_NAME", "GITLAB_USER_ID"] {
        assert_eq!(
            check_variables(&[(name.into(), "x".into())])
                .unwrap_err()
                .code,
            codes::INVALID_VARIABLES,
            "{name}"
        );
    }

    // An https remote on a server under /gitlab keeps the prefix in its URL.
    let web = "https://code.example.com:8443/gitlab";
    let mut repo_json = project(4711, "team/app", web);
    repo_json["http_url_to_repo"] = json!(format!("{web}/team/app.git"));
    let mut t = MockTransport::new().on(
        Method::Get,
        &format!("{SELF_API}/projects/team%2Fapp"),
        200,
        &repo_json.to_string(),
    );
    let out = ops::resolve_remote(
        &g,
        &mut t,
        "code.example.com:8443",
        "/gitlab",
        "https://code.example.com:8443/gitlab/team/app.git",
    )
    .unwrap();
    assert_eq!(items(&out)[0]["id"], "gl:4711");

    // The wait budget counts the requests' time too.
    let (_, g) = cloud();
    let url = "https://gitlab.com/api/v4/projects/4711/pipelines/555";
    let mut t = MockTransport::new().on(Method::Get, url, 200, r#"{"id":555,"status":"running"}"#);
    t.request_ms = 60_000;
    let e = ops::wait_pipeline(&g, &mut t, &repo(), "555", 30).unwrap_err();
    assert_eq!(e.code, codes::TIMED_OUT);
    assert!(
        t.clock <= u64::from(ops::WAIT_BUDGET_SECONDS + 90) * 1_000,
        "{}",
        t.clock
    );
    assert!(t.requests.len() < 10, "{}", t.requests.len());
}
