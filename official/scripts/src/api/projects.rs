//! `deckpilot.projects` and `deckpilot.repositories` (§4.4; grant
//! `projects.read`, and `repositories.paths` for `Repository.path`). The
//! agent answers for the **active profile** only and maps repositories to
//! their machine bindings itself; a path is `null` without
//! `repositories.paths`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{invalid, parse, to_value, valid_repository_key};
use crate::bridge::ScriptFault;

/// Remotes per repository (decision K1; the host's
/// `MAX_REMOTES_PER_REPOSITORY`).
pub const MAX_REMOTES: usize = 8;
/// A git remote name, in characters (the host's `MAX_REMOTE_NAME_CHARS`).
pub const MAX_REMOTE_NAME: usize = 64;
/// The optional members of a remote, besides `name` and `primary`
/// (the host's `scripts::RemoteInfo`).
const REMOTE_TEXT: &[&str] = &["url", "defaultBranch", "connectionId", "remoteRepositoryId"];

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

/// The agent's `project.repositories` reply, with every repository's
/// `remotes` (decision K4) in the guest's form: an array of at most
/// [`MAX_REMOTES`] objects of exactly `name`, `primary`, `url`,
/// `defaultBranch`, `connectionId` and `remoteRepositoryId` (an absent
/// or `null` text member is `null`; members the guest does not know are
/// dropped). The rest of the reply is the prelude's, as before.
///
/// - An agent before K4 sends no `remotes` (the host also omits an empty
///   list): `[]`. It is **not** derived from `remote`, which has neither
///   the name nor the ids (design §11 C1 says nothing of a derivation).
/// - Anything else is refused whole, `invalidResponse`, and no repository
///   is given to the script: `remotes` that is not an array, more than
///   [`MAX_REMOTES`], more than one primary, or an entry that is not an
///   object, or has no `name` (a non-empty string of at most
///   [`MAX_REMOTE_NAME`] characters), no boolean `primary`, or a text
///   member that is neither a string nor `null`. A remote silently
///   dropped could make another one look like the primary.
///
/// The refusal names the repository (only by a valid key) and the
/// member, never a value: no `connectionId` or `remoteRepositoryId`
/// reaches a message.
pub fn reply(mut v: Value) -> Result<Value, ScriptFault> {
    let Some(repositories) = v.get_mut("repositories").and_then(Value::as_array_mut) else {
        return Ok(v);
    };
    for repository in repositories {
        let Some(r) = repository.as_object_mut() else {
            continue;
        };
        let key = r
            .get("key")
            .and_then(Value::as_str)
            .filter(|k| valid_repository_key(k))
            .map(str::to_owned);
        let remotes = remotes(r.get("remotes")).map_err(|why| malformed(key.as_deref(), &why))?;
        r.insert("remotes".into(), Value::Array(remotes));
    }
    Ok(v)
}

fn remotes(given: Option<&Value>) -> Result<Vec<Value>, String> {
    let list = match given {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(list)) => list,
        Some(_) => return Err("`remotes` is not an array".into()),
    };
    if list.len() > MAX_REMOTES {
        return Err(format!("more than {MAX_REMOTES} remotes"));
    }
    let mut out = Vec::with_capacity(list.len());
    let mut primaries = 0;
    for (i, m) in list.iter().enumerate() {
        let n = i + 1;
        let Some(m) = m.as_object() else {
            return Err(format!("remote {n} is not an object"));
        };
        let name = m
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty() && s.chars().count() <= MAX_REMOTE_NAME)
            .ok_or_else(|| format!("remote {n} has no valid `name`"))?;
        let primary = m
            .get("primary")
            .and_then(Value::as_bool)
            .ok_or_else(|| format!("remote {n}: `primary` is not a boolean"))?;
        primaries += usize::from(primary);
        let mut remote = serde_json::Map::new();
        remote.insert("name".into(), Value::from(name));
        remote.insert("primary".into(), Value::Bool(primary));
        for &field in REMOTE_TEXT {
            let value = match m.get(field) {
                None | Some(Value::Null) => Value::Null,
                Some(Value::String(s)) => Value::String(s.clone()),
                Some(_) => return Err(format!("remote {n}: `{field}` is not a string or null")),
            };
            remote.insert(field.into(), value);
        }
        out.push(Value::Object(remote));
    }
    if primaries > 1 {
        return Err("more than one primary remote".into());
    }
    Ok(out)
}

fn malformed(key: Option<&str>, why: &str) -> ScriptFault {
    let mut f = ScriptFault::new(
        "invalidResponse",
        match key {
            Some(k) => {
                format!("malformed reply to project.repositories: {why} (repository \"{k}\")")
            }
            None => format!("malformed reply to project.repositories: {why}"),
        },
    );
    f.repository = key.map(str::to_owned);
    f
}
