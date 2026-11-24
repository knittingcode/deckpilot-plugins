//! The GitHub adapter: GitHub.com and GitHub Enterprise Server (API under
//! `{web}/api/v3`, any port), REST API v3. The check-run and commit-status
//! reduction is the one of `official/github/src/lib.rs` (`checks`); that
//! plugin stays as it is.
//!
//! Repositories are named by their numeric id (`gh:123456789`), never by
//! `owner/name`: the adapter reads the current name from
//! `GET /repositories/{id}` before it uses a `/repos/{owner}/{name}` route,
//! so a rename or a transfer changes nothing.
//!
//! Pipelines are GitHub Actions workflow runs. Starting one is a
//! `workflow_dispatch` of a workflow file; GitHub answers 204 without the
//! run, so the adapter then reads the newest dispatched run of the ref.

#[cfg(test)]
mod tests;

use serde_json::{Map, Value, json};

use crate::client::{Api, boolean, check_ref, number, string, text};
use crate::errors::{Failure, clean, codes, fail};
use crate::provider::{
    Access, BranchQuery, Capabilities, CommitQuery, ConnectionInfo, ConnectionStatus, CrCreate,
    CrFilter, CrQuery, IssueQuery, IssueState, PipelineQuery, PipelineSelector, PipelineTrigger,
    Provider, ProviderKind, RepoPage, RepoQuery, RepoRef, WebTarget,
};
use crate::records::{
    BranchRecord, ChangeRequestRecord, CommitRecord, IssueRecord, Page, PipelineRecord,
    PipelineState, Probe, RepositoryRecord, timestamp,
};
use crate::transport::{HttpRequest, Method, Transport};

/// Extensions (the manifest's `connectionTypes[github].extensions`).
pub const EXTENSIONS: [&str; 3] = [
    "github.checks",
    "github.review-requests",
    "github.workflow-dispatch",
];
/// The API version header GitHub documents for REST v3.
pub const API_VERSION: (&str, &str) = ("X-GitHub-Api-Version", "2022-11-28");
pub const ACCEPT: (&str, &str) = ("Accept", "application/vnd.github+json");
/// Most workflow inputs one dispatch sends (GitHub's own limit is 25).
pub const MAX_INPUTS: usize = 25;
/// Classic token scopes beyond what DeckPilot ever needs.
pub const EXCESS_SCOPES: [&str; 6] = [
    "admin:org",
    "admin:repo_hook",
    "admin:enterprise",
    "delete_repo",
    "site_admin",
    "admin:gpg_key",
];

#[derive(Debug, Clone)]
pub struct Github {
    pub api: Api,
}

/// A repository's current `owner/name`, read by id.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Named {
    owner: String,
    name: String,
    json: Value,
}

fn invalid(what: &str) -> Failure {
    fail(
        codes::INVALID_RESPONSE,
        format!("GitHub's answer for {what} is not what was expected"),
    )
}

fn segment_ok(s: &str) -> bool {
    (1..=100).contains(&s.len())
        && s != "."
        && s != ".."
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// `owner/name`, both GitHub's name characters.
fn split_path(path: &str) -> Option<(&str, &str)> {
    let (owner, name) = path.split_once('/')?;
    (segment_ok(owner) && segment_ok(name)).then_some((owner, name))
}

impl Github {
    pub fn new(connection: &ConnectionInfo) -> Result<Self, Failure> {
        Ok(Self {
            api: Api::new(connection)?,
        })
    }

    fn get(
        &self,
        t: &mut dyn Transport,
        url: String,
        what: &str,
    ) -> Result<crate::client::Reply, Failure> {
        let request = HttpRequest {
            method: Method::Get,
            url,
            headers: vec![
                (ACCEPT.0.into(), ACCEPT.1.into()),
                (API_VERSION.0.into(), API_VERSION.1.into()),
            ],
            body: None,
        };
        self.api.send(t, &request, what)
    }

    fn send_json(
        &self,
        t: &mut dyn Transport,
        method: Method,
        url: String,
        body: &Value,
        what: &str,
    ) -> Result<crate::client::Reply, Failure> {
        let request = HttpRequest::json(method, url, body).header(API_VERSION.0, API_VERSION.1);
        self.api.send(t, &request, what)
    }

    /// The repository with id `r`, and its current name.
    fn named(&self, t: &mut dyn Transport, r: &RepoRef) -> Result<Named, Failure> {
        let what = format!("the repository {}", r.qualified());
        let reply = self.get(t, self.api.endpoint(&["repositories", &r.id], &[]), &what)?;
        let full = string(&reply.json, "full_name").ok_or_else(|| invalid(&what))?;
        let (owner, name) = split_path(&full).ok_or_else(|| invalid(&what))?;
        Ok(Named {
            owner: owner.to_owned(),
            name: name.to_owned(),
            json: reply.json,
        })
    }

    fn repo_url(&self, n: &Named, rest: &[&str], query: &[(&str, &str)]) -> String {
        let mut segments = vec!["repos", n.owner.as_str(), n.name.as_str()];
        segments.extend_from_slice(rest);
        self.api.endpoint(&segments, query)
    }

    fn repository(&self, json: &Value) -> Result<RepositoryRecord, Failure> {
        let id = number(json, "id").ok_or_else(|| invalid("a repository"))?;
        let path = text(json, "full_name", 201).ok_or_else(|| invalid("a repository"))?;
        let visibility = text(json, "visibility", 20).or_else(|| {
            json.get("private")
                .and_then(Value::as_bool)
                .map(|p| if p { "private" } else { "public" }.to_owned())
        });
        Ok(RepositoryRecord {
            id: format!("gh:{id}"),
            name: text(json, "name", 100).unwrap_or_else(|| path.clone()),
            default_branch: text(json, "default_branch", 255),
            visibility,
            archived: boolean(json, "archived"),
            web_url: self
                .api
                .web_link(json.get("html_url").and_then(Value::as_str))
                .unwrap_or_else(|| format!("{}{path}", self.api.web.root())),
            http_url: string(json, "clone_url"),
            ssh_url: string(json, "ssh_url"),
            description: text(json, "description", 500),
            updated_at: json
                .get("pushed_at")
                .or_else(|| json.get("updated_at"))
                .and_then(Value::as_str)
                .and_then(timestamp),
            path,
        })
    }

    fn pull(&self, json: &Value) -> Result<ChangeRequestRecord, Failure> {
        let num = number(json, "number").ok_or_else(|| invalid("a pull request"))?;
        let merged = json.get("merged_at").is_some_and(|m| !m.is_null());
        let state = match json.get("state").and_then(Value::as_str) {
            Some("closed") if merged => "merged".to_owned(),
            Some(state) => clean(state, 20),
            None => "unknown".to_owned(),
        };
        let branch = |side: &str| {
            json.get(side)
                .and_then(|s| text(s, "ref", 255))
                .unwrap_or_default()
        };
        let repository = json
            .get("base")
            .and_then(|b| b.get("repo"))
            .and_then(|r| number(r, "id"))
            .map(|id| format!("gh:{id}"));
        Ok(ChangeRequestRecord {
            number: num,
            title: text(json, "title", 255).unwrap_or_default(),
            state,
            source: branch("head"),
            target: branch("base"),
            author: json.get("user").and_then(|u| text(u, "login", 100)),
            draft: boolean(json, "draft"),
            web_url: self
                .api
                .web_link(json.get("html_url").and_then(Value::as_str))
                .unwrap_or_default(),
            repository,
            existing: false,
        })
    }

    fn run(&self, json: &Value) -> Result<PipelineRecord, Failure> {
        let id = number(json, "id").ok_or_else(|| invalid("a workflow run"))?;
        let status = string(json, "status").unwrap_or_else(|| "unknown".into());
        let conclusion = string(json, "conclusion");
        let shown = match &conclusion {
            Some(c) if status == "completed" => format!("completed/{c}"),
            _ => status.clone(),
        };
        let created = json
            .get("run_started_at")
            .or_else(|| json.get("created_at"))
            .and_then(Value::as_str)
            .and_then(timestamp);
        let finished = (status == "completed")
            .then(|| {
                json.get("updated_at")
                    .and_then(Value::as_str)
                    .and_then(timestamp)
            })
            .flatten();
        Ok(PipelineRecord {
            id: id.to_string(),
            state: run_state(&status, conclusion.as_deref()),
            provider_status: clean(&shown, 40),
            reference: text(json, "head_branch", 255),
            sha: text(json, "head_sha", 64),
            web_url: self
                .api
                .web_link(json.get("html_url").and_then(Value::as_str)),
            created_at: created,
            finished_at: finished,
            duration_seconds: match (created, finished) {
                (Some(start), Some(end)) if end >= start => Some((end - start) / 1_000),
                _ => None,
            },
        })
    }

    fn issue(&self, json: &Value) -> Result<IssueRecord, Failure> {
        let num = number(json, "number").ok_or_else(|| invalid("an issue"))?;
        Ok(IssueRecord {
            number: num,
            title: text(json, "title", 255).unwrap_or_default(),
            state: text(json, "state", 20).unwrap_or_else(|| "unknown".into()),
            author: json.get("user").and_then(|u| text(u, "login", 100)),
            web_url: self
                .api
                .web_link(json.get("html_url").and_then(Value::as_str))
                .unwrap_or_default(),
            updated_at: json
                .get("updated_at")
                .and_then(Value::as_str)
                .and_then(timestamp),
        })
    }

    /// A search result (`/search/issues`) that is a pull request: no
    /// branches in it.
    fn found_pull(&self, json: &Value) -> Result<ChangeRequestRecord, Failure> {
        let num = number(json, "number").ok_or_else(|| invalid("a pull request"))?;
        Ok(ChangeRequestRecord {
            number: num,
            title: text(json, "title", 255).unwrap_or_default(),
            state: text(json, "state", 20).unwrap_or_else(|| "unknown".into()),
            source: String::new(),
            target: String::new(),
            author: json.get("user").and_then(|u| text(u, "login", 100)),
            draft: boolean(json, "draft"),
            web_url: self
                .api
                .web_link(json.get("html_url").and_then(Value::as_str))
                .unwrap_or_default(),
            repository: None,
            existing: false,
        })
    }

    /// The newest workflow run of a ref (a branch, or a commit by sha).
    fn latest_run(
        &self,
        t: &mut dyn Transport,
        n: &Named,
        reference: &str,
        event: Option<&str>,
    ) -> Result<Option<PipelineRecord>, Failure> {
        let reference = check_ref(reference)?;
        let is_sha = reference.len() == 40 && reference.chars().all(|c| c.is_ascii_hexdigit());
        let mut query = vec![
            (if is_sha { "head_sha" } else { "branch" }, reference),
            ("per_page", "1"),
        ];
        if let Some(event) = event {
            query.push(("event", event));
        }
        let what = format!("the workflow runs of {}/{}@{reference}", n.owner, n.name);
        let reply = self.get(t, self.repo_url(n, &["actions", "runs"], &query), &what)?;
        match reply
            .json
            .get("workflow_runs")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid(&what))?
            .first()
        {
            Some(run) => self.run(run).map(Some),
            None => Ok(None),
        }
    }

    /// Searches pull requests (`is:pr is:open` plus `filter`).
    fn search_pulls(
        &self,
        t: &mut dyn Transport,
        filter: &str,
        repo: Option<&Named>,
        limit: u32,
    ) -> Result<Vec<ChangeRequestRecord>, Failure> {
        let mut q = format!("is:pr is:open {filter}");
        if let Some(n) = repo {
            q.push_str(&format!(" repo:{}/{}", n.owner, n.name));
        }
        let size = limit.clamp(1, 100).to_string();
        let url = self.api.endpoint(
            &["search", "issues"],
            &[
                ("q", &q),
                ("sort", "updated"),
                ("order", "desc"),
                ("per_page", &size),
            ],
        );
        let what = "your pull requests";
        let reply = self.get(t, url, what)?;
        reply
            .json
            .get("items")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid(what))?
            .iter()
            .map(|i| self.found_pull(i))
            .collect()
    }

    /// `github.checks`: check runs and commit statuses of a ref, reduced
    /// like the old plugin's `checks` action.
    fn checks(&self, t: &mut dyn Transport, n: &Named, args: &Value) -> Result<Value, Failure> {
        let reference = args
            .get("ref")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| string(&n.json, "default_branch"))
            .ok_or_else(|| fail(codes::INVALID_REF, "choose a branch, tag or commit"))?;
        let reference = check_ref(&reference)?.to_owned();
        let what = format!("the checks of {}/{}@{reference}", n.owner, n.name);
        let runs = self.api.list(
            t,
            self.repo_url(
                n,
                &["commits", &reference, "check-runs"],
                &[("per_page", "100")],
            ),
            Some("check_runs"),
            &what,
            500,
        )?;
        let statuses = self.get(
            t,
            self.repo_url(n, &["commits", &reference, "status"], &[]),
            &what,
        )?;
        let mut counts = Counts::default();
        let mut failing = Vec::new();
        for run in &runs.items {
            match check_run_outcome(run) {
                Outcome::Passed => counts.passed += 1,
                Outcome::Pending => counts.pending += 1,
                Outcome::Failed => {
                    counts.failed += 1;
                    failing.push(clean(&string(run, "name").unwrap_or_default(), 120));
                }
            }
        }
        for status in statuses
            .json
            .get("statuses")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            match status_outcome(status) {
                Outcome::Passed => counts.passed += 1,
                Outcome::Pending => counts.pending += 1,
                Outcome::Failed => {
                    counts.failed += 1;
                    failing.push(clean(&string(status, "context").unwrap_or_default(), 120));
                }
            }
        }
        Ok(json!({
            "state": overall(counts),
            "passed": counts.passed,
            "failed": counts.failed,
            "pending": counts.pending,
            "failing": failing,
            "truncated": runs.more,
        }))
    }

    fn dispatch(
        &self,
        t: &mut dyn Transport,
        n: &Named,
        workflow: &str,
        reference: &str,
        inputs: &Map<String, Value>,
    ) -> Result<PipelineRecord, Failure> {
        let workflow = check_workflow(workflow)?;
        let reference = check_ref(reference)?;
        check_inputs(inputs)?;
        self.send_json(
            t,
            Method::Post,
            self.repo_url(n, &["actions", "workflows", workflow, "dispatches"], &[]),
            &json!({"ref": reference, "inputs": inputs}),
            &format!("a run of {workflow} on {reference}"),
        )?;
        // 204 without the run: the newest dispatched run of the ref.
        Ok(self
            .latest_run(t, n, reference, Some("workflow_dispatch"))?
            .unwrap_or(PipelineRecord {
                id: String::new(),
                state: PipelineState::Pending,
                provider_status: "queued".into(),
                reference: Some(reference.to_owned()),
                sha: None,
                web_url: None,
                created_at: None,
                finished_at: None,
                duration_seconds: None,
            }))
    }
}

/// A workflow file of `.github/workflows` (`ci.yml`) or a workflow id.
pub fn check_workflow(text: &str) -> Result<&str, Failure> {
    let text = text.trim();
    let file = (text.ends_with(".yml") || text.ends_with(".yaml"))
        && (5..=100).contains(&text.len())
        && !text.starts_with('.')
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    let id = (1..=20).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_digit());
    if file || id {
        Ok(text)
    } else {
        Err(fail(
            codes::INVALID_PARAMETERS,
            format!(
                "“{}” is not a workflow file such as ci.yml",
                clean(text, 60)
            ),
        ))
    }
}

/// Workflow inputs (`workflow_dispatch`): at most [`MAX_INPUTS`], names
/// like GitHub's, string values without control characters, ≤ 1 KiB.
pub fn check_inputs(inputs: &Map<String, Value>) -> Result<(), Failure> {
    if inputs.len() > MAX_INPUTS {
        return Err(fail(
            codes::INVALID_VARIABLES,
            format!("at most {MAX_INPUTS} workflow inputs"),
        ));
    }
    for (name, value) in inputs {
        let name_ok = (1..=100).contains(&name.len())
            && name
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
        let value_ok = match value {
            Value::String(s) => s.len() <= 1024 && !s.chars().any(char::is_control),
            Value::Bool(_) | Value::Number(_) => true,
            _ => false,
        };
        if !name_ok || !value_ok {
            return Err(fail(
                codes::INVALID_VARIABLES,
                format!("the workflow input “{}” is not valid", clean(name, 60)),
            ));
        }
    }
    Ok(())
}

/// A workflow run in the closed set (design §2.2).
pub fn run_state(status: &str, conclusion: Option<&str>) -> PipelineState {
    match status {
        "queued" | "waiting" | "requested" | "pending" => PipelineState::Pending,
        "in_progress" => PipelineState::Running,
        "completed" => match conclusion {
            Some("success") => PipelineState::Success,
            Some("failure" | "timed_out" | "startup_failure") => PipelineState::Failed,
            Some("cancelled") => PipelineState::Cancelled,
            Some("skipped" | "neutral") => PipelineState::Skipped,
            Some("action_required") => PipelineState::Manual,
            _ => PipelineState::Unknown,
        },
        _ => PipelineState::Unknown,
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub passed: u64,
    pub failed: u64,
    pub pending: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Passed,
    Failed,
    Pending,
}

/// A check run: completed with success, neutral or skipped passes; any
/// other conclusion fails; not completed is pending.
pub fn check_run_outcome(run: &Value) -> Outcome {
    if run.get("status").and_then(Value::as_str) != Some("completed") {
        return Outcome::Pending;
    }
    match run.get("conclusion").and_then(Value::as_str) {
        Some("success" | "neutral" | "skipped") => Outcome::Passed,
        _ => Outcome::Failed,
    }
}

/// A commit status: success, pending, or failure and error.
pub fn status_outcome(status: &Value) -> Outcome {
    match status.get("state").and_then(Value::as_str) {
        Some("success") => Outcome::Passed,
        Some("pending") => Outcome::Pending,
        _ => Outcome::Failed,
    }
}

pub fn overall(counts: Counts) -> &'static str {
    if counts.failed > 0 {
        "failing"
    } else if counts.pending > 0 {
        "pending"
    } else if counts.passed > 0 {
        "ok"
    } else {
        "none"
    }
}

fn per_page(n: u32) -> String {
    n.clamp(1, 100).to_string()
}

fn matches_search(name: &str, search: Option<&str>) -> bool {
    search
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_none_or(|s| name.to_ascii_lowercase().contains(&s.to_ascii_lowercase()))
}

impl Provider for Github {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Github
    }

    fn probe(&self, t: &mut dyn Transport) -> Result<Probe, Failure> {
        let me = self.get(t, self.api.endpoint(&["user"], &[]), "the signed-in user")?;
        let account = text(&me.json, "login", 100).ok_or_else(|| invalid("the signed-in user"))?;
        // Classic tokens report their scopes; fine-grained ones do not (the
        // host hands the header over only once it allows it).
        let scopes: Option<Vec<String>> = me.raw.header("x-oauth-scopes").map(|s| {
            s.split(',')
                .map(|x| clean(x.trim(), 40))
                .filter(|x| !x.is_empty())
                .collect()
        });
        // GHES says its version in /meta; GitHub.com has none there.
        let version =
            match self
                .api
                .get_optional(t, self.api.endpoint(&["meta"], &[]), "GitHub's version")?
            {
                Some(meta) => {
                    text(&meta.json, "installed_version", 20).map(|v| format!("GHES {v}"))
                }
                None => None,
            };
        let excess_scopes = scopes
            .iter()
            .flatten()
            .filter(|s| EXCESS_SCOPES.contains(&s.as_str()))
            .cloned()
            .collect();
        // Fine-grained tokens: what is allowed shows on first use (a 403
        // marks the connection `insufficientScope`).
        let write = scopes
            .as_ref()
            .is_none_or(|s| s.iter().any(|x| x == "repo" || x == "public_repo"));
        let access = if write {
            Access::ReadWrite
        } else {
            Access::Read
        };
        t.report(ConnectionStatus::Ok, None);
        Ok(Probe {
            account,
            version: version.clone(),
            scopes,
            expires_at: None,
            capabilities: Capabilities {
                repositories: true,
                branches: true,
                commits: true,
                change_requests: access,
                pipelines: access,
                pipeline_trigger: write,
                issues: access,
                extensions: EXTENSIONS.iter().map(|e| (*e).to_owned()).collect(),
                api_version: version,
            },
            excess_scopes,
        })
    }

    fn list_repositories(
        &self,
        t: &mut dyn Transport,
        q: &RepoQuery,
    ) -> Result<Page<RepositoryRecord>, Failure> {
        let page = q.page.page.max(1).to_string();
        let size = per_page(q.page.per_page);
        let reply = self.get(
            t,
            self.api.endpoint(
                &["user", "repos"],
                &[
                    ("sort", "updated"),
                    ("direction", "desc"),
                    ("page", &page),
                    ("per_page", &size),
                ],
            ),
            "your repositories",
        )?;
        let mut items = Vec::new();
        for repo in reply
            .json
            .as_array()
            .ok_or_else(|| invalid("your repositories"))?
        {
            let record = self.repository(repo)?;
            if matches_search(&record.path, q.search.as_deref()) {
                items.push(record);
            }
        }
        Ok(Page {
            items,
            next: reply.next.is_some().then(|| q.page.page.max(1) + 1),
        })
    }

    fn get_repository(
        &self,
        t: &mut dyn Transport,
        r: &RepoRef,
    ) -> Result<RepositoryRecord, Failure> {
        let n = self.named(t, r)?;
        self.repository(&n.json)
    }

    fn repository_by_path(
        &self,
        t: &mut dyn Transport,
        path: &str,
    ) -> Result<Option<RepositoryRecord>, Failure> {
        let Some((owner, name)) = split_path(path) else {
            return Ok(None);
        };
        let what = format!("the repository {owner}/{name}");
        match self
            .api
            .get_optional(t, self.api.endpoint(&["repos", owner, name], &[]), &what)?
        {
            Some(reply) => self.repository(&reply.json).map(Some),
            None => Ok(None),
        }
    }

    fn list_branches(
        &self,
        t: &mut dyn Transport,
        r: &RepoRef,
        q: &BranchQuery,
    ) -> Result<Page<BranchRecord>, Failure> {
        let n = self.named(t, r)?;
        let default = string(&n.json, "default_branch");
        let page = q.page.page.max(1).to_string();
        let size = per_page(q.page.per_page);
        let what = format!("the branches of {}/{}", n.owner, n.name);
        let reply = self.get(
            t,
            self.repo_url(&n, &["branches"], &[("page", &page), ("per_page", &size)]),
            &what,
        )?;
        let items = reply
            .json
            .as_array()
            .ok_or_else(|| invalid(&what))?
            .iter()
            .filter_map(|b| {
                let name = text(b, "name", 255)?;
                matches_search(&name, q.search.as_deref()).then(|| BranchRecord {
                    sha: b.get("commit").and_then(|c| text(c, "sha", 64)),
                    default: default.as_deref() == Some(name.as_str()),
                    protected: boolean(b, "protected"),
                    name,
                })
            })
            .collect();
        Ok(Page {
            items,
            next: reply.next.is_some().then(|| q.page.page.max(1) + 1),
        })
    }

    fn list_commits(
        &self,
        t: &mut dyn Transport,
        r: &RepoRef,
        q: &CommitQuery,
    ) -> Result<Vec<CommitRecord>, Failure> {
        let n = self.named(t, r)?;
        let size = per_page(q.limit.max(1));
        let mut query = vec![("per_page", size.as_str())];
        if let Some(reference) = &q.reference {
            query.push(("sha", check_ref(reference)?));
        }
        let what = format!("the commits of {}/{}", n.owner, n.name);
        let reply = self.get(t, self.repo_url(&n, &["commits"], &query), &what)?;
        Ok(reply
            .json
            .as_array()
            .ok_or_else(|| invalid(&what))?
            .iter()
            .filter_map(|c| {
                let commit = c.get("commit")?;
                Some(CommitRecord {
                    sha: text(c, "sha", 64)?,
                    title: commit
                        .get("message")
                        .and_then(Value::as_str)
                        .and_then(|m| m.lines().next())
                        .map(|l| clean(l, 255))
                        .unwrap_or_default(),
                    author: commit.get("author").and_then(|a| text(a, "name", 255)),
                    committed_at: commit
                        .get("committer")
                        .and_then(|a| a.get("date"))
                        .and_then(Value::as_str)
                        .and_then(timestamp),
                    web_url: self.api.web_link(c.get("html_url").and_then(Value::as_str)),
                })
            })
            .collect())
    }

    fn list_change_requests(
        &self,
        t: &mut dyn Transport,
        r: Option<&RepoRef>,
        q: &CrQuery,
    ) -> Result<Vec<ChangeRequestRecord>, Failure> {
        let named = r.map(|r| self.named(t, r)).transpose()?;
        match (q.filter, &named) {
            (CrFilter::Open, Some(n)) => {
                let size = per_page(q.limit);
                let what = format!("the pull requests of {}/{}", n.owner, n.name);
                let reply = self.get(
                    t,
                    self.repo_url(
                        n,
                        &["pulls"],
                        &[
                            ("state", "open"),
                            ("sort", "updated"),
                            ("direction", "desc"),
                            ("per_page", &size),
                        ],
                    ),
                    &what,
                )?;
                reply
                    .json
                    .as_array()
                    .ok_or_else(|| invalid(&what))?
                    .iter()
                    .map(|p| self.pull(p))
                    .collect()
            }
            (CrFilter::Open, None) => self.search_pulls(t, "involves:@me", None, q.limit),
            (CrFilter::Mine, n) => self.search_pulls(t, "author:@me", n.as_ref(), q.limit),
            (CrFilter::Review, n) => {
                self.search_pulls(t, "review-requested:@me", n.as_ref(), q.limit)
            }
        }
    }

    fn create_change_request(
        &self,
        t: &mut dyn Transport,
        r: &RepoRef,
        c: &CrCreate,
    ) -> Result<ChangeRequestRecord, Failure> {
        let source = check_ref(&c.source)?;
        let target = check_ref(&c.target)?;
        let title = c.title.trim();
        if title.is_empty() || title.chars().count() > 256 {
            return Err(fail(
                codes::INVALID_PARAMETERS,
                "a pull request needs a title of 1 to 256 characters",
            ));
        }
        let n = self.named(t, r)?;
        let head = format!("{}:{source}", n.owner);
        let what = format!("the pull requests of {}/{}", n.owner, n.name);
        let open = self.get(
            t,
            self.repo_url(
                &n,
                &["pulls"],
                &[
                    ("state", "open"),
                    ("head", &head),
                    ("base", target),
                    ("per_page", "1"),
                ],
            ),
            &what,
        )?;
        if let Some(existing) = open.json.as_array().and_then(|l| l.first()) {
            let mut record = self.pull(existing)?;
            record.existing = true;
            return Ok(record);
        }
        let mut body = json!({
            "title": title,
            "head": source,
            "base": target,
            "draft": c.draft,
        });
        if let Some(description) = c.description.as_deref().filter(|d| !d.trim().is_empty()) {
            body["body"] = json!(description.chars().take(10_000).collect::<String>());
        }
        let reply = self.send_json(
            t,
            Method::Post,
            self.repo_url(&n, &["pulls"], &[]),
            &body,
            &format!("a new pull request in {}/{}", n.owner, n.name),
        )?;
        self.pull(&reply.json)
    }

    fn list_pipelines(
        &self,
        t: &mut dyn Transport,
        r: &RepoRef,
        q: &PipelineQuery,
    ) -> Result<Vec<PipelineRecord>, Failure> {
        let n = self.named(t, r)?;
        let size = per_page(q.limit.max(1));
        let mut query = vec![("per_page", size.as_str())];
        if let Some(reference) = &q.reference {
            query.push(("branch", check_ref(reference)?));
        }
        let what = format!("the workflow runs of {}/{}", n.owner, n.name);
        let reply = self.get(t, self.repo_url(&n, &["actions", "runs"], &query), &what)?;
        reply
            .json
            .get("workflow_runs")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid(&what))?
            .iter()
            .map(|run| self.run(run))
            .collect()
    }

    fn pipeline_status(
        &self,
        t: &mut dyn Transport,
        r: &RepoRef,
        s: &PipelineSelector,
    ) -> Result<Option<PipelineRecord>, Failure> {
        let n = self.named(t, r)?;
        match s {
            PipelineSelector::Id(id) => {
                if id.is_empty() || id.len() > 20 || !id.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(fail(codes::INVALID_PARAMETERS, "a run id is a number"));
                }
                let what = format!("the workflow run {id} of {}/{}", n.owner, n.name);
                let reply = self.get(t, self.repo_url(&n, &["actions", "runs", id], &[]), &what)?;
                self.run(&reply.json).map(Some)
            }
            PipelineSelector::Ref(reference) => self.latest_run(t, &n, reference, None),
            PipelineSelector::ChangeRequest(number) => {
                let number = number.max(&1).to_string();
                let what = format!("the pull request {}/{}#{number}", n.owner, n.name);
                let reply = self.get(t, self.repo_url(&n, &["pulls", &number], &[]), &what)?;
                let sha = reply
                    .json
                    .get("head")
                    .and_then(|h| string(h, "sha"))
                    .ok_or_else(|| invalid(&what))?;
                self.latest_run(t, &n, &sha, None)
            }
        }
    }

    fn trigger_pipeline(
        &self,
        t: &mut dyn Transport,
        r: &RepoRef,
        p: &PipelineTrigger,
    ) -> Result<PipelineRecord, Failure> {
        if !p.variables.is_empty() {
            return Err(fail(
                codes::CAPABILITY_UNSUPPORTED,
                "GitHub has no pipeline variables: give workflow inputs to the \
                 github.workflow-dispatch extension",
            ));
        }
        let workflow = p.workflow.as_deref().ok_or_else(|| {
            fail(
                codes::INVALID_PARAMETERS,
                "choose the workflow file to run (such as ci.yml)",
            )
        })?;
        let n = self.named(t, r)?;
        self.dispatch(t, &n, workflow, &p.reference, &Map::new())
    }

    fn list_issues(
        &self,
        t: &mut dyn Transport,
        r: &RepoRef,
        q: &IssueQuery,
    ) -> Result<Vec<IssueRecord>, Failure> {
        let n = self.named(t, r)?;
        let state = match q.state {
            IssueState::Open => "open",
            IssueState::Closed => "closed",
            IssueState::All => "all",
        };
        let size = per_page(q.limit.max(1));
        let what = format!("the issues of {}/{}", n.owner, n.name);
        let reply = self.get(
            t,
            self.repo_url(
                &n,
                &["issues"],
                &[("state", state), ("sort", "updated"), ("per_page", &size)],
            ),
            &what,
        )?;
        reply
            .json
            .as_array()
            .ok_or_else(|| invalid(&what))?
            .iter()
            // GitHub lists pull requests among issues.
            .filter(|i| i.get("pull_request").is_none())
            .map(|i| self.issue(i))
            .collect()
    }

    fn get_issue(
        &self,
        t: &mut dyn Transport,
        r: &RepoRef,
        number: u64,
    ) -> Result<IssueRecord, Failure> {
        let n = self.named(t, r)?;
        let number = number.max(1).to_string();
        let what = format!("the issue {}/{}#{number}", n.owner, n.name);
        let reply = self.get(t, self.repo_url(&n, &["issues", &number], &[]), &what)?;
        if reply.json.get("pull_request").is_some() {
            return Err(fail(
                codes::NOT_FOUND,
                format!("#{number} is a pull request, not an issue"),
            ));
        }
        self.issue(&reply.json)
    }

    fn web_url(&self, target: &WebTarget) -> Result<String, Failure> {
        let root = self.api.web.root();
        let path = |path: &str| -> Result<String, Failure> {
            split_path(path)
                .map(|(o, n)| format!("{o}/{n}"))
                .ok_or_else(|| {
                    fail(
                        codes::INVALID_REPOSITORY,
                        format!("“{}” is not owner/name", clean(path, 60)),
                    )
                })
        };
        let text = match target {
            WebTarget::Repository { path: p, page } => {
                let suffix = match page {
                    RepoPage::Home => "",
                    RepoPage::Branches => "/branches",
                    RepoPage::Commits => "/commits",
                    RepoPage::Pipelines => "/actions",
                    RepoPage::Issues => "/issues",
                    RepoPage::Changes => "/pulls",
                };
                format!("{root}{}{suffix}", path(p)?)
            }
            WebTarget::ChangeRequest { path: p, number } => {
                format!("{root}{}/pull/{}", path(p)?, number.max(&1))
            }
            WebTarget::Issue { path: p, number } => {
                format!("{root}{}/issues/{}", path(p)?, number.max(&1))
            }
            WebTarget::Pipeline { path: p, id } => {
                if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(fail(codes::INVALID_PARAMETERS, "a run id is a number"));
                }
                format!("{root}{}/actions/runs/{id}", path(p)?)
            }
            WebTarget::MyChangeRequests => format!("{root}pulls"),
        };
        self.api.page(&text).ok_or_else(|| {
            fail(
                codes::URL_NOT_ALLOWED,
                format!("only https pages on {} can be opened", self.api.host),
            )
        })
    }

    fn extension(
        &self,
        t: &mut dyn Transport,
        id: &str,
        r: &RepoRef,
        args: &Value,
    ) -> Result<Value, Failure> {
        match id {
            "github.checks" => {
                let n = self.named(t, r)?;
                self.checks(t, &n, args)
            }
            "github.review-requests" => {
                let n = self.named(t, r)?;
                let limit = args
                    .get("limit")
                    .and_then(Value::as_u64)
                    .map_or(20, |l| l.clamp(1, 100) as u32);
                let items = self.search_pulls(t, "review-requested:@me", Some(&n), limit)?;
                Ok(json!({ "items": items }))
            }
            "github.workflow-dispatch" => {
                let workflow = args
                    .get("workflow")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        fail(codes::INVALID_PARAMETERS, "choose the workflow file to run")
                    })?;
                let reference = args.get("ref").and_then(Value::as_str).ok_or_else(|| {
                    fail(codes::INVALID_REF, "choose the branch or tag to run on")
                })?;
                let inputs = match args.get("inputs") {
                    None | Some(Value::Null) => Map::new(),
                    Some(Value::Object(map)) => map.clone(),
                    Some(_) => {
                        return Err(fail(
                            codes::INVALID_VARIABLES,
                            "workflow inputs are an object of names and values",
                        ));
                    }
                };
                let n = self.named(t, r)?;
                let run = self.dispatch(t, &n, workflow, reference, &inputs)?;
                Ok(serde_json::to_value(run).unwrap_or(Value::Null))
            }
            _ => Err(fail(
                codes::CAPABILITY_UNSUPPORTED,
                format!("GitHub has no extension “{}”", clean(id, 64)),
            )),
        }
    }
}
