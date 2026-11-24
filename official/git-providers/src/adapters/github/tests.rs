//! The GitHub adapter against scripted answers only ([`MockTransport`]):
//! never a real GitHub.com or GitHub Enterprise Server. GHES is a
//! fictitious host on a custom port.

use serde_json::{Value, json};

use super::{
    Counts, Github, Outcome, check_inputs, check_run_outcome, check_workflow, overall, run_state,
    status_outcome,
};
use crate::errors::codes;
use crate::ops;
use crate::provider::{
    ConnectionInfo, ConnectionStatus, CrCreate, CrFilter, CrQuery, Deployment, PipelineSelector,
    PipelineTrigger, Provider, ProviderKind, RepoPage, RepoRef, WebTarget,
};
use crate::records::PipelineState;
use crate::transport::Method;
use crate::transport::mock::MockTransport;

const ID: &str = "cn_01JD7Q0000000000000000000G";
const CLOUD: &str = "https://api.github.com";
const GHES: &str = "https://ghe.example.com:8443/api/v3";
const GHES_WEB: &str = "https://ghe.example.com:8443";

fn connection(api: &str, web: &str, write: bool) -> ConnectionInfo {
    ConnectionInfo {
        id: ID.into(),
        type_id: "github".into(),
        deployment: if api == CLOUD {
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
        capabilities: None,
    }
}

fn cloud() -> Github {
    Github::new(&connection(CLOUD, "https://github.com", false)).unwrap()
}

fn ghes() -> (ConnectionInfo, Github) {
    let c = connection(GHES, GHES_WEB, true);
    let g = Github::new(&c).unwrap();
    (c, g)
}

fn repo() -> RepoRef {
    RepoRef::parse("gh:123456789", ProviderKind::Github, ID).unwrap()
}

fn repository(full: &str, web: &str) -> Value {
    json!({
        "id": 123_456_789,
        "name": full.split('/').nth(1).unwrap(),
        "full_name": full,
        "private": true,
        "default_branch": "main",
        "archived": false,
        "html_url": format!("{web}/{full}"),
        "clone_url": format!("{web}/{full}.git"),
        "ssh_url": format!("git@ghe.example.com:{full}.git"),
        "pushed_at": "2026-11-24T21:05:00Z"
    })
}

/// The id lookup every repository operation starts with.
fn named(t: MockTransport, api: &str, full: &str, web: &str) -> MockTransport {
    t.on(
        Method::Get,
        &format!("{api}/repositories/123456789"),
        200,
        &repository(full, web).to_string(),
    )
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

fn text(out: &deckpilot_plugin_sdk::Output, key: &str) -> String {
    match out.data().get(key) {
        Some(deckpilot_plugin_sdk::ParamValue::String(s)) => s.clone(),
        other => panic!("{key}: {other:?}"),
    }
}

#[test]
fn ghes_on_a_port_and_identity_by_id_across_a_rename() {
    let (_, g) = ghes();
    // The repository was renamed: the id still finds it, and the routes use
    // the current name.
    let mut t = named(MockTransport::new(), GHES, "team/renamed", GHES_WEB).on(
        Method::Get,
        &format!("{GHES}/repos/team/renamed/branches?page=1&per_page=100"),
        200,
        r#"[{"name":"main","commit":{"sha":"abc"},"protected":true},{"name":"feature","commit":{"sha":"def"}}]"#,
    );
    let out = ops::list_branches(&g, &mut t, &repo(), None).unwrap();
    let branches = items(&out);
    assert_eq!(branches[0]["name"], "main");
    assert_eq!(branches[0]["default"], true);
    assert_eq!(branches[1]["default"], false);
    // Every request is under the GHES API base, on its port.
    assert!(t.urls().iter().all(|u| u.starts_with(&format!("{GHES}/"))));
    // GitHub's own headers; never a credential from the plugin.
    for request in &t.requests {
        assert!(
            request
                .headers
                .iter()
                .any(|(n, v)| n == "X-GitHub-Api-Version" && v == "2022-11-28")
        );
        assert!(
            request
                .headers
                .iter()
                .all(|(n, _)| !n.eq_ignore_ascii_case("authorization"))
        );
    }
    // Pages on the GHES web origin.
    assert_eq!(
        g.web_url(&WebTarget::Repository {
            path: "team/renamed".into(),
            page: RepoPage::Pipelines
        })
        .unwrap(),
        format!("{GHES_WEB}/team/renamed/actions")
    );
    assert_eq!(
        cloud()
            .web_url(&WebTarget::ChangeRequest {
                path: "octo/site".into(),
                number: 5
            })
            .unwrap(),
        "https://github.com/octo/site/pull/5"
    );
    for bad in ["octo", "octo/site/extra", "../x", "octo/si te"] {
        assert_eq!(
            cloud()
                .web_url(&WebTarget::Repository {
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
fn probing_reads_the_account_ghes_version_and_scopes() {
    let (_, g) = ghes();
    let mut t = MockTransport::new()
        .on_with(
            Method::Get,
            &format!("{GHES}/user"),
            200,
            &[("x-oauth-scopes", "repo, read:org, delete_repo")],
            r#"{"login":"octocat","id":1}"#,
        )
        .on(
            Method::Get,
            &format!("{GHES}/meta"),
            200,
            r#"{"installed_version":"3.14.2"}"#,
        );
    let probe = g.probe(&mut t).unwrap();
    assert_eq!(probe.account, "octocat");
    assert_eq!(probe.version.as_deref(), Some("GHES 3.14.2"));
    assert_eq!(probe.excess_scopes, ["delete_repo"]);
    assert!(probe.capabilities.pipeline_trigger);
    assert_eq!(t.reports, [(ConnectionStatus::Ok, None)]);

    // GitHub.com, a fine-grained token: no scopes header, no version.
    let g = cloud();
    let mut t = MockTransport::new()
        .on(
            Method::Get,
            &format!("{CLOUD}/user"),
            200,
            r#"{"login":"octocat"}"#,
        )
        .on(
            Method::Get,
            &format!("{CLOUD}/meta"),
            200,
            r#"{"verifiable_password_authentication":true}"#,
        );
    let probe = g.probe(&mut t).unwrap();
    assert_eq!((probe.version, probe.scopes), (None, None));
}

#[test]
fn an_exhausted_rate_limit_is_rate_limited_not_a_missing_permission() {
    let g = cloud();
    let url = format!("{CLOUD}/repositories/123456789");
    let mut t = MockTransport::new().on_with(
        Method::Get,
        &url,
        403,
        &[
            ("x-ratelimit-remaining", "0"),
            ("x-ratelimit-reset", "1795554360"),
            ("date", "Tue, 24 Nov 2026 21:05:00 GMT"),
        ],
        r#"{"message":"API rate limit exceeded"}"#,
    );
    let e = g.get_repository(&mut t, &repo()).unwrap_err();
    assert_eq!((e.code, e.retryable), (codes::RATE_LIMITED, true));
    assert!(e.message.contains("60 s"), "{}", e.message);
    assert!(t.reports.is_empty());
    // A plain 403: a missing permission, and the connection says so.
    let mut t = MockTransport::new().on(
        Method::Get,
        &url,
        403,
        r#"{"message":"Resource not accessible by personal access token"}"#,
    );
    let e = g.get_repository(&mut t, &repo()).unwrap_err();
    assert_eq!(e.code, codes::INSUFFICIENT_SCOPE);
    assert_eq!(t.reports[0].0, ConnectionStatus::InsufficientScope);
}

#[test]
fn pull_requests_of_a_repository_and_searches_about_you() {
    let g = cloud();
    let web = "https://github.com";
    let mut t = named(MockTransport::new(), CLOUD, "octo/site", web).on(
        Method::Get,
        &format!(
            "{CLOUD}/repos/octo/site/pulls?state=open&sort=updated&direction=desc&per_page=20"
        ),
        200,
        r#"[{"number":5,"title":"Docs","state":"open","draft":false,"user":{"login":"ada"},
             "head":{"ref":"docs"},"base":{"ref":"main","repo":{"id":123456789}},
             "html_url":"https://github.com/octo/site/pull/5"}]"#,
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
        (
            list[0].number,
            list[0].source.as_str(),
            list[0].target.as_str()
        ),
        (5, "docs", "main")
    );
    assert_eq!(list[0].repository.as_deref(), Some("gh:123456789"));

    let mut t = MockTransport::new().on(
        Method::Get,
        &format!("{CLOUD}/search/issues?q=is%3Apr+is%3Aopen+review-requested%3A%40me&sort=updated&order=desc&per_page=10"),
        200,
        r#"{"items":[{"number":7,"title":"Fix","state":"open","user":{"login":"bob"},"html_url":"https://github.com/octo/site/pull/7"}]}"#,
    );
    let list = g
        .list_change_requests(
            &mut t,
            None,
            &CrQuery {
                filter: CrFilter::Review,
                limit: 10,
            },
        )
        .unwrap();
    assert_eq!(list[0].number, 7);
}

#[test]
fn creating_a_pull_request_is_idempotent_and_needs_changes_allowed() {
    let (c, g) = ghes();
    let create = CrCreate {
        source: "feature/x".into(),
        target: "main".into(),
        title: "Add x".into(),
        description: None,
        draft: true,
    };
    let lookup = format!(
        "{GHES}/repos/team/app/pulls?state=open&head=team%3Afeature%2Fx&base=main&per_page=1"
    );
    let mut t = named(MockTransport::new(), GHES, "team/app", GHES_WEB).on(
        Method::Get,
        &lookup,
        200,
        &format!(r#"[{{"number":3,"title":"Add x","state":"open","head":{{"ref":"feature/x"}},"base":{{"ref":"main"}},"html_url":"{GHES_WEB}/team/app/pull/3"}}]"#),
    );
    let out = ops::create_change_request(&g, &mut t, &c, &repo(), &create).unwrap();
    assert_eq!(
        out.data().get("existing"),
        Some(&deckpilot_plugin_sdk::ParamValue::Bool(true))
    );
    assert!(t.requests.iter().all(|r| r.method == Method::Get));

    let mut t = named(MockTransport::new(), GHES, "team/app", GHES_WEB)
        .on(Method::Get, &lookup, 200, "[]")
        .on(
            Method::Post,
            &format!("{GHES}/repos/team/app/pulls"),
            201,
            &format!(r#"{{"number":4,"title":"Add x","state":"open","draft":true,"head":{{"ref":"feature/x"}},"base":{{"ref":"main"}},"html_url":"{GHES_WEB}/team/app/pull/4"}}"#),
        );
    let out = ops::create_change_request(&g, &mut t, &c, &repo(), &create).unwrap();
    assert_eq!(text(&out, "url"), format!("{GHES_WEB}/team/app/pull/4"));
    let body: Value =
        serde_json::from_str(t.requests.last().unwrap().body.as_deref().unwrap()).unwrap();
    assert_eq!(
        body,
        json!({"title": "Add x", "head": "feature/x", "base": "main", "draft": true})
    );

    let read_only = connection(GHES, GHES_WEB, false);
    let mut t = MockTransport::new();
    assert_eq!(
        ops::create_change_request(&g, &mut t, &read_only, &repo(), &create)
            .unwrap_err()
            .code,
        codes::READ_ONLY_CONNECTION
    );
    assert!(t.requests.is_empty());
}

#[test]
fn workflow_runs_are_pipelines() {
    for (status, conclusion, state) in [
        ("queued", None, PipelineState::Pending),
        ("waiting", None, PipelineState::Pending),
        ("in_progress", None, PipelineState::Running),
        ("completed", Some("success"), PipelineState::Success),
        ("completed", Some("failure"), PipelineState::Failed),
        ("completed", Some("timed_out"), PipelineState::Failed),
        ("completed", Some("cancelled"), PipelineState::Cancelled),
        ("completed", Some("skipped"), PipelineState::Skipped),
        ("completed", Some("action_required"), PipelineState::Manual),
        ("completed", Some("stale"), PipelineState::Unknown),
        ("something", None, PipelineState::Unknown),
    ] {
        assert_eq!(
            run_state(status, conclusion),
            state,
            "{status} {conclusion:?}"
        );
    }
    let g = cloud();
    let web = "https://github.com";
    let mut t = named(MockTransport::new(), CLOUD, "octo/site", web)
        .on(
            Method::Get,
            &format!("{CLOUD}/repos/octo/site/actions/runs?branch=main&per_page=1"),
            200,
            r#"{"workflow_runs":[{"id":99,"status":"completed","conclusion":"success","head_branch":"main","head_sha":"abc",
                "html_url":"https://github.com/octo/site/actions/runs/99",
                "run_started_at":"2026-11-24T21:00:00Z","updated_at":"2026-11-24T21:05:00Z"}]}"#,
        )
        .on(Method::Get, &format!("{CLOUD}/repos/octo/site/pulls/5"), 200, r#"{"head":{"sha":"0123456789abcdef0123456789abcdef01234567"}}"#)
        .on(
            Method::Get,
            &format!("{CLOUD}/repos/octo/site/actions/runs?head_sha=0123456789abcdef0123456789abcdef01234567&per_page=1"),
            200,
            r#"{"workflow_runs":[]}"#,
        );
    let run = g
        .pipeline_status(&mut t, &repo(), &PipelineSelector::Ref("main".into()))
        .unwrap()
        .unwrap();
    assert_eq!(
        (run.id.as_str(), run.state, run.duration_seconds),
        ("99", PipelineState::Success, Some(300))
    );
    assert_eq!(run.provider_status, "completed/success");
    assert_eq!(
        g.pipeline_status(&mut t, &repo(), &PipelineSelector::ChangeRequest(5))
            .unwrap(),
        None
    );
}

#[test]
fn starting_a_pipeline_dispatches_a_workflow() {
    let (c, g) = ghes();
    let now = 1_795_554_300_000; // 2026-11-24T21:05:00Z
    let runs = format!(
        "{GHES}/repos/team/app/actions/runs?event=workflow_dispatch&branch=main&actor=octocat&created=%3E%3D2026-11-24T21%3A04%3A55Z&per_page=10"
    );
    let base = || {
        named(MockTransport::new(), GHES, "team/app", GHES_WEB)
            .on(
                Method::Get,
                &format!("{GHES}/user"),
                200,
                r#"{"login":"octocat"}"#,
            )
            .on(
                Method::Post,
                &format!("{GHES}/repos/team/app/actions/workflows/ci.yml/dispatches"),
                204,
                "",
            )
    };
    let trigger = PipelineTrigger {
        reference: "main".into(),
        variables: Vec::new(),
        workflow: Some("ci.yml".into()),
        inputs: json!({"env": "staging"}).as_object().unwrap().clone(),
    };
    // The run of this dispatch: created since, by the same actor; an
    // older run and someone else's are never taken for it.
    let mut t = base().on(
        Method::Get,
        &runs,
        200,
        &format!(
            r#"{{"workflow_runs":[
                {{"id":9,"status":"queued","created_at":"2026-11-24T21:05:03Z","actor":{{"login":"mallory"}}}},
                {{"id":8,"status":"queued","created_at":"2026-11-24T21:05:02Z","actor":{{"login":"octocat"}},"html_url":"{GHES_WEB}/team/app/actions/runs/8"}},
                {{"id":6,"status":"completed","conclusion":"success","created_at":"2026-11-24T20:00:00Z","actor":{{"login":"octocat"}}}}]}}"#
        ),
    );
    t.clock = now;
    let out = ops::trigger_pipeline(&g, &mut t, &c, &repo(), &trigger).unwrap();
    assert_eq!(text(&out, "pipelineId"), "8");
    assert_eq!(text(&out, "state"), "pending");
    let post = t
        .requests
        .iter()
        .find(|r| r.method == Method::Post)
        .unwrap();
    let body: Value = serde_json::from_str(post.body.as_deref().unwrap()).unwrap();
    assert_eq!(body, json!({"ref": "main", "inputs": {"env": "staging"}}));
    assert_eq!(
        t.requests
            .iter()
            .filter(|r| r.method == Method::Post)
            .count(),
        1
    );

    // Only an older run there: queued without an id, never the old run.
    let mut t = base().on(
        Method::Get,
        &runs,
        200,
        r#"{"workflow_runs":[{"id":6,"status":"completed","conclusion":"success","created_at":"2026-11-24T20:00:00Z","actor":{"login":"octocat"}}]}"#,
    );
    t.clock = now;
    let out = ops::trigger_pipeline(&g, &mut t, &c, &repo(), &trigger).unwrap();
    assert_eq!(text(&out, "pipelineId"), "");
    assert_eq!(text(&out, "providerStatus"), "queued");

    // A read-only connection: no request at all.
    let read_only = connection(GHES, GHES_WEB, false);
    let mut t = base();
    assert_eq!(
        ops::trigger_pipeline(&g, &mut t, &read_only, &repo(), &trigger)
            .unwrap_err()
            .code,
        codes::READ_ONLY_CONNECTION
    );
    assert!(t.requests.is_empty());

    // No workflow, variables (GitLab's), or a bad workflow name: refused
    // before any write.
    for bad in [
        PipelineTrigger {
            workflow: None,
            ..trigger.clone()
        },
        PipelineTrigger {
            variables: vec![("A".into(), "1".into())],
            ..trigger.clone()
        },
        PipelineTrigger {
            workflow: Some("../evil.yml".into()),
            ..trigger.clone()
        },
    ] {
        let mut t = base();
        assert!(
            g.trigger_pipeline(&mut t, &repo(), &bad).is_err(),
            "{bad:?}"
        );
        assert!(t.requests.iter().all(|r| r.method == Method::Get));
    }
    assert!(check_workflow("deploy.yaml").is_ok());
    assert!(check_workflow("1234").is_ok());
    for bad in ["", "ci", ".github/workflows/ci.yml", "a b.yml"] {
        assert!(check_workflow(bad).is_err(), "{bad}");
    }
    let inputs = |v: Value| v.as_object().unwrap().clone();
    assert!(check_inputs(&inputs(json!({"env": "staging", "dry_run": true}))).is_ok());
    for bad in [
        json!({"1env": "x"}),
        json!({"env": "a\nb"}),
        json!({"env": {"nested": 1}}),
    ] {
        assert_eq!(
            check_inputs(&inputs(bad.clone())).unwrap_err().code,
            codes::INVALID_VARIABLES,
            "{bad}"
        );
    }
}

#[test]
fn checks_reduce_check_runs_and_statuses_like_the_old_plugin() {
    assert_eq!(
        check_run_outcome(&json!({"status": "in_progress"})),
        Outcome::Pending
    );
    assert_eq!(
        check_run_outcome(&json!({"status": "completed", "conclusion": "neutral"})),
        Outcome::Passed
    );
    assert_eq!(
        check_run_outcome(&json!({"status": "completed", "conclusion": "failure"})),
        Outcome::Failed
    );
    assert_eq!(status_outcome(&json!({"state": "error"})), Outcome::Failed);
    assert_eq!(overall(Counts::default()), "none");
    assert_eq!(
        overall(Counts {
            passed: 2,
            failed: 0,
            pending: 1
        }),
        "pending"
    );

    let g = cloud();
    let web = "https://github.com";
    let mut t = named(MockTransport::new(), CLOUD, "octo/site", web)
        .on(
            Method::Get,
            &format!("{CLOUD}/repos/octo/site/commits/main/check-runs?per_page=100"),
            200,
            r#"{"check_runs":[{"name":"build","status":"completed","conclusion":"success"},{"name":"lint","status":"completed","conclusion":"failure"}]}"#,
        )
        .on(
            Method::Get,
            &format!("{CLOUD}/repos/octo/site/commits/main/status"),
            200,
            r#"{"statuses":[{"context":"ci/legacy","state":"success"}]}"#,
        );
    let result = g
        .extension(&mut t, "github.checks", &repo(), &json!({}))
        .unwrap();
    assert_eq!(result["state"], "failing");
    assert_eq!(result["failing"], json!(["lint"]));
    assert_eq!(
        (result["passed"].as_u64(), result["failed"].as_u64()),
        (Some(2), Some(1))
    );
    assert_eq!(
        g.extension(
            &mut MockTransport::new(),
            "gitlab.approvals",
            &repo(),
            &json!({})
        )
        .unwrap_err()
        .code,
        codes::CAPABILITY_UNSUPPORTED
    );
}

#[test]
fn issues_leave_out_pull_requests_and_remotes_resolve() {
    let g = cloud();
    let web = "https://github.com";
    let mut t = named(MockTransport::new(), CLOUD, "octo/site", web)
        .on(
            Method::Get,
            &format!("{CLOUD}/repos/octo/site/issues?state=open&sort=updated&per_page=20"),
            200,
            r#"[{"number":1,"title":"Bug","state":"open","html_url":"https://github.com/octo/site/issues/1"},
                {"number":2,"title":"PR","state":"open","pull_request":{},"html_url":"https://github.com/octo/site/pull/2"}]"#,
        )
        .on(Method::Get, &format!("{CLOUD}/repos/octo/site/issues/2"), 200, r#"{"number":2,"pull_request":{}}"#);
    let issues = g
        .list_issues(
            &mut t,
            &repo(),
            &crate::provider::IssueQuery {
                state: Default::default(),
                limit: 20,
            },
        )
        .unwrap();
    assert_eq!(issues.iter().map(|i| i.number).collect::<Vec<_>>(), [1]);
    assert_eq!(
        g.get_issue(&mut t, &repo(), 2).unwrap_err().code,
        codes::NOT_FOUND
    );

    let mut cloud_repo = repository("octo/site", web);
    cloud_repo["ssh_url"] = json!("git@github.com:octo/site.git");
    let mut t = MockTransport::new().on(
        Method::Get,
        &format!("{CLOUD}/repos/octo/site"),
        200,
        &cloud_repo.to_string(),
    );
    let out =
        ops::resolve_remote(&g, &mut t, "github.com", "", "git@github.com:octo/site.git").unwrap();
    assert_eq!(items(&out)[0]["id"], "gh:123456789");
    // The API host of GitHub.com is not where remotes point: matched on the
    // web host.
    let mut t = MockTransport::new();
    let out =
        ops::resolve_remote(&g, &mut t, "github.com", "", "git@gitlab.com:octo/site.git").unwrap();
    assert!(items(&out).is_empty());
    assert!(t.requests.is_empty());
}

#[test]
fn no_read_only_path_writes_and_review_fixes() {
    // Every extension the `extension` action offers, on both providers,
    // sends GET only; a dispatch through it is refused before any request.
    let g = cloud();
    let web = "https://github.com";
    let mut t = named(MockTransport::new(), CLOUD, "octo/site", web);
    let e = g
        .extension(
            &mut t,
            "github.workflow-dispatch",
            &repo(),
            &json!({"workflow": "ci.yml", "ref": "main"}),
        )
        .unwrap_err();
    assert_eq!(e.code, codes::CAPABILITY_UNSUPPORTED);
    assert!(t.requests.is_empty());
    for id in crate::EXTENSIONS {
        let mut t = named(MockTransport::new(), CLOUD, "octo/site", web);
        let _ = g.extension(&mut t, id, &repo(), &json!({"ref": "main"}));
        assert!(t.requests.iter().all(|r| r.method == Method::Get), "{id}");
        let gl = crate::adapters::gitlab::Gitlab::new(&ConnectionInfo {
            type_id: "gitlab".into(),
            api_base: "https://gitlab.com/api/v4".into(),
            web_base: "https://gitlab.com".into(),
            ..connection(CLOUD, web, true)
        })
        .unwrap();
        let gl_repo = RepoRef::parse("gl:1", ProviderKind::Gitlab, ID).unwrap();
        let mut t = MockTransport::new();
        let _ = gl.extension(&mut t, id, &gl_repo, &json!({"changeRequest": 1}));
        assert!(t.requests.iter().all(|r| r.method == Method::Get), "{id}");
    }

    // Fine-grained tokens report no scopes: read until a write proves
    // otherwise.
    let mut t = MockTransport::new()
        .on(
            Method::Get,
            &format!("{CLOUD}/user"),
            200,
            r#"{"login":"octocat"}"#,
        )
        .on(Method::Get, &format!("{CLOUD}/meta"), 200, "{}");
    let probe = g.probe(&mut t).unwrap();
    assert!(!probe.capabilities.pipeline_trigger);
    assert_eq!(probe.capabilities.pipelines, crate::provider::Access::Read);
    assert!(
        !probe
            .capabilities
            .extensions
            .contains("github.workflow-dispatch")
    );

    // A 403 with Retry-After is a rate limit (GitHub's secondary limit).
    let mut t = MockTransport::new().on_with(
        Method::Get,
        &format!("{CLOUD}/repositories/123456789"),
        403,
        &[("retry-after", "30")],
        r#"{"message":"You have exceeded a secondary rate limit"}"#,
    );
    let e = g.get_repository(&mut t, &repo()).unwrap_err();
    assert_eq!((e.code, e.retryable), (codes::RATE_LIMITED, true));
    assert!(t.reports.is_empty());

    // A ref with `/` is one encoded segment or query value.
    let mut t = named(MockTransport::new(), CLOUD, "octo/site", web)
        .on(
            Method::Get,
            &format!("{CLOUD}/repos/octo/site/commits?per_page=20&sha=feature%2Fx"),
            200,
            "[]",
        )
        .on(
            Method::Get,
            &format!("{CLOUD}/repos/octo/site/commits/feature%2Fx/check-runs?per_page=100"),
            200,
            r#"{"check_runs":[]}"#,
        )
        .on(
            Method::Get,
            &format!("{CLOUD}/repos/octo/site/commits/feature%2Fx/status"),
            200,
            r#"{"statuses":[]}"#,
        );
    ops::list_commits(&g, &mut t, &repo(), Some("feature/x".into()), 20).unwrap();
    let checks = g
        .extension(
            &mut t,
            "github.checks",
            &repo(),
            &json!({"ref": "feature/x"}),
        )
        .unwrap();
    assert_eq!(checks["state"], "none");

    // A search reads past the first page.
    let page2 = format!("{CLOUD}/user/repos?sort=updated&direction=desc&per_page=100&page=2");
    let mut t = MockTransport::new()
        .on_with(
            Method::Get,
            &format!("{CLOUD}/user/repos?sort=updated&direction=desc&per_page=100"),
            200,
            &[("link", &format!("<{page2}>; rel=\"next\""))],
            &json!([repository("octo/other", web)]).to_string(),
        )
        .on(
            Method::Get,
            &page2,
            200,
            &json!([repository("octo/site", web)]).to_string(),
        );
    let page = g
        .list_repositories(
            &mut t,
            &crate::provider::RepoQuery {
                search: Some("SITE".into()),
                page: crate::provider::PageQuery::default(),
            },
        )
        .unwrap();
    assert_eq!(
        page.items
            .iter()
            .map(|r| r.path.as_str())
            .collect::<Vec<_>>(),
        ["octo/site"]
    );
}
