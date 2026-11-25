//! `deckpilot.scripts`: the first-party Scripts plugin ("Skrypty";
//! DeckPilot `design-projects-scripts.md` §4, P5). It owns the runtime of
//! user scripts: QuickJS-NG (rquickjs 0.14) compiled into this
//! `wasm32-unknown-unknown` module, which the agent runs in its
//! out-of-process wasm host like every plugin. The script's source, its
//! settings and its grants are the core's and the agent's; this module only
//! receives the source of the one script it is invoked for.
//!
//! Two actions:
//!
//! * `run {script, inputs?}`: `script.load` (the agent refuses an untrusted
//!   or missing script, and answers only for this invocation's own
//!   parameter), then the prelude ([`runtime`], `prelude.js`) and the
//!   script. Every effect is a bridge op ([`bridge`], [`api`]) the agent
//!   decides against the script's grants at the time of the call. The
//!   result is the script's completion value and the outcome of each
//!   repository of `forEachRepository`; a failure is a code and a message
//!   that names the repository and the command.
//! * `check {source}`: compiles a source **without running it** (the
//!   editor's diagnostics): `ok`, or `syntax-error` with the line and
//!   column. No bridge op.
//!
//! The bridge ops are served by the agent (P6); without them `run` fails
//! with `unsupported` and runs nothing. The manifest needs plugin API 1.13,
//! so this plugin is `held` in `official/plugins.json` and not released
//! (`../README.md`).
#![deny(unsafe_code)]
// ScriptFault (a script error's code, message, repository, command, path)
// is the error of every marshaller and bridge call; it is only built on
// the error path, so its size is not worth boxing everywhere.
#![allow(clippy::result_large_err)]

pub mod api;
pub mod bridge;
mod clock;
pub mod runtime;

use deckpilot_plugin_sdk::prelude::*;
use serde_json::{Value, json};

use api::Flavor;
use bridge::{HostCall, ScriptFault, Session, Transport, limits};
use runtime::{Ending, Report, RunSetup};

/// The plugin's id and version, as in `manifest.json`.
pub const ID: &str = "deckpilot.scripts";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The longest `inputs` JSON a run takes.
pub const MAX_INPUTS: u32 = 64 * 1024;

pub fn plugin() -> Plugin {
    Plugin::new(ID, VERSION)
        .action(
            Action::new("run", run_action)
                // A script id (the manifest's `{"type": "script"}` kind,
                // plugin API 1.13); the guest sees its value as text.
                .param(Param::text("script").required().max_length(64))
                .param(Param::text("inputs").max_length(MAX_INPUTS)),
        )
        .action(
            Action::new("check", check_action).param(
                Param::text("source")
                    .required()
                    .max_length(limits::SOURCE as u32),
            ),
        )
}

deckpilot_plugin_sdk::plugin!(plugin);

fn run_action(_ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let script: String = args.get("script")?;
    let inputs: Option<String> = args.get("inputs")?;
    run(Box::new(HostCall), &script, inputs.as_deref())
}

fn check_action(_ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let source: String = args.get("source")?;
    check(&source)
}

/// `check`: compile only.
pub fn check(source: &str) -> ActionResult {
    if source.len() > limits::SOURCE {
        return Err(PluginError::new(
            "too-large",
            "the script is larger than 256 KiB",
        ));
    }
    match runtime::check(source) {
        Ok(()) => Ok(Output::message("no syntax errors")),
        Err(e) => {
            let at = match (e.line, e.column) {
                (Some(l), Some(c)) => format!(" (line {l}, column {c})"),
                (Some(l), None) => format!(" (line {l})"),
                _ => String::new(),
            };
            Err(PluginError::new(
                "syntax-error",
                format!("{}{at}", e.message),
            ))
        }
    }
}

/// A script `ErrorCode` (`notGranted`) as an outcome code (`not-granted`).
pub fn outcome_code(code: &str) -> String {
    let mut out = String::with_capacity(code.len() + 4);
    for c in code.chars() {
        if c.is_ascii_uppercase() {
            out.push('-');
            out.push(c.to_ascii_lowercase());
        } else if c.is_ascii_alphanumeric() || c == '-' {
            out.push(c);
        }
    }
    let out = out
        .trim_start_matches(|c: char| !c.is_ascii_lowercase())
        .to_owned();
    if out.is_empty() {
        "script-error".into()
    } else {
        out
    }
}

fn failure(f: ScriptFault) -> PluginError {
    PluginError::new(outcome_code(&f.code), f.message.unwrap_or(f.code))
}

/// What `script.load` answers.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Loaded {
    source: String,
    platform: String,
    #[serde(default)]
    settings: Settings,
    #[serde(default)]
    grants: Value,
    #[serde(default)]
    run: Value,
    #[serde(default)]
    now_us: Option<f64>,
    /// The run's time left at the load (the host's `ScriptLoaded` always
    /// sends it): the run's deadline as the guest knows it, which clamps
    /// process, shell and sleep times until a poll says more. Only when it
    /// is absent (an agent before deckpilot #481) does the guest fall back
    /// to the script's `timeoutMs` setting, else the default timeout.
    #[serde(default)]
    remaining_ms: Option<u64>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Settings {
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    memory_mib: Option<u32>,
    #[serde(default)]
    stop_on_error: Option<bool>,
}

/// `run` over any transport (the host's, or a native test's mock).
pub fn run(transport: Box<dyn Transport>, script: &str, inputs: Option<&str>) -> ActionResult {
    run_with(transport, script, inputs, limits::INTERRUPTS)
}

fn run_with(
    transport: Box<dyn Transport>,
    script: &str,
    inputs: Option<&str>,
    max_interrupts: u64,
) -> ActionResult {
    let inputs: Value = match inputs.map(str::trim).filter(|s| !s.is_empty()) {
        None => Value::Null,
        Some(text) => serde_json::from_str(text).map_err(|e| {
            PluginError::new("invalid-parameters", format!("`inputs` is not JSON: {e}"))
        })?,
    };
    // A script id, as the core makes them (the host's `is_id`): never a
    // path or anything else to send.
    if script.is_empty()
        || script.len() > 64
        || !script
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(PluginError::new(
            "invalid-parameters",
            "`script` is not a script id",
        ));
    }
    let mut session = Session::new(transport);
    let loaded = session
        .call("script.load", json!({ "script": script }))
        .map_err(failure)?;
    let loaded: Loaded = serde_json::from_value(loaded)
        .map_err(|e| PluginError::new("host-error", format!("malformed script.load reply: {e}")))?;
    if loaded.source.len() > limits::SOURCE {
        return Err(PluginError::new(
            "too-large",
            "the script is larger than 256 KiB",
        ));
    }
    let Some(flavor) = Flavor::from_platform(&loaded.platform) else {
        return Err(PluginError::new(
            "host-error",
            format!("unknown platform `{}`", loaded.platform),
        ));
    };
    if let Some(us) = loaded.now_us {
        session.budget.clock_us = us;
    }
    clock::set(session.budget.clock_us);
    let remaining = loaded
        .remaining_ms
        .or(loaded.settings.timeout_ms)
        .unwrap_or(limits::TIMEOUT_MS_DEFAULT);
    session
        .budget
        .set_remaining(remaining.min(limits::TIMEOUT_MS_MAX));
    let mut run = match loaded.run {
        Value::Object(m) => Value::Object(m),
        _ => json!({}),
    };
    run["inputs"] = inputs;
    let setup = RunSetup {
        source: loaded.source,
        flavor,
        memory_mib: loaded
            .settings
            .memory_mib
            .unwrap_or(limits::MEMORY_MIB_DEFAULT),
        stop_on_error: loaded.settings.stop_on_error.unwrap_or(true),
        run,
        grants: loaded.grants,
        max_interrupts,
    };
    finish(runtime::run(session, setup))
}

/// The most bytes of the `outcomes` output (JSON). The run's reply has to
/// fit the host's reply cap whatever the script recorded.
pub const MAX_OUTCOMES_JSON: usize = 64 * 1024;

/// The outcomes as a JSON array of at most [`MAX_OUTCOMES_JSON`] bytes: as
/// many as fit, in order, then `{"truncated": true, "total": N}` when some
/// were left out.
pub fn outcomes_json(outcomes: &[runtime::Outcome]) -> String {
    let marker = |total: usize| json!({ "truncated": true, "total": total }).to_string();
    let mut parts: Vec<String> = Vec::new();
    let mut bytes = 2;
    let reserve = marker(outcomes.len()).len() + 1;
    for (i, o) in outcomes.iter().enumerate() {
        let item = serde_json::to_string(o).unwrap_or_else(|_| "null".into());
        let rest_fits = i + 1 == outcomes.len() && bytes + item.len() < MAX_OUTCOMES_JSON;
        if !rest_fits && bytes + item.len() + 1 + reserve > MAX_OUTCOMES_JSON {
            parts.push(marker(outcomes.len()));
            break;
        }
        bytes += item.len() + 1;
        parts.push(item);
    }
    format!("[{}]", parts.join(","))
}

fn finish(report: Report) -> ActionResult {
    let failed: Vec<&runtime::Outcome> = report.outcomes.iter().filter(|o| !o.ok).collect();
    let outcomes = outcomes_json(&report.outcomes);
    match report.ending {
        Ending::Failed(f) => Err(failure(f)),
        Ending::Done(_) if !failed.is_empty() => {
            // stopOnError off: every repository ran; the run still fails,
            // naming each one that did.
            let names: Vec<String> = failed
                .iter()
                .map(|o| {
                    o.error
                        .as_ref()
                        .and_then(|e| e.message.clone())
                        .unwrap_or_else(|| format!("repository \"{}\" failed", o.repository))
                })
                .collect();
            let text = format!(
                "{} of {} repositories failed: {}",
                failed.len(),
                report.outcomes.len(),
                names.join("; ")
            );
            Err(PluginError::new(
                "repository-failed",
                api::log::cut(&text, 4 * runtime::MAX_ERROR_TEXT),
            ))
        }
        Ending::Done(value) => {
            let mut out = Output::message("the script finished")
                .with("result", value.to_string())
                .with("outcomes", outcomes)
                .with("hostCalls", f64::from(report.host_calls))
                .with("logLines", f64::from(report.log_lines));
            if report.log_truncated {
                out = out.with("logTruncated", true);
            }
            Ok(out)
        }
    }
}

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_fixture;
#[cfg(test)]
mod tests_io;
#[cfg(test)]
mod tests_remotes;
