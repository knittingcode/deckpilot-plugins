//! The HTTP side every adapter shares: URLs under the connection's API
//! base, answers read as JSON, pages followed only inside the API, and
//! statuses mapped to the plugin's codes, the same for GitHub and GitLab
//! (the old plugins' `api.rs`, `fetch_failure` and `status_failure`).

use deckpilot_first_party_support::{Server, retry_after};
use serde_json::Value;

use crate::errors::{Failure, clean, codes, fail};
use crate::provider::{ConnectionInfo, ConnectionStatus, Deployment, ProviderKind};
use crate::transport::{HttpReply, HttpRequest, Method, Transport};

/// Most pages one list reads.
pub const MAX_PAGES: usize = 10;

/// The fixed API and web bases of the two clouds (the manifest's
/// `connectionTypes[].cloud`): a cloud connection has no other.
pub const CLOUD: [(ProviderKind, &str, &str); 2] = [
    (
        ProviderKind::Github,
        "https://api.github.com",
        "https://github.com",
    ),
    (
        ProviderKind::Gitlab,
        "https://gitlab.com/api/v4",
        "https://gitlab.com",
    ),
];

/// One answer read as JSON.
#[derive(Debug, Clone)]
pub struct Reply {
    pub json: Value,
    /// `link: rel="next"`, checked to be inside the API.
    pub next: Option<String>,
    /// `x-total` (GitLab).
    pub total: Option<u64>,
    pub raw: HttpReply,
}

/// The items of a paged list.
#[derive(Debug, Clone, Default)]
pub struct List {
    pub items: Vec<Value>,
    pub more: bool,
}

/// A connection's API and web addresses, for one provider.
#[derive(Debug, Clone)]
pub struct Api {
    pub kind: ProviderKind,
    pub api: Server,
    pub web: Server,
    /// `host[:port]`, as messages name the server.
    pub host: String,
}

impl Api {
    /// The addresses `connections.get` gave: `https`, without a user name,
    /// query or fragment (the host normalized them). Every URL the plugin
    /// builds or accepts derives from these two. Checked once more here,
    /// as the host checks them when the connection is made (design §4.1
    /// rules 5 and 6): a cloud connection has exactly the cloud's fixed
    /// bases, and any other one has its web pages on the API's own host
    /// and port, so a page is never opened on another server.
    pub fn new(connection: &ConnectionInfo) -> Result<Self, Failure> {
        let kind = connection.kind()?;
        let invalid = || {
            fail(
                codes::CONNECTION_MISSING,
                "the connection's address is not usable; edit the connection",
            )
        };
        let api = Server::parse(&connection.api_base).ok_or_else(invalid)?;
        let web = Server::parse(&connection.web_base).ok_or_else(invalid)?;
        let consistent = match connection.deployment {
            Deployment::Cloud => CLOUD
                .iter()
                .any(|(k, a, w)| *k == kind && api.base() == *a && web.base() == *w),
            Deployment::SelfHosted | Deployment::Other => api.origin() == web.origin(),
        };
        if !consistent {
            return Err(invalid());
        }
        Ok(Self {
            kind,
            host: web.host().to_owned(),
            api,
            web,
        })
    }

    pub fn endpoint(&self, segments: &[&str], query: &[(&str, &str)]) -> String {
        self.api.endpoint(segments, query)
    }

    /// `name (host)`, as messages name the server.
    pub fn named(&self) -> String {
        format!("{} ({})", self.kind.name(), self.host)
    }

    /// A page on the web origin: `https`, the same host and port, under
    /// the web base; `None` for anything else.
    pub fn page(&self, text: &str) -> Option<String> {
        if has_ambiguous_separator(text) {
            return None;
        }
        self.web.page(text).map(|page| page.url)
    }

    /// A URL from the server's answer, kept only if it is a page of this
    /// server (a hostile answer cannot make DeckPilot show or open another
    /// site).
    pub fn web_link(&self, value: Option<&str>) -> Option<String> {
        value.and_then(|v| self.page(v))
    }

    pub fn send(
        &self,
        t: &mut dyn Transport,
        request: &HttpRequest,
        what: &str,
    ) -> Result<Reply, Failure> {
        let raw = t.send(request).map_err(|e| {
            let (failure, status) = e.into_failure(&self.named());
            if let Some(status) = status {
                t.report(status, Some(failure.code));
            }
            failure
        })?;
        if !raw.is_success() {
            let (failure, status) = self.status_failure(&raw, what);
            if let Some(status) = status {
                t.report(status, Some(failure.code));
            }
            return Err(failure);
        }
        let json = if raw.body.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&raw.body).map_err(|_| {
                fail(
                    codes::INVALID_RESPONSE,
                    format!("{}'s answer for {what} is not JSON", self.kind.name()),
                )
            })?
        };
        let next = match raw.link("next") {
            Some(link)
                if self.api.contains(&link)
                    && !has_dot_segment(&link)
                    && !has_ambiguous_separator(&link) =>
            {
                Some(link)
            }
            Some(_) => {
                return Err(fail(
                    codes::INVALID_RESPONSE,
                    format!(
                        "{} pointed to a next page outside its API",
                        self.kind.name()
                    ),
                ));
            }
            None => None,
        };
        Ok(Reply {
            json,
            next,
            total: raw.number("x-total"),
            raw,
        })
    }

    pub fn get(&self, t: &mut dyn Transport, url: String, what: &str) -> Result<Reply, Failure> {
        self.send(t, &HttpRequest::get(url), what)
    }

    /// Like [`Api::get`], but a 404 is `None`.
    pub fn get_optional(
        &self,
        t: &mut dyn Transport,
        url: String,
        what: &str,
    ) -> Result<Option<Reply>, Failure> {
        match self.get(t, url, what) {
            Ok(reply) => Ok(Some(reply)),
            Err(failure) if failure.code == codes::NOT_FOUND => Ok(None),
            Err(failure) => Err(failure),
        }
    }

    pub fn send_json(
        &self,
        t: &mut dyn Transport,
        method: Method,
        url: String,
        body: &Value,
        what: &str,
    ) -> Result<Reply, Failure> {
        self.send(t, &HttpRequest::json(method, url, body), what)
    }

    /// At most `limit` items of a list whose pages are arrays (or, with
    /// `field`, objects with the array in `field`), following `link:
    /// rel="next"` inside the API, at most [`MAX_PAGES`] pages.
    pub fn list(
        &self,
        t: &mut dyn Transport,
        first: String,
        field: Option<&str>,
        what: &str,
        limit: usize,
    ) -> Result<List, Failure> {
        let mut list = List::default();
        let mut url = first;
        for page in 1..=MAX_PAGES {
            let reply = self.get(t, url, what)?;
            let items = match field {
                Some(field) => reply.json.get(field),
                None => Some(&reply.json),
            }
            .and_then(Value::as_array)
            .ok_or_else(|| {
                fail(
                    codes::INVALID_RESPONSE,
                    format!("{}'s answer for {what} has no list", self.kind.name()),
                )
            })?;
            list.items.extend(items.iter().cloned());
            match reply.next {
                Some(next) if list.items.len() < limit && page < MAX_PAGES => url = next,
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

    /// The failure of a non-2xx answer, and the connection status it means.
    fn status_failure(&self, reply: &HttpReply, what: &str) -> (Failure, Option<ConnectionStatus>) {
        let name = self.kind.name();
        let detail = serde_json::from_str::<Value>(&reply.body)
            .ok()
            .and_then(|v| {
                ["message", "error_description", "error"]
                    .iter()
                    .find_map(|key| v.get(*key).and_then(Value::as_str).map(|m| clean(m, 200)))
            })
            .filter(|m| !m.is_empty())
            .map(|m| format!(": {m}"))
            .unwrap_or_default();
        let status = reply.status;
        let exhausted = reply.number("x-ratelimit-remaining") == Some(0)
            || reply.number("ratelimit-remaining") == Some(0)
            || reply.header("retry-after").is_some();
        if status == 429 || (status == 403 && exhausted) {
            let now = reply
                .header("date")
                .and_then(deckpilot_first_party_support::http_date);
            let wait = reply
                .header("retry-after")
                .and_then(|v| retry_after(v, now))
                .or_else(|| {
                    let reset = reply
                        .number("x-ratelimit-reset")
                        .or_else(|| reply.number("ratelimit-reset"))?;
                    Some(reset.saturating_sub(now?))
                })
                .unwrap_or(60);
            return (
                fail(
                    codes::RATE_LIMITED,
                    format!("{name}'s rate limit is reached; try again in {wait} s"),
                )
                .retryable(),
                None,
            );
        }
        match status {
            // The host never follows a redirect of a connection request and
            // refuses one off the connection (`redirect-refused`); should
            // one reach the plugin, it is refused the same way and never
            // followed or retried anywhere else.
            300..=399 => (
                fail(
                    codes::REDIRECT_REFUSED,
                    format!(
                        "{name} redirected the request for {what} ({status}), which is refused"
                    ),
                ),
                None,
            ),
            401 => (
                fail(
                    codes::UNAUTHORIZED,
                    format!(
                        "{} refused the connection's token (401); it may be expired or revoked: \
                         reconnect it in Git Providers",
                        self.named()
                    ),
                ),
                Some(ConnectionStatus::Unauthorized),
            ),
            // A token without a scope (GitLab `insufficient_scope`, GitHub's
            // fine-grained "not accessible by personal access token"): the
            // connection's problem. Any other 403 is this project's
            // permission and leaves the connection alone.
            403 if token_scope_refusal(&reply.body) => (
                fail(
                    codes::INSUFFICIENT_SCOPE,
                    format!(
                        "the connection's token may not access {what} (403){detail}; give it the \
                         missing permission"
                    ),
                ),
                Some(ConnectionStatus::InsufficientScope),
            ),
            403 => (
                fail(
                    codes::FORBIDDEN,
                    format!("you may not access {what} (403){detail}"),
                ),
                None,
            ),
            404 => (
                fail(
                    codes::NOT_FOUND,
                    format!("{what} was not found, or the token cannot see it (404)"),
                ),
                None,
            ),
            400 | 409 | 422 => (
                fail(
                    codes::REJECTED,
                    format!("{name} refused {what} ({status}){detail}"),
                ),
                None,
            ),
            500..=599 => (
                fail(
                    codes::PROVIDER_ERROR,
                    format!("{name} answered {status} for {what}{detail}"),
                )
                .retryable(),
                None,
            ),
            _ => (
                fail(
                    codes::PROVIDER_ERROR,
                    format!("{name} answered {status} for {what}{detail}"),
                ),
                None,
            ),
        }
    }
}

/// Whether a 403's body says the token lacks a scope or permission, as
/// opposed to the user lacking access to one project.
fn token_scope_refusal(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    lower.contains("insufficient_scope")
        || lower.contains("not accessible by personal access token")
        || lower.contains("not accessible by integration")
}

/// Whether a next-page link has a dot segment, raw or encoded (defence in
/// depth: the API base check comes first).
fn has_dot_segment(link: &str) -> bool {
    let path = link.split(['?', '#']).next().unwrap_or_default();
    // A WHATWG parser takes `\` for `/`: both separate segments here.
    path.split(['/', '\\']).any(|segment| {
        let s = segment.to_ascii_lowercase().replace("%2e", ".");
        s == "." || s == ".."
    })
}

/// Whether a link's path has a separator that a URL parser may read
/// otherwise than the prefix check did (threat T1): a backslash, which a
/// WHATWG parser takes for `/`, or an encoded slash or backslash (`%2f`,
/// `%5c`, any case), which a server may decode into one. Such a link could
/// leave the connection's path prefix, so it is refused, never followed
/// or opened.
pub fn has_ambiguous_separator(link: &str) -> bool {
    let path = link.split(['?', '#']).next().unwrap_or_default();
    let lower = path.to_ascii_lowercase();
    path.contains('\\') || lower.contains("%2f") || lower.contains("%5c")
}

/// A branch, tag or commit name (git's rules, as the old plugins check).
pub fn check_ref(text: &str) -> Result<&str, Failure> {
    let text = text.trim();
    let valid = (1..=250).contains(&text.len())
        && !text.starts_with(['/', '-', '.'])
        && !text.ends_with(['/', '.'])
        && !text.ends_with(".lock")
        && !text.contains("..")
        && !text.contains("//")
        && !text.contains("@{")
        && !text
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || "~^:?*[\\".contains(c));
    if valid {
        Ok(text)
    } else {
        Err(fail(
            codes::INVALID_REF,
            format!("“{}” is not a branch, tag or commit name", clean(text, 60)),
        ))
    }
}

pub fn string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

pub fn text(value: &Value, key: &str, max: usize) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(|s| clean(s, max))
        .filter(|s| !s.is_empty())
}

pub fn number(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64)
}

pub fn boolean(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}
