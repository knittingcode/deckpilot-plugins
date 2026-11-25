//! `legacy-github-*`: the actions of the old `deckpilot.github` 0.2.1 on a
//! Git Providers GitHub connection. The code is the old plugin's
//! (`official/github/src/lib.rs` and `api.rs`), with the connection in
//! place of the `apiUrl` setting and the credential slot (see
//! [`super`]); the outputs, messages and error codes are the old ones.

use deckpilot_first_party_support::{bounded_lines, clean, saturating_i64};
use deckpilot_plugin_sdk::prelude::*;
use serde_json::Value;

use crate::provider::ConnectionInfo;
use crate::transport::Transport;
use api::{Api, Failure, Repo, codes, fail};

/// Pull request filters of `pull-requests`.
pub const FILTERS: [&str; 3] = ["all", "author-me", "review-requested"];
/// Most pull requests listed.
pub const MAX_LISTED: i64 = 100;
pub use deckpilot_first_party_support::MAX_OUTPUT_TEXT;

/// The old plugin's API code (`official/github/src/api.rs`) on the
/// connection's transport.
pub mod api {
    use deckpilot_first_party_support::{Server, WebPage};
    use deckpilot_plugin_sdk::prelude::*;
    use serde_json::Value;

    pub use deckpilot_first_party_support::{clean, http_date};

    use crate::provider::ConnectionInfo;
    use crate::transport::Transport;

    /// GitHub's API.
    pub const API: &str = "https://api.github.com";
    /// Most pages read for one list.
    pub const MAX_PAGES: usize = 10;
    /// Items asked for per page: well below the 512 KiB an answer may have.
    pub const PER_PAGE: usize = 50;
    /// The REST API version asked for.
    pub const API_VERSION: &str = "2022-11-28";
    /// GitHub refuses requests without a User-Agent; the old plugin's.
    pub const USER_AGENT: &str = "DeckPilot-GitHub/0.2.1";
    /// Largest answer the agent hands to a plugin.
    const MAX_ANSWER_KIB: usize = 512;

    /// The old plugin's error codes, declared in Git Providers'
    /// `manifest.json` too.
    pub mod codes {
        pub const INVALID_API_URL: &str = "invalid-api-url";
        pub const INVALID_REPOSITORY: &str = "invalid-repository";
        pub const INVALID_REF: &str = "invalid-ref";
        pub const UNAUTHORIZED: &str = "unauthorized";
        pub const FORBIDDEN: &str = "forbidden";
        pub const NOT_FOUND: &str = "not-found";
        pub const RATE_LIMITED: &str = "rate-limited";
        pub const GITHUB_ERROR: &str = "github-error";
        pub const INVALID_RESPONSE: &str = "invalid-response";
        pub const URL_NOT_ALLOWED: &str = "url-not-allowed";
        pub const TIMED_OUT: &str = "timed-out";
        /// Every code above.
        pub const ALL: &[&str] = &[
            INVALID_API_URL,
            INVALID_REPOSITORY,
            INVALID_REF,
            UNAUTHORIZED,
            FORBIDDEN,
            NOT_FOUND,
            RATE_LIMITED,
            GITHUB_ERROR,
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

    /// A repository named `owner/name`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Repo {
        pub owner: String,
        pub name: String,
    }

    impl Repo {
        /// `owner/name`, optionally ending in `.git`.
        pub fn parse(text: &str) -> Result<Self, Failure> {
            let text = text.trim();
            let invalid = || {
                fail(
                    codes::INVALID_REPOSITORY,
                    format!(
                        "“{}” is not a repository such as owner/name",
                        clean(text, 60)
                    ),
                )
            };
            let text = text.strip_suffix(".git").unwrap_or(text);
            let (owner, name) = text.split_once('/').ok_or_else(invalid)?;
            let owner_ok = (1..=39).contains(&owner.len())
                && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
            let name_ok = (1..=100).contains(&name.len())
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
                && name != "."
                && name != "..";
            if owner_ok && name_ok {
                Ok(Self {
                    owner: owner.to_owned(),
                    name: name.to_owned(),
                })
            } else {
                Err(invalid())
            }
        }

        pub fn full_name(&self) -> String {
            format!("{}/{}", self.owner, self.name)
        }
    }

    /// A branch, tag or commit name, safe to put into a URL path.
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
            "the connection's address is not an https address of a GitHub API such as \
             https://api.github.com or https://<host>/api/v3, without a user name, query or \
             fragment; edit the connection in Git Providers",
        )
    }

    /// The API's base: the connection's (`https://api.github.com`, or
    /// `https://<host>/api/v3` for GitHub Enterprise Server).
    pub fn api_base(text: &str) -> Result<Server, Failure> {
        Server::parse(text).ok_or_else(invalid_api)
    }

    /// The web address that belongs to an API, as the old plugin derived
    /// it: `github.com` for `api.github.com`, `<x>.ghe.com` for
    /// `api.<x>.ghe.com`, the same host for GitHub Enterprise Server;
    /// always the host's root. A connection says its web base itself
    /// ([`Api::connect`]); this is only the old rule, kept for its tests.
    pub fn web_base(api: &Server) -> Server {
        let host = api.host();
        let web_host = if host == "api.github.com" {
            "github.com"
        } else if let Some(name) = host
            .strip_prefix("api.")
            .filter(|rest| rest.ends_with(".ghe.com"))
        {
            name
        } else {
            host
        };
        let origin = api.origin().replacen(host, web_host, 1);
        Server::parse(&origin).unwrap_or_else(|| api.without_suffix(api.path()))
    }

    /// The connection's GitHub: its API and web bases, and that it is a
    /// GitHub connection at all.
    pub fn servers(connection: &ConnectionInfo) -> Result<(Server, Server), Failure> {
        if connection.type_id != "github" {
            return Err(fail(
                codes::INVALID_API_URL,
                "this connection is not a GitHub connection",
            ));
        }
        let api = api_base(&connection.api_base)?;
        let web = Server::parse(&connection.web_base).ok_or_else(invalid_api)?;
        Ok((api, web))
    }

    /// A page to open: https, on the GitHub web host, without credentials.
    pub fn web_url(text: &str, web: &Server) -> Result<WebPage, Failure> {
        web.page(text).ok_or_else(|| {
            fail(
                codes::URL_NOT_ALLOWED,
                format!("only https pages on {} can be opened", web.host()),
            )
        })
    }

    /// How long to wait when an answer says the rate limit is reached
    /// (the old plugin's rule): a secondary limit's `Retry-After`, or the
    /// primary limit's reset time when no requests remain, or a minute
    /// for a 429 or a 403 whose `message` is about the rate limit. `None`
    /// when the answer is not about the rate limit.
    pub fn rate_limit_wait(
        status: u16,
        response: &Response,
        message: Option<&str>,
        now: Option<u64>,
    ) -> Option<u64> {
        if status != 403 && status != 429 {
            return None;
        }
        let header = |name: &str| response.header(name).map(str::trim);
        if let Some(value) = header("retry-after") {
            if let Ok(seconds) = value.parse::<u64>() {
                return Some(seconds);
            }
            if let Some(at) = http_date(value) {
                return Some(now.map_or(60, |now| at.saturating_sub(now)));
            }
        }
        if header("x-ratelimit-remaining") == Some("0") {
            let reset = header("x-ratelimit-reset").and_then(|v| v.parse::<u64>().ok());
            return Some(match (reset, now) {
                (Some(reset), Some(now)) => reset.saturating_sub(now),
                _ => 60,
            });
        }
        let about_the_limit =
            message.is_some_and(|m| m.to_ascii_lowercase().contains("rate limit"));
        (status == 429 || about_the_limit).then_some(60)
    }

    /// One successful answer.
    #[derive(Debug)]
    pub struct Reply {
        pub json: Value,
        /// The next page (`link: rel="next"`), on the API.
        pub next: Option<String>,
        pub rate_remaining: Option<u64>,
    }

    /// The items of a paged list.
    #[derive(Debug)]
    pub struct List {
        pub items: Vec<Value>,
        /// The first page's answer, for fields beside the list.
        pub first: Value,
        /// `total_count`, when GitHub gives one.
        pub total: Option<u64>,
        /// More pages exist than were read.
        pub truncated: bool,
        pub pages: usize,
    }

    /// The API for one action: the connection's transport and bases, and
    /// the action's time limit counted with GitHub's `Date`.
    pub struct Api<'t> {
        t: &'t mut dyn Transport,
        api: Server,
        web: Server,
        timeout: u64,
        first_date: Option<u64>,
        latest_date: Option<u64>,
    }

    impl std::fmt::Debug for Api<'_> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Api")
                .field("api", &self.api)
                .field("timeout", &self.timeout)
                .finish_non_exhaustive()
        }
    }

    impl<'t> Api<'t> {
        /// The connection's GitHub. No token check: the connection has
        /// its own, which the plugin never sees.
        pub fn connect(
            connection: &ConnectionInfo,
            t: &'t mut dyn Transport,
        ) -> Result<Self, Failure> {
            let (api, web) = servers(connection)?;
            Ok(Self {
                t,
                web,
                api,
                timeout: super::super::TIMEOUT_SECONDS,
                first_date: None,
                latest_date: None,
            })
        }

        /// The web address of this GitHub, ending in `/`.
        pub fn web(&self) -> String {
            self.web.root()
        }

        /// An API URL: the base, these path segments (each encoded) and
        /// query.
        pub fn endpoint(&self, segments: &[&str], query: &[(&str, &str)]) -> String {
            self.api.endpoint(segments, query)
        }

        /// Fails once the time limit has passed since the first answer.
        fn check_time(&self) -> Result<(), Failure> {
            if let (Some(first), Some(latest)) = (self.first_date, self.latest_date)
                && latest.saturating_sub(first) >= self.timeout
            {
                return Err(fail(
                    codes::TIMED_OUT,
                    "GitHub did not answer within the time limit",
                ));
            }
            Ok(())
        }

        /// Sends `GET url` and reads a JSON answer. `what` names the thing
        /// asked for in messages, e.g. “the repository a/b”.
        pub fn get(&mut self, url: String, what: &str) -> Result<Reply, Failure> {
            self.check_time()?;
            let response = super::super::get(
                self.t,
                url,
                &[
                    ("Accept", "application/vnd.github+json"),
                    ("X-GitHub-Api-Version", API_VERSION),
                    ("User-Agent", USER_AGENT),
                ],
            )
            .map_err(|e| match super::super::refusal(e, "GitHub") {
                super::super::Refused::TooLarge => fail(
                    codes::INVALID_RESPONSE,
                    format!("GitHub's answer for {what} is larger than {MAX_ANSWER_KIB} KiB"),
                ),
                super::super::Refused::Failed(failure) => failure,
            })?;
            let date = response.header("date").and_then(http_date);
            if date.is_some() {
                self.first_date = self.first_date.or(date);
                self.latest_date = date;
            }
            if !response.is_success() {
                return Err(status_failure(&response, what, date));
            }
            let json: Value = response.json().map_err(|_| {
                fail(
                    codes::INVALID_RESPONSE,
                    format!("GitHub's answer for {what} is not JSON"),
                )
            })?;
            let next = match response.link("next") {
                Some(link) if self.api.contains(link) => Some(link.trim().to_owned()),
                Some(_) => {
                    return Err(fail(
                        codes::INVALID_RESPONSE,
                        "GitHub pointed to a next page outside its API",
                    ));
                }
                None => None,
            };
            Ok(Reply {
                json,
                next,
                rate_remaining: response
                    .header("x-ratelimit-remaining")
                    .and_then(|v| v.trim().parse().ok()),
            })
        }

        /// Reads a list from `key` of each page (or the page itself when
        /// it is an array), following `link: rel="next"` for at most
        /// [`MAX_PAGES`] pages.
        pub fn get_list(
            &mut self,
            url: String,
            what: &str,
            key: Option<&str>,
        ) -> Result<List, Failure> {
            let mut url = url;
            let mut list = List {
                items: Vec::new(),
                first: Value::Null,
                total: None,
                truncated: false,
                pages: 0,
            };
            loop {
                let reply = self.get(url, what)?;
                list.pages += 1;
                let items = match key {
                    Some(key) => reply.json.get(key),
                    None => Some(&reply.json),
                }
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    fail(
                        codes::INVALID_RESPONSE,
                        format!("GitHub's answer for {what} has no list"),
                    )
                })?;
                list.items.extend(items.iter().cloned());
                if list.total.is_none() {
                    list.total = reply.json.get("total_count").and_then(Value::as_u64);
                }
                if list.pages == 1 {
                    list.first = reply.json;
                }
                match reply.next {
                    Some(next) if list.pages < MAX_PAGES => url = next,
                    Some(_) => {
                        list.truncated = true;
                        return Ok(list);
                    }
                    None => return Ok(list),
                }
            }
        }
    }

    fn status_failure(response: &Response, what: &str, now: Option<u64>) -> Failure {
        let status = response.status;
        let message = response
            .json::<Value>()
            .ok()
            .and_then(|v| {
                v.get("message")
                    .and_then(Value::as_str)
                    .map(|m| clean(m, 200))
            })
            .filter(|m| !m.is_empty());
        let detail = message
            .as_deref()
            .map(|m| format!(": {m}"))
            .unwrap_or_default();
        if let Some(wait) = rate_limit_wait(status, response, message.as_deref(), now) {
            return fail(
                codes::RATE_LIMITED,
                format!("GitHub's rate limit is reached; try again in {wait} s"),
            )
            .retryable();
        }
        match status {
            401 => fail(
                codes::UNAUTHORIZED,
                "GitHub refused the connection's token (401); it may be expired or revoked: \
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
                codes::GITHUB_ERROR,
                format!("GitHub answered {status} for {what}{detail}"),
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

fn repository(args: &Args<'_>) -> Result<Option<Repo>, Failure> {
    optional(args, "repository")?
        .as_deref()
        .map(Repo::parse)
        .transpose()
}

fn required_repository(args: &Args<'_>) -> Result<Repo, Failure> {
    repository(args)?.ok_or_else(|| {
        fail(
            codes::INVALID_REPOSITORY,
            "choose a repository (owner/name)",
        )
    })
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
        format!("GitHub's answer for {what} is not what was expected"),
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
    let login = string(user, "login").ok_or_else(|| invalid_response("the signed-in user"))?;
    let mut output = Output::message(format!("signed in to GitHub as {login}"))
        .with("login", login.clone())
        .with(
            "url",
            string(user, "html_url").unwrap_or_else(|| format!("{}{login}", api.web())),
        );
    if let Some(name) = string(user, "name").filter(|n| !n.is_empty()) {
        output = output.with("name", clean(&name, 200));
    }
    if let Some(remaining) = reply.rate_remaining {
        output = output.with("rateLimitRemaining", saturating_i64(remaining));
    }
    Ok(output)
}

pub fn repository_info(
    _ctx: &mut Context<'_>,
    args: &Args<'_>,
    connection: &ConnectionInfo,
    t: &mut dyn Transport,
) -> ActionResult {
    let mut api = Api::connect(connection, t)?;
    let repo = required_repository(args)?;
    let what = format!("the repository {}", repo.full_name());
    let reply = api.get(
        api.endpoint(&["repos", &repo.owner, &repo.name], &[]),
        &what,
    )?;
    let json = &reply.json;
    let full_name = string(json, "full_name").ok_or_else(|| invalid_response(&what))?;
    let mut output = Output::message(format!("read {full_name}"))
        .with("fullName", full_name.clone())
        .with(
            "defaultBranch",
            string(json, "default_branch").unwrap_or_default(),
        )
        .with("private", boolean(json, "private"))
        .with("archived", boolean(json, "archived"))
        .with("openIssues", number(json, "open_issues_count"))
        .with("stars", number(json, "stargazers_count"))
        .with("forks", number(json, "forks_count"))
        .with(
            "url",
            string(json, "html_url").unwrap_or_else(|| format!("{}{full_name}", api.web())),
        );
    if let Some(description) = string(json, "description").filter(|d| !d.is_empty()) {
        output = output.with("description", clean(&description, 500));
    }
    if let Some(pushed) = string(json, "pushed_at") {
        output = output.with("pushedAt", pushed);
    }
    Ok(output)
}

/// The search for open pull requests.
fn search_query(repo: Option<&Repo>, filter: &str) -> String {
    let mut query = "is:pr is:open".to_owned();
    match repo {
        Some(repo) => {
            query.push_str(" repo:");
            query.push_str(&repo.full_name());
        }
        None => query.push_str(" archived:false"),
    }
    match filter {
        "author-me" => query.push_str(" author:@me"),
        "review-requested" => query.push_str(" review-requested:@me"),
        _ => {}
    }
    query
}

/// `owner/name#12 Title (@login)`, one line.
fn pull_request_line(item: &Value) -> String {
    let repository = item
        .get("repository_url")
        .and_then(Value::as_str)
        .and_then(|url| {
            let mut parts = url.rsplit('/');
            let name = parts.next()?;
            let owner = parts.next()?;
            Some(format!("{owner}/{name}"))
        })
        .unwrap_or_default();
    let number = number(item, "number");
    let title = clean(&string(item, "title").unwrap_or_default(), 200);
    let author = item
        .get("user")
        .and_then(|u| u.get("login"))
        .and_then(Value::as_str)
        .unwrap_or("?");
    let draft = if item.get("draft").and_then(Value::as_bool) == Some(true) {
        " [draft]"
    } else {
        ""
    };
    format!("{repository}#{number} {title} (@{author}){draft}")
}

pub fn pull_requests(
    _ctx: &mut Context<'_>,
    args: &Args<'_>,
    connection: &ConnectionInfo,
    t: &mut dyn Transport,
) -> ActionResult {
    list_pull_requests(args, connection, t, false)
}

pub fn review_requests(
    _ctx: &mut Context<'_>,
    args: &Args<'_>,
    connection: &ConnectionInfo,
    t: &mut dyn Transport,
) -> ActionResult {
    list_pull_requests(args, connection, t, true)
}

fn list_pull_requests(
    args: &Args<'_>,
    connection: &ConnectionInfo,
    t: &mut dyn Transport,
    review_requests: bool,
) -> ActionResult {
    let mut api = Api::connect(connection, t)?;
    let repo = repository(args)?;
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
    if filter == "all" && repo.is_none() {
        return Err(fail(
            codes::INVALID_REPOSITORY,
            "choose a repository to list all its open pull requests",
        ));
    }
    let limit = args
        .get::<Option<i64>>("limit")?
        .unwrap_or(20)
        .clamp(1, MAX_LISTED);
    let limit = usize::try_from(limit).unwrap_or(api::PER_PAGE);
    let query = search_query(repo.as_ref(), &filter);
    // Pages of at most 50: the first page is the built-in's one request
    // whenever `limit` is 50 or less, a second page (GitHub's `next`
    // link) lists the rest up to 100.
    let per_page = limit.min(api::PER_PAGE).to_string();
    let url = api.endpoint(
        &["search", "issues"],
        &[
            ("q", &query),
            ("sort", "updated"),
            ("order", "desc"),
            ("per_page", &per_page),
        ],
    );
    let what = match &repo {
        Some(repo) => format!("the pull requests of {}", repo.full_name()),
        None => "your pull requests".to_owned(),
    };
    let mut reply = api.get(url, &what)?;
    let total = reply
        .json
        .get("total_count")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid_response(&what))?;
    let mut items = reply
        .json
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_response(&what))?
        .clone();
    let mut pages = 1;
    while items.len() < limit
        && pages < limit.div_ceil(api::PER_PAGE)
        && let Some(next) = reply.next.take()
    {
        reply = api.get(next, &what)?;
        pages += 1;
        let more = reply
            .json
            .get("items")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid_response(&what))?;
        items.extend(more.iter().cloned());
    }
    items.truncate(limit);
    let lines: Vec<String> = items.iter().map(pull_request_line).collect();
    let (text, listed) = bounded_lines(&lines);
    let listed = listed as u64;
    let scope = repo
        .as_ref()
        .map_or_else(String::new, |r| format!(" in {}", r.full_name()));
    let message = if review_requests {
        format!("{total} pull requests wait for your review{scope}")
    } else {
        format!("{total} open pull requests{scope} ({filter})")
    };
    let mut output = Output::message(message)
        .with("count", saturating_i64(total))
        .with("listed", saturating_i64(listed))
        .with("items", text)
        .with("truncated", total > listed);
    if let Some(first) = items.first().and_then(|i| string(i, "html_url")) {
        output = output.with("firstUrl", first);
    }
    Ok(output)
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Counts {
    passed: u64,
    failed: u64,
    pending: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Passed,
    Failed,
    Pending,
}

/// A check run: pending until completed; success, neutral and skipped
/// pass; everything else fails.
fn check_run_outcome(run: &Value) -> Outcome {
    if run.get("status").and_then(Value::as_str) != Some("completed") {
        return Outcome::Pending;
    }
    match run.get("conclusion").and_then(Value::as_str) {
        Some("success" | "neutral" | "skipped") => Outcome::Passed,
        _ => Outcome::Failed,
    }
}

/// A commit status: success, pending, or failure and error.
fn status_outcome(status: &Value) -> Outcome {
    match status.get("state").and_then(Value::as_str) {
        Some("success") => Outcome::Passed,
        Some("pending") => Outcome::Pending,
        _ => Outcome::Failed,
    }
}

fn overall(counts: Counts) -> &'static str {
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

/// The commit to check: the ref, or a pull request's latest commit; how
/// it is named in messages; and the pull request's page.
fn checked_commit(
    api: &mut Api<'_>,
    args: &Args<'_>,
    repo: &Repo,
) -> Result<(String, String, Option<String>), Failure> {
    let full = repo.full_name();
    Ok(
        if let Some(number) = args.get::<Option<i64>>("pullRequest")? {
            let number = number.max(1).to_string();
            let what = format!("the pull request {full}#{number}");
            let url = api.endpoint(&["repos", &repo.owner, &repo.name, "pulls", &number], &[]);
            let reply = api.get(url, &what)?;
            let sha = reply
                .json
                .get("head")
                .and_then(|h| string(h, "sha"))
                .ok_or_else(|| invalid_response(&what))?;
            api::check_ref(&sha)?;
            (
                sha,
                format!("{full}#{number}"),
                string(&reply.json, "html_url"),
            )
        } else {
            let reference = optional(args, "ref")?.ok_or_else(|| {
                fail(
                    deckpilot_plugin_sdk::codes::INVALID_PARAMETERS,
                    "choose a branch, tag, commit or pull request",
                )
            })?;
            let reference = api::check_ref(&reference)?.to_owned();
            let shown = format!("{full}@{reference}");
            (reference, shown, None)
        },
    )
}

pub fn checks(
    _ctx: &mut Context<'_>,
    args: &Args<'_>,
    connection: &ConnectionInfo,
    t: &mut dyn Transport,
) -> ActionResult {
    let mut api = Api::connect(connection, t)?;
    let repo = required_repository(args)?;
    let full = repo.full_name();
    let (reference, shown, page) = checked_commit(&mut api, args, &repo)?;
    let what = format!("the checks of {shown}");
    let per_page = api::PER_PAGE.to_string();
    let url = api.endpoint(
        &[
            "repos",
            &repo.owner,
            &repo.name,
            "commits",
            &reference,
            "check-runs",
        ],
        &[("per_page", &per_page), ("filter", "latest")],
    );
    let runs = api.get_list(url, &what, Some("check_runs"))?;
    let url = api.endpoint(
        &[
            "repos",
            &repo.owner,
            &repo.name,
            "commits",
            &reference,
            "status",
        ],
        &[("per_page", &per_page)],
    );
    let statuses = api.get_list(url, &what, Some("statuses"))?;
    let mut counts = Counts::default();
    let mut failing = Vec::new();
    let named = runs
        .items
        .iter()
        .map(|r| (check_run_outcome(r), string(r, "name")))
        .chain(
            statuses
                .items
                .iter()
                .map(|s| (status_outcome(s), string(s, "context"))),
        );
    for (outcome, name) in named {
        match outcome {
            Outcome::Passed => counts.passed += 1,
            Outcome::Pending => counts.pending += 1,
            Outcome::Failed => {
                counts.failed += 1;
                failing.push(clean(&name.unwrap_or_default(), 120));
            }
        }
    }
    let truncated = runs.truncated
        || statuses.truncated
        || runs.total.is_some_and(|t| t > runs.items.len() as u64);
    let sha = string(&statuses.first, "sha").unwrap_or_else(|| reference.clone());
    let state = overall(counts);
    let total = counts.passed + counts.failed + counts.pending;
    let url = page.unwrap_or_else(|| format!("{}{full}/commit/{sha}", api.web()));
    let (mut failing_text, kept) = bounded_lines(&failing);
    if kept < failing.len() {
        let more = format!("… and {} more", failing.len() - kept);
        while failing_text.len() + more.len() + 1 > MAX_OUTPUT_TEXT {
            match failing_text.rfind('\n') {
                Some(at) => failing_text.truncate(at),
                None => failing_text.clear(),
            }
        }
        if !failing_text.is_empty() {
            failing_text.push('\n');
        }
        failing_text.push_str(&more);
    }
    Ok(Output::message(format!(
        "{shown}: {state} ({} passed, {} failed, {} pending)",
        counts.passed, counts.failed, counts.pending
    ))
    .with("state", state)
    .with("sha", sha)
    .with("total", saturating_i64(total))
    .with("passed", saturating_i64(counts.passed))
    .with("failed", saturating_i64(counts.failed))
    .with("pending", saturating_i64(counts.pending))
    .with("failing", failing_text)
    .with("truncated", truncated)
    .with("url", url))
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
        let repo = required_repository(args)?;
        let root = web.root();
        match args.get::<Option<i64>>("pullRequest")? {
            Some(number) => format!("{root}{}/pull/{}", repo.full_name(), number.max(1)),
            None => format!("{root}{}", repo.full_name()),
        }
    };
    let page = api::web_url(&target, &web)?;
    ctx.open_url(page.url)?;
    Ok(Output::message(format!("opened {}", page.shown)).with("url", page.shown))
}
