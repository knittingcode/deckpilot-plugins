//! Key state of `pipeline-status` keys (design §5 "Key state"): the
//! pipeline's state as the key's mark (green success, red failure, a
//! running mark while it runs) and its branch or change request as the
//! second line, as `deckpilot.git` shows a branch.
//!
//! **What the key shows.** A state call may only read (storage, settings,
//! time, the run context, log): it has no connection and sends no
//! request, and nothing polls a provider in the background (the
//! `background` permission does not exist). So a key shows what the last
//! run of its action found (a press, a workflow node): each run of
//! `pipeline-status` stores the state in the plugin's storage under its
//! parameters (connection, repository, ref, change request), and the
//! state call only reads it back, every 5 s. A key that has not run yet
//! shows its own design.
//!
//! **Only key runs store.** A run from a key or a gesture stores its key's
//! state; a workflow node, the app or a script stores nothing, so dynamic
//! refs in workflows never grow the storage. **Nothing stored is a secret
//! or a server's text:** a closed-set state word and indicator, the key's
//! own ref or change request number, and a failure's code (the key shows
//! a message the plugin writes for it).

use deckpilot_plugin_sdk::ParamValue;
use deckpilot_plugin_sdk::prelude::*;
use serde::{Deserialize, Serialize};

/// What a run stored for a key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Stored {
    State(KeyState),
    /// The failure's code only, never its message (which can hold the
    /// server's text).
    Failed {
        code: String,
    },
}

/// The parameters that make a key: the same ones give the same key state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyParams {
    pub connection: String,
    pub repository: String,
    pub reference: Option<String>,
    pub change_request: Option<i64>,
}

impl KeyParams {
    pub fn read(args: &Args<'_>) -> Self {
        let text = |key: &str| {
            args.get::<Option<String>>(key)
                .ok()
                .flatten()
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        Self {
            connection: text("connection").unwrap_or_default(),
            repository: text("repository").unwrap_or_default(),
            reference: text("ref"),
            change_request: args
                .get::<Option<i64>>("changeRequest")
                .ok()
                .flatten()
                .filter(|n| *n >= 1),
        }
    }

    /// `keyState.pipeline.<hash>`: a storage key is at most 128
    /// characters, and the parameters can be longer (a ref of 250).
    pub fn storage_key(&self) -> String {
        let text = format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}",
            self.connection,
            self.repository,
            self.reference.as_deref().unwrap_or_default(),
            self.change_request
                .map(|n| n.to_string())
                .unwrap_or_default()
        );
        format!("keyState.pipeline.{:016x}", fnv1a(text.as_bytes()))
    }

    /// The key's second line: the branch, or the change request.
    fn detail(&self) -> Option<String> {
        match (self.change_request, &self.reference) {
            (Some(n), _) => Some(format!("#{n}")),
            (None, Some(reference)) => Some(reference.chars().take(40).collect()),
            (None, None) => None,
        }
    }
}

/// FNV-1a, 64 bits: stable across builds (a std hasher is not).
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, b| {
        (hash ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The key state of a pipeline `state` (the closed set, or `none`).
pub fn state_of(state: &str, detail: Option<String>) -> KeyState {
    let indicator = match state {
        "success" => Indicator::Ok,
        "failed" => Indicator::Error,
        "running" | "pending" => Indicator::Running,
        "manual" | "cancelled" => Indicator::Warning,
        _ => Indicator::Unknown,
    };
    let key = KeyState::new().indicator(indicator);
    match detail {
        Some(detail) => key.detail(detail),
        None => key,
    }
}

/// Whether a run is a key's (a key or a gesture), whose state is kept.
pub fn is_key_run(ctx: &mut Context<'_>) -> bool {
    ctx.run_context()
        .is_ok_and(|run| matches!(run.trigger, RunTrigger::Key | RunTrigger::Gesture))
}

/// After a `pipeline-status` run: what its keys show, for a key's run only.
pub fn remember_for_key(ctx: &mut Context<'_>, params: &KeyParams, result: &ActionResult) {
    if is_key_run(ctx) {
        remember(ctx, params, result);
    }
}

/// Stores what the keys with `params` show.
pub fn remember(ctx: &mut Context<'_>, params: &KeyParams, result: &ActionResult) {
    let stored = match result {
        Ok(output) => {
            let state = match output.data().get("state") {
                Some(ParamValue::String(state)) => state.as_str(),
                _ => "unknown",
            };
            Stored::State(state_of(state, params.detail()))
        }
        Err(e) => Stored::Failed {
            code: e.code().to_owned(),
        },
    };
    if let Err(e) = ctx.storage_set(params.storage_key(), &stored) {
        ctx.log(
            LogLevel::Warn,
            format!("the key state was not kept ({:?})", e.kind()),
        );
    }
}

/// The state handler of `pipeline-status` keys: reads only.
pub fn pipeline_status(ctx: &mut Context<'_>, args: &Args<'_>) -> Result<KeyState, PluginError> {
    let params = KeyParams::read(args);
    match ctx.storage_get_as::<Stored>(params.storage_key())? {
        None => Ok(KeyState::new()),
        Some(Stored::State(state)) => Ok(state),
        Some(Stored::Failed { code }) => Err(PluginError::new(
            code.clone(),
            format!("the last pipeline status failed ({code}); press the key to try again"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::rc::Rc;

    use deckpilot_plugin_sdk::testing::MockHost;
    use deckpilot_plugin_sdk::{CallResult, CapabilityRequest, StateOutcome};
    use serde_json::{Value, json};

    use super::*;

    /// A host with a storage, and nothing else.
    fn storage_host() -> (MockHost, Rc<RefCell<BTreeMap<String, Value>>>) {
        let storage = Rc::new(RefCell::new(BTreeMap::new()));
        let kept = Rc::clone(&storage);
        let host = MockHost::new(move |req| match req {
            CapabilityRequest::StorageSet(s) => {
                kept.borrow_mut().insert(s.key.clone(), s.value.clone());
                CallResult::Ok(Value::Null)
            }
            CapabilityRequest::StorageGet(s) => {
                CallResult::Ok(kept.borrow().get(&s.key).cloned().unwrap_or(Value::Null))
            }
            _ => CallResult::Ok(Value::Null),
        });
        (host, storage)
    }

    const CONNECTION: &str = "cn_01JD7Q0000000000000000000A";

    fn params(reference: Option<&str>, change_request: Option<i64>) -> KeyParams {
        KeyParams {
            connection: CONNECTION.into(),
            repository: "gl:4711".into(),
            reference: reference.map(str::to_owned),
            change_request,
        }
    }

    fn args(reference: Option<&str>, change_request: Option<i64>) -> Value {
        let mut a = json!({"connection": CONNECTION, "repository": "gl:4711"});
        if let Some(r) = reference {
            a["ref"] = json!(r);
        }
        if let Some(n) = change_request {
            a["changeRequest"] = json!(n);
        }
        a
    }

    #[test]
    fn a_key_shows_what_the_last_run_found() {
        let plugin = crate::plugin();
        for (state, indicator) in [
            ("success", Indicator::Ok),
            ("failed", Indicator::Error),
            ("running", Indicator::Running),
            ("pending", Indicator::Running),
            ("manual", Indicator::Warning),
            ("cancelled", Indicator::Warning),
            ("skipped", Indicator::Unknown),
            ("none", Indicator::Unknown),
        ] {
            let (mut host, _) = storage_host();
            let p = params(Some("main"), None);
            remember(
                &mut host.context(),
                &p,
                &Ok(Output::message("x").with("state", state)),
            );
            let StateOutcome::Ok(shown) =
                host.key_state(&plugin, "pipeline-status", args(Some("main"), None))
            else {
                panic!("{state}")
            };
            assert_eq!(
                shown,
                KeyState::new().indicator(indicator).detail("main"),
                "{state}"
            );
        }

        // A change request is the second line; another key is another
        // state; a key that never ran shows its own design.
        let (mut host, storage) = storage_host();
        remember(
            &mut host.context(),
            &params(None, Some(12)),
            &Ok(Output::message("x").with("state", "success")),
        );
        let StateOutcome::Ok(shown) =
            host.key_state(&plugin, "pipeline-status", args(None, Some(12)))
        else {
            panic!()
        };
        assert_eq!(
            shown,
            KeyState::new().indicator(Indicator::Ok).detail("#12")
        );
        let StateOutcome::Ok(other) =
            host.key_state(&plugin, "pipeline-status", args(Some("dev"), None))
        else {
            panic!()
        };
        assert_eq!(other, KeyState::new());
        // Bounded keys, no server text stored.
        for key in storage.borrow().keys() {
            assert!(
                key.len() <= 128 && key.starts_with("keyState.pipeline."),
                "{key}"
            );
        }
        let long = params(Some(&"a".repeat(250)), None);
        assert!(long.storage_key().len() <= 128);
        assert_ne!(long.storage_key(), params(Some("a"), None).storage_key());

        // A failed run: the key shows the failure's code with the
        // plugin's own message; the server's text is never stored.
        let (mut host, storage) = storage_host();
        remember(
            &mut host.context(),
            &params(Some("main"), None),
            &Err(PluginError::new(
                "unauthorized",
                "the server said: secret-ish text",
            )),
        );
        let kept = serde_json::to_string(&*storage.borrow()).unwrap();
        assert!(!kept.contains("secret-ish"), "{kept}");
        assert!(matches!(
            host.key_state(&plugin, "pipeline-status", args(Some("main"), None)),
            StateOutcome::Error(f) if f.code == "unauthorized" && !f.message.contains("secret-ish")
        ));
    }

    #[test]
    fn only_a_key_or_gesture_run_stores_a_key_state() {
        let ok = Ok(Output::message("x").with("state", "success"));
        for (run, stores) in [
            (json!({"trigger": "key"}), true),
            (json!({"trigger": "gesture"}), true),
            (json!({"trigger": "workflow", "cause": "user"}), false),
            (json!({"trigger": "workflow", "cause": "schedule"}), false),
            (json!({"trigger": "app"}), false),
            (json!({"trigger": "test"}), false),
            (json!({"trigger": "something-new"}), false),
        ] {
            let storage = Rc::new(RefCell::new(BTreeMap::new()));
            let kept = Rc::clone(&storage);
            let answer = run.clone();
            let mut host = MockHost::new(move |req| match req {
                CapabilityRequest::RunContext(_) => CallResult::Ok(answer.clone()),
                CapabilityRequest::StorageSet(s) => {
                    kept.borrow_mut().insert(s.key.clone(), s.value.clone());
                    CallResult::Ok(Value::Null)
                }
                _ => CallResult::Ok(Value::Null),
            });
            // Many dynamic refs from a workflow: nothing grows.
            for n in 0..20 {
                let reference = format!("feature-{n}");
                remember_for_key(&mut host.context(), &params(Some(&reference), None), &ok);
            }
            let count = storage.borrow().len();
            assert_eq!(count, if stores { 20 } else { 0 }, "{run}");
        }
    }

    #[test]
    fn the_state_call_only_reads() {
        let plugin = crate::plugin();
        let (mut host, _) = storage_host();
        let _ = host.key_state(&plugin, "pipeline-status", args(Some("main"), None));
        // The reads a state call may make (the ABI's `allowed_in_state`).
        assert!(
            host.requests().iter().all(|r| matches!(
                r,
                CapabilityRequest::StorageGet(_)
                    | CapabilityRequest::StorageKeys(_)
                    | CapabilityRequest::SettingsGet(_)
                    | CapabilityRequest::TimeNow(_)
                    | CapabilityRequest::RunContext(_)
                    | CapabilityRequest::Log(_)
            )),
            "{:?}",
            host.requests()
        );
        assert!(!host.requests().is_empty());
    }
}
