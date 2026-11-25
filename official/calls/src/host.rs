//! The Calls operations of plugin API 1.16 (`call.state`, `call.control`),
//! sent through the raw host call of guest ABI 1: the SDK pinned here
//! predates them, and the JSON is exactly the ABI's (DeckPilot
//! `plugins/runtime/crates/abi/src/calls.rs`). Only the host import
//! `deckpilot_v1.host_call` is used.
//!
//! The plugin never names a call, a provider, a device or a layout: the
//! host acts on the focused call, decides through its own gate (the call's
//! state, the provider's capabilities, the user's consents) and answers a
//! control only once the call app confirmed it. An agent that does not
//! carry the operations out answers `unsupported-op`, read as
//! `calls-provider-offline`.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{Value, json};

/// The refusal codes, as the host starts its messages with them and as the
/// manifest's `errorCodes` declare them.
pub mod codes {
    pub const NOT_IN_CALL: &str = "calls-not-in-call";
    pub const UNSUPPORTED: &str = "calls-unsupported";
    pub const UNCONFIRMED: &str = "calls-unconfirmed";
    pub const PROVIDER_OFFLINE: &str = "calls-provider-offline";
    pub const CONSENT: &str = "calls-consent";
    pub const LOCKED: &str = "calls-locked";
    /// The plugin's own: "Back to work" on a key outside the Calls pages.
    pub const PAGES_ONLY: &str = "calls-pages-only";

    /// Every code the host may start a refusal with.
    pub const HOST: [&str; 6] = [
        NOT_IN_CALL,
        UNSUPPORTED,
        UNCONFIRMED,
        PROVIDER_OFFLINE,
        CONSENT,
        LOCKED,
    ];

    /// Every code the module returns.
    pub const ALL: [&str; 7] = [
        NOT_IN_CALL,
        UNSUPPORTED,
        UNCONFIRMED,
        PROVIDER_OFFLINE,
        CONSENT,
        LOCKED,
        PAGES_ONLY,
    ];
}

/// Why an operation did not work: a code of [`codes`] and the host's
/// message (never anything of the call).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub code: &'static str,
    pub message: String,
    /// The host throttled the call: trying again later may work.
    pub retryable: bool,
}

/// One action on the focused call: the ABI's JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallAction {
    Answer,
    Reject,
    CancelOutgoing,
    End,
    MicToggle,
    MicSet { muted: bool },
    CameraToggle,
    CameraSet { on: bool },
}

impl CallAction {
    fn json(self) -> Value {
        match self {
            Self::Answer => json!({"kind": "answer"}),
            Self::Reject => json!({"kind": "reject"}),
            Self::CancelOutgoing => json!({"kind": "cancelOutgoing"}),
            Self::End => json!({"kind": "end"}),
            Self::MicToggle => json!({"kind": "micToggle"}),
            Self::MicSet { muted } => json!({"kind": "micSet", "muted": muted}),
            Self::CameraToggle => json!({"kind": "cameraToggle"}),
            Self::CameraSet { on } => json!({"kind": "cameraSet", "on": on}),
        }
    }
}

/// The control actions (`deckpilot.calls.<id>`) and what each asks for:
/// DeckPilot's `ops::CONTROL_ACTIONS`.
pub const CONTROL_ACTIONS: [(&str, CallAction); 10] = [
    ("answer", CallAction::Answer),
    ("reject", CallAction::Reject),
    ("cancel-outgoing", CallAction::CancelOutgoing),
    ("end", CallAction::End),
    ("mic-toggle", CallAction::MicToggle),
    ("mic-mute", CallAction::MicSet { muted: true }),
    ("mic-unmute", CallAction::MicSet { muted: false }),
    ("camera-toggle", CallAction::CameraToggle),
    ("camera-on", CallAction::CameraSet { on: true }),
    ("camera-off", CallAction::CameraSet { on: false }),
];

/// The action a control action id asks for.
pub fn action_of(id: &str) -> Option<CallAction> {
    CONTROL_ACTIONS
        .iter()
        .find(|(name, _)| *name == id)
        .map(|(_, action)| *action)
}

/// Whether one control can work now, and why not.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlSupport {
    pub supported: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

/// `call.state`'s answer. Unknown fields are ignored (the host may add to
/// it within ABI 1). The title (only with the user's opt-in) is never read
/// here: the keys show states, not text of the call.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallState {
    pub state: String,
    #[serde(default)]
    pub connected: bool,
    #[serde(default)]
    pub mic_muted: Option<bool>,
    #[serde(default)]
    pub camera_on: Option<bool>,
    #[serde(default)]
    pub pending: bool,
    #[serde(default)]
    pub unconfirmed: bool,
    #[serde(default)]
    pub controls: BTreeMap<String, ControlSupport>,
}

impl CallState {
    /// A call is going on (or is being set up or ended).
    pub fn is_live(&self) -> bool {
        matches!(
            self.state.as_str(),
            "incoming" | "outgoing" | "connecting" | "active" | "ending" | "unknown"
        )
    }
}

/// `call.control`'s answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    /// The call app confirmed it.
    Confirmed,
    /// The call already was in that state: nothing was sent.
    AlreadyThere,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::AlreadyThere => "alreadyThere",
        }
    }
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
}

/// The focused call (`call.state`).
pub fn state() -> Result<CallState, Refusal> {
    let value = call(&json!({"op": "call.state", "args": {}}))?;
    serde_json::from_value(value).map_err(|_| malformed())
}

/// One action on the focused call (`call.control`): its confirmed outcome.
pub fn control(action: CallAction) -> Result<Outcome, Refusal> {
    let value = call(&json!({"op": "call.control", "args": {"action": action.json()}}))?;
    serde_json::from_value(value).map_err(|_| malformed())
}

fn malformed() -> Refusal {
    Refusal {
        code: codes::PROVIDER_OFFLINE,
        message: "malformed reply from the host".into(),
        retryable: false,
    }
}

/// One host call.
fn call(request: &Value) -> Result<Value, Refusal> {
    let bytes = serde_json::to_vec(request).map_err(|_| malformed())?;
    let reply = send(&bytes).ok_or_else(|| Refusal {
        code: codes::PROVIDER_OFFLINE,
        message: "Calls is not available here".into(),
        retryable: false,
    })?;
    match serde_json::from_slice::<Reply>(&reply) {
        Ok(Reply::Ok(value)) => Ok(value),
        Ok(Reply::Error(error)) => Err(refusal(&error.code, error.message)),
        Err(_) => Err(malformed()),
    }
}

#[cfg(not(test))]
fn send(bytes: &[u8]) -> Option<Vec<u8>> {
    deckpilot_plugin_guest::__rt::host_call_bytes(bytes)
}

#[cfg(test)]
fn send(bytes: &[u8]) -> Option<Vec<u8>> {
    test_host::answer(bytes)
}

/// The host's refusal as one of [`codes`]: the code it starts its message
/// with (`calls-unsupported: provider-has-no-api`), else by the ABI code.
pub fn refusal(code: &str, message: String) -> Refusal {
    let head = message.trim_start();
    let named = codes::HOST.into_iter().find(|c| {
        head.strip_prefix(c)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(':'))
    });
    let code = match (named, code) {
        (Some(named), _) => named,
        (None, "not-permitted") => codes::CONSENT,
        (None, "throttled") => codes::UNCONFIRMED,
        // `unsupported-op` (an agent without Calls), `unavailable` and
        // anything else: nothing to talk to.
        _ => codes::PROVIDER_OFFLINE,
    };
    Refusal {
        code,
        retryable: matches!(code, codes::UNCONFIRMED) && named.is_none(),
        message,
    }
}

/// Unit tests only: what the host answers, instead of the host (which a
/// native test has not). Every request is recorded.
#[cfg(test)]
pub mod test_host {
    use std::cell::RefCell;

    use serde_json::Value;

    type Answer = Box<dyn Fn(&Value) -> Option<Value>>;

    thread_local! {
        static HOST: RefCell<Option<Answer>> = const { RefCell::new(None) };
        static SENT: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
    }

    /// Answers this thread's host calls with `answer` (`None`: the call
    /// fails as off wasm).
    pub fn set(answer: impl Fn(&Value) -> Option<Value> + 'static) {
        HOST.with(|h| *h.borrow_mut() = Some(Box::new(answer)));
        SENT.with(|s| s.borrow_mut().clear());
    }

    /// The requests sent since [`set`].
    pub fn sent() -> Vec<Value> {
        SENT.with(|s| s.borrow().clone())
    }

    pub(super) fn answer(bytes: &[u8]) -> Option<Vec<u8>> {
        let request: Value = serde_json::from_slice(bytes).ok()?;
        SENT.with(|s| s.borrow_mut().push(request.clone()));
        let reply = HOST.with(|h| h.borrow().as_ref().and_then(|a| a(&request)))?;
        serde_json::to_vec(&reply).ok()
    }
}
