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
/// The arguments, the stdin and a shell script are each at most 48 KiB, so
/// one request stays under the host's 64 KiB (P6 decision D1); a larger
/// input goes through a file the script writes. The whole request is
/// checked against the 64 KiB as well (`bridge`), since JSON escaping and
/// the environment add to it.
pub const MAX_ARGS_TOTAL: usize = 48 * 1024;
pub const MAX_STDIN: usize = 48 * 1024;
pub const MAX_ENV: usize = 64;
pub const MAX_ENV_VALUE: usize = 32 * 1024;
pub const MAX_COMMAND: usize = 4096;
pub const MAX_SHELL_SCRIPT: usize = 48 * 1024;
pub const SHELLS: &[&str] = &["sh", "bash", "zsh", "pwsh", "powershell", "cmd"];

/// Environment names a script may never set, whatever its grant, compared
/// without case: exactly the host's `scripts::env_denied` (DeckPilot
/// `plugins/runtime/crates/abi/src/scripts.rs`), so the guest refuses
/// early what the agent refuses. Denied: what changes which program runs
/// or where it finds its configuration (`PATH`, `HOME`, `XDG_*`, every
/// `GIT_*` but the author and committer, `DOCKER_*` but BuildKit), what
/// loads code into a program (`LD_*`, `DYLD_*`, interpreter options and
/// paths, shell start-up files), what redirects the network or its trust
/// (proxies, certificate bundles, `SSH_*`), and DeckPilot's own.
pub fn env_denied(name: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "LD_",
        "DYLD_",
        "DECKPILOT_",
        "GIT_",
        "SSH_",
        "XDG_",
        "DOCKER_",
        "PYTHON",
        "PERL",
        "RUBY",
        "NPM_CONFIG_",
        "JAVA_",
        "_JAVA_",
        "JDK_JAVA_",
        "RUSTC",
        "RUSTDOC",
        "CARGO_",
        "SSL_CERT_",
        "LUA_",
        "TCL",
        "DOTNET_",
        "COR_",
        "CORECLR_",
        "PHP",
        "GEM_",
        "BUNDLE_",
        "PIP_",
        "AWS_",
        "ELECTRON_",
    ];
    const EXACT: &[&str] = &[
        "PATH",
        "PATHEXT",
        "HOME",
        "USERPROFILE",
        "HOMEDRIVE",
        "HOMEPATH",
        "APPDATA",
        "LOCALAPPDATA",
        "SYSTEMROOT",
        "WINDIR",
        "COMSPEC",
        "PSMODULEPATH",
        "SHELL",
        "BASH_ENV",
        "ENV",
        "ZDOTDIR",
        "IFS",
        "PS4",
        "PROMPT_COMMAND",
        "CDPATH",
        "TMPDIR",
        "TEMP",
        "TMP",
        "EDITOR",
        "VISUAL",
        "PAGER",
        "BROWSER",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "FTP_PROXY",
        "CURL_CA_BUNDLE",
        "REQUESTS_CA_BUNDLE",
        "NODE_OPTIONS",
        "NODE_PATH",
        "NODE_EXTRA_CA_CERTS",
        "NODE_TLS_REJECT_UNAUTHORIZED",
        "NODE_REPL_EXTERNAL_MODULE",
        "GCONV_PATH",
        "LOCPATH",
        "NLSPATH",
        "MALLOC_CONF",
        "HOSTALIASES",
        "RES_OPTIONS",
        "LOCALDOMAIN",
        "OPENSSL_CONF",
        "CLASSPATH",
        "GOFLAGS",
        "MAKEFLAGS",
        "MFLAGS",
        "LESSOPEN",
        "LESSCLOSE",
        "SHELLOPTS",
        "BASHOPTS",
        "KUBECONFIG",
    ];
    /// `GIT_*` names that only label a commit.
    const GIT_ALLOWED: &[&str] = &[
        "GIT_AUTHOR_NAME",
        "GIT_AUTHOR_EMAIL",
        "GIT_AUTHOR_DATE",
        "GIT_COMMITTER_NAME",
        "GIT_COMMITTER_EMAIL",
        "GIT_COMMITTER_DATE",
    ];
    let upper = name.to_ascii_uppercase();
    if GIT_ALLOWED.contains(&upper.as_str()) || upper == "DOCKER_BUILDKIT" {
        return false;
    }
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

/// A bare command name: `^[A-Za-z0-9][A-Za-z0-9._+-]*$`, at most 255
/// bytes, not ending in `.`, never a batch file (the host's
/// `is_command_name`).
pub fn is_command_name(name: &str) -> bool {
    let b = name.as_bytes();
    !b.is_empty()
        && b.len() <= 255
        && b[0].is_ascii_alphanumeric()
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'+' | b'-'))
        && !name.ends_with('.')
        && !is_batch(name)
}

/// A Windows batch file, by its extension (CVE-2024-24576): only
/// `shell.run` with `cmd` may start one.
pub fn is_batch(command: &str) -> bool {
    let lower = command.trim_end_matches([' ', '.']).to_ascii_lowercase();
    lower.ends_with(".bat") || lower.ends_with(".cmd")
}

/// A bare command name (`git`, `docker`, `make`) or an absolute path of a
/// drive or the Unix root that the user picked when granting it (never
/// UNC, never a batch file); never a command line.
fn check_command(command: &str, flavor: Flavor) -> Result<(), ScriptFault> {
    if command.is_empty() || command.len() > MAX_COMMAND || command.contains('\0') {
        return Err(invalid(
            "process.run: `command` is a command name or an absolute path",
        ));
    }
    if is_batch(command) {
        return Err(invalid(format!(
            "process.run: `{command}` is a batch file, which only shell.run with cmd may start"
        )));
    }
    if super::path::is_fully_qualified(flavor, command) {
        let unc = command.starts_with("\\\\") || command.starts_with("//");
        if unc || !super::fs::is_canonical_absolute(command) {
            return Err(invalid(format!(
                "process.run: `{command}` is not a command path the agent runs (a canonical \
                 path of a drive or the root, never UNC)"
            )));
        }
        return Ok(());
    }
    if !is_command_name(command) {
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

/// A process or shell timeout never outlasts the run: `timeoutMs` becomes
/// `min(timeoutMs, remaining)` with the remaining time the guest knows
/// (the agent applies the same rule with its own clock). Without a
/// `timeoutMs` the agent's default, the remaining time, applies.
pub fn clamp_timeout(args: &mut Value, remaining_ms: Option<u64>) {
    if let (Some(remaining), Some(t)) =
        (remaining_ms, args.get("timeoutMs").and_then(Value::as_u64))
    {
        args["timeoutMs"] = Value::from(t.min(remaining.max(1)));
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NoArgs {}

pub fn no_args(args: Value) -> Result<Value, ScriptFault> {
    let _: NoArgs = parse("arguments", args)?;
    Ok(serde_json::json!({}))
}
