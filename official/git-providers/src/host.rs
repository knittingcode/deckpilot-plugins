//! The connection operations of plugin API 1.14 (`connections.get`,
//! `connections.reportStatus`, `time.sleep`, `http.fetch` with
//! `connection`), sent through the raw host call of guest ABI 1: the SDK
//! pinned here predates them, and the JSON is exactly the ABI's
//! (DeckPilot `plugins/runtime/crates/abi/src/connections.rs`). Only the
//! host import `deckpilot_v1.host_call` is used.
//!
//! An agent that does not carry them out answers `unsupported-op`; then
//! every action fails with `unsupported` and nothing else happens.

use serde::Deserialize;
use serde_json::{Value, json};

use crate::errors::{Failure, codes, fail};
use crate::provider::{ConnectionInfo, ConnectionStatus};
use crate::transport::{HttpReply, HttpRequest, Transport, TransportError};

/// `cn_` and 26 Crockford base32 characters (the ABI's
/// `is_connection_id`).
pub fn is_connection_id(id: &str) -> bool {
    id.strip_prefix("cn_").is_some_and(|body| {
        body.len() == 26
            && body
                .bytes()
                .all(|b| b.is_ascii_digit() || (b.is_ascii_uppercase() && !b"ILOU".contains(&b)))
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Reply {
    Ok(Value),
    Error(CallError),
}

#[derive(Debug, Deserialize)]
struct CallError {
    code: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    kind: Option<String>,
}

/// One host call; `None` off wasm.
fn call(request: &Value) -> Result<Value, TransportError> {
    let bytes = serde_json::to_vec(request).map_err(|_| TransportError::Other("request".into()))?;
    let Some(reply) = deckpilot_plugin_guest::__rt::host_call_bytes(&bytes) else {
        return Err(TransportError::Unsupported);
    };
    match serde_json::from_slice::<Reply>(&reply) {
        Ok(Reply::Ok(value)) => Ok(value),
        Ok(Reply::Error(error)) => Err(map_error(error)),
        Err(_) => Err(TransportError::Other(
            "malformed reply from the host".into(),
        )),
    }
}

/// How the host's executor wraps a refused redirect (knittingcode/deckpilot
/// #467 `executor/mod.rs`); the broker's code follows it.
pub const REDIRECT_WRAPPER: &str = "a redirect was refused by the permission broker (";

/// How the host's supervisor says that it stopped a call because the
/// invocation was cancelled or ran out of time (#467 `supervisor/src/host.rs`).
pub const SUPERVISOR_CANCELLED: [&str; 2] = [
    "the action was cancelled or ran out of time; the call was stopped",
    "the action was cancelled; no further capability calls are carried out",
];

/// The host's refusal as the transport's error. The host names the
/// connection's own refusals by their code in the message
/// (`connection-missing`, `connection-disabled`, `redirect-refused`).
fn map_error(error: CallError) -> TransportError {
    let has = |token: &str| error.message.contains(token);
    // The host names its connection refusals by a code at the start of the
    // message (`redirect-refused: …`, knittingcode/deckpilot#467); the same
    // word later in a message (a server's text, a URL) is not one.
    // A refused redirect comes wrapped (#467's executor): "a redirect was
    // refused by the permission broker (<code>: …)".
    let message = error.message.trim_start();
    let head = message.strip_prefix(REDIRECT_WRAPPER).unwrap_or(message);
    let starts = |token: &str| {
        head.strip_prefix(token)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with([':', ' ', ';', ',', '.']))
    };
    // A stopped call: `time.sleep`'s "cancelled: …", or the supervisor's
    // own words when the invocation is cancelled or out of time.
    let cancelled =
        starts("cancelled") || SUPERVISOR_CANCELLED.iter().any(|s| message.starts_with(s));
    match error.code.as_str() {
        "unsupported-op" => TransportError::Unsupported,
        "throttled" => TransportError::Throttled,
        _ if starts("connection-missing") => TransportError::ConnectionMissing,
        _ if starts("connection-disabled") => TransportError::ConnectionDisabled,
        _ if starts("redirect-refused") => TransportError::RedirectRefused,
        _ if cancelled => TransportError::Cancelled,
        "failed" => match error.kind {
            Some(kind) => TransportError::Network(kind),
            None if has("too large") || has("exceeds") || has("larger than") => {
                TransportError::TooLarge
            }
            None => TransportError::Other(error.message),
        },
        "not-permitted" => TransportError::Denied(error.message),
        _ => TransportError::Other(error.message),
    }
}

/// Tests only: what the module makes of a host refusal with `code`,
/// `message` and `kind` (the reply of a refused `http.fetch`), so tests
/// that script refusals go through the same reading.
#[cfg(test)]
pub(crate) fn refusal_of(code: &str, message: &str, kind: Option<&str>) -> TransportError {
    map_error(CallError {
        code: code.into(),
        message: message.into(),
        kind: kind.map(str::to_owned),
    })
}

/// The connection `id`, when the invocation is bound to it.
pub fn connection(id: &str) -> Result<ConnectionInfo, Failure> {
    if !is_connection_id(id) {
        return Err(fail(
            codes::CONNECTION_MISSING,
            "choose a connection in the action's settings",
        ));
    }
    let value = call(&json!({"op": "connections.get", "args": {"connection": id}}))
        .map_err(|e| e.into_failure("the connection").0)?;
    serde_json::from_value(value).map_err(|_| {
        fail(
            codes::INVALID_RESPONSE,
            "DeckPilot's answer about the connection is not readable",
        )
    })
}

/// Unit tests only: the connection and transport the next action binds
/// to, instead of the host (which a native test has not). Every action
/// then runs as on the agent, with the plugin's own parameter checks.
#[cfg(test)]
pub mod test_binding {
    use std::cell::RefCell;

    use crate::provider::ConnectionInfo;
    use crate::transport::Transport;

    type Binding = (ConnectionInfo, Box<dyn Transport>);

    thread_local! {
        static NEXT: RefCell<Option<Binding>> = const { RefCell::new(None) };
    }

    /// Binds the next action of this thread.
    pub fn set(connection: ConnectionInfo, transport: Box<dyn Transport>) {
        NEXT.with(|next| *next.borrow_mut() = Some((connection, transport)));
    }

    pub fn take() -> Option<Binding> {
        NEXT.with(|next| next.borrow_mut().take())
    }
}

/// The transport of the wasm module: every request through the host, on
/// one bound connection.
#[derive(Debug)]
pub struct HostTransport {
    connection: String,
}

impl HostTransport {
    pub fn new(connection: &ConnectionInfo) -> Self {
        Self {
            connection: connection.id.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct Response {
    status: u16,
    #[serde(default)]
    headers: Vec<Header>,
    #[serde(default)]
    body: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Header {
    name: String,
    value: String,
}

impl Transport for HostTransport {
    fn send(&mut self, request: &HttpRequest) -> Result<HttpReply, TransportError> {
        let mut args = json!({
            "method": request.method.as_str(),
            "url": request.url,
            "connection": self.connection,
        });
        if !request.headers.is_empty() {
            args["headers"] = Value::Array(
                request
                    .headers
                    .iter()
                    .map(|(name, value)| json!({"name": name, "value": value}))
                    .collect(),
            );
        }
        if let Some(body) = &request.body {
            args["body"] = json!(body);
        }
        let value = call(&json!({"op": "http.fetch", "args": args}))?;
        let response: Response = serde_json::from_value(value)
            .map_err(|_| TransportError::Other("the reply is not an HTTP response".into()))?;
        Ok(HttpReply {
            status: response.status,
            headers: response
                .headers
                .into_iter()
                .map(|h| (h.name.to_ascii_lowercase(), h.value))
                .collect(),
            body: response.body.unwrap_or_default(),
        })
    }

    fn report(&mut self, status: ConnectionStatus, detail: Option<&str>) {
        let mut args = json!({"connection": self.connection, "status": status});
        if let Some(detail) = detail {
            args["detail"] = json!(detail);
        }
        // Best effort: an older agent's refusal changes nothing here.
        let _ = call(&json!({"op": "connections.reportStatus", "args": args}));
    }

    fn sleep(&mut self, ms: u32) -> Result<(), TransportError> {
        call(&json!({"op": "time.sleep", "args": {"ms": ms.clamp(1, 30_000)}})).map(drop)
    }

    fn now_ms(&mut self) -> Option<u64> {
        call(&json!({"op": "time.now", "args": {}}))
            .ok()?
            .get("unixMs")?
            .as_u64()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_errors_map_to_the_transport_errors() {
        let e = |code: &str, message: &str, kind: Option<&str>| {
            map_error(CallError {
                code: code.into(),
                message: message.into(),
                kind: kind.map(str::to_owned),
            })
        };
        assert_eq!(e("unsupported-op", "x", None), TransportError::Unsupported);
        assert_eq!(e("throttled", "x", None), TransportError::Throttled);
        // A too-large answer, in each wording the agent has used.
        for message in [
            "the response is larger than 524288 bytes",
            "the answer is too large",
            "the body exceeds 512 KiB",
        ] {
            assert_eq!(
                e("failed", message, None),
                TransportError::TooLarge,
                "{message}"
            );
        }
        assert_eq!(
            e("failed", "no route", None),
            TransportError::Other("no route".into())
        );
        assert_eq!(
            e("failed", "x", Some("tls-failed")),
            TransportError::Network("tls-failed".into())
        );
        assert_eq!(
            e("not-permitted", "connection-missing: removed", None),
            TransportError::ConnectionMissing
        );
        assert_eq!(
            e(
                "not-permitted",
                "redirect-refused to https://other.example",
                None
            ),
            TransportError::RedirectRefused
        );
        assert!(matches!(
            e("not-permitted", "origin not granted", None),
            TransportError::Denied(_)
        ));
        // The host's codes are prefixes (#467), with or without a reason.
        for (message, want) in [
            (
                "connection-disabled: turned off",
                TransportError::ConnectionDisabled,
            ),
            ("connection-missing", TransportError::ConnectionMissing),
            (
                "redirect-refused: to https://other.example",
                TransportError::RedirectRefused,
            ),
            (
                "cancelled: the action was cancelled",
                TransportError::Cancelled,
            ),
            (" cancelled", TransportError::Cancelled),
            // #467's literal strings: the executor's wrapped redirect
            // refusals and the supervisor's cancellations.
            (
                "a redirect was refused by the permission broker (redirect-refused: the server \
                 redirected the request away from the connection's server)",
                TransportError::RedirectRefused,
            ),
            (
                "a redirect was refused by the permission broker (connection-missing: the \
                 connection is not one this action was given, or it was removed)",
                TransportError::ConnectionMissing,
            ),
            (
                "a redirect was refused by the permission broker (connection-disabled: the \
                 connection is turned off in DeckPilot)",
                TransportError::ConnectionDisabled,
            ),
            (
                "the action was cancelled or ran out of time; the call was stopped",
                TransportError::Cancelled,
            ),
            (
                "the action was cancelled; no further capability calls are carried out",
                TransportError::Cancelled,
            ),
        ] {
            assert_eq!(e("not-permitted", message, None), want, "{message}");
        }
        // The supervisor's cancellation, with its own code.
        for message in SUPERVISOR_CANCELLED {
            assert_eq!(e("unavailable", message, None), TransportError::Cancelled);
        }
        // The same words anywhere else are no refusal of the host's: a
        // server's text or a URL in the middle of a message.
        for message in [
            "the server said: redirect-refused",
            "origin https://connection-missing.example not granted",
            "GET /api/v4/projects/connection-disabled failed",
            "500 Internal Server Error: pipeline status unknown",
            "redirect-refusedx: not a code",
            // The wrapper with another reason, or the code elsewhere.
            "a redirect was refused by the permission broker (too-many-redirects)",
            "a redirect was refused (redirect-refused: not the executor's words)",
            "the action was cancelled by the server's job",
        ] {
            assert!(
                matches!(e("not-permitted", message, None), TransportError::Denied(_)),
                "{message}"
            );
            assert!(
                matches!(e("failed", message, None), TransportError::Other(_)),
                "{message}"
            );
        }
    }

    #[test]
    fn off_wasm_there_is_no_host() {
        let id = "cn_01JD7Q0000000000000000000A";
        assert!(is_connection_id(id));
        assert!(!is_connection_id("cn_01jd7q0000000000000000000a"));
        assert_eq!(connection(id).unwrap_err().code, codes::UNSUPPORTED);
        assert_eq!(
            connection("github").unwrap_err().code,
            codes::CONNECTION_MISSING
        );
    }
}
