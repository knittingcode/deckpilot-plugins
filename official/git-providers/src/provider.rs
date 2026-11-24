//! The provider trait (design §2.2): one implementation per provider
//! (`adapters/`), one set of operations behind the actions, the app's
//! screen, the pickers and, later, Scripts.
//!
//! Records hold only what the providers share without translation;
//! provider-only facts are extensions with namespaced ids
//! (`gitlab.pipeline-variables`). "Pull request" and "merge request" stay
//! the provider's words in what users see; the code says change request.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::errors::{Failure, codes, fail};
use crate::records::{
    BranchRecord, ChangeRequestRecord, CommitRecord, IssueRecord, Page, PipelineRecord, Probe,
    RepositoryRecord,
};
use crate::transport::{Method, Transport};

/// The providers the plugin knows (the manifest's connection types).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderKind {
    Github,
    Gitlab,
}

impl ProviderKind {
    pub fn from_type(id: &str) -> Option<Self> {
        match id {
            "github" => Some(Self::Github),
            "gitlab" => Some(Self::Gitlab),
            _ => None,
        }
    }

    /// The prefix of the provider's repository ids (design §9.1, K2).
    pub fn id_prefix(self) -> &'static str {
        match self {
            Self::Github => "gh",
            Self::Gitlab => "gl",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Github => "GitHub",
            Self::Gitlab => "GitLab",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Deployment {
    Cloud,
    SelfHosted,
    #[serde(other)]
    Other,
}

/// A connection's status (the ABI's `ConnectionStatus` of plugin API 1.14;
/// mirrored, as the SDK pinned here predates it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionStatus {
    Ok,
    Unauthorized,
    InsufficientScope,
    Unreachable,
    TlsFailed,
    NeedsReconnect,
    Disabled,
    Untested,
    #[serde(other)]
    Other,
}

/// `connections.get`'s answer: everything but the secret.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInfo {
    pub id: String,
    #[serde(rename = "type")]
    pub type_id: String,
    pub deployment: Deployment,
    pub api_base: String,
    pub web_base: String,
    #[serde(default)]
    pub methods: Vec<Method>,
    pub status: ConnectionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Value>,
}

impl ConnectionInfo {
    pub fn kind(&self) -> Result<ProviderKind, Failure> {
        ProviderKind::from_type(&self.type_id).ok_or_else(|| {
            fail(
                codes::CAPABILITY_UNSUPPORTED,
                "this connection's provider is not supported",
            )
        })
    }

    /// Whether the user allowed changes on this connection.
    pub fn can_write(&self) -> bool {
        self.methods.contains(&Method::Post)
    }

    pub fn require_write(&self) -> Result<(), Failure> {
        if self.can_write() {
            Ok(())
        } else {
            Err(fail(
                codes::READ_ONLY_CONNECTION,
                "this connection is read-only; turn on “Allow changes” for it in Git Providers",
            ))
        }
    }
}

/// None, read, or read and write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Access {
    #[default]
    None,
    Read,
    ReadWrite,
}

/// What a connection can do, as its last probe found.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub repositories: bool,
    pub branches: bool,
    pub commits: bool,
    pub change_requests: Access,
    pub pipelines: Access,
    pub pipeline_trigger: bool,
    pub issues: Access,
    pub extensions: BTreeSet<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_version: Option<String>,
}

/// A repository by the provider's immutable id, never by its name:
/// `gl:4711`, `gh:123456789` (design §3.2, K2). A rename changes nothing.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepoRef {
    pub kind: ProviderKind,
    /// The digits.
    pub id: String,
}

impl RepoRef {
    /// `gl:4711`, or a stored `"<connectionId>:gl:4711"` whose connection
    /// is `bound` (a reference to another connection is refused).
    pub fn parse(text: &str, kind: ProviderKind, bound: &str) -> Result<Self, Failure> {
        let text = text.trim();
        let rest = match text.split_once(':') {
            Some((connection, rest)) if connection.starts_with("cn_") => {
                if connection != bound {
                    return Err(fail(
                        codes::INVALID_REPOSITORY,
                        "this repository belongs to another connection",
                    ));
                }
                rest
            }
            _ => text,
        };
        let invalid = || {
            fail(
                codes::INVALID_REPOSITORY,
                format!(
                    "choose a repository of this {} connection (an id such as {}:12345)",
                    kind.name(),
                    kind.id_prefix()
                ),
            )
        };
        let (prefix, id) = rest.split_once(':').ok_or_else(invalid)?;
        if prefix != kind.id_prefix()
            || id.is_empty()
            || id.len() > 20
            || !id.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(invalid());
        }
        Ok(Self {
            kind,
            id: id.to_owned(),
        })
    }

    /// `gl:4711`.
    pub fn qualified(&self) -> String {
        format!("{}:{}", self.kind.id_prefix(), self.id)
    }
}

/// A page of a list: the provider's page number (1 when absent).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageQuery {
    pub page: u32,
    pub per_page: u32,
}

impl Default for PageQuery {
    fn default() -> Self {
        Self {
            page: 1,
            per_page: 30,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RepoQuery {
    pub search: Option<String>,
    pub page: PageQuery,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BranchQuery {
    pub search: Option<String>,
    pub page: PageQuery,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommitQuery {
    pub reference: Option<String>,
    pub limit: u32,
}

/// Which change requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CrFilter {
    /// All open ones of the repository.
    #[default]
    Open,
    /// Open ones the signed-in user wrote.
    Mine,
    /// Open ones the signed-in user is asked to review.
    Review,
}

impl CrFilter {
    pub const NAMES: [&'static str; 3] = ["open", "mine", "review"];

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "open" => Some(Self::Open),
            "mine" => Some(Self::Mine),
            "review" => Some(Self::Review),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CrQuery {
    pub filter: CrFilter,
    pub limit: u32,
}

/// A change request to create.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrCreate {
    pub source: String,
    pub target: String,
    pub title: String,
    pub description: Option<String>,
    pub draft: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PipelineQuery {
    pub reference: Option<String>,
    pub limit: u32,
}

/// The pipeline to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineSelector {
    /// The latest pipeline of a branch, tag or commit.
    Ref(String),
    /// One pipeline, by the provider's id.
    Id(String),
    /// The pipeline of a change request's head.
    ChangeRequest(u64),
}

/// A pipeline to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineTrigger {
    pub reference: String,
    /// GitLab only (`gitlab.pipeline-variables`): `NAME=value` pairs, the
    /// names checked, never secrets.
    pub variables: Vec<(String, String)>,
    /// GitHub only: the Actions workflow file (`ci.yml`) to dispatch.
    pub workflow: Option<String>,
    /// GitHub only: the workflow's `workflow_dispatch` inputs.
    pub inputs: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IssueState {
    #[default]
    Open,
    Closed,
    All,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IssueQuery {
    pub state: IssueState,
    pub limit: u32,
}

/// A page to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebTarget {
    /// The repository's page, by its path (from a record).
    Repository {
        path: String,
        page: RepoPage,
    },
    ChangeRequest {
        path: String,
        number: u64,
    },
    Issue {
        path: String,
        number: u64,
    },
    Pipeline {
        path: String,
        id: String,
    },
    /// The signed-in user's change requests on this server.
    MyChangeRequests,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RepoPage {
    #[default]
    Home,
    Branches,
    Commits,
    Pipelines,
    Issues,
    Changes,
}

impl RepoPage {
    pub const NAMES: [&'static str; 6] = [
        "home",
        "branches",
        "commits",
        "pipelines",
        "issues",
        "changes",
    ];

    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "home" => Self::Home,
            "branches" => Self::Branches,
            "commits" => Self::Commits,
            "pipelines" => Self::Pipelines,
            "issues" => Self::Issues,
            "changes" => Self::Changes,
            _ => return None,
        })
    }
}

/// One provider. Every method that talks to the server takes the
/// invocation's transport, bound to one connection.
pub trait Provider {
    fn kind(&self) -> ProviderKind;
    fn probe(&self, t: &mut dyn Transport) -> Result<Probe, Failure>;
    fn list_repositories(
        &self,
        t: &mut dyn Transport,
        q: &RepoQuery,
    ) -> Result<Page<RepositoryRecord>, Failure>;
    fn get_repository(
        &self,
        t: &mut dyn Transport,
        r: &RepoRef,
    ) -> Result<RepositoryRecord, Failure>;
    /// The repository at `path` (`group/sub/project`), `None` when there
    /// is none: only to identify a git remote, whose record then names it
    /// by id.
    fn repository_by_path(
        &self,
        t: &mut dyn Transport,
        path: &str,
    ) -> Result<Option<RepositoryRecord>, Failure>;
    fn list_branches(
        &self,
        t: &mut dyn Transport,
        r: &RepoRef,
        q: &BranchQuery,
    ) -> Result<Page<BranchRecord>, Failure>;
    fn list_commits(
        &self,
        t: &mut dyn Transport,
        r: &RepoRef,
        q: &CommitQuery,
    ) -> Result<Vec<CommitRecord>, Failure>;
    fn list_change_requests(
        &self,
        t: &mut dyn Transport,
        r: Option<&RepoRef>,
        q: &CrQuery,
    ) -> Result<Vec<ChangeRequestRecord>, Failure>;
    /// Creates one, or returns the open one for the same source and target
    /// (`existing: true`): idempotent by lookup.
    fn create_change_request(
        &self,
        t: &mut dyn Transport,
        r: &RepoRef,
        c: &CrCreate,
    ) -> Result<ChangeRequestRecord, Failure>;
    fn list_pipelines(
        &self,
        t: &mut dyn Transport,
        r: &RepoRef,
        q: &PipelineQuery,
    ) -> Result<Vec<PipelineRecord>, Failure>;
    /// `None`: no pipeline for the selector.
    fn pipeline_status(
        &self,
        t: &mut dyn Transport,
        r: &RepoRef,
        s: &PipelineSelector,
    ) -> Result<Option<PipelineRecord>, Failure>;
    fn trigger_pipeline(
        &self,
        t: &mut dyn Transport,
        r: &RepoRef,
        p: &PipelineTrigger,
    ) -> Result<PipelineRecord, Failure>;
    fn list_issues(
        &self,
        t: &mut dyn Transport,
        r: &RepoRef,
        q: &IssueQuery,
    ) -> Result<Vec<IssueRecord>, Failure>;
    fn get_issue(
        &self,
        t: &mut dyn Transport,
        r: &RepoRef,
        number: u64,
    ) -> Result<IssueRecord, Failure>;
    /// Pure: no request.
    fn web_url(&self, target: &WebTarget) -> Result<String, Failure>;
    /// A provider-only operation (`gitlab.approvals`).
    fn extension(
        &self,
        t: &mut dyn Transport,
        id: &str,
        r: &RepoRef,
        args: &Value,
    ) -> Result<Value, Failure>;
}
