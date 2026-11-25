//! The legacy GitHub and GitLab actions (DeckPilot `design-git-providers.md`
//! §11.3, phase 2 of the migration): `legacy-github-*` and
//! `legacy-gitlab-*`, one per action of the old `deckpilot.github` 0.2.1
//! and `deckpilot.gitlab` 0.1.2 plugins, with **exactly** their
//! parameters, outputs, error codes and node specs
//! (`plugins/first-party/contracts/deckpilot.{github,gitlab}/manifest.json`
//! at the DeckPilot pin), plus the `connection` that the host injects
//! (`ParamRule::InjectConnection`).
//!
//! They are `"deprecated": true`, so the Action Library never offers
//! them: they exist only as targets of the old action ids once the user
//! switches the old plugin's keys to Git Providers. Their code is the old
//! plugins' own (`official/github/src`, `official/gitlab/src`), with two
//! changes and no others:
//!
//! - the server is the connection's (`connections.get`'s API and web
//!   bases) instead of the old `apiUrl` setting, and every request goes
//!   through the connection's transport (`http.fetch` with `connection`)
//!   instead of the old credential slot: the plugin never sees a token,
//!   and the host copies the old one into the connection (DeckPilot PR-10);
//! - the time limit is the old default (20 s, counted with the server's
//!   `Date`), as Git Providers has no `timeoutSeconds` setting.
//!
//! Read-only: `GET` only, whatever the connection allows. The old plugins
//! stay as they are.

pub mod github;
pub mod gitlab;
#[cfg(test)]
mod parity_github;
#[cfg(test)]
mod parity_gitlab;
#[cfg(test)]
mod tests;

use deckpilot_plugin_sdk::prelude::*;
use deckpilot_plugin_sdk::{Header, Response, codes as shared};

use crate::transport::{HttpRequest, Method, Transport, TransportError};

/// The old plugins' default `timeoutSeconds`, now fixed.
pub const TIMEOUT_SECONDS: u64 = 20;

/// The legacy actions: `(id, old plugin, old action)`.
pub const ACTIONS: [(&str, &str, &str); 12] = [
    (
        "legacy-github-current-user",
        "deckpilot.github",
        "current-user",
    ),
    ("legacy-github-repository", "deckpilot.github", "repository"),
    (
        "legacy-github-pull-requests",
        "deckpilot.github",
        "pull-requests",
    ),
    (
        "legacy-github-review-requests",
        "deckpilot.github",
        "review-requests",
    ),
    ("legacy-github-checks", "deckpilot.github", "checks"),
    ("legacy-github-open", "deckpilot.github", "open"),
    (
        "legacy-gitlab-current-user",
        "deckpilot.gitlab",
        "current-user",
    ),
    ("legacy-gitlab-project", "deckpilot.gitlab", "project"),
    (
        "legacy-gitlab-merge-requests",
        "deckpilot.gitlab",
        "merge-requests",
    ),
    (
        "legacy-gitlab-review-requests",
        "deckpilot.gitlab",
        "review-requests",
    ),
    ("legacy-gitlab-pipeline", "deckpilot.gitlab", "pipeline"),
    ("legacy-gitlab-open", "deckpilot.gitlab", "open"),
];

/// The error codes of the old plugins that Git Providers did not have.
pub const CODES: [&str; 5] = [
    "invalid-api-url",
    "invalid-project",
    "github-error",
    "gitlab-error",
    "timed-out",
];

/// Why a request got no answer, as the old plugins reported it: what the
/// SDK made of the agent's refusal (`PluginError::from(CapabilityError)`),
/// with the network kind's code and retryability, or `request-failed`.
/// The connection's own refusals keep Git Providers' codes (an old plugin
/// had no connection).
#[derive(Debug)]
pub enum Refused {
    /// The answer is larger than the host hands to a plugin.
    TooLarge,
    Failed(PluginError),
}

impl From<PluginError> for Refused {
    fn from(error: PluginError) -> Self {
        Self::Failed(error)
    }
}

pub fn refusal(error: TransportError, server: &str) -> Refused {
    let failed = |code: &str, message: String| PluginError::new(code, message);
    Refused::Failed(match error {
        TransportError::TooLarge => return Refused::TooLarge,
        TransportError::Network(kind) => {
            let message = format!("the request to {server} failed ({kind})");
            match kind.as_str() {
                "dns-failed" | "connection-failed" | "timed-out" => {
                    failed(&kind, message).retryable()
                }
                "tls-failed" | "too-many-redirects" | "insecure-redirect" | "blocked-address" => {
                    failed(&kind, message)
                }
                _ => failed(shared::REQUEST_FAILED, message).retryable(),
            }
        }
        TransportError::Denied(reason) => failed(
            shared::NOT_PERMITTED,
            format!(
                "DeckPilot refused the request to {server}: {}",
                crate::errors::clean(&reason, 200)
            ),
        ),
        TransportError::Throttled => failed(
            shared::THROTTLED,
            "too many requests; try again shortly".into(),
        )
        .retryable(),
        TransportError::Unsupported => failed(
            shared::UNSUPPORTED,
            "this version of DeckPilot cannot use connections yet".into(),
        ),
        TransportError::Other(reason) => failed(
            shared::REQUEST_FAILED,
            format!(
                "the request to {server} failed: {}",
                crate::errors::clean(&reason, 200)
            ),
        )
        .retryable(),
        // The connection's refusals, and a cancelled run: Git Providers'
        // own failures.
        other => other.into_failure(server).0.into(),
    })
}

/// `GET url` on the connection with `headers`, as the old plugins'
/// `ctx.fetch`: the answer as the SDK's [`Response`], so the old code
/// reads it unchanged. Never a credential: the host adds the
/// connection's secret.
pub fn get(
    t: &mut dyn Transport,
    url: String,
    headers: &[(&str, &str)],
) -> Result<Response, TransportError> {
    let request = HttpRequest {
        method: Method::Get,
        url,
        headers: headers
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect(),
        body: None,
    };
    let reply = t.send(&request)?;
    Ok(Response {
        status: reply.status,
        headers: reply
            .headers
            .into_iter()
            .map(|(name, value)| Header { name, value })
            .collect(),
        body: Some(reply.body),
    })
}

/// A legacy action's handler: the old action's code, on the connection.
pub type Handler = fn(
    &mut Context<'_>,
    &Args<'_>,
    &crate::provider::ConnectionInfo,
    &mut dyn Transport,
) -> ActionResult;
