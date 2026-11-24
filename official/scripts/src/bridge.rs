//! The guest side of the Scripts host bridge (DeckPilot
//! `design-projects-scripts.md` §4.5): one JSON request per
//! `deckpilot_v1.host_call`, answered by the agent's broker against the
//! script's **current** grants.
//!
//! Wire format (ABI 1, added compatibly; the same envelope as
//! `CapabilityRequest`): the request is `{"op": "<op>", "args": {…}}`, the
//! reply `{"ok": <value>}` or `{"error": {"code", "message", "script"?}}`,
//! where `code` is a `CallErrorCode` (`not-permitted`, `bad-request`,
//! `unsupported-op`, `failed`, `unavailable`, `throttled`) and the optional
//! `script` object carries the script-level detail ([`ScriptFault`]). An
//! agent without the bridge answers `unsupported-op`.
//!
//! Every request leaves through [`Session::call`], which counts it against
//! the per-run budget ([`limits::HOST_CALLS`]) before anything is sent, and
//! refuses every op once the run is cancelled. Arguments are validated by
//! [`crate::api`] first; the agent checks everything again, since the guest
//! is not trusted.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The limits of one run that the guest enforces itself (§5.4). The agent
/// enforces its own copy of each; these only fail earlier and with a
/// clearer message.
pub mod limits {
    /// Bridge calls per run (`runtime.hostCalls`), `script.poll` excluded.
    pub const HOST_CALLS: u32 = 4_096;
    /// `script.poll` calls per run (cancellation and the remaining time).
    pub const POLLS: u32 = 8_192;
    /// Log lines per run; one more line, the truncation marker, follows.
    pub const LOG_LINES: u32 = 1_000;
    /// Log bytes per run.
    pub const LOG_BYTES: usize = 256 * 1024;
    /// One log line, in bytes (longer lines are cut on a char boundary).
    pub const LOG_LINE: usize = 8 * 1024;
    /// One request, in bytes (the host's limit is 64 KiB per request; a
    /// `fs.writeFile` of up to 4 MiB is the exception the agent's bridge
    /// raises for this plugin, see `api::fs`).
    pub const REQUEST: usize = 64 * 1024;
    /// One reply, in bytes (the host's `MAX_REPLY`, 1 MiB, raised for
    /// `fs.readFile` like requests).
    pub const REPLY: usize = 6 * 1024 * 1024;
    /// The script source (§5.4, checked by the core too).
    pub const SOURCE: usize = 256 * 1024;
    /// Memory: default, minimum and maximum MiB, and the headroom kept
    /// between QuickJS's own limit and the wasm store's.
    pub const MEMORY_MIB_DEFAULT: u32 = 64;
    pub const MEMORY_MIB_MIN: u32 = 16;
    pub const MEMORY_MIB_MAX: u32 = 128;
    pub const MEMORY_HEADROOM_MIB: u32 = 8;
    /// QuickJS's stack limit (the wasm shadow stack is 1 MiB).
    pub const STACK: usize = 256 * 1024;
    /// The wall time of a run: default and maximum (ms).
    pub const TIMEOUT_MS_DEFAULT: u64 = 60_000;
    pub const TIMEOUT_MS_MAX: u64 = 600_000;
    /// Interrupts (QuickJS calls the handler about every 10,000 bytecode
    /// operations) between two `script.poll`s.
    pub const INTERRUPTS_PER_POLL: u32 = 256;
    /// The fuel-independent instruction budget: interrupts per run.
    pub const INTERRUPTS: u64 = 2_000_000;
    /// The JSON of the run's result value.
    pub const RESULT: usize = 64 * 1024;
}

/// One host call: request bytes out, reply bytes back. On wasm this is
/// `deckpilot_v1.host_call`; native tests put a mock behind it.
pub trait Transport {
    /// `None`: there is no host (a native build outside the tests).
    fn call(&mut self, request: &[u8]) -> Option<Vec<u8>>;
}

/// The real transport: the guest ABI's only import.
#[derive(Debug, Default)]
pub struct HostCall;

impl Transport for HostCall {
    fn call(&mut self, request: &[u8]) -> Option<Vec<u8>> {
        deckpilot_plugin_guest::__rt::host_call_bytes(request)
    }
}

/// The script-level detail of a refusal or failure, as the agent sends it
/// in `error.script` and as `ScriptError` shows it to the script.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptFault {
    /// One of the `ErrorCode`s of `deckpilot.d.ts`.
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// For `needsConfirmation`: the token of the plan the agent computed,
    /// which `script.confirm` shows; nothing else is trusted from here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmation: Option<Value>,
}

impl ScriptFault {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: Some(message.into()),
            ..Self::default()
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Reply {
    Ok(Value),
    Error(ReplyError),
}

#[derive(Debug, Deserialize)]
struct ReplyError {
    code: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    script: Option<ScriptFault>,
}

/// A host `CallErrorCode` as a script `ErrorCode`, for a refusal without a
/// `script` detail.
fn script_code(call_code: &str) -> &'static str {
    match call_code {
        "not-permitted" => "notGranted",
        "bad-request" => "invalidArgument",
        "throttled" => "limit",
        "unsupported-op" => "unsupported",
        "unavailable" => "unavailable",
        _ => "hostError",
    }
}

/// Where a run is; shared by the bridge, the log and the interrupt handler.
#[derive(Debug, Default)]
pub struct Budget {
    pub calls: u32,
    pub polls: u32,
    pub interrupts: u64,
    pub log_lines: u32,
    pub log_bytes: usize,
    pub log_truncated: bool,
    /// Set once the agent says the run is cancelled (or out of time): every
    /// later op fails locally and the interrupt handler stops the script.
    pub cancelled: bool,
    pub timed_out: bool,
    /// The milliseconds the agent said were left at the last poll.
    pub remaining_ms: Option<u64>,
    /// The agent's clock (µs since the epoch) at the last reply that said,
    /// for `Date.now()` without a host call of its own.
    pub clock_us: f64,
}

/// One run's bridge: the transport and the budget.
pub struct Session {
    transport: Box<dyn Transport>,
    pub budget: Budget,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("budget", &self.budget)
            .finish_non_exhaustive()
    }
}

impl Session {
    pub fn new(transport: Box<dyn Transport>) -> Self {
        Self {
            transport,
            budget: Budget::default(),
        }
    }

    /// Sends one op (already validated by [`crate::api`]) and returns its
    /// value, or the fault the script sees.
    pub fn call(&mut self, op: &str, args: Value) -> Result<Value, ScriptFault> {
        if self.budget.cancelled {
            return Err(self.stopped());
        }
        if self.budget.calls >= limits::HOST_CALLS {
            return Err(ScriptFault::new(
                "limit",
                format!(
                    "the script made {} bridge calls, the most one run may make",
                    limits::HOST_CALLS
                ),
            ));
        }
        self.budget.calls += 1;
        self.send(op, args)
    }

    /// `script.poll`, outside the call budget (its own, [`limits::POLLS`]).
    /// Returns whether the script must stop.
    pub fn poll(&mut self) -> bool {
        if self.budget.cancelled {
            return true;
        }
        if self.budget.polls >= limits::POLLS {
            // Out of polls: the run cannot learn of a Stop any more, so it
            // stops itself rather than run on unobserved.
            self.budget.cancelled = true;
            self.budget.timed_out = true;
            return true;
        }
        self.budget.polls += 1;
        match self.send("script.poll", json!({})) {
            Ok(v) => {
                let cancelled = v.get("cancelled").and_then(Value::as_bool).unwrap_or(false);
                let remaining = v.get("remainingMs").and_then(Value::as_u64);
                self.budget.remaining_ms = remaining;
                if cancelled {
                    self.budget.cancelled = true;
                } else if remaining == Some(0) {
                    self.budget.cancelled = true;
                    self.budget.timed_out = true;
                }
                self.budget.cancelled
            }
            // A host that cannot answer a poll cannot be told of a Stop:
            // stop. `unsupported` (an agent without the bridge) cannot
            // happen here, since `script.load` came first.
            Err(_) => {
                self.budget.cancelled = true;
                true
            }
        }
    }

    /// The fault of an op attempted after the run stopped.
    pub fn stopped(&self) -> ScriptFault {
        if self.budget.timed_out {
            ScriptFault::new("timedOut", "the script ran out of time")
        } else {
            ScriptFault::new("cancelled", "the run was stopped")
        }
    }

    fn send(&mut self, op: &str, args: Value) -> Result<Value, ScriptFault> {
        let request = json!({ "op": op, "args": args });
        let Ok(bytes) = serde_json::to_vec(&request) else {
            return Err(ScriptFault::new(
                "invalidArgument",
                "the request is not JSON",
            ));
        };
        if bytes.len() > max_request(op) {
            return Err(ScriptFault::new(
                "tooLarge",
                format!(
                    "the {op} request is {} bytes, more than one call may send",
                    bytes.len()
                ),
            ));
        }
        let Some(reply) = self.transport.call(&bytes) else {
            return Err(ScriptFault::new(
                "unavailable",
                "not running in DeckPilot's plugin host",
            ));
        };
        if reply.len() > limits::REPLY {
            return Err(ScriptFault::new(
                "hostError",
                "the host's reply is too large",
            ));
        }
        let reply: Reply = serde_json::from_slice(&reply)
            .map_err(|_| ScriptFault::new("hostError", format!("malformed reply to {op}")))?;
        match reply {
            Reply::Ok(v) => {
                if let Some(us) = v.get("nowUs").and_then(Value::as_f64) {
                    self.budget.clock_us = self.budget.clock_us.max(us);
                    crate::clock::set(us);
                }
                Ok(v)
            }
            Reply::Error(e) => {
                let mut fault = e.script.unwrap_or_default();
                if fault.code.is_empty() {
                    fault.code = script_code(&e.code).to_owned();
                }
                if fault.message.as_deref().is_none_or(str::is_empty) {
                    fault.message = Some(if e.message.is_empty() {
                        format!("{op} failed ({})", e.code)
                    } else {
                        e.message
                    });
                }
                match fault.code.as_str() {
                    "cancelled" => self.budget.cancelled = true,
                    "timedOut" if op == "script.sleep" => {
                        self.budget.cancelled = true;
                        self.budget.timed_out = true;
                    }
                    _ => {}
                }
                Err(fault)
            }
        }
    }
}

/// The largest request an op may send: `fs.writeFile` carries its content
/// (4 MiB of text, or its base64), everything else fits the host's 64 KiB.
fn max_request(op: &str) -> usize {
    match op {
        "fs.writeFile" => 6 * 1024 * 1024,
        "process.run" | "shell.run" => 1024 * 1024 + 300 * 1024,
        _ => limits::REQUEST,
    }
}
