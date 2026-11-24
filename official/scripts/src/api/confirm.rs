//! `script.confirm` and `script.sleep`.
//!
//! **Destructive operations** (§5.3 item 7): with the grant's
//! `destructive: "ask"`, the agent answers an `fs.remove` (recursive),
//! `fs.removeContents`, or an `fs.copy`/`fs.move` with `overwrite`, with
//! `needsConfirmation` and an opaque token for the plan **it** computed.
//! The prelude then sends `script.confirm {token}`: the agent shows its own
//! stored plan (counts and a sample) in a dialogs 1.5 confirm, only in a
//! run the user started, and answers `{confirmed}`. Only then does the
//! prelude repeat the op with `confirmation: token`; the agent recomputes
//! the plan and refuses if it grew. The guest never supplies the text the
//! user sees, so a script cannot misdescribe what it deletes.
//!
//! `script.sleep {ms}` waits in the agent, so a Stop ends it at once.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{invalid, parse, to_value};
use crate::bridge::ScriptFault;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Confirm {
    token: String,
}

pub fn confirm(args: Value) -> Result<Value, ScriptFault> {
    let a: Confirm = parse("script.confirm", args)?;
    if a.token.is_empty() || a.token.len() > 128 || !a.token.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(invalid("script.confirm: not a token the agent issued"));
    }
    to_value(&a)
}

/// The longest single sleep; a longer one is several calls (each
/// cancellable), and the run's own deadline bounds them all.
pub const MAX_SLEEP_MS: u64 = 600_000;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Sleep {
    ms: u64,
}

pub fn sleep(args: Value) -> Result<Value, ScriptFault> {
    let a: Sleep = parse("script.sleep", args)?;
    if a.ms > MAX_SLEEP_MS {
        return Err(invalid(format!("sleep: at most {MAX_SLEEP_MS} ms")));
    }
    to_value(&a)
}
