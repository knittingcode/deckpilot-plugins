//! The GitLab adapter: GitLab.com and self-managed GitLab (any port, any
//! path prefix: `https://code.example.com:8443/gitlab/api/v4`), REST API
//! v4. Moved and adapted from `official/gitlab/src/api.rs` and `lib.rs`
//! (paging, rate limits, the reference check, the pipeline-state
//! reduction); that plugin stays as it is.
//!
//! Repositories are named by their numeric id (`gl:4711`), never by path:
//! a rename or a transfer changes nothing.

pub mod compat;
#[cfg(test)]
mod tests;

use serde_json::{Value, json};

use crate::client::{Api, boolean, check_ref, number, string, text};
use crate::errors::{Failure, clean, codes, fail};
use crate::provider::{
    Access, BranchQuery, Capabilities, CommitQuery, ConnectionInfo, CrCreate, CrFilter, CrQuery,
    IssueQuery, IssueState, PipelineQuery, PipelineSelector, PipelineTrigger, Provider,
    ProviderKind, RepoPage, RepoQuery, RepoRef, WebTarget,
};
use crate::records::{
    BranchRecord, ChangeRequestRecord, CommitRecord, IssueRecord, Page, PipelineRecord,
    PipelineState, Probe, RepositoryRecord, timestamp,
};
use crate::transport::{Method, Transport};
use compat::{Gates, Version};

/// Extensions the `extension` action runs. Pipeline variables
/// (`gitlab.pipeline-variables` of the manifest's connection type) are a
/// parameter of `trigger-pipeline`, not an extension call.
pub const EXTENSIONS: [&str; 2] = ["gitlab.review-requests", "gitlab.approvals"];
/// Variable name prefixes GitLab reserves for itself.
pub const RESERVED_VARIABLE_PREFIXES: [&str; 2] = ["CI_", "GITLAB_"];
/// Most pipeline variables one trigger sends.
pub const MAX_VARIABLES: usize = 50;
/// Longest pipeline variable value, in bytes.
pub const MAX_VARIABLE_VALUE: usize = 1024;
/// Scopes beyond what DeckPilot ever needs: the app warns about them.
pub const EXCESS_SCOPES: [&str; 5] = [
    "sudo",
    "admin_mode",
    "write_repository",
    "create_runner",
    "k8s_proxy",
];

#[derive(Debug, Clone)]
pub struct Gitlab {
    pub api: Api,
    pub gates: Gates,
}

impl Gitlab {
    /// The adapter for `connection`; the version comes from its last
    /// probe (`capabilities.apiVersion`, "GitLab 16.11.2"), if any.
    pub fn new(connection: &ConnectionInfo) -> Result<Self, Failure> {
        let version = connection
            .capabilities
            .as_ref()
            .and_then(|c| c.get("apiVersion"))
            .and_then(Value::as_str)
            .and_then(|v| Version::parse(v.trim_start_matches("GitLab ")));
        Ok(Self {
            api: Api::new(connection)?,
            gates: Gates::new(version),
        })
    }

    fn project_url(&self, r: &RepoRef, rest: &[&str], query: &[(&str, &str)]) -> String {
        let mut segments = vec!["projects", r.id.as_str()];
        segments.extend_from_slice(rest);
        self.api.endpoint(&segments, query)
    }

    fn repository(&self, json: &Value) -> Result<RepositoryRecord, Failure> {
        let id = number(json, "id").ok_or_else(|| invalid("a project"))?;
        let path = text(json, "path_with_namespace", 1000).ok_or_else(|| invalid("a project"))?;
        Ok(RepositoryRecord {
            id: format!("gl:{id}"),
            name: text(json, "name", 255).unwrap_or_else(|| path.clone()),
            default_branch: text(json, "default_branch", 255),
            visibility: text(json, "visibility", 20),
            archived: boolean(json, "archived"),
            web_url: self
                .api
                .web_link(json.get("web_url").and_then(Value::as_str))
                .unwrap_or_else(|| format!("{}{path}", self.api.web.root())),
            http_url: string(json, "http_url_to_repo"),
            ssh_url: string(json, "ssh_url_to_repo"),
            description: text(json, "description", 500),
            updated_at: json
                .get("last_activity_at")
                .and_then(Value::as_str)
                .and_then(timestamp),
            path,
        })
    }

    fn change_request(&self, json: &Value) -> Result<ChangeRequestRecord, Failure> {
        let iid = number(json, "iid").ok_or_else(|| invalid("a merge request"))?;
        let draft = ["draft", "work_in_progress"]
            .iter()
            .any(|key| boolean(json, key));
        let state = match json.get("state").and_then(Value::as_str) {
            Some("opened") => "open".to_owned(),
            Some(other) => clean(other, 20),
            None => "unknown".to_owned(),
        };
        Ok(ChangeRequestRecord {
            number: iid,
            title: text(json, "title", 255).unwrap_or_default(),
            state,
            source: text(json, "source_branch", 255).unwrap_or_default(),
            target: text(json, "target_branch", 255).unwrap_or_default(),
            author: json.get("author").and_then(|a| text(a, "username", 255)),
            draft,
            web_url: self
                .api
                .web_link(json.get("web_url").and_then(Value::as_str))
                .or_else(|| {
                    let path = reference_path(json, '!')?;
                    self.web_url(&WebTarget::ChangeRequest { path, number: iid })
                        .ok()
                })
                .unwrap_or_default(),
            repository: json
                .get("project_id")
                .and_then(Value::as_u64)
                .map(|id| format!("gl:{id}")),
            existing: false,
        })
    }

    fn pipeline(&self, json: &Value) -> Result<PipelineRecord, Failure> {
        let id = number(json, "id").ok_or_else(|| invalid("a pipeline"))?;
        let status = string(json, "status").unwrap_or_else(|| "unknown".into());
        Ok(PipelineRecord {
            id: id.to_string(),
            state: pipeline_state(&status),
            provider_status: clean(&status, 40),
            reference: text(json, "ref", 255),
            sha: text(json, "sha", 64),
            web_url: self
                .api
                .web_link(json.get("web_url").and_then(Value::as_str)),
            created_at: json
                .get("created_at")
                .and_then(Value::as_str)
                .and_then(timestamp),
            finished_at: json
                .get("finished_at")
                .and_then(Value::as_str)
                .and_then(timestamp),
            duration_seconds: number(json, "duration"),
        })
    }

    fn issue(&self, json: &Value) -> Result<IssueRecord, Failure> {
        let iid = number(json, "iid").ok_or_else(|| invalid("an issue"))?;
        Ok(IssueRecord {
            number: iid,
            title: text(json, "title", 255).unwrap_or_default(),
            state: match json.get("state").and_then(Value::as_str) {
                Some("opened") => "open".to_owned(),
                Some(other) => clean(other, 20),
                None => "unknown".to_owned(),
            },
            author: json.get("author").and_then(|a| text(a, "username", 255)),
            web_url: self
                .api
                .web_link(json.get("web_url").and_then(Value::as_str))
                .or_else(|| {
                    let path = reference_path(json, '#')?;
                    self.web_url(&WebTarget::Issue { path, number: iid }).ok()
                })
                .unwrap_or_default(),
            updated_at: json
                .get("updated_at")
                .and_then(Value::as_str)
                .and_then(timestamp),
        })
    }

    /// The signed-in user: `(id, username)`.
    fn me(&self, t: &mut dyn Transport) -> Result<(u64, String), Failure> {
        let reply = self
            .api
            .get(t, self.api.endpoint(&["user"], &[]), "the signed-in user")?;
        let id = number(&reply.json, "id").ok_or_else(|| invalid("the signed-in user"))?;
        let username =
            text(&reply.json, "username", 255).ok_or_else(|| invalid("the signed-in user"))?;
        Ok((id, username))
    }

    fn approvals(
        &self,
        t: &mut dyn Transport,
        r: &RepoRef,
        args: &Value,
    ) -> Result<Value, Failure> {
        let iid = args
            .get("changeRequest")
            .and_then(Value::as_u64)
            .filter(|n| *n >= 1)
            .ok_or_else(|| {
                fail(
                    codes::INVALID_PARAMETERS,
                    "choose a merge request (its number)",
                )
            })?
            .to_string();
        let what = format!("the approvals of !{iid}");
        let reply = self.api.get(
            t,
            self.project_url(r, &["merge_requests", &iid, "approvals"], &[]),
            &what,
        )?;
        let approved_by: Vec<String> = reply
            .json
            .get("approved_by")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|a| a.get("user").and_then(|u| text(u, "username", 255)))
                    .collect()
            })
            .unwrap_or_default();
        Ok(json!({
            "approved": boolean(&reply.json, "approved"),
            "approvalsLeft": number(&reply.json, "approvals_left").unwrap_or(0),
            "approvedBy": approved_by,
        }))
    }
}

/// The project path of `references.full` (`group/proj!3`, `group/proj#5`).
fn reference_path(json: &Value, marker: char) -> Option<String> {
    let full = json.get("references")?.get("full")?.as_str()?;
    let (path, _) = full.rsplit_once(marker)?;
    Some(path.to_owned())
}

fn invalid(what: &str) -> Failure {
    fail(
        codes::INVALID_RESPONSE,
        format!("GitLab's answer for {what} is not what was expected"),
    )
}

/// GitLab's pipeline status in the closed set (design §2.2): 1:1, with the
/// waiting states as `pending`.
pub fn pipeline_state(status: &str) -> PipelineState {
    match status {
        "created" | "waiting_for_resource" | "preparing" | "pending" | "scheduled" => {
            PipelineState::Pending
        }
        "running" => PipelineState::Running,
        "success" => PipelineState::Success,
        "failed" => PipelineState::Failed,
        "canceled" | "canceling" => PipelineState::Cancelled,
        "skipped" => PipelineState::Skipped,
        "manual" => PipelineState::Manual,
        _ => PipelineState::Unknown,
    }
}

/// Pipeline variables (threat T9): names `[A-Z_][A-Z0-9_]{0,63}`, values
/// at most [`MAX_VARIABLE_VALUE`] bytes without control characters, at
/// most [`MAX_VARIABLES`], each name once.
pub fn check_variables(variables: &[(String, String)]) -> Result<(), Failure> {
    if variables.len() > MAX_VARIABLES {
        return Err(fail(
            codes::INVALID_VARIABLES,
            format!("at most {MAX_VARIABLES} variables"),
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    for (name, value) in variables {
        let mut chars = name.chars();
        let name_ok = name.len() <= 64
            && chars
                .next()
                .is_some_and(|c| c.is_ascii_uppercase() || c == '_')
            && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
        if !name_ok {
            return Err(fail(
                codes::INVALID_VARIABLES,
                format!(
                    "“{}” is not a variable name (capital letters, digits and _)",
                    clean(name, 64)
                ),
            ));
        }
        if RESERVED_VARIABLE_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
        {
            return Err(fail(
                codes::INVALID_VARIABLES,
                format!("{name} is one of GitLab's own variables (CI_*, GITLAB_*)"),
            ));
        }
        if !seen.insert(name.as_str()) {
            return Err(fail(
                codes::INVALID_VARIABLES,
                format!("{name} is given twice"),
            ));
        }
        if value.len() > MAX_VARIABLE_VALUE
            || value.chars().any(char::is_control)
            || crate::errors::looks_like_secret(value)
        {
            return Err(fail(
                codes::INVALID_VARIABLES,
                format!(
                    "the value of {name} is too long, has control characters or looks like a secret"
                ),
            ));
        }
    }
    Ok(())
}

fn per_page(n: u32) -> String {
    n.clamp(1, 100).to_string()
}

impl Provider for Gitlab {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Gitlab
    }

    fn probe(&self, t: &mut dyn Transport) -> Result<Probe, Failure> {
        let version = self
            .api
            .get_optional(t, self.api.endpoint(&["version"], &[]), "GitLab's version")?
            .and_then(|reply| string(&reply.json, "version"));
        let parsed = version.as_deref().and_then(Version::parse);
        let gates = Gates::new(parsed);
        let (_, account) = self.me(t)?;
        let (scopes, expires_at) = if gates.token_self() {
            match self.api.get_optional(
                t,
                self.api.endpoint(&["personal_access_tokens", "self"], &[]),
                "the token",
            )? {
                Some(reply) => (
                    reply.json.get("scopes").and_then(Value::as_array).map(|s| {
                        s.iter()
                            .filter_map(Value::as_str)
                            .map(|s| clean(s, 40))
                            .collect::<Vec<_>>()
                    }),
                    reply
                        .json
                        .get("expires_at")
                        .and_then(Value::as_str)
                        .and_then(|d| timestamp(&format!("{d}T00:00:00Z"))),
                ),
                None => (None, None),
            }
        } else {
            (None, None)
        };
        // Only what the token is known to allow: unknown scopes (an old
        // GitLab, a 404 on the token endpoint) read, and a 403 on first
        // use of a change says what is missing.
        let write = scopes
            .as_ref()
            .is_some_and(|s| s.iter().any(|x| x == "api"));
        let access = if write {
            Access::ReadWrite
        } else {
            Access::Read
        };
        let excess_scopes = scopes
            .iter()
            .flatten()
            .filter(|s| EXCESS_SCOPES.contains(&s.as_str()))
            .cloned()
            .collect();
        t.report(crate::provider::ConnectionStatus::Ok, None);
        Ok(Probe {
            account,
            version: parsed.map(|v| format!("GitLab {v}")),
            scopes,
            expires_at,
            capabilities: Capabilities {
                repositories: true,
                branches: true,
                commits: true,
                change_requests: access,
                pipelines: access,
                pipeline_trigger: write,
                issues: access,
                extensions: EXTENSIONS.iter().map(|e| (*e).to_owned()).collect(),
                api_version: parsed.map(|v| format!("GitLab {v}")),
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
        let mut query = vec![
            ("membership", "true"),
            ("order_by", "last_activity_at"),
            ("sort", "desc"),
            ("page", page.as_str()),
            ("per_page", size.as_str()),
        ];
        let search = q.search.as_deref().map(|s| clean(s.trim(), 100));
        if let Some(search) = search.as_deref().filter(|s| !s.is_empty()) {
            query.push(("search", search));
        }
        let reply = self
            .api
            .get(t, self.api.endpoint(&["projects"], &query), "your projects")?;
        let items = reply
            .json
            .as_array()
            .ok_or_else(|| invalid("your projects"))?
            .iter()
            .map(|p| self.repository(p))
            .collect::<Result<Vec<_>, _>>()?;
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
        let what = format!("the project {}", r.qualified());
        let reply = self.api.get(t, self.project_url(r, &[], &[]), &what)?;
        self.repository(&reply.json)
    }

    fn repository_by_path(
        &self,
        t: &mut dyn Transport,
        path: &str,
    ) -> Result<Option<RepositoryRecord>, Failure> {
        let what = format!("the project {}", clean(path, 100));
        match self
            .api
            .get_optional(t, self.api.endpoint(&["projects", path], &[]), &what)?
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
        let page = q.page.page.max(1).to_string();
        let size = per_page(q.page.per_page);
        let mut query = vec![("page", page.as_str()), ("per_page", size.as_str())];
        let search = q.search.as_deref().map(|s| clean(s.trim(), 100));
        if let Some(search) = search.as_deref().filter(|s| !s.is_empty()) {
            query.push(("search", search));
        }
        let what = format!("the branches of {}", r.qualified());
        let reply = self.api.get(
            t,
            self.project_url(r, &["repository", "branches"], &query),
            &what,
        )?;
        let items = reply
            .json
            .as_array()
            .ok_or_else(|| invalid(&what))?
            .iter()
            .filter_map(|b| {
                Some(BranchRecord {
                    name: text(b, "name", 255)?,
                    sha: b.get("commit").and_then(|c| text(c, "id", 64)),
                    default: boolean(b, "default"),
                    protected: boolean(b, "protected"),
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
        let size = per_page(q.limit.max(1));
        let mut query = vec![("per_page", size.as_str())];
        if let Some(reference) = &q.reference {
            query.push(("ref_name", check_ref(reference)?));
        }
        let what = format!("the commits of {}", r.qualified());
        let reply = self.api.get(
            t,
            self.project_url(r, &["repository", "commits"], &query),
            &what,
        )?;
        Ok(reply
            .json
            .as_array()
            .ok_or_else(|| invalid(&what))?
            .iter()
            .filter_map(|c| {
                Some(CommitRecord {
                    sha: text(c, "id", 64)?,
                    title: text(c, "title", 255).unwrap_or_default(),
                    author: text(c, "author_name", 255),
                    committed_at: c
                        .get("committed_date")
                        .and_then(Value::as_str)
                        .and_then(timestamp),
                    web_url: self.api.web_link(c.get("web_url").and_then(Value::as_str)),
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
        let limit = q.limit.clamp(1, 100) as usize;
        let size = per_page(q.limit);
        let mut query: Vec<(&str, String)> = vec![
            ("state", "opened".into()),
            ("order_by", "updated_at".into()),
            ("sort", "desc".into()),
            ("per_page", size),
        ];
        // Review requests on a GitLab before 13.8: every open one, filtered
        // by its reviewers here.
        let mut reviewer_filter = None;
        match q.filter {
            CrFilter::Open => {
                if r.is_none() {
                    query.push(("scope", "assigned_to_me".into()));
                }
            }
            CrFilter::Mine => query.push(("scope", "created_by_me".into())),
            CrFilter::Review => {
                let (_, me) = self.me(t)?;
                if self.gates.reviewer_filter() {
                    query.push(("reviewer_username", me));
                } else {
                    reviewer_filter = Some(me);
                    // Every open one is read and filtered: full pages.
                    if let Some(size) = query.iter_mut().find(|(k, _)| *k == "per_page") {
                        size.1 = "100".into();
                    }
                }
                if r.is_none() {
                    query.push(("scope", "all".into()));
                }
            }
        }
        let query: Vec<(&str, &str)> = query.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let (url, what) = match r {
            Some(r) => (
                self.project_url(r, &["merge_requests"], &query),
                format!("the merge requests of {}", r.qualified()),
            ),
            None => (
                self.api.endpoint(&["merge_requests"], &query),
                "your merge requests".to_owned(),
            ),
        };
        let wanted = if reviewer_filter.is_some() {
            500
        } else {
            limit
        };
        let list = self.api.list(t, url, None, &what, wanted)?;
        let mut out = Vec::new();
        for item in &list.items {
            if let Some(me) = &reviewer_filter {
                let reviewing = item
                    .get("reviewers")
                    .and_then(Value::as_array)
                    .is_some_and(|rs| {
                        rs.iter()
                            .any(|u| u.get("username").and_then(Value::as_str) == Some(me))
                    });
                if !reviewing {
                    continue;
                }
            }
            out.push(self.change_request(item)?);
            if out.len() == limit {
                break;
            }
        }
        Ok(out)
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
        if title.is_empty() || title.chars().count() > 255 {
            return Err(fail(
                codes::INVALID_PARAMETERS,
                "a merge request needs a title of 1 to 255 characters",
            ));
        }
        let what = format!("the merge requests of {}", r.qualified());
        let open = self.api.get(
            t,
            self.project_url(
                r,
                &["merge_requests"],
                &[
                    ("state", "opened"),
                    ("source_branch", source),
                    ("target_branch", target),
                    ("per_page", "1"),
                ],
            ),
            &what,
        )?;
        if let Some(existing) = open.json.as_array().and_then(|l| l.first()) {
            let mut record = self.change_request(existing)?;
            record.existing = true;
            return Ok(record);
        }
        // "Draft: " works on every GitLab; `draft` was added later.
        let title = if c.draft && !title.to_ascii_lowercase().starts_with("draft:") {
            format!("Draft: {title}")
        } else {
            title.to_owned()
        };
        let mut body = json!({
            "source_branch": source,
            "target_branch": target,
            "title": title,
        });
        if let Some(description) = c.description.as_deref().filter(|d| !d.trim().is_empty()) {
            body["description"] = json!(description.chars().take(10_000).collect::<String>());
        }
        let reply = self.api.send_json(
            t,
            Method::Post,
            self.project_url(r, &["merge_requests"], &[]),
            &body,
            &format!("a new merge request in {}", r.qualified()),
        )?;
        self.change_request(&reply.json)
    }

    fn list_pipelines(
        &self,
        t: &mut dyn Transport,
        r: &RepoRef,
        q: &PipelineQuery,
    ) -> Result<Vec<PipelineRecord>, Failure> {
        let size = per_page(q.limit.max(1));
        let mut query = vec![
            ("order_by", "id"),
            ("sort", "desc"),
            ("per_page", size.as_str()),
        ];
        if let Some(reference) = &q.reference {
            query.push(("ref", check_ref(reference)?));
        }
        let what = format!("the pipelines of {}", r.qualified());
        let reply = self
            .api
            .get(t, self.project_url(r, &["pipelines"], &query), &what)?;
        reply
            .json
            .as_array()
            .ok_or_else(|| invalid(&what))?
            .iter()
            .map(|p| self.pipeline(p))
            .collect()
    }

    fn pipeline_status(
        &self,
        t: &mut dyn Transport,
        r: &RepoRef,
        s: &PipelineSelector,
    ) -> Result<Option<PipelineRecord>, Failure> {
        match s {
            PipelineSelector::Id(id) => {
                if id.is_empty() || id.len() > 20 || !id.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(fail(codes::INVALID_PARAMETERS, "a pipeline id is a number"));
                }
                let what = format!("the pipeline {id} of {}", r.qualified());
                let reply = self
                    .api
                    .get(t, self.project_url(r, &["pipelines", id], &[]), &what)?;
                self.pipeline(&reply.json).map(Some)
            }
            PipelineSelector::ChangeRequest(number) => {
                let number = number.max(&1).to_string();
                let what = format!("the merge request !{number} of {}", r.qualified());
                let reply = self.api.get(
                    t,
                    self.project_url(r, &["merge_requests", &number], &[]),
                    &what,
                )?;
                match reply.json.get("head_pipeline").filter(|p| p.is_object()) {
                    Some(p) => self.pipeline(p).map(Some),
                    None => Ok(None),
                }
            }
            PipelineSelector::Ref(reference) => {
                let reference = check_ref(reference)?;
                let is_sha =
                    reference.len() == 40 && reference.chars().all(|c| c.is_ascii_hexdigit());
                let what = format!("the pipelines of {}@{reference}", r.qualified());
                let reply = self.api.get(
                    t,
                    self.project_url(
                        r,
                        &["pipelines"],
                        &[
                            (if is_sha { "sha" } else { "ref" }, reference),
                            ("order_by", "id"),
                            ("sort", "desc"),
                            ("per_page", "1"),
                        ],
                    ),
                    &what,
                )?;
                match reply.json.as_array().ok_or_else(|| invalid(&what))?.first() {
                    Some(p) => self.pipeline(p).map(Some),
                    None => Ok(None),
                }
            }
        }
    }

    fn check_trigger(&self, p: &PipelineTrigger) -> Result<(), Failure> {
        if p.workflow.is_some() || !p.inputs.is_empty() {
            return Err(fail(
                codes::CAPABILITY_UNSUPPORTED,
                "GitLab has no workflow files or inputs: leave them empty and use variables",
            ));
        }
        check_ref(&p.reference)?;
        check_variables(&p.variables)
    }

    fn list_workflows(
        &self,
        _t: &mut dyn Transport,
        _r: &RepoRef,
        _search: Option<&str>,
    ) -> Result<Vec<crate::records::WorkflowRecord>, Failure> {
        Err(fail(
            codes::CAPABILITY_UNSUPPORTED,
            "GitLab has no workflow files to choose from: a GitLab pipeline runs the project's \
             .gitlab-ci.yml, so leave `workflow` empty",
        ))
    }

    fn trigger_pipeline(
        &self,
        t: &mut dyn Transport,
        r: &RepoRef,
        p: &PipelineTrigger,
    ) -> Result<PipelineRecord, Failure> {
        self.check_trigger(p)?;
        let reference = check_ref(&p.reference)?;
        let mut body = json!({ "ref": reference });
        if !p.variables.is_empty() {
            body["variables"] = Value::Array(
                p.variables
                    .iter()
                    .map(|(k, v)| json!({"key": k, "value": v, "variable_type": "env_var"}))
                    .collect(),
            );
        }
        let reply = self.api.send_json(
            t,
            Method::Post,
            self.project_url(r, &["pipeline"], &[]),
            &body,
            &format!("a new pipeline on {reference} in {}", r.qualified()),
        )?;
        self.pipeline(&reply.json)
    }

    fn list_issues(
        &self,
        t: &mut dyn Transport,
        r: &RepoRef,
        q: &IssueQuery,
    ) -> Result<Vec<IssueRecord>, Failure> {
        let state = match q.state {
            IssueState::Open => "opened",
            IssueState::Closed => "closed",
            IssueState::All => "all",
        };
        let size = per_page(q.limit.max(1));
        let what = format!("the issues of {}", r.qualified());
        let reply = self.api.get(
            t,
            self.project_url(
                r,
                &["issues"],
                &[
                    ("state", state),
                    ("order_by", "updated_at"),
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
            .map(|i| self.issue(i))
            .collect()
    }

    fn get_issue(
        &self,
        t: &mut dyn Transport,
        r: &RepoRef,
        number: u64,
    ) -> Result<IssueRecord, Failure> {
        let n = number.max(1).to_string();
        let what = format!("the issue #{n} of {}", r.qualified());
        let reply = self
            .api
            .get(t, self.project_url(r, &["issues", &n], &[]), &what)?;
        self.issue(&reply.json)
    }

    fn web_url(&self, target: &WebTarget) -> Result<String, Failure> {
        let root = self.api.web.root();
        let check_path = |path: &str| -> Result<String, Failure> {
            let ok = !path.is_empty()
                && path.len() <= 1000
                && path.split('/').count() >= 2
                && path.split('/').all(|s| {
                    !s.is_empty()
                        && s != "."
                        && s != ".."
                        && s.chars()
                            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
                });
            if ok {
                Ok(path.to_owned())
            } else {
                Err(fail(
                    codes::INVALID_REPOSITORY,
                    format!("“{}” is not a project path", clean(path, 60)),
                ))
            }
        };
        let text = match target {
            WebTarget::Repository { path, page } => {
                let path = check_path(path)?;
                let suffix = match page {
                    RepoPage::Home => "",
                    RepoPage::Branches => "/-/branches",
                    RepoPage::Commits => "/-/commits",
                    RepoPage::Pipelines => "/-/pipelines",
                    RepoPage::Issues => "/-/issues",
                    RepoPage::Changes => "/-/merge_requests",
                };
                format!("{root}{path}{suffix}")
            }
            WebTarget::ChangeRequest { path, number } => {
                format!(
                    "{root}{}/-/merge_requests/{}",
                    check_path(path)?,
                    number.max(&1)
                )
            }
            WebTarget::Issue { path, number } => {
                format!("{root}{}/-/issues/{}", check_path(path)?, number.max(&1))
            }
            WebTarget::Pipeline { path, id } => {
                if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(fail(codes::INVALID_PARAMETERS, "a pipeline id is a number"));
                }
                format!("{root}{}/-/pipelines/{id}", check_path(path)?)
            }
            WebTarget::MyChangeRequests => format!("{root}dashboard/merge_requests"),
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
            "gitlab.approvals" => self.approvals(t, r, args),
            "gitlab.review-requests" => {
                let limit = args
                    .get("limit")
                    .and_then(Value::as_u64)
                    .map_or(20, |l| l.clamp(1, 100) as u32);
                let items = self.list_change_requests(
                    t,
                    Some(r),
                    &CrQuery {
                        filter: CrFilter::Review,
                        limit,
                    },
                )?;
                Ok(json!({ "items": items }))
            }
            _ => Err(fail(
                codes::CAPABILITY_UNSUPPORTED,
                format!("GitLab has no extension “{}”", clean(id, 64)),
            )),
        }
    }
}
