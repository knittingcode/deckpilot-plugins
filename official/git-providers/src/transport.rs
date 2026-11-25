//! How the adapters reach a provider: an HTTP request on the invocation's
//! bound connection, sent by the host (`http.fetch` with `connection`,
//! plugin API 1.14). The adapters never see a secret, an auth header or a
//! connection id: the [`Transport`] is bound to one connection, and the
//! host adds its secret and checks its origin, path prefix and methods.
//!
//! [`crate::host::HostTransport`] is the transport of the wasm module; the
//! unit tests use [`mock::MockTransport`], so no test ever talks to a real
//! GitHub, GitLab or company server.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::errors::{Failure, clean, codes, fail};
use crate::provider::ConnectionStatus;

/// The methods a provider operation uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Method {
    Get,
    Post,
    Put,
    /// A method the plugin never sends (`HEAD`, `PATCH`, `DELETE`).
    #[serde(other)]
    Other,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Other => "OTHER",
        }
    }
}

/// One request. Headers are only content negotiation: the host refuses
/// `Authorization` and the rest of its forbidden headers anyway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

impl HttpRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: Method::Get,
            url: url.into(),
            headers: vec![("Accept".into(), "application/json".into())],
            body: None,
        }
    }

    /// A JSON body (`Content-Type: application/json`).
    pub fn json(method: Method, url: impl Into<String>, body: &Value) -> Self {
        Self {
            method,
            url: url.into(),
            headers: vec![
                ("Accept".into(), "application/json".into()),
                ("Content-Type".into(), "application/json".into()),
            ],
            body: Some(body.to_string()),
        }
    }

    #[must_use]
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

/// An answer, any status.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HttpReply {
    pub status: u16,
    /// Lower-case names, as the host hands them over (its allowlist).
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl HttpReply {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.trim())
    }

    pub fn number(&self, name: &str) -> Option<u64> {
        self.header(name).and_then(|v| v.parse().ok())
    }

    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// The target of `link: <…>; rel="next"` (RFC 8288, as GitHub and
    /// GitLab send it).
    pub fn link(&self, rel: &str) -> Option<String> {
        let value = self.header("link")?;
        link_entries(value).into_iter().find_map(|entry| {
            // `<target>; params`: the target may itself hold `;`.
            let (target, params) = entry.trim().strip_prefix('<')?.split_once('>')?;
            let params = params.trim_start().strip_prefix(';')?;
            params
                .split(';')
                .filter_map(|p| p.trim().split_once('='))
                .any(|(k, v)| {
                    k.trim().eq_ignore_ascii_case("rel")
                        && v.trim().trim_matches('"').split(' ').any(|r| r == rel)
                })
                .then(|| target.to_owned())
        })
    }

    /// Whether the `link` header names a `rel` relation at all, readable
    /// or not: a `next` that [`HttpReply::link`] cannot read is then an
    /// incomplete answer, never the end of a list.
    pub fn mentions_link(&self, rel: &str) -> bool {
        self.header("link").is_some_and(|value| {
            let lower = value.to_ascii_lowercase();
            let rel = rel.to_ascii_lowercase();
            lower.contains(&format!("rel=\"{rel}\""))
                || lower.contains(&format!("rel={rel}"))
                || lower.contains(&format!(" {rel}\""))
                || lower.contains(&format!("\"{rel} "))
        })
    }
}

/// The entries of a `link` header (RFC 8288), split at the commas that
/// separate them: never one inside `<…>` or a quoted parameter value.
pub fn link_entries(value: &str) -> Vec<&str> {
    let mut entries = Vec::new();
    let (mut start, mut in_target, mut in_quotes, mut escaped) = (0, false, false, false);
    for (i, c) in value.char_indices() {
        if in_quotes {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_quotes = false,
                _ => {}
            }
            continue;
        }
        match c {
            '<' if !in_target => in_target = true,
            '>' if in_target => in_target = false,
            '"' if !in_target => in_quotes = true,
            ',' if !in_target => {
                entries.push(&value[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    entries.push(&value[start..]);
    entries
}

/// Why a request got no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// The host refused it: not granted, not bound, a URL outside the
    /// connection, a method the connection is not allowed.
    Denied(String),
    /// The connection is gone or turned off.
    ConnectionMissing,
    ConnectionDisabled,
    /// A network failure, with the transport's code (`dns-failed`,
    /// `connection-failed`, `timed-out`, `tls-failed`, `blocked-address`…).
    Network(String),
    /// A redirect off the connection's origin or prefix.
    RedirectRefused,
    /// The answer is larger than the host hands to a plugin.
    TooLarge,
    /// The host does not carry the operation out (an agent without
    /// plugin API 1.14).
    Unsupported,
    /// The run was cancelled.
    Cancelled,
    /// The rate limit of the host.
    Throttled,
    Other(String),
}

impl TransportError {
    /// The failure an action reports, and the status the connection gets.
    pub fn into_failure(self, provider: &str) -> (Failure, Option<ConnectionStatus>) {
        match self {
            Self::Denied(reason) => (
                fail(
                    codes::URL_NOT_ALLOWED,
                    format!(
                        "DeckPilot refused the request to {provider}: {}",
                        clean(&reason, 200)
                    ),
                ),
                None,
            ),
            Self::ConnectionMissing => (
                fail(
                    codes::CONNECTION_MISSING,
                    "the connection is not on this computer any more; choose another one",
                ),
                None,
            ),
            Self::ConnectionDisabled => (
                fail(
                    codes::CONNECTION_DISABLED,
                    "the connection is turned off; turn it on in Git Providers",
                ),
                None,
            ),
            Self::Network(code) if code == "tls-failed" => (
                fail(
                    codes::TLS_FAILED,
                    format!(
                        "the certificate of {provider} is not trusted by this computer; add your \
                         company's certificate authority to the system"
                    ),
                ),
                Some(ConnectionStatus::TlsFailed),
            ),
            Self::Network(code) => {
                let retryable = matches!(
                    code.as_str(),
                    "dns-failed" | "connection-failed" | "timed-out"
                );
                let failure = fail(
                    codes::UNREACHABLE,
                    format!(
                        "can't reach {provider} ({}); check the network or VPN",
                        clean(&code, 40)
                    ),
                );
                (
                    if retryable {
                        failure.retryable()
                    } else {
                        failure
                    },
                    Some(ConnectionStatus::Unreachable),
                )
            }
            Self::RedirectRefused => (
                fail(
                    codes::REDIRECT_REFUSED,
                    format!("{provider} redirected the request somewhere else, which is refused"),
                ),
                None,
            ),
            Self::TooLarge => (
                fail(
                    codes::INVALID_RESPONSE,
                    format!("{provider}'s answer is too large"),
                ),
                None,
            ),
            Self::Unsupported => (
                fail(
                    codes::UNSUPPORTED,
                    "this version of DeckPilot cannot use connections yet",
                ),
                None,
            ),
            Self::Cancelled => (fail(codes::CANCELLED, "cancelled"), None),
            Self::Throttled => (
                fail(codes::RATE_LIMITED, "too many requests; try again shortly").retryable(),
                None,
            ),
            Self::Other(reason) => (
                fail(
                    codes::PROVIDER_ERROR,
                    format!("the request to {provider} failed: {}", clean(&reason, 200)),
                ),
                None,
            ),
        }
    }
}

/// One connection's way to the provider, for one invocation.
pub trait Transport {
    fn send(&mut self, request: &HttpRequest) -> Result<HttpReply, TransportError>;

    /// Tells the host what a request found about the connection. Best
    /// effort: a failure to report changes nothing for the action.
    fn report(&mut self, status: ConnectionStatus, detail: Option<&str>);

    /// Waits (at most 30 s per call); cancelling the run ends it with
    /// [`TransportError::Cancelled`].
    fn sleep(&mut self, ms: u32) -> Result<(), TransportError>;

    /// The agent's clock (Unix ms, `time.now`), to count a time budget
    /// that includes the requests; `None` when it cannot be read.
    fn now_ms(&mut self) -> Option<u64>;
}

/// A boxed transport is a transport (the actions hold the invocation's
/// one as `Box<dyn Transport>`).
impl<T: Transport + ?Sized> Transport for Box<T> {
    fn send(&mut self, request: &HttpRequest) -> Result<HttpReply, TransportError> {
        (**self).send(request)
    }

    fn report(&mut self, status: ConnectionStatus, detail: Option<&str>) {
        (**self).report(status, detail);
    }

    fn sleep(&mut self, ms: u32) -> Result<(), TransportError> {
        (**self).sleep(ms)
    }

    fn now_ms(&mut self) -> Option<u64> {
        (**self).now_ms()
    }
}

/// A scripted transport for unit tests: routes by method and URL, records
/// every request. Never a network.
pub mod mock {
    use std::collections::VecDeque;

    use super::{HttpReply, HttpRequest, Method, Transport, TransportError};
    use crate::provider::ConnectionStatus;

    type Route = (Method, String, VecDeque<Result<HttpReply, TransportError>>);

    /// Headers and query members that carry a credential. The host adds
    /// the connection's secret; the guest never builds one, and every test
    /// that sends a request through the mock checks it.
    pub const CREDENTIAL_HEADERS: [&str; 8] = [
        "authorization",
        "proxy-authorization",
        "private-token",
        "job-token",
        "deploy-token",
        "cookie",
        "x-api-key",
        "x-gitlab-token",
    ];
    pub const CREDENTIAL_QUERY: [&str; 4] =
        ["private_token=", "access_token=", "job_token=", "token="];

    /// Panics when `request` carries a credential the guest built.
    pub fn assert_no_credential(request: &HttpRequest) {
        for (name, _) in &request.headers {
            assert!(
                !CREDENTIAL_HEADERS.contains(&name.to_ascii_lowercase().as_str()),
                "the plugin built a credential header `{name}` for {}",
                request.url
            );
        }
        let query = request
            .url
            .split_once('?')
            .map(|(_, q)| q.to_ascii_lowercase())
            .unwrap_or_default();
        for member in query.split('&') {
            assert!(
                !CREDENTIAL_QUERY.iter().any(|c| member.starts_with(c)),
                "the plugin put a credential in the URL {}",
                request.url
            );
        }
    }

    #[derive(Debug, Default)]
    pub struct MockTransport {
        routes: Vec<Route>,
        pub requests: Vec<HttpRequest>,
        pub reports: Vec<(ConnectionStatus, Option<String>)>,
        pub slept: Vec<u32>,
        /// `sleep` answers this once it is set (a cancelled run).
        pub cancel_sleep_after: Option<usize>,
        /// A fake clock (Unix ms): a sleep advances it by its length, a
        /// request by [`MockTransport::request_ms`].
        pub clock: u64,
        pub request_ms: u64,
        /// `now_ms` answers `None` (an agent whose clock cannot be read).
        pub no_clock: bool,
        /// Answers every request no route names, any method (contract
        /// tests that only watch what is sent).
        pub fallback: Option<HttpReply>,
    }

    impl MockTransport {
        pub fn new() -> Self {
            Self::default()
        }

        /// Answers `method url` (the whole URL, query included) with
        /// `status` and `body`; repeated calls queue answers, the last one
        /// stays.
        #[must_use]
        pub fn on(self, method: Method, url: &str, status: u16, body: &str) -> Self {
            self.reply(
                method,
                url,
                Ok(HttpReply {
                    status,
                    headers: Vec::new(),
                    body: body.into(),
                }),
            )
        }

        /// [`MockTransport::on`] with headers.
        #[must_use]
        pub fn on_with(
            self,
            method: Method,
            url: &str,
            status: u16,
            headers: &[(&str, &str)],
            body: &str,
        ) -> Self {
            self.reply(
                method,
                url,
                Ok(HttpReply {
                    status,
                    headers: headers
                        .iter()
                        .map(|(n, v)| ((*n).to_ascii_lowercase(), (*v).to_owned()))
                        .collect(),
                    body: body.into(),
                }),
            )
        }

        #[must_use]
        pub fn reply(
            mut self,
            method: Method,
            url: &str,
            answer: Result<HttpReply, TransportError>,
        ) -> Self {
            if let Some(route) = self
                .routes
                .iter_mut()
                .find(|(m, u, _)| *m == method && u == url)
            {
                route.2.push_back(answer);
            } else {
                self.routes
                    .push((method, url.to_owned(), VecDeque::from([answer])));
            }
            self
        }

        /// The URLs requested, in order.
        pub fn urls(&self) -> Vec<String> {
            self.requests.iter().map(|r| r.url.clone()).collect()
        }
    }

    impl Transport for MockTransport {
        fn send(&mut self, request: &HttpRequest) -> Result<HttpReply, TransportError> {
            assert_no_credential(request);
            self.requests.push(request.clone());
            self.clock += self.request_ms;
            let route = self
                .routes
                .iter_mut()
                .find(|(m, u, _)| *m == request.method && *u == request.url);
            match route {
                Some((_, _, answers)) if answers.len() > 1 => answers
                    .pop_front()
                    .unwrap_or(Err(TransportError::Unsupported)),
                Some((_, _, answers)) => answers
                    .front()
                    .cloned()
                    .unwrap_or(Err(TransportError::Unsupported)),
                None if self.fallback.is_some() => Ok(self.fallback.clone().unwrap_or_default()),
                None => Err(TransportError::Denied(format!(
                    "no mock route for {} {}",
                    request.method.as_str(),
                    request.url
                ))),
            }
        }

        fn report(&mut self, status: ConnectionStatus, detail: Option<&str>) {
            self.reports.push((status, detail.map(str::to_owned)));
        }

        fn sleep(&mut self, ms: u32) -> Result<(), TransportError> {
            self.slept.push(ms);
            self.clock += u64::from(ms);
            match self.cancel_sleep_after {
                Some(n) if self.slept.len() > n => Err(TransportError::Cancelled),
                _ => Ok(()),
            }
        }

        fn now_ms(&mut self) -> Option<u64> {
            (!self.no_clock).then_some(self.clock)
        }
    }
}
