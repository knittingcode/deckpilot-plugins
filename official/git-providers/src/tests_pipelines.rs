//! The pipeline actions end to end against design §5 and §10:
//! `list-workflows`, `pipeline-status`, `trigger-pipeline` (its variables,
//! ref, confirmation, unattended runs and retries), `wait-pipeline` (its
//! sleeps, cancellation, deadline and bounds), GitHub's dispatch-then-
//! locate, and the error mapping. Scripted answers only
//! ([`MockTransport`]): never a real GitHub, GitLab or company server.

use deckpilot_plugin_sdk::ParamValue;
use deckpilot_plugin_sdk::testing::MockHost;
use deckpilot_plugin_sdk::{CapabilityRequest, Output};
use serde_json::{Value, json};

use crate::adapters::github::Github;
use crate::adapters::gitlab::Gitlab;
use crate::changes::tests::{accepted, declined, key, mock_host, schedule};
use crate::errors::codes;
use crate::ops;
use crate::provider::{
    ConnectionInfo, ConnectionStatus, Deployment, PipelineSelector, PipelineTrigger, Provider,
    ProviderKind, RepoRef,
};
use crate::transport::mock::MockTransport;
use crate::transport::{Method, TransportError};

const ID: &str = "cn_01JD7Q0000000000000000000A";
const GL: &str = "https://gitlab.com/api/v4";
const GH: &str = "https://api.github.com";
const GH_WEB: &str = "https://github.com";

fn connection(type_id: &str, api: &str, web: &str, write: bool) -> ConnectionInfo {
    ConnectionInfo {
        id: ID.into(),
        type_id: type_id.into(),
        deployment: Deployment::Cloud,
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

fn gitlab(write: bool) -> (ConnectionInfo, Gitlab) {
    let c = connection("gitlab", GL, "https://gitlab.com", write);
    let g = Gitlab::new(&c).unwrap();
    (c, g)
}

fn github(write: bool) -> (ConnectionInfo, Github) {
    let c = connection("github", GH, GH_WEB, write);
    let g = Github::new(&c).unwrap();
    (c, g)
}

fn gl_repo() -> RepoRef {
    RepoRef::parse("gl:4711", ProviderKind::Gitlab, ID).unwrap()
}

fn gh_repo() -> RepoRef {
    RepoRef::parse("gh:123456789", ProviderKind::Github, ID).unwrap()
}

fn gl_project(t: MockTransport) -> MockTransport {
    t.on(
        Method::Get,
        &format!("{GL}/projects/4711"),
        200,
        r#"{"id":4711,"path_with_namespace":"team/app","default_branch":"main","web_url":"https://gitlab.com/team/app"}"#,
    )
}

fn gh_named(t: MockTransport) -> MockTransport {
    t.on(
        Method::Get,
        &format!("{GH}/repositories/123456789"),
        200,
        r#"{"id":123456789,"full_name":"octo/site","name":"site","default_branch":"main","html_url":"https://github.com/octo/site"}"#,
    )
}

fn value<'a>(out: &'a Output, key: &str) -> Option<&'a ParamValue> {
    out.data().get(key)
}

fn text(out: &Output, key: &str) -> String {
    match value(out, key) {
        Some(ParamValue::String(s)) => s.clone(),
        other => panic!("{key}: {other:?}"),
    }
}

fn integer(out: &Output, key: &str) -> i64 {
    match value(out, key) {
        Some(ParamValue::Integer(n)) => *n,
        other => panic!("{key}: {other:?}"),
    }
}

fn boolean(out: &Output, key: &str) -> bool {
    match value(out, key) {
        Some(ParamValue::Bool(b)) => *b,
        other => panic!("{key}: {other:?}"),
    }
}

fn records(out: &Output) -> Vec<Value> {
    match value(out, "items") {
        Some(ParamValue::List(list)) => list
            .iter()
            .map(|v| match v {
                ParamValue::String(s) => serde_json::from_str(s).unwrap(),
                other => panic!("{other:?}"),
            })
            .collect(),
        other => panic!("{other:?}"),
    }
}

fn writes(t: &MockTransport) -> usize {
    t.requests
        .iter()
        .filter(|r| r.method != Method::Get)
        .count()
}

fn asked(host: &MockHost) -> bool {
    host.requests()
        .iter()
        .any(|r| matches!(r, CapabilityRequest::DialogOpen(_)))
}

fn manifest_action(id: &str) -> Value {
    let m: Value = serde_json::from_str(include_str!("../manifest.json")).unwrap();
    m["contributes"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == id)
        .unwrap()
        .clone()
}

// ---- list-workflows ----

#[test]
fn list_workflows_offers_dispatchable_files_on_github_only() {
    let (_, g) = github(false);
    let mut t = gh_named(MockTransport::new()).on(
        Method::Get,
        &format!("{GH}/repos/octo/site/actions/workflows?per_page=100"),
        200,
        r#"{"total_count":4,"workflows":[
            {"id":1,"name":"CI","path":".github/workflows/ci.yml","state":"active","html_url":"https://github.com/octo/site/blob/main/.github/workflows/ci.yml"},
            {"id":2,"name":"Deploy","path":".github/workflows/deploy.yaml","state":"disabled_manually","html_url":"https://evil.example.com/x"},
            {"id":3,"name":"CodeQL","path":"dynamic/github-code-scanning/codeql","state":"active"},
            {"id":4,"path":".github/workflows/nested/x.yml","state":"active"}]}"#,
    );
    let out = ops::list_workflows(&g, &mut t, &gh_repo(), None).unwrap();
    let items = records(&out);
    assert_eq!(integer(&out, "count"), 2);
    assert_eq!(
        items[0],
        json!({"id":"1","name":"CI","file":"ci.yml","path":".github/workflows/ci.yml","state":"active",
               "webUrl":"https://github.com/octo/site/blob/main/.github/workflows/ci.yml"})
    );
    // An off-server page is dropped, the record kept.
    assert_eq!(items[1]["file"], "deploy.yaml");
    assert_eq!(items[1].get("webUrl"), None);
    assert!(t.requests.iter().all(|r| r.method == Method::Get));

    // Search: by name or file, case-insensitive.
    let mut t = gh_named(MockTransport::new()).on(
        Method::Get,
        &format!("{GH}/repos/octo/site/actions/workflows?per_page=100"),
        200,
        r#"{"workflows":[{"id":1,"name":"CI","path":".github/workflows/ci.yml","state":"active"},
                         {"id":2,"name":"Deploy","path":".github/workflows/deploy.yaml","state":"active"}]}"#,
    );
    let out = ops::list_workflows(&g, &mut t, &gh_repo(), Some("DEP")).unwrap();
    assert_eq!(records(&out)[0]["file"], "deploy.yaml");
    assert_eq!(integer(&out, "count"), 1);

    // GitLab has none: refused with a clear code, no request.
    let (_, g) = gitlab(false);
    let mut t = MockTransport::new();
    let e = ops::list_workflows(&g, &mut t, &gl_repo(), None).unwrap_err();
    assert_eq!(e.code, codes::CAPABILITY_UNSUPPORTED);
    assert!(e.message.contains(".gitlab-ci.yml"), "{}", e.message);
    assert!(t.requests.is_empty());

    // The manifest: internal, readOnly, records of `workflow`, 30 s.
    let a = manifest_action("list-workflows");
    assert_eq!(a["internal"], true);
    assert_eq!(a["node"]["effect"], "readOnly");
    assert_eq!(a["node"]["retrySafe"], true);
    assert_eq!(a["node"]["timeoutMs"], 30_000);
    assert_eq!(a["node"]["outputs"][0]["schema"], "workflow");
}

// ---- pipeline-status ----

#[test]
fn pipeline_status_by_ref_or_change_request_with_its_outputs() {
    let (_, g) = gitlab(false);
    let latest = format!("{GL}/projects/4711/pipelines?ref=main&order_by=id&sort=desc&per_page=1");
    let pipeline = r#"[{"id":77,"status":"failed","ref":"main","sha":"abc123","web_url":"https://gitlab.com/team/app/-/pipelines/77","created_at":"2026-11-24T21:00:00Z","finished_at":"2026-11-24T21:05:00Z","duration":300}]"#;

    // By ref.
    let mut t = MockTransport::new().on(Method::Get, &latest, 200, pipeline);
    let s = ops::pipeline_selector(&g, &mut t, &gl_repo(), None, Some("main")).unwrap();
    assert_eq!(s, PipelineSelector::Ref("main".into()));
    let out = ops::pipeline_status(&g, &mut t, &gl_repo(), &s).unwrap();
    assert_eq!(text(&out, "state"), "failed");
    assert_eq!(text(&out, "providerStatus"), "failed");
    assert_eq!(text(&out, "pipelineId"), "77");
    assert_eq!(
        text(&out, "url"),
        "https://gitlab.com/team/app/-/pipelines/77"
    );
    assert_eq!(text(&out, "sha"), "abc123");
    assert_eq!(integer(&out, "finishedAt"), 1_795_554_300_000);
    assert_eq!(integer(&out, "durationSeconds"), 300);
    assert_eq!(integer(&out, "count"), 1);
    assert_eq!(records(&out)[0]["id"], "77");

    // A change request wins over a ref; the MR's head pipeline.
    let mut t = MockTransport::new().on(
        Method::Get,
        &format!("{GL}/projects/4711/merge_requests/12"),
        200,
        r#"{"iid":12,"head_pipeline":{"id":78,"status":"running"}}"#,
    );
    let s = ops::pipeline_selector(&g, &mut t, &gl_repo(), Some(12), Some("main")).unwrap();
    assert_eq!(s, PipelineSelector::ChangeRequest(12));
    let out = ops::pipeline_status(&g, &mut t, &gl_repo(), &s).unwrap();
    assert_eq!(
        (text(&out, "state"), text(&out, "pipelineId")),
        ("running".into(), "78".into())
    );

    // Neither: the default branch's latest.
    let mut t = gl_project(MockTransport::new()).on(Method::Get, &latest, 200, "[]");
    let s = ops::pipeline_selector(&g, &mut t, &gl_repo(), None, None).unwrap();
    assert_eq!(s, PipelineSelector::Ref("main".into()));
    // No pipeline: `none`, an empty list, never a failure.
    let out = ops::pipeline_status(&g, &mut t, &gl_repo(), &s).unwrap();
    assert_eq!(text(&out, "state"), "none");
    assert_eq!(integer(&out, "count"), 0);
    assert!(records(&out).is_empty());
    assert_eq!(value(&out, "pipelineId"), None);

    // A bad ref is refused before any request.
    let mut t = MockTransport::new();
    for bad in ["ma\u{7}in", "a b", "x..y", "-x", "ref\nINJECTED"] {
        assert_eq!(
            ops::pipeline_selector(&g, &mut t, &gl_repo(), None, Some(bad))
                .unwrap_err()
                .code,
            codes::INVALID_REF,
            "{bad:?}"
        );
    }
    assert!(t.requests.is_empty());

    // The manifest: readOnly, retry-safe, 30 s, its key state.
    let a = manifest_action("pipeline-status");
    assert_eq!(a["node"]["effect"], "readOnly");
    assert_eq!(a["node"]["retrySafe"], true);
    assert_eq!(a["node"]["timeoutMs"], 30_000);
    assert_eq!(a["state"]["refresh"], "every");
}

// ---- trigger-pipeline ----

/// What the trigger runs: a writable GitLab, the project read, the POST.
fn gl_trigger_transport() -> MockTransport {
    gl_project(MockTransport::new()).on(
        Method::Post,
        &format!("{GL}/projects/4711/pipeline"),
        201,
        r#"{"id":5,"status":"created","ref":"main","web_url":"https://gitlab.com/team/app/-/pipelines/5"}"#,
    )
}

fn gl_trigger(variables: Vec<(&str, String)>) -> PipelineTrigger {
    PipelineTrigger {
        reference: "main".into(),
        variables: variables
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
        workflow: None,
        inputs: Default::default(),
    }
}

#[test]
fn trigger_variables_are_checked_before_anyone_is_asked() {
    let (c, g) = gitlab(true);
    // Accepted names and values: one POST with them.
    let ok = gl_trigger(vec![
        ("DEPLOY", "staging".into()),
        ("_X1", "a".repeat(1024)),
        ("A".repeat(64).leak(), String::new()),
    ]);
    let mut host = mock_host(key(), accepted());
    let mut t = gl_trigger_transport();
    let out = crate::changes::trigger_pipeline(
        &mut host.context(),
        &g,
        &mut t,
        &c,
        &gl_repo(),
        true,
        &ok,
    )
    .unwrap();
    assert_eq!(text(&out, "pipelineId"), "5");
    let post = t
        .requests
        .iter()
        .find(|r| r.method == Method::Post)
        .unwrap();
    let body: Value = serde_json::from_str(post.body.as_deref().unwrap()).unwrap();
    assert_eq!(
        body["variables"][0],
        json!({"key":"DEPLOY","value":"staging","variable_type":"env_var"})
    );

    // Refused: never asked, nothing sent (not even the project read).
    let fake_token = format!("glpat-{}", "x".repeat(20));
    for bad in [
        gl_trigger(vec![("lower", "x".into())]),
        gl_trigger(vec![("1START", "x".into())]),
        gl_trigger(vec![("A-B", "x".into())]),
        gl_trigger(vec![("A".repeat(65).leak(), "x".into())]),
        gl_trigger(vec![("", "x".into())]),
        gl_trigger(vec![("CI_JOB_TOKEN", "x".into())]),
        gl_trigger(vec![("X", "a".repeat(1025))]),
        gl_trigger(vec![("X", "a\u{0}b".into())]),
        gl_trigger(vec![("X", fake_token.clone())]),
        gl_trigger(vec![("X", "1".into()), ("X", "2".into())]),
        PipelineTrigger {
            reference: "main\u{1b}[2J".into(),
            ..gl_trigger(Vec::new())
        },
        PipelineTrigger {
            workflow: Some("ci.yml".into()),
            ..gl_trigger(Vec::new())
        },
    ] {
        let mut host = mock_host(key(), accepted());
        let mut t = gl_trigger_transport();
        let e = crate::changes::trigger_pipeline(
            &mut host.context(),
            &g,
            &mut t,
            &c,
            &gl_repo(),
            true,
            &bad,
        )
        .unwrap_err();
        assert!(
            [
                codes::INVALID_VARIABLES,
                codes::INVALID_REF,
                codes::CAPABILITY_UNSUPPORTED
            ]
            .contains(&e.code()),
            "{bad:?}: {e}"
        );
        assert!(!asked(&host), "{bad:?}");
        assert!(t.requests.is_empty(), "{bad:?}");
    }
    let many: Vec<(&str, String)> = (0..51)
        .map(|i| (format!("V{i}").leak() as &str, "x".to_owned()))
        .collect();
    assert!(g.check_trigger(&gl_trigger(many)).is_err());

    // Variables on GitHub: refused the same way, before asking.
    let (c, g) = github(true);
    let mut host = mock_host(key(), accepted());
    let mut t = gh_named(MockTransport::new());
    let e = crate::changes::trigger_pipeline(
        &mut host.context(),
        &g,
        &mut t,
        &c,
        &gh_repo(),
        true,
        &PipelineTrigger {
            workflow: Some("ci.yml".into()),
            ..gl_trigger(vec![("DEPLOY", "1".into())])
        },
    )
    .unwrap_err();
    assert_eq!(e.code(), codes::CAPABILITY_UNSUPPORTED);
    assert!(!asked(&host));
    assert!(t.requests.is_empty());

    // The manifest: `pairs` with `noSecrets`, at most 50.
    let a = manifest_action("trigger-pipeline");
    let variables = a["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["key"] == "variables")
        .unwrap();
    assert_eq!(variables["kind"]["type"], "pairs");
    assert_eq!(variables["kind"]["noSecrets"], true);
    assert_eq!(variables["kind"]["maxItems"], 50);
}

#[test]
fn trigger_asks_from_keys_and_gestures_and_never_runs_unattended_unasked() {
    let (c, g) = gitlab(true);
    let run = |host: &mut MockHost, confirm: bool| {
        let mut t = gl_trigger_transport();
        let result = crate::changes::trigger_pipeline(
            &mut host.context(),
            &g,
            &mut t,
            &c,
            &gl_repo(),
            confirm,
            &gl_trigger(Vec::new()),
        );
        (result, writes(&t))
    };
    // Key and gesture: asked; declined → no POST; accepted → one.
    for trigger in [key(), json!({"trigger": "gesture"})] {
        let mut host = mock_host(trigger.clone(), declined());
        let (result, posts) = run(&mut host, true);
        assert_eq!(result.unwrap_err().code(), codes::CANCELLED, "{trigger}");
        assert_eq!(posts, 0);
        assert!(asked(&host));
        let mut host = mock_host(trigger.clone(), accepted());
        let (result, posts) = run(&mut host, true);
        result.unwrap();
        assert_eq!(posts, 1, "{trigger}");
    }
    // A workflow the user started: the intent, not asked.
    let mut host = mock_host(json!({"trigger": "workflow", "cause": "user"}), declined());
    let (result, posts) = run(&mut host, true);
    result.unwrap();
    assert_eq!(posts, 1);
    assert!(!asked(&host));
    // Schedules, events, startup, unknown runs: needs-confirmation and
    // nothing sent, unless `confirm` is off.
    for unattended in [
        schedule(),
        json!({"trigger": "workflow", "cause": "event"}),
        json!({"trigger": "workflow", "cause": "startup"}),
        json!({"trigger": "workflow"}),
        json!({"trigger": "something-new"}),
    ] {
        let mut host = mock_host(unattended.clone(), accepted());
        let (result, posts) = run(&mut host, true);
        assert_eq!(
            result.unwrap_err().code(),
            codes::NEEDS_CONFIRMATION,
            "{unattended}"
        );
        assert_eq!(posts, 0);
        assert!(!asked(&host));
        let mut host = mock_host(unattended.clone(), declined());
        let (result, posts) = run(&mut host, false);
        result.unwrap();
        assert_eq!(posts, 1, "{unattended}");
    }
}

#[test]
fn a_trigger_is_never_retried() {
    let (c, g) = gitlab(true);
    for answer in [
        Ok((502, "")),
        Ok((500, r#"{"message":"boom"}"#)),
        Err(TransportError::Network("timed-out".into())),
        Err(TransportError::Network("connection-failed".into())),
    ] {
        let t = gl_project(MockTransport::new());
        let mut t = match &answer {
            Ok((status, body)) => t.on(
                Method::Post,
                &format!("{GL}/projects/4711/pipeline"),
                *status,
                body,
            ),
            Err(e) => t.reply(
                Method::Post,
                &format!("{GL}/projects/4711/pipeline"),
                Err(e.clone()),
            ),
        };
        let e =
            ops::trigger_pipeline(&g, &mut t, &c, &gl_repo(), &gl_trigger(Vec::new())).unwrap_err();
        assert!(!e.retryable, "{answer:?}: {e}");
        assert_eq!(writes(&t), 1, "{answer:?}");
    }
    // The manifest: modifies, not retry-safe, not idempotent, 60 s.
    let a = manifest_action("trigger-pipeline");
    assert_eq!(a["node"]["effect"], "modifies");
    assert_eq!(a["node"].get("retrySafe"), None);
    assert_eq!(a["node"]["idempotent"], false);
    assert_eq!(a["node"]["timeoutMs"], 60_000);
}

// ---- wait-pipeline ----

fn gl_wait(states: &[&str]) -> MockTransport {
    let url = format!("{GL}/projects/4711/pipelines/555");
    states.iter().fold(MockTransport::new(), |t, s| {
        t.on(
            Method::Get,
            &url,
            200,
            &format!(r#"{{"id":555,"status":"{s}"}}"#),
        )
    })
}

#[test]
fn wait_sleeps_in_bounded_steps_and_ends_at_once_when_it_can() {
    let (_, g) = gitlab(false);
    // Terminal at the first read: no sleep at all.
    for done in ["success", "failed", "canceled", "skipped", "manual"] {
        let mut t = gl_wait(&[done]);
        let out = ops::wait_pipeline(&g, &mut t, &gl_repo(), "555", 15).unwrap();
        assert!(t.slept.is_empty(), "{done}");
        assert_eq!(t.requests.len(), 1);
        assert!(!boolean(&out, "timedOut"));
    }
    // Every sleep is at most 30 s; pollSeconds is clamped to 10..=120.
    for (poll, steps) in [
        (1, vec![10_000]),
        (10, vec![10_000]),
        (45, vec![30_000, 15_000]),
        (120, vec![30_000, 30_000, 30_000, 30_000]),
        (10_000, vec![30_000, 30_000, 30_000, 30_000]),
    ] {
        let mut t = gl_wait(&["running", "success"]);
        ops::wait_pipeline(&g, &mut t, &gl_repo(), "555", poll).unwrap();
        assert_eq!(t.slept, steps, "{poll}");
        assert!(t.slept.iter().all(|ms| *ms <= 30_000));
    }
    // An empty id (a dispatch not listed yet): refused, no request.
    let mut t = MockTransport::new();
    let e = ops::wait_pipeline(&g, &mut t, &gl_repo(), "  ", 15).unwrap_err();
    assert_eq!(e.code, codes::INVALID_PARAMETERS);
    assert!(e.message.contains("pipeline-status"), "{}", e.message);
    assert!(t.requests.is_empty());
    // The manifest: 600 s, cancellable, retry-safe, pollSeconds 10–120.
    let a = manifest_action("wait-pipeline");
    assert_eq!(a["node"]["timeoutMs"], 600_000);
    assert_eq!(a["node"]["cancellable"], true);
    assert_eq!(a["node"]["retrySafe"], true);
    let poll = a["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["key"] == "pollSeconds")
        .unwrap();
    assert_eq!(
        (poll["kind"]["min"].as_u64(), poll["kind"]["max"].as_u64()),
        (Some(10), Some(120))
    );
    assert!(
        a["node"]["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o["key"] == "timedOut" && o["always"] == true && o["kind"] == "boolean")
    );
}

#[test]
fn wait_is_cancelled_promptly() {
    let (_, g) = gitlab(false);
    // During the first, the second, or a later 30 s step of a 120 s poll.
    for after in [0, 1, 3, 5] {
        let mut t = gl_wait(&["running"]);
        t.cancel_sleep_after = Some(after);
        let e = ops::wait_pipeline(&g, &mut t, &gl_repo(), "555", 120).unwrap_err();
        assert_eq!(e.code, codes::CANCELLED, "{after}");
        assert!(!e.retryable);
        // Nothing after the interrupted sleep: no more reads, no sleeps.
        assert_eq!(t.slept.len(), after + 1);
        assert_eq!(t.requests.len(), 1 + after / 4, "{after}");
    }
}

#[test]
fn at_the_deadline_wait_ends_with_the_state_it_read() {
    let (_, g) = gitlab(false);
    for (state, poll) in [("running", 15), ("pending", 120), ("running", 10)] {
        let mut t = gl_wait(&[state]);
        t.clock = 1_795_554_300_000;
        t.request_ms = 300;
        let out = ops::wait_pipeline(&g, &mut t, &gl_repo(), "555", poll).unwrap();
        assert!(boolean(&out, "timedOut"), "{state}");
        assert_eq!(text(&out, "state"), state);
        assert_eq!(text(&out, "pipelineId"), "555");
        assert!(text(&out, "providerStatus").contains(state));
        // Well inside the node's 600 s (the host's own `timeout`), with
        // the requests' time counted.
        let spent = t.clock - 1_795_554_300_000;
        assert!(
            spent <= u64::from(ops::WAIT_BUDGET_SECONDS + 5) * 1_000,
            "{spent}"
        );
        assert!(
            spent >= u64::from(ops::WAIT_BUDGET_SECONDS - poll) * 1_000,
            "{spent}"
        );
    }
    // Without a clock the sleeps and the reads count (a fixed time per
    // read): the total, reads included, stays within the budget, for any
    // poll interval, even when each request really took its time.
    // Each request taking the host's whole 15 s, GitLab (one request per
    // read) and GitHub (two): the real time, which the mock's unread clock
    // counts, stays under the node's 600 s.
    let (_, gh) = github(false);
    for poll in [10, 15, 30, 120] {
        for github_too in [false, true] {
            let (provider, repo, mut t): (&dyn Provider, RepoRef, MockTransport) = if github_too {
                let url = format!("{GH}/repos/octo/site/actions/runs/555");
                (
                    &gh,
                    gh_repo(),
                    gh_named(MockTransport::new()).on(
                        Method::Get,
                        &url,
                        200,
                        r#"{"id":555,"status":"in_progress"}"#,
                    ),
                )
            } else {
                (&g, gl_repo(), gl_wait(&["running"]))
            };
            t.no_clock = true;
            t.request_ms = 15_000;
            let out = ops::wait_pipeline(provider, &mut t, &repo, "555", poll).unwrap();
            assert!(boolean(&out, "timedOut"));
            let reads = u32::try_from(t.requests.len()).unwrap();
            let slept = t.slept.iter().sum::<u32>() / 1_000;
            assert!(
                slept + reads * ops::REQUEST_SECONDS <= ops::WAIT_BUDGET_SECONDS,
                "{poll}: {slept} s asleep, {reads} requests"
            );
            assert!(t.clock <= 540_000, "{poll} {github_too}: {}", t.clock);
        }
    }
    const { assert!(ops::WAIT_BUDGET_SECONDS + 2 * ops::REQUEST_SECONDS < 600) };
}

#[test]
fn wait_reads_github_runs_too() {
    let (_, g) = github(false);
    let url = format!("{GH}/repos/octo/site/actions/runs/99");
    let mut t = gh_named(MockTransport::new())
        .on(Method::Get, &url, 200, r#"{"id":99,"status":"in_progress"}"#)
        .on(
            Method::Get,
            &url,
            200,
            r#"{"id":99,"status":"completed","conclusion":"success","html_url":"https://github.com/octo/site/actions/runs/99"}"#,
        );
    let out = ops::wait_pipeline(&g, &mut t, &gh_repo(), "99", 10).unwrap();
    assert_eq!(text(&out, "state"), "success");
    assert_eq!(text(&out, "providerStatus"), "completed/success");
    assert_eq!(
        text(&out, "url"),
        "https://github.com/octo/site/actions/runs/99"
    );
    assert_eq!(t.slept, [10_000]);
    // A non-numeric run id is refused before the run is read.
    let mut t = gh_named(MockTransport::new());
    assert_eq!(
        ops::wait_pipeline(&g, &mut t, &gh_repo(), "99/../1", 10)
            .unwrap_err()
            .code,
        codes::INVALID_PARAMETERS
    );
    assert!(t.urls().iter().all(|u| !u.contains("/runs/")));
}

// ---- GitHub: dispatch, then locate the run ----

fn dispatch_base() -> MockTransport {
    gh_named(MockTransport::new())
        .on(
            Method::Get,
            &format!("{GH}/user"),
            200,
            r#"{"login":"octocat"}"#,
        )
        .on(
            Method::Post,
            &format!("{GH}/repos/octo/site/actions/workflows/ci.yml/dispatches"),
            204,
            "",
        )
}

fn ci() -> PipelineTrigger {
    PipelineTrigger {
        reference: "main".into(),
        variables: Vec::new(),
        workflow: Some("ci.yml".into()),
        inputs: Default::default(),
    }
}

const NOW: u64 = 1_795_554_300_000; // 2026-11-24T21:05:00Z

fn runs_url() -> String {
    format!(
        "{GH}/repos/octo/site/actions/workflows/ci.yml/runs?event=workflow_dispatch&branch=main&actor=octocat&created=%3E%3D2026-11-24T21%3A04%3A55Z&per_page=10"
    )
}

#[test]
fn a_dispatch_finds_only_its_own_run_or_none() {
    let (c, g) = github(true);
    // Only this workflow's runs are read (another workflow's run at the
    // same moment is on another URL); of those, the oldest one since the
    // dispatch by the same actor. A stale run, someone else's and a run
    // without an id are never taken for it.
    let mut t = dispatch_base().on(
        Method::Get,
        &runs_url(),
        200,
        r#"{"workflow_runs":[
            {"status":"queued","created_at":"2026-11-24T21:05:01Z","actor":{"login":"octocat"}},
            {"id":40,"status":"queued","created_at":"2026-11-24T21:05:09Z","actor":{"login":"octocat"}},
            {"id":41,"status":"queued","created_at":"2026-11-24T21:05:02Z","actor":{"login":"mallory"}},
            {"id":39,"status":"completed","conclusion":"success","created_at":"2026-11-24T20:59:00Z","actor":{"login":"octocat"}},
            {"id":42,"status":"queued","created_at":"2026-11-24T21:05:03Z","actor":{"login":"octocat"},"html_url":"https://github.com/octo/site/actions/runs/42"}]}"#,
    );
    t.clock = NOW;
    let out = ops::trigger_pipeline(&g, &mut t, &c, &gh_repo(), &ci()).unwrap();
    assert_eq!(text(&out, "pipelineId"), "42");
    assert!(t.slept.is_empty());
    assert!(
        t.urls()
            .iter()
            .filter(|u| u.contains("/runs"))
            .all(|u| u.contains("/actions/workflows/ci.yml/runs?")),
        "{:?}",
        t.urls()
    );
    assert_eq!(writes(&t), 1);

    assert_eq!(
        text(&out, "url"),
        "https://github.com/octo/site/actions/runs/42"
    );

    // Only stale runs and others' runs: looked for three times, then
    // queued without an id, never one of them.
    let mut t = dispatch_base().on(
        Method::Get,
        &runs_url(),
        200,
        r#"{"workflow_runs":[
            {"id":41,"status":"queued","created_at":"2026-11-24T21:05:02Z","actor":{"login":"mallory"}},
            {"id":39,"status":"completed","conclusion":"success","created_at":"2026-11-24T21:04:00Z","actor":{"login":"octocat"}}]}"#,
    );
    t.clock = NOW;
    let out = ops::trigger_pipeline(&g, &mut t, &c, &gh_repo(), &ci()).unwrap();
    assert_eq!(value(&out, "pipelineId"), None);
    assert_eq!(t.slept, [2_000, 2_000]);
    assert_eq!(writes(&t), 1);

    // Without the agent's clock the run cannot be told from an older
    // one: no lookup at all, queued without an id, the runs page as url.
    let mut t = dispatch_base();
    t.no_clock = true;
    let out = ops::trigger_pipeline(&g, &mut t, &c, &gh_repo(), &ci()).unwrap();
    assert_eq!(value(&out, "pipelineId"), None);
    assert_eq!(text(&out, "state"), "pending");
    assert_eq!(
        text(&out, "url"),
        "https://github.com/octo/site/actions/workflows/ci.yml"
    );
    assert!(text_message(&out).contains("not listed it yet"));
    assert!(t.urls().iter().all(|u| !u.contains("/runs?")));
    assert_eq!(writes(&t), 1);

    // Mapped into wait-pipeline, the missing id is a clear refusal.
    let (_, ro) = github(false);
    let mut t = MockTransport::new();
    assert_eq!(
        ops::wait_pipeline(&ro, &mut t, &gh_repo(), "", 15)
            .unwrap_err()
            .code,
        codes::INVALID_PARAMETERS
    );
}

fn text_message(out: &Output) -> String {
    format!("{out:?}")
}

// ---- error mapping ----

#[test]
fn http_failures_map_to_codes_on_github() {
    let (_, g) = github(false);
    let url = format!("{GH}/repositories/123456789");
    let run = |t: &mut MockTransport| g.get_repository(t, &gh_repo()).unwrap_err();
    let reply = |status: u16, headers: &[(&str, &str)], body: &str| {
        MockTransport::new().on_with(Method::Get, &url, status, headers, body)
    };

    let mut t = reply(401, &[], r#"{"message":"Bad credentials"}"#);
    let e = run(&mut t);
    assert_eq!((e.code, e.retryable), (codes::UNAUTHORIZED, false));
    assert_eq!(
        t.reports,
        [(ConnectionStatus::Unauthorized, Some("unauthorized".into()))]
    );

    // 403: a token without the permission, versus a rate limit, versus
    // this repository's access (the connection is fine).
    let mut t = reply(
        403,
        &[],
        r#"{"message":"Resource not accessible by personal access token"}"#,
    );
    let e = run(&mut t);
    assert_eq!(e.code, codes::INSUFFICIENT_SCOPE);
    assert_eq!(t.reports[0].0, ConnectionStatus::InsufficientScope);
    let mut t = reply(
        403,
        &[
            ("x-ratelimit-remaining", "0"),
            ("x-ratelimit-reset", "1795554360"),
            ("date", "Tue, 24 Nov 2026 21:05:00 GMT"),
        ],
        r#"{"message":"API rate limit exceeded"}"#,
    );
    let e = run(&mut t);
    assert_eq!((e.code, e.retryable), (codes::RATE_LIMITED, true));
    assert!(e.message.contains("60 s"), "{}", e.message);
    assert!(t.reports.is_empty());
    let mut t = reply(
        403,
        &[("retry-after", "45")],
        r#"{"message":"secondary rate limit"}"#,
    );
    let e = run(&mut t);
    assert_eq!(e.code, codes::RATE_LIMITED);
    assert!(e.message.contains("45 s"), "{}", e.message);
    let mut t = reply(403, &[], r#"{"message":"Must have admin rights"}"#);
    let e = run(&mut t);
    assert_eq!((e.code, e.retryable), (codes::FORBIDDEN, false));
    assert!(t.reports.is_empty());

    let mut t = reply(404, &[], r#"{"message":"Not Found"}"#);
    assert_eq!(run(&mut t).code, codes::NOT_FOUND);

    // 429 with Retry-After in seconds or as an HTTP date.
    let mut t = reply(429, &[("retry-after", "30")], "");
    let e = run(&mut t);
    assert_eq!((e.code, e.retryable), (codes::RATE_LIMITED, true));
    assert!(e.message.contains("30 s"), "{}", e.message);
    let mut t = reply(
        429,
        &[
            ("retry-after", "Tue, 24 Nov 2026 21:07:00 GMT"),
            ("date", "Tue, 24 Nov 2026 21:05:00 GMT"),
        ],
        "",
    );
    let e = run(&mut t);
    assert!(e.message.contains("120 s"), "{}", e.message);
    // Without any hint: a minute.
    let mut t = reply(429, &[], "");
    assert!(run(&mut t).message.contains("60 s"));

    // Each failure was one request: nothing is retried inside the plugin.
    for status in [401, 403, 404, 429, 500] {
        let mut t = reply(status, &[], "{}");
        let _ = run(&mut t);
        assert_eq!(t.requests.len(), 1, "{status}");
    }
}

#[test]
fn a_status_report_carries_a_code_never_the_servers_text() {
    // A status report's detail is a code, never the server's text (the
    // ABI refuses anything else).
    let (_, g) = gitlab(false);
    let url = format!("{GL}/projects/4711");
    let mut t = MockTransport::new().on(
        Method::Get,
        &url,
        401,
        r#"{"message":"401 Unauthorized: the token was revoked"}"#,
    );
    let _ = g.get_repository(&mut t, &gl_repo());
    for (_, detail) in &t.reports {
        let detail = detail.as_deref().unwrap_or("x");
        assert!(
            detail.len() <= 48
                && detail.starts_with(|c: char| c.is_ascii_lowercase())
                && detail
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
            "{detail}"
        );
    }
}

#[test]
fn the_trigger_question_names_the_workflow_and_the_variable_names_only() {
    use crate::changes::{MAX_QUESTION, trigger_question};
    // GitHub: the workflow file.
    let q = trigger_question(&ci(), "octo/site", "github.com");
    assert_eq!(
        q,
        "Start a pipeline on main in octo/site (github.com) with the workflow ci.yml?"
    );
    // GitLab: the names, never the values.
    let t = gl_trigger(vec![
        ("DEPLOY", "staging-secret-value".into()),
        ("DRY_RUN", "1".into()),
    ]);
    let q = trigger_question(&t, "team/app", "gitlab.com");
    assert!(q.contains("DEPLOY, DRY_RUN"), "{q}");
    assert!(
        !q.contains("staging-secret-value") && !q.contains("=1"),
        "{q}"
    );
    // Many long names: bounded, the rest counted.
    let many: Vec<(&str, String)> = (0..50)
        .map(|i| {
            (
                format!("VARIABLE_WITH_A_LONG_NAME_{i:02}").leak() as &str,
                "x".into(),
            )
        })
        .collect();
    let q = trigger_question(&gl_trigger(many), "team/app", "gitlab.com");
    assert!(q.chars().count() <= MAX_QUESTION, "{}", q.chars().count());
    assert!(q.contains(" more?"), "{q}");
    assert!(!q.contains("=x"));
    // The dialog the user sees asks exactly that (from a key).
    let (c, g) = gitlab(true);
    let mut host = mock_host(key(), declined());
    let mut tr = gl_trigger_transport();
    let trigger = gl_trigger(vec![("DEPLOY", "hidden-value".into())]);
    let _ = crate::changes::trigger_pipeline(
        &mut host.context(),
        &g,
        &mut tr,
        &c,
        &gl_repo(),
        true,
        &trigger,
    );
    let dialog = host
        .requests()
        .iter()
        .find_map(|r| match r {
            CapabilityRequest::DialogOpen(d) => Some(serde_json::to_string(d).unwrap()),
            _ => None,
        })
        .unwrap();
    assert!(
        dialog.contains("DEPLOY") && !dialog.contains("hidden-value"),
        "{dialog}"
    );
}

#[test]
fn the_trigger_question_keeps_the_workflow_and_a_name_whatever_the_rest() {
    use crate::changes::{MAX_QUESTION, trigger_question};
    let long_path = "group/".repeat(60);
    let long_host = format!("{}.example.com", "h".repeat(200));
    let long_ref = "r".repeat(250);
    let workflow = format!("{}.yml", "w".repeat(96));
    let t = PipelineTrigger {
        reference: long_ref.clone(),
        workflow: Some(workflow.clone()),
        ..ci()
    };
    let q = trigger_question(&t, &long_path, &long_host);
    assert!(q.chars().count() <= MAX_QUESTION, "{}", q.chars().count());
    assert!(
        q.ends_with(&format!("with the workflow {workflow}?")),
        "{q}"
    );
    let name = "V".repeat(64);
    let many: Vec<(&str, String)> = std::iter::once((name.clone().leak() as &str, "x".to_owned()))
        .chain((0..49).map(|i| (format!("N{i}").leak() as &str, "y".to_owned())))
        .collect();
    let t = PipelineTrigger {
        reference: long_ref,
        ..gl_trigger(many)
    };
    let q = trigger_question(&t, &long_path, &long_host);
    assert!(q.chars().count() <= MAX_QUESTION, "{}", q.chars().count());
    assert!(q.contains(&format!("with the variables {name}")), "{q}");
    assert!(q.ends_with(" more?"), "{q}");
    assert!(!q.contains("  "), "{q}");
}

#[test]
fn the_trigger_question_is_one_line_whatever_the_texts() {
    use crate::changes::{MAX_QUESTION, trigger_question};
    // A hostile path and host: runs of spaces, tabs, newlines, control
    // and bidirectional characters. One line, single spaces, no panic.
    let path = "team/\n\n  app\t\t \u{202e}x \u{0}\u{7}  y";
    let host = "code.example.com  \r\n  evil";
    let t = PipelineTrigger {
        reference: "main  \t next".into(),
        ..ci()
    };
    let q = trigger_question(&t, path, host);
    assert!(!q.contains("  "), "{q:?}");
    assert!(!q.chars().any(char::is_control), "{q:?}");
    assert!(q.chars().count() <= MAX_QUESTION);
    assert!(q.contains("team/ app x y"), "{q:?}");
    assert!(q.contains("(code.example.com evil)"), "{q:?}");
    let long = format!("{}{}", " ".repeat(300), "a ".repeat(300));
    let q = trigger_question(&t, &long, &long);
    assert!(
        !q.contains("  ") && q.chars().count() <= MAX_QUESTION,
        "{q:?}"
    );
}
