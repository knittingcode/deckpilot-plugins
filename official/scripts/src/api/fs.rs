//! `deckpilot.fs` (§4.4, §5.3). Paths are [`PathArg`]s: an absolute path
//! (usable only with `repositories.paths` or a directory scope) or a
//! repository-relative reference. The agent resolves every one against the
//! script's granted scopes with handle-relative operations; the guest only
//! refuses what can never be valid (a relative string, NUL, a `..` that
//! leaves its repository, oversized values).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::path::{self, Flavor};
use super::{invalid, parse, to_value, valid_repository_key};
use crate::bridge::ScriptFault;

/// Read and write size limit (bytes of content).
pub const MAX_FILE: u64 = 4 * 1024 * 1024;
/// The longest path accepted, in bytes.
pub const MAX_PATH: usize = 4096;
/// The most `keep` entries of `removeContents`.
pub const MAX_KEEP: usize = 256;

/// A path as the bridge carries it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathArg {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relative: Option<String>,
}

impl PathArg {
    /// Checks the form and normalizes a repository-relative part to `/`
    /// separators without `.` segments.
    pub fn checked(mut self, what: &str, flavor: Flavor) -> Result<Self, ScriptFault> {
        match (&self.path, &self.repository) {
            (Some(p), None) => {
                if self.relative.is_some() {
                    return Err(invalid(format!(
                        "{what}: an absolute path has no `relative` part"
                    )));
                }
                plain(what, p)?;
                if !path::is_fully_qualified(flavor, p) {
                    return Err(invalid(format!(
                        "{what}: `{p}` is a relative path; there is no working directory, use \
                         repo.join(...) or an absolute path"
                    )));
                }
                Ok(self)
            }
            (None, Some(key)) => {
                if !valid_repository_key(key) {
                    return Err(invalid(format!("{what}: `{key}` is not a repository key")));
                }
                let relative = self.relative.take().unwrap_or_default();
                plain(what, &relative)?;
                self.relative = Some(repository_relative(what, key, &relative)?);
                Ok(self)
            }
            _ => Err(invalid(format!(
                "{what}: a path is an absolute string, a repository or repo.join(...)"
            ))),
        }
    }
}

fn plain(what: &str, s: &str) -> Result<(), ScriptFault> {
    if s.contains('\0') {
        return Err(invalid(format!("{what}: a path never contains NUL")));
    }
    if s.len() > MAX_PATH {
        return Err(invalid(format!(
            "{what}: the path is longer than {MAX_PATH} bytes"
        )));
    }
    Ok(())
}

/// The relative part of a repository reference: `/` or `\` separated,
/// never absolute, never escaping the repository root, no drive, stream
/// or device syntax (`:`), normalized to `a/b/c` (`""` is the root).
pub fn repository_relative(what: &str, key: &str, relative: &str) -> Result<String, ScriptFault> {
    let escape = || ScriptFault {
        repository: Some(key.to_owned()),
        ..invalid(format!(
            "{what}: `{relative}` is not a path inside repository \"{key}\""
        ))
    };
    if relative.starts_with('/') || relative.starts_with('\\') || relative.contains(':') {
        return Err(escape());
    }
    let mut parts: Vec<&str> = Vec::new();
    for segment in relative.split(['/', '\\']) {
        match segment {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(escape());
                }
            }
            s => parts.push(s),
        }
    }
    Ok(parts.join("/"))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OnePath {
    path: PathArg,
}

pub fn one_path(args: Value, flavor: Flavor) -> Result<Value, ScriptFault> {
    let a: OnePath = parse("path", args)?;
    Ok(serde_json::json!({ "path": a.path.checked("path", flavor)? }))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct List {
    path: PathArg,
    #[serde(default)]
    recursive: bool,
}

pub fn list(args: Value, flavor: Flavor) -> Result<Value, ScriptFault> {
    let a: List = parse("fs.list", args)?;
    if a.recursive {
        return Err(invalid(
            "fs.list: `recursive` is not supported in this version",
        ));
    }
    Ok(serde_json::json!({ "path": a.path.checked("path", flavor)? }))
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Mkdir {
    path: PathArg,
    #[serde(default)]
    recursive: bool,
}

pub fn mkdir(args: Value, flavor: Flavor) -> Result<Value, ScriptFault> {
    let mut a: Mkdir = parse("fs.mkdir", args)?;
    a.path = a.path.checked("path", flavor)?;
    to_value(&a)
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Encoding {
    #[default]
    Utf8,
    Base64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ReadFile {
    path: PathArg,
    #[serde(default)]
    encoding: Encoding,
    #[serde(default)]
    max_bytes: Option<u64>,
}

pub fn read_file(args: Value, flavor: Flavor) -> Result<Value, ScriptFault> {
    let mut a: ReadFile = parse("fs.readFile", args)?;
    a.path = a.path.checked("path", flavor)?;
    let max = a.max_bytes.unwrap_or(MAX_FILE);
    if max == 0 || max > MAX_FILE {
        return Err(invalid(format!("fs.readFile: maxBytes is 1 to {MAX_FILE}")));
    }
    a.max_bytes = Some(max);
    to_value(&a)
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WriteFile {
    path: PathArg,
    content: String,
    #[serde(default)]
    encoding: Encoding,
    #[serde(default)]
    create_only: bool,
}

pub fn write_file(args: Value, flavor: Flavor) -> Result<Value, ScriptFault> {
    let mut a: WriteFile = parse("fs.writeFile", args)?;
    a.path = a.path.checked("path", flavor)?;
    let bytes = match a.encoding {
        Encoding::Utf8 => a.content.len() as u64,
        Encoding::Base64 => {
            if !a
                .content
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
                || !a.content.len().is_multiple_of(4)
            {
                return Err(invalid("fs.writeFile: the content is not base64"));
            }
            a.content.len() as u64 / 4 * 3
        }
    };
    if bytes > MAX_FILE {
        return Err(ScriptFault::new(
            "tooLarge",
            format!("fs.writeFile: the content is more than {MAX_FILE} bytes"),
        ));
    }
    to_value(&a)
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Copy {
    src: PathArg,
    dst: PathArg,
    #[serde(default)]
    recursive: bool,
    #[serde(default)]
    overwrite: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    confirmation: Option<String>,
}

pub fn copy(args: Value, flavor: Flavor) -> Result<Value, ScriptFault> {
    let mut a: Copy = parse("fs.copy", args)?;
    a.src = a.src.checked("src", flavor)?;
    a.dst = a.dst.checked("dst", flavor)?;
    token(a.confirmation.as_deref())?;
    to_value(&a)
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Move {
    src: PathArg,
    dst: PathArg,
    #[serde(default)]
    overwrite: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    confirmation: Option<String>,
}

pub fn move_(args: Value, flavor: Flavor) -> Result<Value, ScriptFault> {
    let mut a: Move = parse("fs.move", args)?;
    a.src = a.src.checked("src", flavor)?;
    a.dst = a.dst.checked("dst", flavor)?;
    token(a.confirmation.as_deref())?;
    to_value(&a)
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Remove {
    path: PathArg,
    #[serde(default)]
    recursive: bool,
    #[serde(default)]
    dry_run: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    confirmation: Option<String>,
}

pub fn remove(args: Value, flavor: Flavor) -> Result<Value, ScriptFault> {
    let mut a: Remove = parse("fs.remove", args)?;
    a.path = a.path.checked("path", flavor)?;
    token(a.confirmation.as_deref())?;
    to_value(&a)
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RemoveContents {
    path: PathArg,
    #[serde(default)]
    dry_run: bool,
    #[serde(default)]
    keep: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    confirmation: Option<String>,
}

pub fn remove_contents(args: Value, flavor: Flavor) -> Result<Value, ScriptFault> {
    let mut a: RemoveContents = parse("fs.removeContents", args)?;
    a.path = a.path.checked("path", flavor)?;
    if a.keep.len() > MAX_KEEP {
        return Err(invalid(format!(
            "fs.removeContents: at most {MAX_KEEP} `keep` names"
        )));
    }
    for name in &a.keep {
        // A name directly inside the directory: never a path.
        if name.is_empty()
            || name == "."
            || name == ".."
            || name.contains(['/', '\\', '\0', ':'])
            || name.len() > 255
        {
            return Err(invalid(format!(
                "fs.removeContents: `keep` holds names inside the directory, not `{name}`"
            )));
        }
    }
    token(a.confirmation.as_deref())?;
    to_value(&a)
}

/// A confirmation token is the agent's, opaque; only its shape is checked.
fn token(t: Option<&str>) -> Result<(), ScriptFault> {
    match t {
        Some(t) if t.is_empty() || t.len() > 128 || !t.bytes().all(|b| b.is_ascii_graphic()) => {
            Err(invalid(
                "the confirmation token is not one the agent issued",
            ))
        }
        _ => Ok(()),
    }
}
