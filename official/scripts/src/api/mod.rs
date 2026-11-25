//! The marshallers of the JS API (§4.4): for each bridge op, the arguments
//! the prelude passes are parsed into a typed request (unknown fields
//! refused), checked against the bounds of §5.3, and sent as canonical
//! JSON. The agent checks every one of them again; these exist so that a
//! script gets a precise `invalidArgument` before any host call, and so
//! that the wire format is defined (and tested) in one place.
//!
//! The prelude reaches them only through [`dispatch`], which knows exactly
//! the ops below: a script cannot name another op.

pub mod base64;
pub mod confirm;
pub mod fs;
pub mod log;
pub mod path;
pub mod process;
pub mod projects;

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::bridge::{ScriptFault, Session};
pub use path::Flavor;

/// The ops a script can reach through the prelude (besides `script.load`,
/// `script.poll` and `script.log`, which the runtime sends itself).
/// `fs.readFile` and `fs.writeFile` are the script's; on the wire they are
/// chunks ([`WIRE_OPS`], `fs`).
pub const OPS: &[&str] = &[
    "fs.copy",
    "fs.exists",
    "fs.list",
    "fs.mkdir",
    "fs.move",
    "fs.readFile",
    "fs.remove",
    "fs.removeContents",
    "fs.stat",
    "fs.writeFile",
    "process.run",
    "project.list",
    "project.repositories",
    "script.confirm",
    "script.sleep",
    "shell.available",
    "shell.run",
];

/// Every op the guest sends to the agent: the P6 bridge, the host's
/// `scripts::OPS` (`tests/fixtures/script-requests.json`, the host's own
/// fixture, has a request of each).
pub const WIRE_OPS: &[&str] = &[
    "script.load",
    "script.poll",
    "script.log",
    "script.sleep",
    "script.confirm",
    "project.list",
    "project.repositories",
    "fs.exists",
    "fs.stat",
    "fs.list",
    "fs.mkdir",
    "fs.readFile",
    "fs.writeFile",
    "fs.write.open",
    "fs.write.chunk",
    "fs.write.commit",
    "fs.write.abort",
    "fs.copy",
    "fs.move",
    "fs.remove",
    "fs.removeContents",
    "process.run",
    "shell.run",
    "shell.available",
];

/// Runs one op of the prelude's: validated by [`dispatch`], then sent, as
/// one request or, for `fs.readFile` and `fs.writeFile`, as chunks.
pub fn perform(
    session: &mut Session,
    op: &str,
    args: Value,
    flavor: Flavor,
) -> Result<Value, ScriptFault> {
    match op {
        "fs.readFile" => fs::read(session, args, flavor),
        "fs.writeFile" => fs::write(session, args, flavor),
        // The agent's answer, reduced to the one boolean the prelude reads.
        "script.confirm" => {
            let args = dispatch(op, args, flavor)?;
            let v = session.call(op, args)?;
            Ok(serde_json::json!({ "confirmed": v.get("confirmed") == Some(&Value::Bool(true)) }))
        }
        // A sleep never outlasts the run either.
        "script.sleep" => {
            let mut args = dispatch(op, args, flavor)?;
            if session.budget.cancelled {
                return Err(session.stopped());
            }
            let remaining = session.budget.remaining_now();
            if remaining == Some(0) {
                return Err(session.out_of_time());
            }
            if let (Some(r), Some(ms)) = (remaining, args.get("ms").and_then(Value::as_u64)) {
                args["ms"] = Value::from(ms.min(r));
            }
            session.call(op, args)
        }
        "process.run" | "shell.run" => {
            let mut args = dispatch(op, args, flavor)?;
            if session.budget.cancelled {
                return Err(session.stopped());
            }
            let remaining = session.budget.remaining_now();
            if remaining == Some(0) {
                return Err(session.out_of_time());
            }
            process::clamp_timeout(&mut args, remaining);
            session.call(op, args)
        }
        _ => {
            let args = dispatch(op, args, flavor)?;
            session.call(op, args)
        }
    }
}

/// Validates `args` for `op` and returns the canonical request arguments
/// (for `fs.readFile` and `fs.writeFile`, the script's, before chunking).
pub fn dispatch(op: &str, args: Value, flavor: Flavor) -> Result<Value, ScriptFault> {
    match op {
        "fs.exists" | "fs.stat" => fs::one_path(args, flavor),
        "fs.list" => fs::list(args, flavor),
        "fs.mkdir" => fs::mkdir(args, flavor),
        "fs.readFile" => fs::read_file(args, flavor),
        "fs.writeFile" => fs::write_file(args, flavor),
        "fs.copy" => fs::copy(args, flavor),
        "fs.move" => fs::move_(args, flavor),
        "fs.remove" => fs::remove(args, flavor),
        "fs.removeContents" => fs::remove_contents(args, flavor),
        "process.run" => process::run(args, flavor),
        "shell.run" => process::shell_run(args, flavor),
        "shell.available" => process::no_args(args),
        "project.list" => projects::list(args),
        "project.repositories" => projects::repositories(args),
        "script.confirm" => confirm::confirm(args),
        "script.sleep" => confirm::sleep(args),
        _ => Err(invalid(format!("{op} is not an operation of deckpilot"))),
    }
}

pub(crate) fn invalid(message: impl Into<String>) -> ScriptFault {
    ScriptFault::new("invalidArgument", message)
}

pub(crate) fn parse<T: DeserializeOwned>(what: &str, args: Value) -> Result<T, ScriptFault> {
    serde_json::from_value(args).map_err(|e| invalid(format!("{what}: {e}")))
}

pub(crate) fn to_value<T: serde::Serialize>(request: &T) -> Result<Value, ScriptFault> {
    serde_json::to_value(request).map_err(|e| invalid(e.to_string()))
}

/// The repository key grammar (`RepositoryKey`, `^[a-z][a-z0-9_]{0,31}$`).
pub fn valid_repository_key(key: &str) -> bool {
    let bytes = key.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 32
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
}

#[cfg(test)]
mod tests;
