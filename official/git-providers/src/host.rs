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

/// The host's refusal as the transport's error. The host names the
/// connection's own refusals by their code in the message
/// (`connection-missing`, `connection-disabled`, `redirect-refused`).
fn map_error(error: CallError) -> TransportError {
    let has = |token: &str| error.message.contains(token);
    match error.code.as_str() {
        "unsupported-op" => TransportError::Unsupported,
        "throttled" => TransportError::Throttled,
        _ if has("connection-missing") => TransportError::ConnectionMissing,
        _ if has("connection-disabled") => TransportError::ConnectionDisabled,
        _ if has("redirect-refused") => TransportError::RedirectRefused,
        _ if has("cancelled") => TransportError::Cancelled,
        "failed" => match error.kind {
            Some(kind) => TransportError::Network(kind),
            None if has("too large") || has("exceeds") => TransportError::TooLarge,
            None => TransportError::Other(error.message),
        },
        "not-permitted" => TransportError::Denied(error.message),
        _ => TransportError::Other(error.message),
    }
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
