//! The plugin's failures: a stable code, a message for the user and
//! whether trying again may help. Codes are declared in `manifest.json`
//! (`contributes.errorCodes`); the shared codes of the SDK are used where
//! they fit. A message never carries a token, a header or a URL with a
//! query: only what failed and what to do.

use std::fmt;

/// Stable error codes (design §9.3 "Errors", §4).
pub mod codes {
    pub const UNAUTHORIZED: &str = "unauthorized";
    pub const INSUFFICIENT_SCOPE: &str = "insufficient-scope";
    pub const FORBIDDEN: &str = "forbidden";
    pub const NOT_FOUND: &str = "not-found";
    pub const RATE_LIMITED: &str = "rate-limited";
    pub const PROVIDER_ERROR: &str = "provider-error";
    pub const REJECTED: &str = "rejected";
    pub const INVALID_RESPONSE: &str = "invalid-response";
    pub const INVALID_REPOSITORY: &str = "invalid-repository";
    pub const INVALID_REF: &str = "invalid-ref";
    pub const INVALID_VARIABLES: &str = "invalid-variables";
    pub const UNREACHABLE: &str = "unreachable";
    pub const REDIRECT_REFUSED: &str = "redirect-refused";
    pub const CONNECTION_MISSING: &str = "connection-missing";
    pub const CONNECTION_DISABLED: &str = "connection-disabled";
    pub const READ_ONLY_CONNECTION: &str = "read-only-connection";
    pub const CAPABILITY_UNSUPPORTED: &str = "capability-unsupported";
    pub const NEEDS_CONFIRMATION: &str = "needs-confirmation";
    pub const URL_NOT_ALLOWED: &str = "url-not-allowed";
    pub const TIMED_OUT: &str = "timed-out";
    /// The old GitHub and GitLab plugins' own codes, which only the
    /// legacy actions return (`legacy`).
    pub const INVALID_API_URL: &str = "invalid-api-url";
    pub const INVALID_PROJECT: &str = "invalid-project";
    pub const GITHUB_ERROR: &str = "github-error";
    pub const GITLAB_ERROR: &str = "gitlab-error";
    /// Shared codes of the SDK (`deckpilot_plugin_sdk::codes`), declared
    /// nowhere: every plugin may return them.
    pub const INVALID_PARAMETERS: &str = "invalid-parameters";
    pub const UNSUPPORTED: &str = "unsupported";
    pub const TLS_FAILED: &str = "tls-failed";
    pub const CANCELLED: &str = "cancelled";

    /// The codes this plugin declares in `manifest.json`.
    pub const DECLARED: &[&str] = &[
        UNAUTHORIZED,
        INSUFFICIENT_SCOPE,
        FORBIDDEN,
        NOT_FOUND,
        RATE_LIMITED,
        PROVIDER_ERROR,
        REJECTED,
        INVALID_RESPONSE,
        INVALID_REPOSITORY,
        INVALID_REF,
        INVALID_VARIABLES,
        UNREACHABLE,
        REDIRECT_REFUSED,
        CONNECTION_MISSING,
        CONNECTION_DISABLED,
        READ_ONLY_CONNECTION,
        CAPABILITY_UNSUPPORTED,
        NEEDS_CONFIRMATION,
        URL_NOT_ALLOWED,
        TIMED_OUT,
        CANCELLED,
        INVALID_API_URL,
        INVALID_PROJECT,
        GITHUB_ERROR,
        GITLAB_ERROR,
    ];
}

/// A failed operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
}

impl Failure {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: false,
        }
    }

    #[must_use]
    pub fn retryable(mut self) -> Self {
        self.retryable = true;
        self
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for Failure {}

/// Whether a value a user typed into a pipeline variable or workflow input
/// looks like a secret (a token of a known shape, a private key): such
/// values are refused, as the `noSecrets` rule of `pairs` parameters does,
/// so a key or a workflow never carries one to CI logs.
pub fn looks_like_secret(value: &str) -> bool {
    const PREFIXES: [&str; 11] = [
        "ghp_",
        "gho_",
        "ghu_",
        "ghs_",
        "ghr_",
        "github_pat_",
        "glpat-",
        "gloas-",
        "glptt-",
        "glrt-",
        "xoxb-",
    ];
    let v = value.trim();
    PREFIXES.iter().any(|p| v.starts_with(p))
        || v.contains("-----BEGIN")
        || (v.len() == 20 && v.starts_with("AKIA") && v.bytes().all(|b| b.is_ascii_alphanumeric()))
}

pub fn fail(code: &'static str, message: impl Into<String>) -> Failure {
    Failure::new(code, message)
}

/// A text from a server, safe to show: control characters replaced, at
/// most `max` characters (the support crate's rule).
pub fn clean(text: &str, max: usize) -> String {
    deckpilot_first_party_support::clean(text, max)
}
