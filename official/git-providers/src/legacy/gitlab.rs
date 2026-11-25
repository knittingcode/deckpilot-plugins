//! `legacy-gitlab-*`: the actions of the old `deckpilot.gitlab` 0.1.2 on a
//! Git Providers GitLab connection. The code is the old plugin's
//! (`official/gitlab/src/lib.rs` and `api.rs`), with the connection in
//! place of the `apiUrl` setting and the credential slot (see
//! [`super`]); the outputs, messages and error codes are the old ones.

use deckpilot_first_party_support::{bounded_lines, bounded_lines_with_rest, saturating_i64};
use deckpilot_plugin_sdk::prelude::*;
use serde_json::Value;

use crate::provider::ConnectionInfo;
use crate::transport::Transport;
use api::{Api, Failure, Project, clean, codes, fail};

/// Merge request filters of `merge-requests`.
pub const FILTERS: [&str; 3] = ["all", "author-me", "review-requested"];
/// Most merge requests listed.
pub const MAX_LISTED: i64 = 100;

/// The old plugin's API code (`official/gitlab/src/api.rs`) on the
/// connection's transport.
pub mod api {
    use deckpilot_first_party_support::{Deadline, MAX_ANSWER_KIB, Server, retry_after};
    use deckpilot_plugin_sdk::prelude::*;
    use serde_json::Value;

    pub use deckpilot_first_party_support::clean;

    use crate::provider::ConnectionInfo;
    use crate::transport::Transport;

    /// GitLab.com's API.
    pub const DEFAULT_API_URL: &str = "https://gitlab.com/api/v4";
    /// The path of the API under a GitLab's web address.
    pub const API_SUFFIX: &str = "/api/v4";
    /// Items asked for per page: well below the 512 KiB an answer may have.
    pub const PER_PAGE: usize = 50;
    /// Items per page when a page of [`PER_PAGE`] is too large.
    pub const SMALL_PAGE: usize = 10;
    /// Most items read to count a list or the jobs of a pipeline.
    pub const MAX_ITEMS: usize = 500;

    /// The old plugin's error codes, declared in Git Providers'
    /// `manifest.json` too.
    pub mod codes {
        pub const INVALID_API_URL: &str = "invalid-api-url";
        pub const INVALID_PROJECT: &str = "invalid-project";
        pub const INVALID_REF: &str = "invalid-ref";
        pub const UNAUTHORIZED: &str = "unauthorized";
        pub const FORBIDDEN: &str = "forbidden";
        pub const NOT_FOUND: &str = "not-found";
        pub const RATE_LIMITED: &str = "rate-limited";
        pub const GITLAB_ERROR: &str = "gitlab-error";
        pub const INVALID_RESPONSE: &str = "invalid-response";
        pub const URL_NOT_ALLOWED: &str = "url-not-allowed";
        pub const TIMED_OUT: &str = "timed-out";
        /// Every code above.
        pub const ALL: &[&str] = &[
            INVALID_API_URL,
            INVALID_PROJECT,
            INVALID_REF,
            UNAUTHORIZED,
            FORBIDDEN,
            NOT_FOUND,
            RATE_LIMITED,
            GITLAB_ERROR,
            INVALID_RESPONSE,
            URL_NOT_ALLOWED,
            TIMED_OUT,
        ];
    }

    /// A failed action.
    pub type Failure = PluginError;

    pub fn fail(code: &str, message: impl Into<String>) -> Failure {
        PluginError::new(code, message)
    }

    /// A project: its full path such as `group/subgroup/project`, or its
    /// numeric id.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Project(String);

    impl Project {
        pub fn parse(text: &str) -> Result<Self, Failure> {
            let text = text.trim();
            let invalid = || {
                fail(
                    codes::INVALID_PROJECT,
                    format!(
                        "“{}” is not a project such as group/project",
                        clean(text, 60)
                    ),
                )
            };
            let text = text.strip_suffix(".git").unwrap_or(text);
            if !text.is_empty() && text.len() <= 20 && text.chars().all(|c| c.is_ascii_digit()) {
                return Ok(Self(text.to_owned()));
            }
            let segments: Vec<&str> = text.split('/').collect();
            let valid = text.len() <= 1000
                && (2..=21).contains(&segments.len())
                && segments.iter().all(|s| {
                    (1..=255).contains(&s.len())
                        && *s != "."
                        && *s != ".."
                        && !s.starts_with('-')
                        && s.chars()
                            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
                });
            if valid {
                Ok(Self(text.to_owned()))
            } else {
                Err(invalid())
            }
        }

        /// The path or id, as GitLab takes it in one (encoded) URL segment.
        pub fn id(&self) -> &str {
            &self.0
        }

        pub fn is_numeric(&self) -> bool {
            self.0.chars().all(|c| c.is_ascii_digit())
        }
    }

    /// A branch, tag or commit name.
    pub fn check_ref(text: &str) -> Result<&str, Failure> {
        let text = text.trim();
        let valid = (1..=250).contains(&text.len())
            && !text.starts_with(['/', '-', '.'])
            && !text.ends_with(['/', '.'])
            && !text.contains("..")
            && !text.contains("//")
            && !text
                .chars()
                .any(|c| c.is_whitespace() || c.is_control() || "~^:?*[\\@{}".contains(c));
        if valid {
            Ok(text)
        } else {
            Err(fail(
                codes::INVALID_REF,
                format!("“{}” is not a branch, tag or commit name", clean(text, 60)),
            ))
        }
    }

    fn invalid_api() -> Failure {
        fail(
            codes::INVALID_API_URL,
            "the connection's address is not an https address of a GitLab API such as \
             https://gitlab.com/api/v4, without a user name, query or fragment; edit the \
             connection in Git Providers",
        )
    }

    /// The API's base: the connection's.
    pub fn api_base(text: &str) -> Result<Server, Failure> {
        Server::parse(text).ok_or_else(invalid_api)
    }

    /// The web address that belongs to an API, as the old plugin derived
    /// it: the API URL without its `/api/v4`. A connection says its web
    /// base itself ([`Api::connect`]); this is only the old rule.
    pub fn web_base(api: &Server) -> Server {
        api.without_suffix(API_SUFFIX)
    }

    /// The connection's GitLab: its API and web bases, and that it is a
    /// GitLab connection at all.
    pub fn servers(connection: &ConnectionInfo) -> Result<(Server, Server), Failure> {
        if connection.type_id != "gitlab" {
            return Err(fail(
                codes::INVALID_API_URL,
                "this connection is not a GitLab connection",
            ));
        }
        let api = api_base(&connection.api_base)?;
        let web = Server::parse(&connection.web_base).ok_or_else(invalid_api)?;
        Ok((api, web))
    }

    /// A page to open: https, on the GitLab web address, without
    /// credentials.
    pub fn web_url(
        text: &str,
        web: &Server,
    ) -> Result<deckpilot_first_party_support::WebPage, Failure> {
        web.page(text).ok_or_else(|| {
            fail(
                codes::URL_NOT_ALLOWED,
                format!("only https pages on {} can be opened", web.host()),
            )
        })
    }

    /// How long to wait when GitLab answers 429: `Retry-After`, else the
    /// limit's reset time (`RateLimit-Reset`, Unix seconds), else a
    /// minute.
    pub fn rate_limit_wait(status: u16, response: &Response, now: Option<u64>) -> Option<u64> {
        if status != 429 {
            return None;
        }
        let header = |name: &str| response.header(name).map(str::trim);
        if let Some(seconds) = header("retry-after").and_then(|v| retry_after(v, now)) {
            return Some(seconds);
        }
        let reset = header("ratelimit-reset").and_then(|v| v.parse::<u64>().ok());
        Some(match (reset, now) {
            (Some(reset), Some(now)) => reset.saturating_sub(now),
            _ => 60,
        })
    }

    /// One successful answer.
    #[derive(Debug)]
    pub struct Reply {
        pub json: Value,
        /// The next page (`link: rel="next"`), on the API.
        pub next: Option<String>,
        /// `X-Total`: how many items the whole list has, when GitLab says.
        pub total: Option<u64>,
        pub rate_remaining: Option<u64>,
    }

    /// The items of a paged list.
    #[derive(Debug)]
    pub struct List {
        pub items: Vec<Value>,
        /// `X-Total` of the first page, when given.
        pub total: Option<u64>,
        /// More items exist than were read.
        pub more: bool,
        pub pages: usize,
    }

    use super::super::Refused;

    fn too_large_failure(what: &str) -> Failure {
        fail(
            codes::INVALID_RESPONSE,
            format!("GitLab's answer for {what} is larger than {MAX_ANSWER_KIB} KiB"),
        )
    }

    /// The API for one action: the connection's transport and bases, and
    /// the action's time limit counted with GitLab's `Date`.
    pub struct Api<'t> {
        t: &'t mut dyn Transport,
        api: Server,
        web: Server,
        deadline: Deadline,
    }

    impl std::fmt::Debug for Api<'_> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Api")
                .field("api", &self.api)
                .field("deadline", &self.deadline)
                .finish_non_exhaustive()
        }
    }

    impl<'t> Api<'t> {
        /// The connection's GitLab. No token check: the connection has its
        /// own, which the plugin never sees.
        pub fn connect(
            connection: &ConnectionInfo,
            t: &'t mut dyn Transport,
        ) -> Result<Self, Failure> {
            let (api, web) = servers(connection)?;
            Ok(Self {
                t,
                web,
                api,
                deadline: Deadline::new(super::super::TIMEOUT_SECONDS),
            })
        }

        /// The web address of this GitLab, ending in `/`.
        pub fn web(&self) -> String {
            self.web.root()
        }

        /// An API URL: the base, these path segments (each encoded, so a
        /// project path becomes one segment) and query.
        pub fn endpoint(&self, segments: &[&str], query: &[(&str, &str)]) -> String {
            self.api.endpoint(segments, query)
        }

        fn fetch(&mut self, url: String, what: &str) -> Result<Reply, Refused> {
            if self.deadline.passed() {
                return Err(fail(
                    codes::TIMED_OUT,
                    "GitLab did not answer within the time limit",
                )
                .into());
            }
            let response = super::super::get(self.t, url, &[("Accept", "application/json")])
                .map_err(|e| super::super::refusal(e, "GitLab"))?;
            let date = self.deadline.observe(&response);
            if !response.is_success() {
                return Err(status_failure(&response, what, date).into());
            }
            let json: Value = response.json().map_err(|_| {
                fail(
                    codes::INVALID_RESPONSE,
                    format!("GitLab's answer for {what} is not JSON"),
                )
            })?;
            let next = match response.link("next") {
                Some(link) if self.api.contains(link) => Some(link.trim().to_owned()),
                Some(_) => {
                    return Err(fail(
                        codes::INVALID_RESPONSE,
                        "GitLab pointed to a next page outside its API",
                    )
                    .into());
                }
                None => None,
            };
            let number = |name: &str| response.header(name).and_then(|v| v.trim().parse().ok());
            Ok(Reply {
                json,
                next,
                total: number("x-total"),
                rate_remaining: number("ratelimit-remaining"),
            })
        }

        /// Sends `GET url` and reads a JSON answer.
        pub fn get(&mut self, url: String, what: &str) -> Result<Reply, Failure> {
            self.fetch(url, what).map_err(|refused| match refused {
                Refused::TooLarge => too_large_failure(what),
                Refused::Failed(failure) => failure,
            })
        }

        /// Reads at most `limit` items of a list (each page an array) in
        /// pages of `per_page`, following `link: rel="next"`.
        fn pages(
            &mut self,
            segments: &[&str],
            query: &[(&str, &str)],
            what: &str,
            limit: usize,
            per_page: usize,
        ) -> Result<List, Refused> {
            let size = per_page.to_string();
            let mut query = query.to_vec();
            query.push(("per_page", &size));
            let mut url = self.endpoint(segments, &query);
            let mut list = List {
                items: Vec::new(),
                total: None,
                more: false,
                pages: 0,
            };
            let max_pages = limit.div_ceil(per_page).max(1);
            loop {
                let reply = self.fetch(url, what)?;
                list.pages += 1;
                let items = reply.json.as_array().ok_or_else(|| {
                    fail(
                        codes::INVALID_RESPONSE,
                        format!("GitLab's answer for {what} has no list"),
                    )
                })?;
                list.items.extend(items.iter().cloned());
                if list.pages == 1 {
                    list.total = reply.total;
                }
                match reply.next {
                    Some(next) if list.pages < max_pages && list.items.len() < limit => url = next,
                    Some(_) => {
                        list.more = true;
                        break;
                    }
                    None => break,
                }
            }
            if list.items.len() > limit {
                list.items.truncate(limit);
                list.more = true;
            }
            Ok(list)
        }

        /// Reads at most `limit` items of a list: [`PER_PAGE`] per page,
        /// and [`SMALL_PAGE`] per page when such a page is too large.
        pub fn get_list(
            &mut self,
            segments: &[&str],
            query: &[(&str, &str)],
            what: &str,
            limit: usize,
        ) -> Result<List, Failure> {
            let limit = limit.max(1);
            let first = limit.min(PER_PAGE);
            match self.pages(segments, query, what, limit, first) {
                Ok(list) => Ok(list),
                Err(Refused::Failed(failure)) => Err(failure),
                Err(Refused::TooLarge) if first > SMALL_PAGE => {
                    match self.pages(segments, query, what, limit, SMALL_PAGE) {
                        Ok(list) => Ok(list),
                        Err(Refused::Failed(failure)) => Err(failure),
                        Err(Refused::TooLarge) => Err(too_large_failure(what)),
                    }
                }
                Err(Refused::TooLarge) => Err(too_large_failure(what)),
            }
        }
    }

    fn status_failure(response: &Response, what: &str, now: Option<u64>) -> Failure {
        let status = response.status;
        let detail = response
            .json::<Value>()
            .ok()
            .and_then(|v| {
                ["message", "error_description", "error"]
                    .iter()
                    .find_map(|key| v.get(*key).and_then(Value::as_str).map(|m| clean(m, 200)))
            })
            .filter(|m| !m.is_empty())
            .map(|m| format!(": {m}"))
            .unwrap_or_default();
        if let Some(wait) = rate_limit_wait(status, response, now) {
            return fail(
                codes::RATE_LIMITED,
                format!("GitLab's rate limit is reached; try again in {wait} s"),
            )
            .retryable();
        }
        match status {
            401 => fail(
                codes::UNAUTHORIZED,
                "GitLab refused the connection's token (401); it may be expired or revoked: \
                 reconnect it in Git Providers",
            ),
            403 => fail(
                codes::FORBIDDEN,
                format!("the token may not read {what} (403){detail}"),
            ),
            404 => fail(
                codes::NOT_FOUND,
                format!("{what} was not found, or the token cannot see it (404)"),
            ),
            _ => fail(
                codes::GITLAB_ERROR,
                format!("GitLab answered {status} for {what}{detail}"),
            ),
        }
    }
}

fn optional(args: &Args<'_>, key: &str) -> Result<Option<String>, Failure> {
    Ok(args
        .get::<Option<String>>(key)?
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty()))
}

fn project(args: &Args<'_>) -> Result<Option<Project>, Failure> {
    optional(args, "project")?
        .as_deref()
        .map(Project::parse)
        .transpose()
}

fn required_project(args: &Args<'_>) -> Result<Project, Failure> {
    project(args)?.ok_or_else(|| fail(codes::INVALID_PROJECT, "choose a project (group/project)"))
}

fn string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn number(value: &Value, key: &str) -> i64 {
    value
        .get(key)
        .and_then(Value::as_u64)
        .map_or(0, saturating_i64)
}

fn boolean(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn invalid_response(what: &str) -> Failure {
    fail(
        codes::INVALID_RESPONSE,
        format!("GitLab's answer for {what} is not what was expected"),
    )
}

pub fn current_user(
    _ctx: &mut Context<'_>,
    _args: &Args<'_>,
    connection: &ConnectionInfo,
    t: &mut dyn Transport,
) -> ActionResult {
    let mut api = Api::connect(connection, t)?;
    let reply = api.get(api.endpoint(&["user"], &[]), "the signed-in user")?;
    let user = &reply.json;
    let username =
        string(user, "username").ok_or_else(|| invalid_response("the signed-in user"))?;
    let mut output = Output::message(format!("signed in to GitLab as {username}"))
        .with("username", username.clone())
        .with(
            "url",
            string(user, "web_url").unwrap_or_else(|| format!("{}{username}", api.web())),
        );
    if let Some(name) = string(user, "name").filter(|n| !n.is_empty()) {
        output = output.with("name", clean(&name, 200));
    }
    if let Some(remaining) = reply.rate_remaining {
        output = output.with("rateLimitRemaining", saturating_i64(remaining));
    }
    Ok(output)
}

pub fn project_info(
    _ctx: &mut Context<'_>,
    args: &Args<'_>,
    connection: &ConnectionInfo,
    t: &mut dyn Transport,
) -> ActionResult {
    let mut api = Api::connect(connection, t)?;
    let project = required_project(args)?;
    let what = format!("the project {}", project.id());
    let reply = api.get(api.endpoint(&["projects", project.id()], &[]), &what)?;
    let json = &reply.json;
    let path = string(json, "path_with_namespace").ok_or_else(|| invalid_response(&what))?;
    let mut output = Output::message(format!("read {path}"))
        .with("path", path.clone())
        .with(
            "visibility",
            string(json, "visibility").unwrap_or_else(|| "unknown".into()),
        )
        .with("archived", boolean(json, "archived"))
        .with("stars", number(json, "star_count"))
        .with("forks", number(json, "forks_count"))
        .with(
            "url",
            string(json, "web_url").unwrap_or_else(|| format!("{}{path}", api.web())),
        );
    if let Some(branch) = string(json, "default_branch") {
        output = output.with("defaultBranch", branch);
    }
    // Absent when the project's issues are turned off.
    if let Some(issues) = json.get("open_issues_count").and_then(Value::as_u64) {
        output = output.with("openIssues", saturating_i64(issues));
    }
    if let Some(description) = string(json, "description").filter(|d| !d.is_empty()) {
        output = output.with("description", clean(&description, 500));
    }
    if let Some(activity) = string(json, "last_activity_at") {
        output = output.with("lastActivityAt", activity);
    }
    Ok(output)
}

/// `group/project!12 Title (@username)`, one line.
fn merge_request_line(item: &Value) -> String {
    let reference = item
        .get("references")
        .and_then(|r| string(r, "full"))
        .unwrap_or_else(|| format!("!{}", number(item, "iid")));
    let title = clean(&string(item, "title").unwrap_or_default(), 200);
    let author = item
        .get("author")
        .and_then(|u| u.get("username"))
        .and_then(Value::as_str)
        .unwrap_or("?");
    let draft = ["draft", "work_in_progress"]
        .iter()
        .any(|key| item.get(*key).and_then(Value::as_bool) == Some(true));
    format!(
        "{reference} {title} (@{author}){}",
        if draft { " [draft]" } else { "" }
    )
}

/// `author_id` or `reviewer_id` of the signed-in user, for the filters
/// about you.
fn me_filter(api: &mut Api<'_>, filter: &str) -> Result<Option<(&'static str, String)>, Failure> {
    let key = match filter {
        "author-me" => "author_id",
        "review-requested" => "reviewer_id",
        _ => return Ok(None),
    };
    let me = api.get(api.endpoint(&["user"], &[]), "the signed-in user")?;
    let id = me
        .json
        .get("id")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid_response("the signed-in user"))?;
    Ok(Some((key, id.to_string())))
}

pub fn merge_requests(
    _ctx: &mut Context<'_>,
    args: &Args<'_>,
    connection: &ConnectionInfo,
    t: &mut dyn Transport,
) -> ActionResult {
    list_merge_requests(args, connection, t, false)
}

pub fn review_requests(
    _ctx: &mut Context<'_>,
    args: &Args<'_>,
    connection: &ConnectionInfo,
    t: &mut dyn Transport,
) -> ActionResult {
    list_merge_requests(args, connection, t, true)
}

fn list_merge_requests(
    args: &Args<'_>,
    connection: &ConnectionInfo,
    t: &mut dyn Transport,
    review_requests: bool,
) -> ActionResult {
    let mut api = Api::connect(connection, t)?;
    let project = project(args)?;
    let filter = if review_requests {
        "review-requested".to_owned()
    } else {
        args.get::<Option<String>>("filter")?
            .unwrap_or_else(|| "all".to_owned())
    };
    if !FILTERS.contains(&filter.as_str()) {
        return Err(fail(
            deckpilot_plugin_sdk::codes::INVALID_PARAMETERS,
            format!("“{}” is not a filter", clean(&filter, 40)),
        ));
    }
    if filter == "all" && project.is_none() {
        return Err(fail(
            codes::INVALID_PROJECT,
            "choose a project to list all its open merge requests",
        ));
    }
    let limit = args
        .get::<Option<i64>>("limit")?
        .unwrap_or(20)
        .clamp(1, MAX_LISTED);
    let limit = usize::try_from(limit).unwrap_or(api::PER_PAGE);
    let mut query: Vec<(&str, String)> = vec![
        ("state", "opened".into()),
        ("order_by", "updated_at".into()),
        ("sort", "desc".into()),
    ];
    if let Some(me) = me_filter(&mut api, &filter)? {
        query.push(me);
    }
    let (segments, what): (Vec<&str>, String) = if let Some(project) = &project {
        (
            vec!["projects", project.id(), "merge_requests"],
            format!("the merge requests of {}", project.id()),
        )
    } else {
        query.push(("scope", "all".into()));
        (vec!["merge_requests"], "your merge requests".to_owned())
    };
    let query: Vec<(&str, &str)> = query.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let list = api.get_list(&segments, &query, &what, limit)?;
    let items = list.items;
    // GitLab leaves `X-Total` out for very large lists; then count by
    // reading a bounded number of pages.
    let (count, exact) = match (list.total, list.more) {
        (Some(total), _) => (total, true),
        (None, false) => (items.len() as u64, true),
        (None, true) => {
            let counted = api.get_list(&segments, &query, &what, api::MAX_ITEMS)?;
            (counted.items.len() as u64, !counted.more)
        }
    };
    let lines: Vec<String> = items.iter().map(merge_request_line).collect();
    let (text, listed) = bounded_lines(&lines);
    let listed = listed as u64;
    let scope = project
        .as_ref()
        .map_or_else(String::new, |p| format!(" in {}", p.id()));
    let shown = if exact {
        count.to_string()
    } else {
        format!("at least {count}")
    };
    let message = if review_requests {
        format!("{shown} merge requests wait for your review{scope}")
    } else {
        format!("{shown} open merge requests{scope} ({filter})")
    };
    let mut output = Output::message(message)
        .with("count", saturating_i64(count))
        .with("countExact", exact)
        .with("listed", saturating_i64(listed))
        .with("items", text)
        .with("truncated", count > listed || !exact);
    if let Some(first) = items.first().and_then(|i| string(i, "web_url")) {
        output = output.with("firstUrl", first);
    }
    Ok(output)
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Counts {
    passed: u64,
    failed: u64,
    pending: u64,
    skipped: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Passed,
    Failed,
    Pending,
    Skipped,
}

/// A job: success passes; failed fails unless failure is allowed;
/// canceled fails; skipped and manual did not run; the rest are pending.
fn job_outcome(job: &Value) -> Outcome {
    match job.get("status").and_then(Value::as_str) {
        Some("success") => Outcome::Passed,
        Some("failed") if job.get("allow_failure").and_then(Value::as_bool) == Some(true) => {
            Outcome::Passed
        }
        Some("failed" | "canceled") => Outcome::Failed,
        Some("skipped" | "manual") => Outcome::Skipped,
        _ => Outcome::Pending,
    }
}

/// The pipeline's own status in DeckPilot's terms.
fn overall(status: &str) -> &'static str {
    match status {
        "success" => "ok",
        "failed" | "canceled" => "failing",
        "skipped" => "none",
        _ => "pending",
    }
}

/// The pipeline to check, how it is named in messages and the merge
/// request's page.
fn chosen_pipeline(
    api: &mut Api<'_>,
    args: &Args<'_>,
    project: &Project,
) -> Result<(Option<Value>, String, Option<String>), Failure> {
    if let Some(number) = args.get::<Option<i64>>("mergeRequest")? {
        let number = number.max(1).to_string();
        let what = format!("the merge request {}!{number}", project.id());
        let url = api.endpoint(&["projects", project.id(), "merge_requests", &number], &[]);
        let reply = api.get(url, &what)?;
        let pipeline = reply
            .json
            .get("head_pipeline")
            .filter(|p| p.is_object())
            .cloned();
        return Ok((
            pipeline,
            format!("{}!{number}", project.id()),
            string(&reply.json, "web_url"),
        ));
    }
    let reference = optional(args, "ref")?.ok_or_else(|| {
        fail(
            deckpilot_plugin_sdk::codes::INVALID_PARAMETERS,
            "choose a branch, tag, commit or merge request",
        )
    })?;
    let reference = api::check_ref(&reference)?;
    let is_sha = reference.len() == 40 && reference.chars().all(|c| c.is_ascii_hexdigit());
    let what = format!("the pipelines of {}@{reference}", project.id());
    let url = api.endpoint(
        &["projects", project.id(), "pipelines"],
        &[
            (if is_sha { "sha" } else { "ref" }, reference),
            ("order_by", "id"),
            ("sort", "desc"),
            ("per_page", "1"),
        ],
    );
    let reply = api.get(url, &what)?;
    let pipeline = reply
        .json
        .as_array()
        .ok_or_else(|| invalid_response(&what))?
        .first()
        .cloned();
    Ok((pipeline, format!("{}@{reference}", project.id()), None))
}

pub fn pipeline(
    _ctx: &mut Context<'_>,
    args: &Args<'_>,
    connection: &ConnectionInfo,
    t: &mut dyn Transport,
) -> ActionResult {
    let mut api = Api::connect(connection, t)?;
    let project = required_project(args)?;
    let (pipeline, shown, page) = chosen_pipeline(&mut api, args, &project)?;
    let Some(pipeline) = pipeline else {
        let mut output = Output::message(format!("{shown}: no pipeline"))
            .with("state", "none")
            .with("total", 0_i64)
            .with("passed", 0_i64)
            .with("failed", 0_i64)
            .with("pending", 0_i64)
            .with("skipped", 0_i64)
            .with("failing", "")
            .with("truncated", false);
        if let Some(page) = page {
            output = output.with("url", page);
        }
        return Ok(output);
    };
    let what = format!("the pipeline of {shown}");
    let id = pipeline
        .get("id")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid_response(&what))?;
    let status = string(&pipeline, "status").unwrap_or_else(|| "unknown".into());
    let jobs = api.get_list(
        &[
            "projects",
            project.id(),
            "pipelines",
            &id.to_string(),
            "jobs",
        ],
        &[],
        &format!("the jobs of {shown}"),
        api::MAX_ITEMS,
    )?;
    let mut counts = Counts::default();
    let mut failing = Vec::new();
    for job in &jobs.items {
        match job_outcome(job) {
            Outcome::Passed => counts.passed += 1,
            Outcome::Pending => counts.pending += 1,
            Outcome::Skipped => counts.skipped += 1,
            Outcome::Failed => {
                counts.failed += 1;
                failing.push(clean(&string(job, "name").unwrap_or_default(), 120));
            }
        }
    }
    let state = overall(&status);
    let truncated = jobs.more || jobs.total.is_some_and(|t| t > jobs.items.len() as u64);
    let mut output = Output::message(format!(
        "{shown}: {state} ({} passed, {} failed, {} pending)",
        counts.passed, counts.failed, counts.pending
    ))
    .with("state", state)
    .with("status", clean(&status, 40))
    .with("pipelineId", saturating_i64(id))
    .with("total", saturating_i64(jobs.items.len() as u64))
    .with("passed", saturating_i64(counts.passed))
    .with("failed", saturating_i64(counts.failed))
    .with("pending", saturating_i64(counts.pending))
    .with("skipped", saturating_i64(counts.skipped))
    .with("failing", bounded_lines_with_rest(&failing))
    .with("truncated", truncated);
    if let Some(sha) = string(&pipeline, "sha") {
        output = output.with("sha", sha);
    }
    if let Some(url) = page.or_else(|| string(&pipeline, "web_url")) {
        output = output.with("url", url);
    }
    Ok(output)
}

pub fn open(
    ctx: &mut Context<'_>,
    args: &Args<'_>,
    connection: &ConnectionInfo,
    _t: &mut dyn Transport,
) -> ActionResult {
    let (_, web) = api::servers(connection)?;
    let target = if let Some(url) = optional(args, "url")? {
        url
    } else {
        let project = required_project(args)?;
        if project.is_numeric() {
            return Err(fail(
                codes::INVALID_PROJECT,
                "a project is opened by its path (group/project), not its id",
            ));
        }
        match args.get::<Option<i64>>("mergeRequest")? {
            Some(number) => format!(
                "{}{}/-/merge_requests/{}",
                web.root(),
                project.id(),
                number.max(1)
            ),
            None => format!("{}{}", web.root(), project.id()),
        }
    };
    let page = api::web_url(&target, &web)?;
    ctx.open_url(page.url)?;
    Ok(Output::message(format!("opened {}", page.shown)).with("url", page.shown))
}
