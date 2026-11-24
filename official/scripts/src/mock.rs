//! A mock of the agent's Scripts bridge for native tests: it records every
//! request exactly as the guest serialized it and answers from a handler.
//! It never touches the file system or starts a process: `process.run`
//! (Docker Compose, `git pull --ff-only`) is answered from canned results,
//! so no test ever runs against a real repository.

use std::cell::RefCell;
use std::rc::Rc;

use serde_json::{Value, json};

use crate::bridge::Transport;

pub type Handler = Box<dyn FnMut(&str, &Value) -> Value>;

pub struct State {
    /// Every request after `script.load`, `script.poll` excluded:
    /// `(op, args)`.
    pub requests: Vec<(String, Value)>,
    pub polls: u32,
    /// `script.load`'s reply value (or, with `load_error`, its refusal).
    pub load: Value,
    pub load_error: Option<Value>,
    /// The poll at which the agent says the run was stopped / is out of
    /// time.
    pub cancel_at_poll: Option<u32>,
    pub timeout_at_poll: Option<u32>,
    pub handler: Handler,
}

impl State {
    pub fn ops(&self) -> Vec<&str> {
        self.requests.iter().map(|(op, _)| op.as_str()).collect()
    }

    pub fn of(&self, op: &str) -> Vec<&Value> {
        self.requests
            .iter()
            .filter(|(o, _)| o == op)
            .map(|(_, a)| a)
            .collect()
    }

    pub fn logs(&self) -> Vec<String> {
        self.of("script.log")
            .iter()
            .map(|a| a["message"].as_str().unwrap_or("").to_owned())
            .collect()
    }
}

#[derive(Clone)]
pub struct Mock(pub Rc<RefCell<State>>);

pub fn ok(v: Value) -> Value {
    json!({ "ok": v })
}

/// A refusal with the script-level detail.
pub fn refuse(call_code: &str, script_code: &str, message: &str) -> Value {
    json!({ "error": { "code": call_code, "message": message, "script": { "code": script_code, "message": message } } })
}

/// A ProcessResult.
pub fn exit(code: i64, stdout: &str, stderr: &str) -> Value {
    ok(json!({
        "ok": code == 0, "exitCode": code, "signal": null, "stdout": stdout, "stderr": stderr,
        "truncated": false, "durationMs": 12, "timedOut": false, "cancelled": false
    }))
}

pub fn repository(key: &str, default: bool) -> Value {
    json!({
        "id": format!("00000000-0000-4000-8000-00000000{:04}", key.len()),
        "key": key, "name": key.to_uppercase(), "description": null, "isDefault": default,
        "path": null, "exists": true, "remote": null
    })
}

impl Mock {
    pub fn new(source: &str) -> Self {
        Self::with(
            source,
            json!({}),
            Box::new(|op, _| {
                refuse(
                    "unsupported-op",
                    "unsupported",
                    &format!("{op}: not in this mock"),
                )
            }),
        )
    }

    pub fn with(source: &str, grants: Value, handler: Handler) -> Self {
        Mock(Rc::new(RefCell::new(State {
            requests: Vec::new(),
            polls: 0,
            load: json!({
                "source": source,
                "platform": "linux",
                "settings": { "timeoutMs": 60000, "stopOnError": true, "memoryMiB": 64 },
                "grants": grants,
                "run": { "id": "run-1", "origin": "editor", "userInitiated": true },
                "nowUs": 1_700_000_000_000_000.0_f64,
            }),
            load_error: None,
            cancel_at_poll: None,
            timeout_at_poll: None,
            handler,
        })))
    }

    pub fn set(&self, f: impl FnOnce(&mut State)) -> &Self {
        f(&mut self.0.borrow_mut());
        self
    }

    pub fn boxed(&self) -> Box<dyn Transport> {
        Box::new(self.clone())
    }
}

impl Transport for Mock {
    fn call(&mut self, request: &[u8]) -> Option<Vec<u8>> {
        let request: Value = serde_json::from_slice(request).expect("the guest sends JSON");
        let obj = request.as_object().expect("an object");
        assert_eq!(obj.len(), 2, "exactly op and args: {request}");
        let op = request["op"].as_str().expect("op").to_owned();
        let args = request["args"].clone();
        assert!(args.is_object(), "args is an object: {request}");
        let mut s = self.0.borrow_mut();
        let reply = match op.as_str() {
            "script.load" => {
                assert!(
                    s.requests.is_empty() && s.polls == 0,
                    "script.load comes first"
                );
                s.load_error.clone().unwrap_or_else(|| ok(s.load.clone()))
            }
            "script.poll" => {
                s.polls += 1;
                let n = s.polls;
                let cancelled = s.cancel_at_poll.is_some_and(|c| n >= c);
                let remaining = if s.timeout_at_poll.is_some_and(|t| n >= t) {
                    0
                } else {
                    30_000
                };
                ok(json!({ "cancelled": cancelled, "remainingMs": remaining }))
            }
            _ => {
                s.requests.push((op.clone(), args.clone()));
                (s.handler)(&op, &args)
            }
        };
        Some(serde_json::to_vec(&reply).unwrap())
    }
}
