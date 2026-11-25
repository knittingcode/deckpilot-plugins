//! Self-managed GitLab and GitHub Enterprise Server addresses, against
//! scripted answers only ([`MockTransport`]): never a network, never a
//! real host, no TLS of any kind. Every URL the plugin sends or opens
//! derives from `connections.get`'s `apiBase` and `webBase`; a page, a
//! next-page link or a redirect off them is refused, and the plugin never
//! builds a credential (the mock itself asserts that on every request).

use serde_json::json;

use crate::adapters::for_connection;
use crate::client::Api;
use crate::errors::codes;
use crate::ops;
use crate::provider::{
    ConnectionInfo, ConnectionStatus, CrFilter, CrQuery, Deployment, PipelineSelector, Provider,
    ProviderKind, RepoPage, RepoRef, WebTarget,
};
use crate::transport::mock::MockTransport;
use crate::transport::{HttpReply, Method, TransportError};

const ID: &str = "cn_01JD7Q0000000000000000000A";

fn connection(type_id: &str, deployment: Deployment, api: &str, web: &str) -> ConnectionInfo {
    ConnectionInfo {
        id: ID.into(),
        type_id: type_id.into(),
        deployment,
        api_base: api.into(),
        web_base: web.into(),
        methods: vec![Method::Get],
        status: ConnectionStatus::Ok,
        capabilities: None,
    }
}

fn repo(kind: ProviderKind) -> RepoRef {
    let text = match kind {
        ProviderKind::Gitlab => "gl:4711",
        ProviderKind::Github => "gh:4711",
    };
    RepoRef::parse(text, kind, ID).unwrap()
}

/// Answers everything with `body`: only what is sent is looked at.
fn permissive(body: &str) -> MockTransport {
    let mut t = MockTransport::new();
    t.fallback = Some(HttpReply {
        status: 200,
        headers: Vec::new(),
        body: body.into(),
    });
    t
}

/// Runs the read operations a key or a picker uses, on any answer.
fn exercise(p: &dyn Provider, t: &mut MockTransport, kind: ProviderKind) {
    let r = repo(kind);
    let _ = ops::get_repository(p, t, &r);
    let _ = ops::list_branches(p, t, &r, None);
    let _ = ops::list_commits(p, t, &r, Some("main".into()), 5);
    let _ = ops::list_pipelines(p, t, &r, Some("main".into()), 5);
    let _ = ops::list_workflows(p, t, &r, None);
    let _ = ops::list_issues(p, t, &r, &Default::default());
    let _ = ops::list_change_requests(p, t, Some(&r), CrFilter::Open, 5);
    let _ = ops::pipeline_status(p, t, &r, &PipelineSelector::Ref("main".into()));
    let _ = ops::pipeline_status(p, t, &r, &PipelineSelector::Id("7".into()));
    let _ = ops::pipeline_status(p, t, &r, &PipelineSelector::ChangeRequest(3));
    let _ = ops::probe(p, t);
}

const REPO_BODY: &str = r#"{"id":4711,"full_name":"team/app","path_with_namespace":"team/app","default_branch":"main","status":"running","workflow_runs":[],"workflows":[],"items":[]}"#;

#[test]
fn a_gitlab_under_a_path_prefix_on_a_port() {
    let c = connection(
        "gitlab",
        Deployment::SelfHosted,
        "https://code.example.com:8443/gitlab/api/v4",
        "https://code.example.com:8443/gitlab",
    );
    let p = for_connection(&c).unwrap();
    let mut t = permissive(REPO_BODY);
    exercise(p.as_ref(), &mut t, ProviderKind::Gitlab);
    assert!(t.requests.len() >= 10, "{}", t.requests.len());
    for url in t.urls() {
        assert!(
            url.starts_with("https://code.example.com:8443/gitlab/api/v4/"),
            "{url}"
        );
        let path = &url["https://".len()..];
        assert!(!path.contains("//"), "{url}");
        assert!(!url.contains("/api/v40"), "{url}");
        assert!(!url.contains("/gitlab/gitlab"), "{url}");
    }
    // Pages are under the prefix too.
    assert_eq!(
        p.web_url(&WebTarget::Repository {
            path: "team/app".into(),
            page: RepoPage::Pipelines
        })
        .unwrap(),
        "https://code.example.com:8443/gitlab/team/app/-/pipelines"
    );
    // A trailing slash or the default port in what the host gave changes
    // nothing.
    let c = connection(
        "gitlab",
        Deployment::SelfHosted,
        "https://code.example.com:443/gitlab/api/v4/",
        "https://code.example.com/gitlab/",
    );
    let api = Api::new(&c).unwrap();
    assert_eq!(
        api.endpoint(&["projects", "4711"], &[]),
        "https://code.example.com/gitlab/api/v4/projects/4711"
    );
}

#[test]
fn ghes_under_api_v3_with_its_pages_on_the_same_host_and_port() {
    let c = connection(
        "github",
        Deployment::SelfHosted,
        "https://ghe.example.com:8443/api/v3",
        "https://ghe.example.com:8443",
    );
    let p = for_connection(&c).unwrap();
    let mut t = permissive(REPO_BODY);
    exercise(p.as_ref(), &mut t, ProviderKind::Github);
    for url in t.urls() {
        assert!(
            url.starts_with("https://ghe.example.com:8443/api/v3/"),
            "{url}"
        );
        assert!(!url["https://".len()..].contains("//"), "{url}");
    }
    for (target, page) in [
        (
            WebTarget::Repository {
                path: "team/app".into(),
                page: RepoPage::Home,
            },
            "https://ghe.example.com:8443/team/app",
        ),
        (
            WebTarget::Pipeline {
                path: "team/app".into(),
                id: "9".into(),
            },
            "https://ghe.example.com:8443/team/app/actions/runs/9",
        ),
        (
            WebTarget::MyChangeRequests,
            "https://ghe.example.com:8443/pulls",
        ),
    ] {
        assert_eq!(p.web_url(&target).unwrap(), page);
    }
}

#[test]
fn the_clouds_have_their_fixed_origins_only() {
    let gh = connection(
        "github",
        Deployment::Cloud,
        "https://api.github.com",
        "https://github.com",
    );
    let gl = connection(
        "gitlab",
        Deployment::Cloud,
        "https://gitlab.com/api/v4",
        "https://gitlab.com",
    );
    for (c, kind, prefix) in [
        (&gh, ProviderKind::Github, "https://api.github.com/"),
        (&gl, ProviderKind::Gitlab, "https://gitlab.com/api/v4/"),
    ] {
        let p = for_connection(c).unwrap();
        let mut t = permissive(REPO_BODY);
        exercise(p.as_ref(), &mut t, kind);
        assert!(t.urls().iter().all(|u| u.starts_with(prefix)), "{prefix}");
    }
    // A "cloud" connection anywhere else, or with its pages anywhere
    // else, is not used at all.
    for (c, why) in [
        (
            ConnectionInfo {
                web_base: "https://evil.example.com".into(),
                ..gh.clone()
            },
            "github web",
        ),
        (
            ConnectionInfo {
                api_base: "https://api.github.com.evil.example.com".into(),
                ..gh.clone()
            },
            "github api",
        ),
        (
            ConnectionInfo {
                api_base: "https://gitlab.com/api/v3".into(),
                ..gl.clone()
            },
            "gitlab api path",
        ),
        (
            ConnectionInfo {
                type_id: "gitlab".into(),
                ..gh.clone()
            },
            "github's bases as gitlab",
        ),
        // Self-hosted: pages on another host or port than the API's.
        (
            connection(
                "gitlab",
                Deployment::SelfHosted,
                "https://code.example.com:8443/gitlab/api/v4",
                "https://code.example.com/gitlab",
            ),
            "another port",
        ),
        (
            connection(
                "github",
                Deployment::SelfHosted,
                "https://ghe.example.com/api/v3",
                "https://pages.example.com",
            ),
            "another host",
        ),
        (
            connection(
                "github",
                Deployment::SelfHosted,
                "http://ghe.example.com/api/v3",
                "http://ghe.example.com",
            ),
            "http",
        ),
        (
            connection(
                "gitlab",
                Deployment::SelfHosted,
                "https://user@code.example.com/api/v4",
                "https://user@code.example.com",
            ),
            "userinfo",
        ),
    ] {
        let e = for_connection(&c).err().unwrap_or_else(|| panic!("{why}"));
        assert_eq!(e.code, codes::CONNECTION_MISSING, "{why}");
    }
}

/// The GitLab and GitHub answers for one pipeline or project whose links
/// point somewhere else.
#[test]
fn links_in_answers_off_the_web_origin_are_never_kept_or_opened() {
    let base = "https://code.example.com:8443/gitlab";
    let api = format!("{base}/api/v4");
    let c = connection("gitlab", Deployment::SelfHosted, &api, base);
    let p = for_connection(&c).unwrap();
    for hostile in [
        "https://evil.example.com/team/app/-/pipelines/7",
        "https://code.example.com/gitlab/team/app/-/pipelines/7",
        "https://code.example.com:8443/other/team/app",
        "https://code.example.com:8443/gitlab/../other/app",
        "https://code.example.com:8443/gitlab/%2e%2e/other/app",
        "http://code.example.com:8443/gitlab/team/app",
        "https://user:pw@code.example.com:8443/gitlab/team/app",
        "javascript:alert(1)",
        "//evil.example.com/x",
    ] {
        let mut t = MockTransport::new().on(
            Method::Get,
            &format!("{api}/projects/4711/pipelines/7"),
            200,
            &json!({"id": 7, "status": "running", "web_url": hostile}).to_string(),
        );
        let out = ops::pipeline_status(
            p.as_ref(),
            &mut t,
            &repo(ProviderKind::Gitlab),
            &PipelineSelector::Id("7".into()),
        )
        .unwrap();
        assert_eq!(out.data().get("url"), None, "{hostile}");

        // A repository's page is built on the server, never taken from
        // the answer, so open-repository opens only the server's page.
        let mut t = MockTransport::new().on(
            Method::Get,
            &format!("{api}/projects/4711"),
            200,
            &json!({"id": 4711, "path_with_namespace": "team/app", "web_url": hostile}).to_string(),
        );
        let page = ops::repository_page(
            p.as_ref(),
            &mut t,
            &repo(ProviderKind::Gitlab),
            RepoPage::Home,
        )
        .unwrap();
        assert_eq!(page, format!("{base}/team/app"), "{hostile}");
    }
    // The same on GHES, for a workflow run.
    let c = connection(
        "github",
        Deployment::SelfHosted,
        "https://ghe.example.com:8443/api/v3",
        "https://ghe.example.com:8443",
    );
    let p = for_connection(&c).unwrap();
    let mut t = MockTransport::new()
        .on(
            Method::Get,
            "https://ghe.example.com:8443/api/v3/repositories/4711",
            200,
            r#"{"id":4711,"full_name":"team/app"}"#,
        )
        .on(
            Method::Get,
            "https://ghe.example.com:8443/api/v3/repos/team/app/actions/runs/7",
            200,
            r#"{"id":7,"status":"queued","html_url":"https://ghe.example.com/team/app/actions/runs/7"}"#,
        );
    let out = ops::pipeline_status(
        p.as_ref(),
        &mut t,
        &repo(ProviderKind::Github),
        &PipelineSelector::Id("7".into()),
    )
    .unwrap();
    assert_eq!(out.data().get("url"), None);
}

#[test]
fn next_pages_off_the_origin_or_the_prefix_are_refused() {
    let base = "https://code.example.com:8443/gitlab";
    let api = format!("{base}/api/v4");
    let c = connection("gitlab", Deployment::SelfHosted, &api, base);
    let p = for_connection(&c).unwrap();
    for next in [
        "https://evil.example.com/gitlab/api/v4/projects?page=2",
        "https://code.example.com/gitlab/api/v4/projects?page=2",
        "https://code.example.com:9443/gitlab/api/v4/projects?page=2",
        "https://code.example.com:8443/gitlab/api/v40/projects?page=2",
        "https://code.example.com:8443/gitlab/other/projects?page=2",
        "https://code.example.com:8443/api/v4/projects?page=2",
        "https://code.example.com:8443/gitlab/api/v4/../../admin?page=2",
        "https://code.example.com:8443/gitlab/api/v4/%2e%2e/admin?page=2",
        "http://code.example.com:8443/gitlab/api/v4/projects?page=2",
        "https://user@code.example.com:8443/gitlab/api/v4/projects?page=2",
    ] {
        let url = format!(
            "{api}/projects/4711/merge_requests?state=opened&order_by=updated_at&sort=desc&per_page=20"
        );
        let mut t = MockTransport::new().on_with(
            Method::Get,
            &url,
            200,
            &[("link", &format!("<{next}>; rel=\"next\""))],
            "[]",
        );
        let e = p
            .list_change_requests(
                &mut t,
                Some(&repo(ProviderKind::Gitlab)),
                &CrQuery {
                    filter: CrFilter::Open,
                    limit: 20,
                },
            )
            .unwrap_err();
        assert_eq!(e.code, codes::INVALID_RESPONSE, "{next}");
        assert_eq!(t.requests.len(), 1, "{next}: never followed");
    }

    // GHES: the same rule under /api/v3 (a paged list: check runs).
    let c = connection(
        "github",
        Deployment::SelfHosted,
        "https://ghe.example.com:8443/api/v3",
        "https://ghe.example.com:8443",
    );
    let p = for_connection(&c).unwrap();
    let named = |t: MockTransport| {
        t.on(
            Method::Get,
            "https://ghe.example.com:8443/api/v3/repositories/4711",
            200,
            r#"{"id":4711,"full_name":"team/app"}"#,
        )
    };
    let first = "https://ghe.example.com:8443/api/v3/repos/team/app/actions/workflows?per_page=100";
    let second = "https://ghe.example.com:8443/api/v3/repositories/4711/actions/workflows?per_page=100&page=2";
    // Inside the API: followed, and only there.
    let mut t = named(MockTransport::new())
        .on_with(
            Method::Get,
            first,
            200,
            &[("link", &format!("<{second}>; rel=\"next\""))],
            r#"{"workflows":[{"id":1,"name":"CI","path":".github/workflows/ci.yml","state":"active"}]}"#,
        )
        .on(
            Method::Get,
            second,
            200,
            r#"{"workflows":[{"id":2,"name":"Deploy","path":".github/workflows/deploy.yml","state":"active"}]}"#,
        );
    let found = p
        .list_workflows(&mut t, &repo(ProviderKind::Github), None)
        .unwrap();
    assert_eq!(found.len(), 2);
    assert_eq!(t.urls().last().map(String::as_str), Some(second));
    for next in [
        "https://ghe.example.com:8443/api/v30/x?page=2",
        "https://ghe.example.com:8443/x?page=2",
        "https://api.github.com/repositories?page=2",
    ] {
        let mut t = named(MockTransport::new()).on_with(
            Method::Get,
            first,
            200,
            &[("link", &format!("<{next}>; rel=\"next\""))],
            r#"{"workflows":[]}"#,
        );
        let e = p
            .list_workflows(&mut t, &repo(ProviderKind::Github), None)
            .unwrap_err();
        assert_eq!(e.code, codes::INVALID_RESPONSE, "{next}");
        assert_eq!(t.requests.len(), 2, "{next}: never followed");
    }
}

#[test]
fn a_redirect_is_refused_and_never_followed() {
    for (type_id, api, web, first) in [
        (
            "gitlab",
            "https://code.example.com:8443/gitlab/api/v4",
            "https://code.example.com:8443/gitlab",
            "https://code.example.com:8443/gitlab/api/v4/projects/4711",
        ),
        (
            "github",
            "https://ghe.example.com:8443/api/v3",
            "https://ghe.example.com:8443",
            "https://ghe.example.com:8443/api/v3/repositories/4711",
        ),
    ] {
        let c = connection(type_id, Deployment::SelfHosted, api, web);
        let p = for_connection(&c).unwrap();
        let kind = p.kind();
        // The host refused a hop off the connection.
        let mut t =
            MockTransport::new().reply(Method::Get, first, Err(TransportError::RedirectRefused));
        let e = p.get_repository(&mut t, &repo(kind)).unwrap_err();
        assert_eq!((e.code, e.retryable), (codes::REDIRECT_REFUSED, false));
        assert_eq!(t.requests.len(), 1);
        assert!(t.reports.is_empty());
        // A redirect answer that reached the plugin: refused the same way,
        // its target never requested.
        for status in [301, 302, 303, 307, 308] {
            let mut t = MockTransport::new().on_with(
                Method::Get,
                first,
                status,
                &[("location", "https://evil.example.com/steal")],
                "",
            );
            let e = p.get_repository(&mut t, &repo(kind)).unwrap_err();
            assert_eq!(e.code, codes::REDIRECT_REFUSED, "{type_id} {status}");
            assert!(!e.message.contains("evil.example.com"), "{}", e.message);
            assert_eq!(t.urls(), [first], "{type_id} {status}");
        }
    }
}

#[test]
fn the_plugin_never_builds_a_credential() {
    // The mock asserts it on every request of every test; here, the
    // headers of a whole run of reads on both providers are exactly the
    // content negotiation ones.
    for (c, kind) in [
        (
            connection(
                "gitlab",
                Deployment::SelfHosted,
                "https://code.example.com:8443/gitlab/api/v4",
                "https://code.example.com:8443/gitlab",
            ),
            ProviderKind::Gitlab,
        ),
        (
            connection(
                "github",
                Deployment::SelfHosted,
                "https://ghe.example.com:8443/api/v3",
                "https://ghe.example.com:8443",
            ),
            ProviderKind::Github,
        ),
    ] {
        let p = for_connection(&c).unwrap();
        let mut t = permissive(REPO_BODY);
        exercise(p.as_ref(), &mut t, kind);
        for request in &t.requests {
            for (name, _) in &request.headers {
                assert!(
                    ["accept", "content-type", "x-github-api-version"]
                        .contains(&name.to_ascii_lowercase().as_str()),
                    "{name} on {}",
                    request.url
                );
            }
            assert!(!request.url.contains('@'), "{}", request.url);
        }
    }
    // And the mock's own check does refuse one.
    let refused = std::panic::catch_unwind(|| {
        crate::transport::mock::assert_no_credential(
            &crate::transport::HttpRequest::get("https://code.example.com/api/v4/user")
                .header("PRIVATE-TOKEN", "x"),
        );
    });
    assert!(refused.is_err());
    let refused = std::panic::catch_unwind(|| {
        crate::transport::mock::assert_no_credential(&crate::transport::HttpRequest::get(
            "https://code.example.com/api/v4/user?private_token=x",
        ));
    });
    assert!(refused.is_err());
}
