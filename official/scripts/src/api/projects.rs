//! `deckpilot.projects` and `deckpilot.repositories` (§4.4; grant
//! `projects.read`, and `repositories.paths` for `Repository.path`). The
//! agent answers for the **active profile** only and maps repositories to
//! their machine bindings itself; a path is `null` without
//! `repositories.paths`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{invalid, parse, to_value};
use crate::bridge::ScriptFault;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct List {}

pub fn list(args: Value) -> Result<Value, ScriptFault> {
    let _: List = parse("project.list", args)?;
    Ok(serde_json::json!({}))
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Repositories {
    /// A project id; `null` is the run's current project.
    #[serde(default)]
    project: Option<String>,
}

pub fn repositories(args: Value) -> Result<Value, ScriptFault> {
    let a: Repositories = parse("project.repositories", args)?;
    if let Some(id) = &a.project
        && (id.is_empty()
            || id.len() > 64
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'))
    {
        return Err(invalid("project.repositories: `project` is a project id"));
    }
    to_value(&a)
}
