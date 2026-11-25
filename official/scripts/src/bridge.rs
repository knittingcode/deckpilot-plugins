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
//! refuses every op once the run is cancelled (the one exception is
//! [`Session::cleanup`], the abort of an open write). Every request is at
//! most [`limits::REQUEST`] and every reply at most [`limits::REPLY`], the
//! host's caps, for every op (P6 decision D1: large values are chunked). Arguments are validated by
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
    /// One request, in bytes: the host's `MAX_HOST_CALL_REQUEST`, for every
    /// op (D1: large values move in chunks, `api::fs`).
    pub const REQUEST: usize = 64 * 1024;
    /// One reply, in bytes: the host's `MAX_HOST_CALL_REPLY`, for every op.
    pub const REPLY: usize = 1024 * 1024;
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
    /// Set when the run stopped itself because it spent its polls.
    pub polls_spent: bool,
    /// The milliseconds the agent said were left (`script.load` or the
    /// last `script.poll`), and the agent's clock when it said so.
    pub remaining_ms: Option<u64>,
    pub remaining_at_us: Option<f64>,
    /// The agent's clock (µs since the epoch) at the last reply that said,
    /// for `Date.now()` without a host call of its own.
    pub clock_us: f64,
}

impl Budget {
    /// Records the time the agent said is left, as of its clock now.
    pub fn set_remaining(&mut self, ms: u64) {
        self.remaining_ms = Some(ms);
        self.remaining_at_us = (self.clock_us > 0.0).then_some(self.clock_us);
    }

    /// The run's remaining time as the guest knows it: what the agent said
    /// last, less the agent time that passed since (the clock moves only
    /// with replies, so this errs long; the agent clamps again).
    pub fn remaining_now(&self) -> Option<u64> {
        let ms = self.remaining_ms?;
        let spent = match self.remaining_at_us {
            Some(at) if self.clock_us > at => ((self.clock_us - at) / 1000.0) as u64,
            _ => 0,
        };
        Some(ms.saturating_sub(spent))
    }

    /// The calls left in the run's budget.
    pub fn calls_left(&self) -> u32 {
        limits::HOST_CALLS.saturating_sub(self.calls)
    }
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
        self.reserve(1)?;
        self.budget.calls += 1;
        self.send(op, args)
    }

    /// Refuses unless `calls` more calls fit the run's budget: an op of
    /// several calls (a chunked write and its abort) starts only when all
    /// of them fit, so it never stops half way for want of calls.
    pub fn reserve(&self, calls: u32) -> Result<(), ScriptFault> {
        if calls > self.budget.calls_left() {
            return Err(ScriptFault::new(
                "limit",
                format!(
                    "the script made {} of the {} bridge calls one run may make; this \
                     operation needs {calls} more",
                    self.budget.calls,
                    limits::HOST_CALLS
                ),
            ));
        }
        Ok(())
    }

    /// The cleanup of an op of several calls (`fs.write.abort`): sent even
    /// after the run was cancelled, so the agent can drop what the op holds
    /// at once (it drops it at the run's end anyway). Its call was reserved
    /// with the op's own, so it is counted without a budget check.
    pub fn cleanup(&mut self, op: &str, args: Value) -> Result<Value, ScriptFault> {
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
            self.budget.polls_spent = true;
            return true;
        }
        self.budget.polls += 1;
        match self.send("script.poll", json!({})) {
            Ok(v) => {
                let cancelled = v.get("cancelled").and_then(Value::as_bool).unwrap_or(false);
                let remaining = v.get("remainingMs").and_then(Value::as_u64);
                if let Some(ms) = remaining {
                    self.budget.set_remaining(ms);
                }
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
        if self.budget.polls_spent {
            polls_spent()
        } else if self.budget.timed_out {
            ScriptFault::new("timedOut", "the script ran out of time")
        } else {
            ScriptFault::new("cancelled", "the run was stopped")
        }
    }

    /// Ends the run for want of time: the agent said none is left.
    pub fn out_of_time(&mut self) -> ScriptFault {
        self.budget.cancelled = true;
        self.budget.timed_out = true;
        self.stopped()
    }

    fn send(&mut self, op: &str, args: Value) -> Result<Value, ScriptFault> {
        let request = json!({ "op": op, "args": args });
        let Ok(bytes) = serde_json::to_vec(&request) else {
            return Err(ScriptFault::new(
                "invalidArgument",
                "the request is not JSON",
            ));
        };
        if bytes.len() > limits::REQUEST {
            // Before any host call: the host refuses (traps) a larger one.
            return Err(ScriptFault::new(
                "limit",
                format!(
                    "the {op} request is {} bytes, more than the {} one call may send",
                    bytes.len(),
                    limits::REQUEST
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
                // A refusal with `cancelled` or `timedOut` is the run's
                // end (a process's own timeout is a result, not a
                // refusal): every later op fails here.
                match fault.code.as_str() {
                    "cancelled" => self.budget.cancelled = true,
                    "timedOut" => {
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

/// The fault of a run that spent its polls ([`limits::POLLS`]).
pub fn polls_spent() -> ScriptFault {
    ScriptFault::new(
        "limit",
        format!(
            "the script ran {} polls of the agent, the most one run may make, and stopped \
             (an endless loop?)",
            limits::POLLS
        ),
    )
}

#[cfg(test)]
mod tests;
