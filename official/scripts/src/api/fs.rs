//! `deckpilot.fs` (§4.4, §5.3). Paths are [`PathArg`]s: an absolute path
//! (usable only with `repositories.paths` or a directory scope) or a
//! repository-relative reference. The agent resolves every one against the
//! script's granted scopes with handle-relative operations; the guest only
//! refuses what can never be valid (a relative string, NUL, a `..` that
//! leaves its repository, oversized values, and on Windows the device,
//! verbatim, stream and reserved-name forms §5.3 refuses everywhere).
//!
//! **Chunked I/O** (P6 decision D1): every request fits the host's 64 KiB
//! and every reply its 1 MiB, so a file of up to [`MAX_FILE`] moves in
//! chunks, while the script sees one call:
//!
//! * `readFile` is a loop of `fs.readFile {path, encoding: "base64",
//!   offset, length}` (at most [`READ_CHUNK`] bytes each), answered with
//!   `{content, size, eof}`. A file whose size changes between chunks
//!   fails with `fileChanged`; two versions are never spliced together.
//! * `writeFile` is `fs.write.open {path, encoding: "base64", createOnly}`
//!   → `{handle}`, then `fs.write.chunk {handle, data}` (at most
//!   [`WRITE_CHUNK`] bytes of data each), then `fs.write.commit {handle}`.
//!   The agent writes a temporary file in the same directory and renames
//!   it into place on commit. Any failure after the open sends
//!   `fs.write.abort {handle}` (also after a Stop), and the agent drops
//!   the temporary file at the run's end in any case.
//!
//! The wire encoding is always base64: a chunk of text cut at a byte
//! offset may split a character, and JSON escaping could grow 32 KiB of
//! text past the request cap (a control character is six bytes).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::base64;
use super::path::{self, Flavor};
use super::{invalid, parse, to_value, valid_repository_key};
use crate::bridge::{ScriptFault, Session};

/// Read and write size limit (bytes of content).
pub const MAX_FILE: u64 = 4 * 1024 * 1024;
/// The most bytes one `fs.readFile` reply carries (its base64, about
/// 683 KiB, is under the host's 1 MiB reply cap).
pub const READ_CHUNK: u64 = 512 * 1024;
/// The most bytes of data one `fs.write.chunk` carries (its base64, about
/// 43 KiB, is under the host's 64 KiB request cap).
pub const WRITE_CHUNK: usize = 32 * 1024;
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
                if flavor == Flavor::Windows
                    && let Some(why) = windows_problem(p)
                {
                    return Err(invalid(format!("{what}: `{p}` is {why}")));
                }
                Ok(self)
            }
            (None, Some(key)) => {
                if !valid_repository_key(key) {
                    return Err(invalid(format!("{what}: `{key}` is not a repository key")));
                }
                let relative = self.relative.take().unwrap_or_default();
                plain(what, &relative)?;
                let relative = repository_relative(what, key, &relative)?;
                if flavor == Flavor::Windows
                    && let Some(name) = relative.split('/').find(|s| windows_reserved(s))
                {
                    return Err(ScriptFault {
                        repository: Some(key.clone()),
                        ..invalid(format!(
                            "{what}: `{name}` is a reserved device name on Windows"
                        ))
                    });
                }
                self.relative = Some(relative);
                Ok(self)
            }
            _ => Err(invalid(format!(
                "{what}: a path is an absolute string, a repository or repo.join(...)"
            ))),
        }
    }
}

/// A device name Windows reserves in every directory (`CON`, `nul.txt`,
/// `COM1`, …), whatever the extension.
pub fn windows_reserved(segment: &str) -> bool {
    let stem = segment
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches(' ');
    let upper = stem.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || (upper.len() == 4
        && (upper.starts_with("COM") || upper.starts_with("LPT"))
        && matches!(upper.as_bytes()[3], b'1'..=b'9'))
}

/// What §5.3 refuses in a fully qualified Windows path whatever the
/// grants: the device and verbatim namespaces (`\\.\`, `\\?\`), an
/// alternate data stream (a `:` after the drive) and a reserved device
/// name. A UNC path (`\\server\share\…`) passes: the agent allows it
/// when a scope is UNC.
pub fn windows_problem(p: &str) -> Option<&'static str> {
    let sep = |c: u8| c == b'/' || c == b'\\';
    let b = p.as_bytes();
    if b.len() >= 4 && sep(b[0]) && sep(b[1]) && (b[2] == b'?' || b[2] == b'.') && sep(b[3]) {
        return Some("a device or verbatim path (\\\\?\\ or \\\\.\\), which scripts never use");
    }
    let after_root = if b.len() >= 2 && b[1] == b':' {
        &p[2..]
    } else {
        p
    };
    if after_root.contains(':') {
        return Some("an alternate data stream (a `:` after the drive)");
    }
    if after_root.split(['/', '\\']).any(windows_reserved) {
        return Some("a reserved device name on Windows (CON, NUL, COM1, …)");
    }
    None
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

/// `readFile`'s arguments as the script gave them, checked (the wire
/// requests are [`read`]'s chunks).
pub fn read_file(args: Value, flavor: Flavor) -> Result<Value, ScriptFault> {
    to_value(&read_args(args, flavor)?)
}

fn read_args(args: Value, flavor: Flavor) -> Result<ReadFile, ScriptFault> {
    let mut a: ReadFile = parse("fs.readFile", args)?;
    a.path = a.path.checked("path", flavor)?;
    let max = a.max_bytes.unwrap_or(MAX_FILE);
    if max == 0 || max > MAX_FILE {
        return Err(invalid(format!("fs.readFile: maxBytes is 1 to {MAX_FILE}")));
    }
    a.max_bytes = Some(max);
    Ok(a)
}

/// One `fs.readFile` chunk request.
#[derive(Debug, Serialize)]
struct ReadChunk<'a> {
    path: &'a PathArg,
    encoding: Encoding,
    offset: u64,
    length: u64,
}

/// One `fs.readFile` reply.
#[derive(Debug, Deserialize)]
struct ReadReply {
    content: String,
    size: u64,
    eof: bool,
}

fn changed(path: &PathArg) -> ScriptFault {
    ScriptFault {
        repository: path.repository.clone(),
        path: path.path.clone(),
        ..ScriptFault::new(
            "fileChanged",
            "fs.readFile: the file changed while it was read; read it again",
        )
    }
}

fn malformed(op: &str) -> ScriptFault {
    ScriptFault::new("hostError", format!("malformed reply to {op}"))
}

/// `readFile`: the chunks, in order, of one size of the file. The first
/// reply's `size` is the file's; a reply with another size, or a chunk
/// that is not exactly the bytes from its offset to `min(offset + length,
/// size)` with `eof` exactly at the end, means the file changed in
/// between, and the read fails.
pub fn read(session: &mut Session, args: Value, flavor: Flavor) -> Result<Value, ScriptFault> {
    let a = read_args(args, flavor)?;
    let max = a.max_bytes.unwrap_or(MAX_FILE);
    let mut bytes: Vec<u8> = Vec::new();
    let mut size: Option<u64> = None;
    loop {
        let offset = bytes.len() as u64;
        let length = match size {
            None => READ_CHUNK.min(max),
            Some(size) => READ_CHUNK.min(size - offset),
        };
        let request = ReadChunk {
            path: &a.path,
            encoding: Encoding::Base64,
            offset,
            length,
        };
        let reply = session.call("fs.readFile", to_value(&request)?)?;
        let reply: ReadReply =
            serde_json::from_value(reply).map_err(|_| malformed("fs.readFile"))?;
        let chunk = base64::decode(&reply.content).ok_or_else(|| malformed("fs.readFile"))?;
        let total = match size {
            None => {
                if reply.size > max {
                    return Err(ScriptFault {
                        repository: a.path.repository.clone(),
                        path: a.path.path.clone(),
                        ..ScriptFault::new(
                            "tooLarge",
                            format!(
                                "fs.readFile: the file is {} bytes, more than {max}",
                                reply.size
                            ),
                        )
                    });
                }
                // Every later chunk must fit the run's call budget.
                let chunks = reply.size.saturating_sub(length).div_ceil(READ_CHUNK);
                session.reserve(u32::try_from(chunks).unwrap_or(u32::MAX))?;
                size = Some(reply.size);
                reply.size
            }
            Some(size) if size != reply.size => return Err(changed(&a.path)),
            Some(size) => size,
        };
        let want = length.min(total.saturating_sub(offset));
        let end = offset + chunk.len() as u64;
        if chunk.len() as u64 != want || reply.eof != (end == total) {
            return Err(changed(&a.path));
        }
        bytes.extend_from_slice(&chunk);
        if reply.eof {
            break;
        }
    }
    let content = match a.encoding {
        Encoding::Base64 => base64::encode(&bytes),
        Encoding::Utf8 => String::from_utf8(bytes).map_err(|_| ScriptFault {
            repository: a.path.repository.clone(),
            path: a.path.path.clone(),
            ..invalid(
                "fs.readFile: the file is not UTF-8 text; read it with { encoding: \"base64\" }",
            )
        })?,
    };
    Ok(serde_json::json!({ "content": content }))
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

/// `writeFile`'s arguments as the script gave them, checked (the wire
/// requests are [`write`]'s).
pub fn write_file(args: Value, flavor: Flavor) -> Result<Value, ScriptFault> {
    let (a, _) = write_args(args, flavor)?;
    to_value(&a)
}

fn write_args(args: Value, flavor: Flavor) -> Result<(WriteFile, Vec<u8>), ScriptFault> {
    let mut a: WriteFile = parse("fs.writeFile", args)?;
    a.path = a.path.checked("path", flavor)?;
    let bytes = match a.encoding {
        Encoding::Utf8 => a.content.as_bytes().to_vec(),
        Encoding::Base64 => base64::decode(&a.content)
            .ok_or_else(|| invalid("fs.writeFile: the content is not base64"))?,
    };
    if bytes.len() as u64 > MAX_FILE {
        return Err(ScriptFault::new(
            "tooLarge",
            format!("fs.writeFile: the content is more than {MAX_FILE} bytes"),
        ));
    }
    Ok((a, bytes))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WriteOpen<'a> {
    path: &'a PathArg,
    encoding: Encoding,
    create_only: bool,
}

/// An agent's handle, opaque: only its shape is checked.
fn handle_of(reply: &Value) -> Option<String> {
    let h = reply.get("handle")?.as_str()?;
    (!h.is_empty() && h.len() <= 128 && h.bytes().all(|b| b.is_ascii_graphic()))
        .then(|| h.to_owned())
}

/// `writeFile`: open, the chunks, commit. It starts only when every call
/// it may need fits the run's budget (open, the chunks, commit, and an
/// abort), and once the agent holds a handle, any failure aborts it.
pub fn write(session: &mut Session, args: Value, flavor: Flavor) -> Result<Value, ScriptFault> {
    let (a, bytes) = write_args(args, flavor)?;
    let chunks = bytes.len().div_ceil(WRITE_CHUNK);
    session.reserve(u32::try_from(chunks + 3).unwrap_or(u32::MAX))?;
    let open = WriteOpen {
        path: &a.path,
        encoding: Encoding::Base64,
        create_only: a.create_only,
    };
    let reply = session.call("fs.write.open", to_value(&open)?)?;
    let Some(handle) = handle_of(&reply) else {
        // No handle the guest can name: nothing to abort. The agent drops
        // whatever it opened at the run's end.
        return Err(malformed("fs.write.open"));
    };
    let written = (|| {
        for chunk in bytes.chunks(WRITE_CHUNK) {
            session.call(
                "fs.write.chunk",
                serde_json::json!({ "handle": handle, "data": base64::encode(chunk) }),
            )?;
        }
        session.call("fs.write.commit", serde_json::json!({ "handle": handle }))
    })();
    match written {
        Ok(_) => Ok(serde_json::json!({})),
        Err(fault) => {
            // Best effort: the fault the script sees is the write's.
            let _ = session.cleanup("fs.write.abort", serde_json::json!({ "handle": handle }));
            Err(fault)
        }
    }
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
