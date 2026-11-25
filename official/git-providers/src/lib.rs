//! `deckpilot.git-providers` ("Platformy Git"): repositories, change
//! requests, pipelines and issues of the GitHub and GitLab servers the user
//! connects (DeckPilot `design-git-providers.md`). One plugin, one
//! [`Provider`](provider::Provider) trait, one adapter per provider: GitLab
//! (GitLab.com and self-managed) and GitHub (GitHub.com and GitHub
//! Enterprise Server).
//!
//! **Connections, plugin API 1.14.** Every action takes a `connection`:
//! the id of a server the user connected in the app, with its own token.
//! The host makes every request on that connection, adds its secret, and
//! keeps it inside the connection's origin, path prefix and methods. The
//! plugin never sees a token. A connection is read-only until the user
//! allows changes on it.
//!
//! **Held.** No DeckPilot provides plugin API 1.14 yet: the plugin is
//! `held` in `official/plugins.json` (never packaged, signed or released)
//! until its host side and bundling land.
#![deny(unsafe_code)]

pub mod adapters;
pub mod changes;
pub mod client;
pub mod errors;
pub mod host;
pub mod legacy;
pub mod ops;
pub mod provider;
pub mod records;
pub mod remote;
pub mod state;
pub mod transport;

use deckpilot_plugin_sdk::prelude::*;

use errors::{Failure, clean, codes, fail};
use provider::{
    ConnectionInfo, CrFilter, IssueQuery, IssueState, PipelineTrigger, Provider, RepoPage, RepoRef,
    WebTarget,
};

/// The plugin's id and version, as in `manifest.json`.
pub const ID: &str = "deckpilot.git-providers";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The internal actions (not in the Action Library, never on a key).
pub const INTERNAL: [&str; 12] = [
    "probe",
    "list-repositories",
    "get-repository",
    "list-branches",
    "list-commits",
    "list-change-requests",
    "list-pipelines",
    "list-workflows",
    "list-issues",
    "get-issue",
    "resolve-remote",
    "extension",
];

/// The extensions an `extension` call may name: read-only ones only.
/// Changes (a workflow dispatch, pipeline variables) go through
/// `trigger-pipeline`, which needs "Allow changes" and asks first.
pub const EXTENSIONS: [&str; 4] = [
    "github.checks",
    "github.review-requests",
    "gitlab.review-requests",
    "gitlab.approvals",
];

impl From<Failure> for PluginError {
    fn from(f: Failure) -> Self {
        let error = PluginError::new(f.code, f.message);
        if f.retryable {
            error.retryable()
        } else {
            error
        }
    }
}

/// The plugin definition; `deckpilot_plugin_sdk::plugin!` exports it.
pub fn plugin() -> Plugin {
    let connection = || Param::text("connection").required().max_length(64);
    let repository = || Param::text("repository").max_length(128);
    let limit = || Param::integer("limit").min(1.0).max(100.0).default(20_i64);
    let reference = || Param::text("ref").max_length(250);
    let plugin = Plugin::new(ID, VERSION)
        .action(Action::new("probe", probe).param(connection()))
        .action(
            Action::new("list-repositories", list_repositories)
                .param(connection())
                .param(Param::text("search").max_length(100))
                .param(Param::integer("page").min(1.0).max(100.0).default(1_i64)),
        )
        .action(
            Action::new("get-repository", get_repository)
                .param(connection())
                .param(repository().required()),
        )
        .action(
            Action::new("list-branches", list_branches)
                .param(connection())
                .param(repository().required())
                .param(Param::text("search").max_length(100)),
        )
        .action(
            Action::new("list-commits", list_commits)
                .param(connection())
                .param(repository().required())
                .param(reference())
                .param(limit()),
        )
        .action(
            Action::new("list-change-requests", list_change_requests)
                .param(connection())
                .param(repository())
                .param(Param::choice("filter", &CrFilter::NAMES).default("open"))
                .param(limit()),
        )
        .action(
            Action::new("list-pipelines", list_pipelines)
                .param(connection())
                .param(repository().required())
                .param(reference())
                .param(limit()),
        )
        .action(
            Action::new("list-workflows", list_workflows)
                .param(connection())
                .param(repository().required())
                .param(Param::text("search").max_length(100)),
        )
        .action(
            Action::new("list-issues", list_issues)
                .param(connection())
                .param(repository().required())
                .param(Param::choice("state", &["open", "closed", "all"]).default("open"))
                .param(limit()),
        )
        .action(
            Action::new("get-issue", get_issue)
                .param(connection())
                .param(repository().required())
                .param(Param::integer("issue").required().min(1.0)),
        )
        .action(
            Action::new("resolve-remote", resolve_remote)
                .param(connection())
                .param(Param::text("remoteUrl").required().max_length(2048)),
        )
        .action(
            Action::new("extension", extension)
                .param(connection())
                .param(repository().required())
                .param(Param::choice("id", &EXTENSIONS).required())
                .param(Param::text("args").max_length(4096)),
        )
        .action(
            Action::new("open-repository", open_repository)
                .param(connection())
                .param(repository().required())
                .param(Param::choice("page", &RepoPage::NAMES).default("home")),
        )
        .action(
            Action::new("open-change-requests", open_change_requests)
                .param(connection())
                .param(repository()),
        )
        .action(
            Action::new("open-issue", open_issue)
                .param(connection())
                .param(repository().required())
                .param(Param::integer("issue").required().min(1.0)),
        )
        .action(
            Action::new("pipeline-status", pipeline_status)
                .state(state::pipeline_status)
                .param(connection())
                .param(repository().required())
                .param(reference())
                .param(Param::integer("changeRequest").min(1.0)),
        )
        .action(
            Action::new("create-change-request", create_change_request)
                .param(connection())
                .param(repository().required())
                .param(Param::text("source").required().max_length(250))
                .param(Param::text("target").max_length(250))
                .param(Param::text("title").required().max_length(255))
                .param(Param::text("description").max_length(10_000))
                .param(Param::boolean("draft").default(false))
                .param(Param::boolean("openAfter").default(false))
                .param(Param::boolean("confirm").default(true)),
        )
        .action(
            Action::new("trigger-pipeline", trigger_pipeline)
                .param(connection())
                .param(repository().required())
                .param(Param::text("ref").required().max_length(250))
                .param(Param::text("variables").max_length(60_000))
                .param(Param::text("workflow").max_length(255))
                .param(Param::text("inputs").max_length(4096))
                .param(Param::boolean("confirm").default(true)),
        )
        .action(
            Action::new("wait-pipeline", wait_pipeline)
                .param(connection())
                .param(repository().required())
                .param(Param::text("pipelineId").required().max_length(20))
                .param(
                    Param::integer("pollSeconds")
                        .min(10.0)
                        .max(120.0)
                        .default(15_i64),
                ),
        );
    legacy_actions(plugin)
}

/// The legacy GitHub and GitLab actions (`legacy`): the old plugins'
/// parameters, exactly, after the `connection` the host injects.
fn legacy_actions(plugin: Plugin) -> Plugin {
    let action = |id: &str, handler: deckpilot_plugin_sdk::Handler| {
        Action::new(id, handler).param(legacy_connection())
    };
    let repository = || Param::text("repository").max_length(140);
    let pull_request = || Param::integer("pullRequest").min(1.0);
    let project = || Param::text("project").max_length(1000);
    let merge_request = || Param::integer("mergeRequest").min(1.0);
    let limit = || Param::integer("limit").min(1.0).max(100.0).default(20_i64);
    let filter = || Param::choice("filter", &legacy::github::FILTERS).default("all");
    plugin
        .action(action(
            "legacy-github-current-user",
            legacy_github_current_user,
        ))
        .action(
            action("legacy-github-repository", legacy_github_repository)
                .param(repository().required()),
        )
        .action(
            action("legacy-github-pull-requests", legacy_github_pull_requests)
                .param(repository())
                .param(filter())
                .param(limit()),
        )
        .action(
            action(
                "legacy-github-review-requests",
                legacy_github_review_requests,
            )
            .param(repository())
            .param(limit()),
        )
        .action(
            action("legacy-github-checks", legacy_github_checks)
                .param(repository().required())
                .param(Param::text("ref").max_length(250))
                .param(pull_request()),
        )
        .action(
            action("legacy-github-open", legacy_github_open)
                .param(Param::text("url").max_length(2048))
                .param(repository())
                .param(pull_request()),
        )
        .action(action(
            "legacy-gitlab-current-user",
            legacy_gitlab_current_user,
        ))
        .action(action("legacy-gitlab-project", legacy_gitlab_project).param(project().required()))
        .action(
            action("legacy-gitlab-merge-requests", legacy_gitlab_merge_requests)
                .param(project())
                .param(Param::choice("filter", &legacy::gitlab::FILTERS).default("all"))
                .param(limit()),
        )
        .action(
            action(
                "legacy-gitlab-review-requests",
                legacy_gitlab_review_requests,
            )
            .param(project())
            .param(limit()),
        )
        .action(
            action("legacy-gitlab-pipeline", legacy_gitlab_pipeline)
                .param(project().required())
                .param(Param::text("ref").max_length(250))
                .param(merge_request()),
        )
        .action(
            action("legacy-gitlab-open", legacy_gitlab_open)
                .param(Param::text("url").max_length(2048))
                .param(project())
                .param(merge_request()),
        )
}

/// The connection of a legacy action: the one the host injects
/// (`ParamRule::InjectConnection`), never one the user picks.
fn legacy_connection() -> Param {
    Param::text("connection").required().max_length(64)
}

/// Runs an old action's code on the bound connection.
fn legacy_run(ctx: &mut Context<'_>, args: &Args<'_>, run: legacy::Handler) -> ActionResult {
    let (connection, mut transport) = connect(args)?;
    run(ctx, args, &connection, transport.as_mut())
}

macro_rules! legacy_handlers {
    ($($name:ident => $run:path),* $(,)?) => {
        $(
            fn $name(ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
                legacy_run(ctx, args, $run)
            }
        )*
    };
}

legacy_handlers! {
    legacy_github_current_user => legacy::github::current_user,
    legacy_github_repository => legacy::github::repository_info,
    legacy_github_pull_requests => legacy::github::pull_requests,
    legacy_github_review_requests => legacy::github::review_requests,
    legacy_github_checks => legacy::github::checks,
    legacy_github_open => legacy::github::open,
    legacy_gitlab_current_user => legacy::gitlab::current_user,
    legacy_gitlab_project => legacy::gitlab::project_info,
    legacy_gitlab_merge_requests => legacy::gitlab::merge_requests,
    legacy_gitlab_review_requests => legacy::gitlab::review_requests,
    legacy_gitlab_pipeline => legacy::gitlab::pipeline,
    legacy_gitlab_open => legacy::gitlab::open,
}

deckpilot_plugin_sdk::plugin!(plugin);

/// The bound connection, its adapter and its transport.
struct Bound {
    connection: ConnectionInfo,
    provider: Box<dyn Provider>,
    transport: Box<dyn transport::Transport>,
}

/// The connection the invocation names, and how to reach it.
fn connect(args: &Args<'_>) -> Result<(ConnectionInfo, Box<dyn transport::Transport>), Failure> {
    #[cfg(test)]
    if let Some(bound) = host::test_binding::take() {
        return Ok(bound);
    }
    let id: String = args
        .get("connection")
        .map_err(|_| fail(codes::CONNECTION_MISSING, "choose a connection"))?;
    let connection = host::connection(id.trim())?;
    let transport = Box::new(host::HostTransport::new(&connection));
    Ok((connection, transport))
}

fn bind(args: &Args<'_>) -> Result<Bound, PluginError> {
    let (connection, transport) = connect(args)?;
    let provider = adapters::for_connection(&connection)?;
    Ok(Bound {
        transport,
        provider,
        connection,
    })
}

impl Bound {
    fn repo(&self, args: &Args<'_>) -> Result<RepoRef, Failure> {
        let text: Option<String> = args
            .get("repository")
            .map_err(|_| fail(codes::INVALID_REPOSITORY, "choose a repository"))?;
        let text = text
            .filter(|t| !t.trim().is_empty())
            .ok_or_else(|| fail(codes::INVALID_REPOSITORY, "choose a repository"))?;
        RepoRef::parse(&text, self.provider.kind(), &self.connection.id)
    }

    fn optional_repo(&self, args: &Args<'_>) -> Result<Option<RepoRef>, Failure> {
        match args.get::<Option<String>>("repository") {
            Ok(Some(text)) if !text.trim().is_empty() => {
                RepoRef::parse(&text, self.provider.kind(), &self.connection.id).map(Some)
            }
            _ => Ok(None),
        }
    }
}

fn text(args: &Args<'_>, key: &str) -> Result<Option<String>, PluginError> {
    Ok(args
        .get::<Option<String>>(key)?
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty()))
}

fn limit(args: &Args<'_>) -> Result<u32, PluginError> {
    Ok(u32::try_from(
        args.get::<Option<i64>>("limit")?
            .unwrap_or(20)
            .clamp(1, 100),
    )
    .unwrap_or(20))
}

fn positive(args: &Args<'_>, key: &str) -> Result<Option<u64>, PluginError> {
    Ok(args
        .get::<Option<i64>>(key)?
        .and_then(|n| u64::try_from(n).ok())
        .filter(|n| *n >= 1))
}

fn probe(_ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let mut b = bind(args)?;
    Ok(ops::probe(b.provider.as_ref(), &mut b.transport)?)
}

fn list_repositories(_ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let mut b = bind(args)?;
    let page = u32::try_from(args.get::<Option<i64>>("page")?.unwrap_or(1)).unwrap_or(1);
    Ok(ops::list_repositories(
        b.provider.as_ref(),
        &mut b.transport,
        text(args, "search")?,
        page,
    )?)
}

fn get_repository(_ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let mut b = bind(args)?;
    let r = b.repo(args)?;
    Ok(ops::get_repository(
        b.provider.as_ref(),
        &mut b.transport,
        &r,
    )?)
}

fn list_branches(_ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let mut b = bind(args)?;
    let r = b.repo(args)?;
    Ok(ops::list_branches(
        b.provider.as_ref(),
        &mut b.transport,
        &r,
        text(args, "search")?,
    )?)
}

fn list_commits(_ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let mut b = bind(args)?;
    let r = b.repo(args)?;
    Ok(ops::list_commits(
        b.provider.as_ref(),
        &mut b.transport,
        &r,
        text(args, "ref")?,
        limit(args)?,
    )?)
}

fn filter(args: &Args<'_>) -> Result<CrFilter, PluginError> {
    let name = text(args, "filter")?.unwrap_or_else(|| "open".into());
    CrFilter::parse(&name).ok_or_else(|| {
        PluginError::new(
            codes::INVALID_PARAMETERS,
            format!("“{}” is not a filter", clean(&name, 40)),
        )
    })
}

fn list_change_requests(_ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let mut b = bind(args)?;
    let r = b.optional_repo(args)?;
    Ok(ops::list_change_requests(
        b.provider.as_ref(),
        &mut b.transport,
        r.as_ref(),
        filter(args)?,
        limit(args)?,
    )?)
}

fn list_pipelines(_ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let mut b = bind(args)?;
    let r = b.repo(args)?;
    Ok(ops::list_pipelines(
        b.provider.as_ref(),
        &mut b.transport,
        &r,
        text(args, "ref")?,
        limit(args)?,
    )?)
}

fn list_workflows(_ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let mut b = bind(args)?;
    let r = b.repo(args)?;
    Ok(ops::list_workflows(
        b.provider.as_ref(),
        &mut b.transport,
        &r,
        text(args, "search")?.as_deref(),
    )?)
}

fn list_issues(_ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let mut b = bind(args)?;
    let r = b.repo(args)?;
    let state = match text(args, "state")?.as_deref() {
        Some("closed") => IssueState::Closed,
        Some("all") => IssueState::All,
        _ => IssueState::Open,
    };
    Ok(ops::list_issues(
        b.provider.as_ref(),
        &mut b.transport,
        &r,
        &IssueQuery {
            state,
            limit: limit(args)?,
        },
    )?)
}

fn issue_number(args: &Args<'_>) -> Result<u64, PluginError> {
    positive(args, "issue")?
        .ok_or_else(|| PluginError::new(codes::INVALID_PARAMETERS, "choose an issue (its number)"))
}

fn get_issue(_ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let mut b = bind(args)?;
    let r = b.repo(args)?;
    Ok(ops::get_issue(
        b.provider.as_ref(),
        &mut b.transport,
        &r,
        issue_number(args)?,
    )?)
}

fn resolve_remote(_ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let mut b = bind(args)?;
    let remote: String = args.get("remoteUrl")?;
    let api = client::Api::new(&b.connection)?;
    Ok(ops::resolve_remote(
        b.provider.as_ref(),
        &mut b.transport,
        &api.host,
        api.web.path(),
        &remote,
    )?)
}

fn extension(_ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let mut b = bind(args)?;
    let r = b.repo(args)?;
    let id: String = args.get("id")?;
    let raw = text(args, "args")?.unwrap_or_else(|| "{}".into());
    let value: serde_json::Value = serde_json::from_str(&raw).map_err(|_| {
        PluginError::new(
            codes::INVALID_PARAMETERS,
            "the extension's arguments are not JSON",
        )
    })?;
    Ok(ops::extension(
        b.provider.as_ref(),
        &mut b.transport,
        &id,
        &r,
        &value,
    )?)
}

fn open(ctx: &mut Context<'_>, url: String) -> ActionResult {
    ctx.open_url(url.clone())?;
    Ok(Output::message("opened").with("url", url))
}

fn open_repository(ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let mut b = bind(args)?;
    let r = b.repo(args)?;
    let page =
        RepoPage::parse(&text(args, "page")?.unwrap_or_else(|| "home".into())).unwrap_or_default();
    let url = ops::repository_page(b.provider.as_ref(), &mut b.transport, &r, page)?;
    open(ctx, url)
}

fn open_change_requests(ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let mut b = bind(args)?;
    let url = match b.optional_repo(args)? {
        Some(r) => {
            ops::repository_page(b.provider.as_ref(), &mut b.transport, &r, RepoPage::Changes)?
        }
        None => b.provider.web_url(&WebTarget::MyChangeRequests)?,
    };
    open(ctx, url)
}

fn open_issue(ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let mut b = bind(args)?;
    let r = b.repo(args)?;
    let number = issue_number(args)?;
    let url = ops::issue_page(b.provider.as_ref(), &mut b.transport, &r, number)?;
    open(ctx, url)
}

/// `pipeline-status`, and what its keys show next (`state.rs`).
fn pipeline_status(ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let result = read_pipeline_status(args);
    state::remember_for_key(ctx, &state::KeyParams::read(args), &result);
    result
}

fn read_pipeline_status(args: &Args<'_>) -> ActionResult {
    let mut b = bind(args)?;
    let r = b.repo(args)?;
    let selector = ops::pipeline_selector(
        b.provider.as_ref(),
        &mut b.transport,
        &r,
        positive(args, "changeRequest")?,
        text(args, "ref")?.as_deref(),
    )?;
    Ok(ops::pipeline_status(
        b.provider.as_ref(),
        &mut b.transport,
        &r,
        &selector,
    )?)
}

fn confirm(args: &Args<'_>) -> Result<bool, PluginError> {
    Ok(args.get::<Option<bool>>("confirm")?.unwrap_or(true))
}

fn create_change_request(ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let mut b = bind(args)?;
    b.connection.require_write()?;
    let r = b.repo(args)?;
    let request = changes::CrRequest {
        source: ops::reference(&text(args, "source")?.unwrap_or_default())?,
        target: text(args, "target")?
            .map(|t| ops::reference(&t))
            .transpose()?,
        title: text(args, "title")?.unwrap_or_default(),
        description: text(args, "description")?,
        draft: args.get::<Option<bool>>("draft")?.unwrap_or(false),
        open_after: args.get::<Option<bool>>("openAfter")?.unwrap_or(false),
    };
    changes::create_change_request(
        ctx,
        b.provider.as_ref(),
        &mut b.transport,
        &b.connection,
        &r,
        confirm(args)?,
        request,
    )
}

fn trigger_pipeline(ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let mut b = bind(args)?;
    b.connection.require_write()?;
    let r = b.repo(args)?;
    let trigger = PipelineTrigger {
        reference: ops::reference(&text(args, "ref")?.unwrap_or_default())?,
        variables: ops::parse_variables(&text(args, "variables")?.unwrap_or_default())?,
        workflow: text(args, "workflow")?,
        inputs: match text(args, "inputs")? {
            None => serde_json::Map::new(),
            Some(raw) => match serde_json::from_str::<serde_json::Value>(&raw) {
                Ok(serde_json::Value::Object(map)) => map,
                _ => {
                    return Err(PluginError::new(
                        codes::INVALID_VARIABLES,
                        "workflow inputs are a JSON object of names and values",
                    ));
                }
            },
        },
    };
    changes::trigger_pipeline(
        ctx,
        b.provider.as_ref(),
        &mut b.transport,
        &b.connection,
        &r,
        confirm(args)?,
        &trigger,
    )
}

fn wait_pipeline(_ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let mut b = bind(args)?;
    let r = b.repo(args)?;
    let id: String = args.get("pipelineId")?;
    let poll = u32::try_from(args.get::<Option<i64>>("pollSeconds")?.unwrap_or(15)).unwrap_or(15);
    Ok(ops::wait_pipeline(
        b.provider.as_ref(),
        &mut b.transport,
        &r,
        &id,
        poll,
    )?)
}

#[cfg(test)]
mod contract;
#[cfg(test)]
mod tests_base_urls;
#[cfg(test)]
mod tests_pipelines;
