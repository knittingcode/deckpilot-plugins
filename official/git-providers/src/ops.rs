//! One implementation per operation (design §5, §9.3), behind the actions,
//! the app's Git Providers screen, the pickers and, later, Scripts. Each
//! takes the adapter and the invocation's transport and answers an
//! [`Output`] with the action's declared outputs; a `records` output is a
//! list of JSON object texts ([`crate::records::to_output`]).

use deckpilot_plugin_sdk::Output;
use serde::Serialize;
use serde_json::Value;

use crate::client::check_ref;
use crate::errors::{Failure, clean, codes, fail};
use crate::provider::{
    BranchQuery, CommitQuery, ConnectionInfo, CrCreate, CrFilter, CrQuery, IssueQuery, PageQuery,
    PipelineQuery, PipelineSelector, PipelineTrigger, Provider, RepoQuery, RepoRef, WebTarget,
};
use crate::records::{PipelineRecord, PipelineState, to_output};
use crate::remote;
use crate::transport::Transport;

/// Most items a list operation returns.
pub const MAX_LIMIT: u32 = 100;
/// `wait-pipeline`'s poll interval bounds, in seconds.
pub const POLL_SECONDS: (u32, u32) = (10, 120);
/// The time `wait-pipeline` may take, in seconds: the node limit (600 s)
/// less a margin for the last read after the last sleep (GitHub reads two
/// URLs per poll, each at most the host's 15 s per request).
pub const WAIT_BUDGET_SECONDS: u32 = 540;
/// The host's time limit of one request, in seconds.
pub const REQUEST_SECONDS: u32 = 15;
/// What one read of the pipeline counts toward the budget when there is
/// no clock: a second per request, two requests (GitHub reads the
/// repository and the run).
pub const READ_SECONDS_WITHOUT_CLOCK: u32 = 2;

fn n(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn sat(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// `items` (records), `count`, `truncated`.
fn records<T: Serialize>(output: Output, items: &[T]) -> Output {
    let (texts, truncated) = to_output(items);
    output
        .with("count", n(items.len()))
        .with("items", texts)
        .with("truncated", truncated)
}

/// The flat `probe` record (`contributes.recordSchemas`): the
/// capabilities as JSON text, which the host stores with the connection.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProbeRecord {
    account: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_at: Option<u64>,
    can_write: bool,
    excess_scopes: String,
    capabilities: String,
}

pub fn probe(p: &dyn Provider, t: &mut dyn Transport) -> Result<Output, Failure> {
    let probe = p.probe(t)?;
    let record = ProbeRecord {
        account: probe.account.clone(),
        version: probe.version.clone(),
        expires_at: probe.expires_at,
        can_write: probe.capabilities.pipeline_trigger,
        excess_scopes: probe.excess_scopes.join(", "),
        capabilities: serde_json::to_string(&probe.capabilities).unwrap_or_default(),
    };
    let mut output = Output::message(format!(
        "connected to {} as {}",
        p.kind().name(),
        probe.account
    ))
    .with("account", probe.account.clone())
    .with("canWrite", probe.capabilities.pipeline_trigger);
    if let Some(version) = &probe.version {
        output = output.with("version", version.clone());
    }
    if !probe.excess_scopes.is_empty() {
        output = output.with("excessScopes", probe.excess_scopes.join(", "));
    }
    Ok(records(output, &[record]))
}

pub fn list_repositories(
    p: &dyn Provider,
    t: &mut dyn Transport,
    search: Option<String>,
    page: u32,
) -> Result<Output, Failure> {
    let page = p.list_repositories(
        t,
        &RepoQuery {
            search,
            page: PageQuery {
                page: page.max(1),
                per_page: 50,
            },
        },
    )?;
    let mut output = records(
        Output::message(format!("{} repositories", page.items.len())),
        &page.items,
    );
    if let Some(next) = page.next {
        output = output.with("next", i64::from(next));
    }
    Ok(output)
}

pub fn get_repository(
    p: &dyn Provider,
    t: &mut dyn Transport,
    r: &RepoRef,
) -> Result<Output, Failure> {
    let repo = p.get_repository(t, r)?;
    let output = Output::message(format!("read {}", repo.path))
        .with("path", repo.path.clone())
        .with("url", repo.web_url.clone());
    Ok(records(output, &[repo]))
}

pub fn list_branches(
    p: &dyn Provider,
    t: &mut dyn Transport,
    r: &RepoRef,
    search: Option<String>,
) -> Result<Output, Failure> {
    let page = p.list_branches(
        t,
        r,
        &BranchQuery {
            search,
            page: PageQuery {
                page: 1,
                per_page: MAX_LIMIT,
            },
        },
    )?;
    Ok(records(
        Output::message(format!("{} branches", page.items.len())),
        &page.items,
    ))
}

pub fn list_commits(
    p: &dyn Provider,
    t: &mut dyn Transport,
    r: &RepoRef,
    reference: Option<String>,
    limit: u32,
) -> Result<Output, Failure> {
    let items = p.list_commits(
        t,
        r,
        &CommitQuery {
            reference,
            limit: limit.clamp(1, MAX_LIMIT),
        },
    )?;
    Ok(records(
        Output::message(format!("{} commits", items.len())),
        &items,
    ))
}

pub fn list_change_requests(
    p: &dyn Provider,
    t: &mut dyn Transport,
    r: Option<&RepoRef>,
    filter: CrFilter,
    limit: u32,
) -> Result<Output, Failure> {
    let items = p.list_change_requests(
        t,
        r,
        &CrQuery {
            filter,
            limit: limit.clamp(1, MAX_LIMIT),
        },
    )?;
    let mut output = Output::message(format!("{} open", items.len()));
    if let Some(first) = items.first() {
        output = output.with("firstUrl", first.web_url.clone());
    }
    Ok(records(output, &items))
}

/// `create-change-request`: needs a connection that may make changes.
pub fn create_change_request(
    p: &dyn Provider,
    t: &mut dyn Transport,
    connection: &ConnectionInfo,
    r: &RepoRef,
    c: &CrCreate,
) -> Result<Output, Failure> {
    connection.require_write()?;
    let record = p.create_change_request(t, r, c).map_err(never_retried)?;
    let message = if record.existing {
        format!(
            "#{} was already open for {} → {}",
            record.number, record.source, record.target
        )
    } else {
        format!(
            "opened #{} for {} → {}",
            record.number, record.source, record.target
        )
    };
    Ok(Output::message(message)
        .with("number", sat(record.number))
        .with("url", record.web_url.clone())
        .with("state", record.state.clone())
        .with("existing", record.existing))
}

/// The pipeline outputs. `pipelineId` only when there is one: a GitHub
/// dispatch whose run has not shown up yet has none, and an empty id
/// mapped into `wait-pipeline` would only fail there.
/// A change is never retried (design §10 "Retries"): its node is not
/// `retrySafe`, and its failures are not marked retryable either, so a
/// 5xx or a lost connection after the request was sent can never start a
/// second pipeline or open a second change request.
fn never_retried(mut failure: Failure) -> Failure {
    failure.retryable = false;
    failure
}

/// Which pipeline `pipeline-status` reads: a change request's when one is
/// given (it wins over `ref`), else the ref's latest, else the latest of
/// the repository's default branch.
pub fn pipeline_selector(
    p: &dyn Provider,
    t: &mut dyn Transport,
    r: &RepoRef,
    change_request: Option<u64>,
    reference: Option<&str>,
) -> Result<PipelineSelector, Failure> {
    Ok(match (change_request, reference) {
        (Some(number), _) => PipelineSelector::ChangeRequest(number),
        (None, Some(reference)) => PipelineSelector::Ref(self::reference(reference)?),
        (None, None) => {
            PipelineSelector::Ref(p.get_repository(t, r)?.default_branch.ok_or_else(|| {
                fail(
                    codes::INVALID_REF,
                    "choose a branch: the repository has none",
                )
            })?)
        }
    })
}

fn pipeline_output(output: Output, record: &PipelineRecord) -> Output {
    let mut output = output
        .with("state", record.state.as_str())
        .with("providerStatus", record.provider_status.clone());
    if !record.id.is_empty() {
        output = output.with("pipelineId", record.id.clone());
    }
    if let Some(url) = &record.web_url {
        output = output.with("url", url.clone());
    }
    if let Some(sha) = &record.sha {
        output = output.with("sha", sha.clone());
    }
    if let Some(finished) = record.finished_at {
        output = output.with("finishedAt", sat(finished));
    }
    if let Some(duration) = record.duration_seconds {
        output = output.with("durationSeconds", sat(duration));
    }
    output
}

pub fn pipeline_status(
    p: &dyn Provider,
    t: &mut dyn Transport,
    r: &RepoRef,
    selector: &PipelineSelector,
) -> Result<Output, Failure> {
    match p.pipeline_status(t, r, selector)? {
        Some(record) => {
            let output = pipeline_output(
                Output::message(format!("pipeline {}: {}", record.id, record.state.as_str())),
                &record,
            );
            Ok(records(output, &[record]))
        }
        None => Ok(Output::message("no pipeline")
            .with("state", "none")
            .with("count", 0_i64)
            .with("items", Vec::<String>::new())
            .with("truncated", false)),
    }
}

pub fn list_pipelines(
    p: &dyn Provider,
    t: &mut dyn Transport,
    r: &RepoRef,
    reference: Option<String>,
    limit: u32,
) -> Result<Output, Failure> {
    let items = p.list_pipelines(
        t,
        r,
        &PipelineQuery {
            reference,
            limit: limit.clamp(1, MAX_LIMIT),
        },
    )?;
    Ok(records(
        Output::message(format!("{} pipelines", items.len())),
        &items,
    ))
}

/// `NAME=value` lines (the `variables` parameter, `pairs` with `=`).
pub fn parse_variables(text: &str) -> Result<Vec<(String, String)>, Failure> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            line.split_once('=')
                .map(|(k, v)| (k.trim().to_owned(), v.to_owned()))
                .ok_or_else(|| {
                    fail(
                        codes::INVALID_VARIABLES,
                        format!("“{}” is not NAME=value", clean(line, 60)),
                    )
                })
        })
        .collect()
}

pub fn trigger_pipeline(
    p: &dyn Provider,
    t: &mut dyn Transport,
    connection: &ConnectionInfo,
    r: &RepoRef,
    trigger: &PipelineTrigger,
) -> Result<Output, Failure> {
    connection.require_write()?;
    let record = p.trigger_pipeline(t, r, trigger).map_err(never_retried)?;
    let message = if record.id.is_empty() {
        format!(
            "started a pipeline on {}; {} has not listed it yet",
            clean(&trigger.reference, 60),
            p.kind().name()
        )
    } else {
        format!(
            "started pipeline {} on {}",
            record.id,
            clean(&trigger.reference, 60)
        )
    };
    Ok(pipeline_output(Output::message(message), &record))
}

/// `list-workflows`: the workflow files a GitHub pipeline can be started
/// from (`trigger-pipeline`'s `workflow` picker). Refused on GitLab.
pub fn list_workflows(
    p: &dyn Provider,
    t: &mut dyn Transport,
    r: &RepoRef,
    search: Option<&str>,
) -> Result<Output, Failure> {
    let items = p.list_workflows(t, r, search)?;
    Ok(records(
        Output::message(format!("{} workflows", items.len())),
        &items,
    ))
}

/// `wait-pipeline`: reads the pipeline every `poll_seconds` (sleeps of
/// at most 30 s through `time.sleep`) until it ends (at once, with no
/// sleep when it already has), the run is cancelled (`cancelled`, at
/// once), or the time budget is spent.
///
/// At the budget the node **succeeds** with `timedOut: true` and `state`
/// the last one read (`running` or `pending`), so a Condition on
/// `state == "success"` or a Loop around the node goes on: a failure of
/// ABI 1 carries no outputs, and a Loop would stop at the first one. The
/// budget ([`WAIT_BUDGET_SECONDS`]) ends before the node's 600 s, so the
/// plugin answers before the host's `timeout`.
pub fn wait_pipeline(
    p: &dyn Provider,
    t: &mut dyn Transport,
    r: &RepoRef,
    pipeline_id: &str,
    poll_seconds: u32,
) -> Result<Output, Failure> {
    let pipeline_id = pipeline_id.trim();
    if pipeline_id.is_empty() {
        return Err(fail(
            codes::INVALID_PARAMETERS,
            "no pipeline id: the pipeline has not been listed yet; read it with pipeline-status \
             on its branch instead",
        ));
    }
    let poll = poll_seconds.clamp(POLL_SECONDS.0, POLL_SECONDS.1);
    let selector = PipelineSelector::Id(pipeline_id.to_owned());
    let started = t.now_ms();
    let mut waited = 0;
    loop {
        let record = p
            .pipeline_status(t, r, &selector)?
            .ok_or_else(|| fail(codes::NOT_FOUND, "the pipeline was not found"))?;
        if record.state.is_terminal() || record.state == PipelineState::Manual {
            let message = format!("pipeline {}: {}", record.id, record.state.as_str());
            return Ok(pipeline_output(Output::message(message), &record).with("timedOut", false));
        }
        // The requests count too: the clock when there is one, else the
        // sleeps and a fixed time per read.
        // Without a clock the next read is counted ahead too, so the total
        // with every read stays within the budget.
        let next_read = match (started, t.now_ms()) {
            (Some(start), Some(now)) => {
                waited = u32::try_from(now.saturating_sub(start) / 1_000).unwrap_or(u32::MAX);
                0
            }
            _ => {
                waited = waited.saturating_add(READ_SECONDS_WITHOUT_CLOCK);
                READ_SECONDS_WITHOUT_CLOCK
            }
        };
        if waited.saturating_add(poll).saturating_add(next_read) > WAIT_BUDGET_SECONDS {
            let message = format!(
                "timed out: pipeline {} is still {} after {waited} s; wait again in a Loop",
                record.id,
                record.state.as_str()
            );
            return Ok(pipeline_output(Output::message(message), &record).with("timedOut", true));
        }
        let mut left = poll * 1_000;
        while left > 0 {
            let step = left.min(30_000);
            t.sleep(step)
                .map_err(|e| e.into_failure(p.kind().name()).0)?;
            left -= step;
        }
        waited += poll;
    }
}

pub fn list_issues(
    p: &dyn Provider,
    t: &mut dyn Transport,
    r: &RepoRef,
    q: &IssueQuery,
) -> Result<Output, Failure> {
    let items = p.list_issues(t, r, q)?;
    Ok(records(
        Output::message(format!("{} issues", items.len())),
        &items,
    ))
}

pub fn get_issue(
    p: &dyn Provider,
    t: &mut dyn Transport,
    r: &RepoRef,
    number: u64,
) -> Result<Output, Failure> {
    let issue = p.get_issue(t, r, number)?;
    let output = Output::message(format!("#{} {}", issue.number, issue.title))
        .with("url", issue.web_url.clone());
    Ok(records(output, &[issue]))
}

/// The page to open for a repository (its path from `get-repository`).
pub fn repository_page(
    p: &dyn Provider,
    t: &mut dyn Transport,
    r: &RepoRef,
    page: crate::provider::RepoPage,
) -> Result<String, Failure> {
    let repo = p.get_repository(t, r)?;
    p.web_url(&WebTarget::Repository {
        path: repo.path,
        page,
    })
}

/// The page of issue `number`: read first (it exists, and is not a pull
/// request), then built for this server, never an empty or foreign link.
pub fn issue_page(
    p: &dyn Provider,
    t: &mut dyn Transport,
    r: &RepoRef,
    number: u64,
) -> Result<String, Failure> {
    p.get_issue(t, r, number)?;
    let path = p.get_repository(t, r)?.path;
    p.web_url(&WebTarget::Issue { path, number })
}

/// `resolve-remote`: the repository whose clone URL is `remote_url`, found
/// by its path and returned only when a clone URL matches (0 or 1
/// records). The remote's user name or password is never sent.
pub fn resolve_remote(
    p: &dyn Provider,
    t: &mut dyn Transport,
    connection_host: &str,
    web_prefix: &str,
    remote_url: &str,
) -> Result<Output, Failure> {
    let normalized = remote::normalize(remote_url).ok_or_else(|| {
        fail(
            codes::INVALID_REPOSITORY,
            "that is not a git remote URL (https, ssh or git@host:path)",
        )
    })?;
    let host = connection_host.split(':').next().unwrap_or_default();
    let found = if normalized
        .split('/')
        .next()
        .is_some_and(|h| h.eq_ignore_ascii_case(host))
    {
        let path = remote::project_path(remote_url, web_prefix).unwrap_or_default();
        p.repository_by_path(t, &path)?.filter(|repo| {
            remote::matches(
                remote_url,
                &[repo.http_url.as_deref(), repo.ssh_url.as_deref()],
            )
        })
    } else {
        None
    };
    Ok(match found {
        Some(repo) => records(Output::message(format!("this is {}", repo.path)), &[repo]),
        None => records(
            Output::message("no repository of this connection has that remote"),
            &[] as &[Value],
        ),
    })
}

/// `extension`: a provider-only operation by id, on a repository.
pub fn extension(
    p: &dyn Provider,
    t: &mut dyn Transport,
    id: &str,
    r: &RepoRef,
    args: &Value,
) -> Result<Output, Failure> {
    let value = p.extension(t, id, r, args)?;
    let text = value.to_string();
    if text.len() > crate::records::MAX_RECORDS_BYTES {
        return Err(fail(codes::INVALID_RESPONSE, "the answer is too large"));
    }
    Ok(Output::message(format!("{} done", clean(id, 64))).with("result", text))
}

/// A reference given by the user (`ref`, `source`, `target`).
pub fn reference(text: &str) -> Result<String, Failure> {
    check_ref(text).map(str::to_owned)
}
