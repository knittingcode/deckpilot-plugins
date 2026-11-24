//! `deckpilot.process.run` and `deckpilot.shell.run` (§5.3 "Process").
//!
//! The request is structured: a command name, an argument **array**, never
//! a command line. There is no shell here, no glob and no variable
//! expansion; `shell.run` is a separate op under a separate, high-tier
//! grant, with the script passed as one value. The agent resolves the
//! command, applies the grant's `cwd` and `env` rules, the environment
//! denylist and `BASE_ENVIRONMENT`, and runs it in its own process group.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::fs::PathArg;
use super::path::Flavor;
use super::{invalid, parse, to_value};
use crate::bridge::ScriptFault;

pub const MAX_ARGS: usize = 256;
pub const MAX_ARG: usize = 32 * 1024;
pub const MAX_ARGS_TOTAL: usize = 256 * 1024;
pub const MAX_STDIN: usize = 1024 * 1024;
pub const MAX_ENV: usize = 64;
pub const MAX_ENV_VALUE: usize = 32 * 1024;
pub const MAX_COMMAND: usize = 4096;
pub const MAX_SHELL_SCRIPT: usize = 256 * 1024;
pub const SHELLS: &[&str] = &["sh", "bash", "zsh", "pwsh", "powershell", "cmd"];

/// Environment names a script may never set, whatever its grant (§5.3,
/// OD-7). The agent applies the same list; the guest refuses early.
pub fn env_denied(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    const PREFIXES: &[&str] = &[
        "LD_",
        "DYLD_",
        "DECKPILOT_",
        "GIT_CONFIG",
        "GIT_SSH",
        "PYTHON",
    ];
    const EXACT: &[&str] = &["GIT_EXEC_PATH", "DOCKER_HOST", "NODE_OPTIONS", "COMSPEC"];
    PREFIXES.iter().any(|p| upper.starts_with(p)) || EXACT.contains(&upper.as_str())
}

fn env_name_ok(name: &str) -> bool {
    let b = name.as_bytes();
    !b.is_empty()
        && b.len() <= 128
        && (b[0].is_ascii_alphabetic() || b[0] == b'_')
        && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_')
}

fn check_env(env: &BTreeMap<String, String>) -> Result<(), ScriptFault> {
    if env.len() > MAX_ENV {
        return Err(invalid(format!("env: at most {MAX_ENV} variables")));
    }
    for (name, value) in env {
        if !env_name_ok(name) {
            return Err(invalid(format!(
                "env: `{name}` is not a variable name ([A-Za-z_][A-Za-z0-9_]*)"
            )));
        }
        if env_denied(name) {
            return Err(ScriptFault::new(
                "notGranted",
                format!("env: {name} may never be set by a script"),
            ));
        }
        if value.contains('\0') || value.len() > MAX_ENV_VALUE {
            return Err(invalid(format!(
                "env: the value of {name} is invalid or too long"
            )));
        }
    }
    Ok(())
}

/// A bare command name (`git`, `docker`, `make`) or an absolute path the
/// user picked when granting it; never a command line.
fn check_command(command: &str, flavor: Flavor) -> Result<(), ScriptFault> {
    if command.is_empty() || command.len() > MAX_COMMAND || command.contains('\0') {
        return Err(invalid(
            "process.run: `command` is a command name or an absolute path",
        ));
    }
    if super::path::is_fully_qualified(flavor, command) {
        return Ok(());
    }
    if command.contains(['/', '\\'])
        || command.chars().any(|c| c.is_whitespace() || c.is_control())
        || command.starts_with('-')
    {
        return Err(invalid(format!(
            "process.run: `{command}` is not a command name; put arguments in `args` (there is \
             no shell)"
        )));
    }
    Ok(())
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Run {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathArg>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdin: Option<String>,
}

pub fn run(args: Value, flavor: Flavor) -> Result<Value, ScriptFault> {
    let mut a: Run = parse("process.run", args)?;
    check_command(&a.command, flavor)?;
    if a.args.len() > MAX_ARGS {
        return Err(invalid(format!(
            "process.run: at most {MAX_ARGS} arguments"
        )));
    }
    let mut total = 0;
    for arg in &a.args {
        if arg.contains('\0') {
            return Err(invalid("process.run: an argument never contains NUL"));
        }
        if arg.len() > MAX_ARG {
            return Err(invalid(format!(
                "process.run: an argument is longer than {MAX_ARG} bytes"
            )));
        }
        total += arg.len();
    }
    if total > MAX_ARGS_TOTAL {
        return Err(invalid(format!(
            "process.run: the arguments are longer than {MAX_ARGS_TOTAL} bytes in total"
        )));
    }
    if let Some(stdin) = &a.stdin
        && stdin.len() > MAX_STDIN
    {
        return Err(invalid(format!(
            "process.run: stdin is longer than {MAX_STDIN} bytes"
        )));
    }
    check_env(&a.env)?;
    timeout(a.timeout_ms)?;
    a.cwd = a.cwd.map(|c| c.checked("cwd", flavor)).transpose()?;
    to_value(&a)
}

fn timeout(t: Option<u64>) -> Result<(), ScriptFault> {
    match t {
        Some(0) => Err(invalid("timeoutMs is at least 1")),
        Some(t) if t > crate::bridge::limits::TIMEOUT_MS_MAX => Err(invalid(format!(
            "timeoutMs is at most {}",
            crate::bridge::limits::TIMEOUT_MS_MAX
        ))),
        _ => Ok(()),
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ShellRun {
    shell: String,
    script: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cwd: Option<PathArg>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    timeout_ms: Option<u64>,
}

pub fn shell_run(args: Value, flavor: Flavor) -> Result<Value, ScriptFault> {
    let mut a: ShellRun = parse("shell.run", args)?;
    if !SHELLS.contains(&a.shell.as_str()) {
        return Err(invalid(format!(
            "shell.run: `{}` is not one of {}",
            a.shell,
            SHELLS.join(", ")
        )));
    }
    if a.script.contains('\0') || a.script.len() > MAX_SHELL_SCRIPT {
        return Err(invalid(format!(
            "shell.run: the script is invalid or longer than {MAX_SHELL_SCRIPT} bytes"
        )));
    }
    check_env(&a.env)?;
    timeout(a.timeout_ms)?;
    a.cwd = a.cwd.map(|c| c.checked("cwd", flavor)).transpose()?;
    to_value(&a)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NoArgs {}

pub fn no_args(args: Value) -> Result<Value, ScriptFault> {
    let _: NoArgs = parse("arguments", args)?;
    Ok(serde_json::json!({}))
}
