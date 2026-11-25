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
//!   maxBytes, offset, length}` (at most [`READ_CHUNK`] bytes each),
//!   answered with `{content, bytes, size, eof, version?}`. `version` is
//!   absent only when the first chunk already reaches the end. Every chunk
//!   after the first sends the first reply's `version` back as
//!   `ifVersion`, and the agent serves it from the state of the file the
//!   first chunk saw, refusing with `fileChanged` when the file was
//!   replaced, or its size or modification time changed, in between. The guest checks too: every reply names the same version
//!   and the same size, or the read fails with `fileChanged`. So a script
//!   gets the bytes of one version of the file, or `fileChanged`; never a
//!   splice of two.
//! * `writeFile` of at most [`WRITE_CHUNK`] bytes is one `fs.writeFile
//!   {path, content, encoding, createOnly}`. A larger file is
//!   `fs.write.open {path, createOnly}` → `{handle}`, then
//!   `fs.write.chunk {handle, content, encoding: "base64"}` (at most
//!   [`WRITE_CHUNK`] bytes each), then `fs.write.commit {handle}`. The
//!   agent writes a temporary file in the same directory and renames it
//!   into place on commit. Any failure after the open sends
//!   `fs.write.abort {handle}` (also after a Stop), and the agent drops
//!   the temporary file at the run's end in any case.
//!
//! Chunks travel as base64: a chunk of text cut at a byte offset may split
//! a character, and JSON escaping could grow 32 KiB of text past the
//! request cap (a control character is six bytes).
//!
//! The request shapes are the host's
//! (`tests/fixtures/script-requests.json`, DeckPilot's
//! `plugins/runtime/crates/abi`).

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
                if !is_canonical_absolute(p) {
                    return Err(invalid(format!(
                        "{what}: `{p}` is not a canonical absolute path (no `.`, `..` or empty \
                         names, no stream, no control or bidirectional character); use \
                         deckpilot.path.normalize"
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

/// A character no path or command holds: a control character or a
/// bidirectional formatting character (which could make a path in a
/// confirmation read as another). The host's `is_hidden_char`.
pub fn is_hidden_char(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061c}'
                | '\u{200b}'..='\u{200f}'
                | '\u{2028}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
                | '\u{feff}'
        )
}

fn is_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.chars().any(is_hidden_char)
}

/// The canonical absolute form the host accepts (its `is_absolute_path`):
/// `/a/b`, `C:\\a\\b` or `C:/a/b`, or UNC `\\\\server\\share\\a`; never
/// verbatim or device, without `.`, `..` or empty names (a trailing
/// separator is fine), a stream or a hidden character. Separators are
/// `/` and `\\` on every host, as on the host's side.
pub fn is_canonical_absolute(path: &str) -> bool {
    let b = path.as_bytes();
    let rest = if let Some(rest) = path.strip_prefix('/') {
        rest
    } else if b.len() >= 3
        && b[0].is_ascii_alphabetic()
        && b[1] == b':'
        && matches!(b[2], b'\\' | b'/')
    {
        &path[3..]
    } else if let Some(unc) = path.strip_prefix("\\\\") {
        if unc.starts_with(['?', '.']) {
            return false;
        }
        unc
    } else {
        return false;
    };
    if rest.contains(':') {
        return false;
    }
    let rest = rest.strip_suffix(['/', '\\']).unwrap_or(rest);
    rest.is_empty() || rest.split(['/', '\\']).all(is_name)
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
    if relative.chars().any(is_hidden_char) {
        return Err(ScriptFault {
            repository: Some(key.to_owned()),
            ..invalid(format!(
                "{what}: a path never holds a control or bidirectional formatting character"
            ))
        });
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

/// One `fs.readFile` chunk request. `ifVersion` is on every chunk after
/// the first (the host refuses a chunk past offset 0 without it).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReadChunk<'a> {
    path: &'a PathArg,
    encoding: Encoding,
    max_bytes: u64,
    offset: u64,
    length: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    if_version: Option<&'a str>,
}

/// One `fs.readFile` reply: `content` holds `bytes` bytes from the
/// request's offset, `size` is the whole file's, `eof` whether they reach
/// its end, and `version` the read's (the host's `FsRead`).
#[derive(Debug, Deserialize)]
struct ReadReply {
    content: String,
    #[serde(default)]
    bytes: Option<u64>,
    size: u64,
    eof: bool,
    /// Absent when the first chunk already reaches the end; otherwise a
    /// string (anything else is `invalidResponse`).
    #[serde(default)]
    version: Option<Value>,
}

/// The longest read version the host accepts back (`MAX_READ_VERSION`;
/// the agent's are 32 hex digits).
pub const MAX_READ_VERSION: usize = 64;

/// A version the host would accept as `ifVersion` (its `is_opaque`):
/// 1 to [`MAX_READ_VERSION`] printable ASCII bytes, no space.
pub fn valid_read_version(v: &str) -> bool {
    !v.is_empty() && v.len() <= MAX_READ_VERSION && v.bytes().all(|b| b.is_ascii_graphic())
}

/// The path as a script wrote it, for a message.
fn shown(path: &PathArg) -> String {
    match (&path.path, &path.repository) {
        (Some(p), _) => format!("`{p}`"),
        (None, Some(key)) => match path.relative.as_deref() {
            None | Some("") => format!("the root of repository \"{key}\""),
            Some(r) => format!("`{r}` in repository \"{key}\""),
        },
        (None, None) => "the file".to_owned(),
    }
}

/// `fileChanged`: the file changed while it was being read (the agent
/// found it replaced or rewritten, or the guest saw another version or
/// size), so nothing of it is returned.
fn changed(path: &PathArg) -> ScriptFault {
    ScriptFault {
        repository: path.repository.clone(),
        path: path.path.clone(),
        ..ScriptFault::new(
            "fileChanged",
            format!(
                "fs.readFile: {} changed while it was being read; read it again",
                shown(path)
            ),
        )
    }
}

/// The agent's own `fileChanged` refusal of a later chunk, with the
/// guest's message (naming the path as the script gave it) and the
/// agent's `path` when it sent one.
fn host_changed(fault: ScriptFault, path: &PathArg) -> ScriptFault {
    let mut out = changed(path);
    if fault.path.is_some() && out.path.is_none() && out.repository.is_none() {
        out.path = fault.path;
    }
    out
}

/// `invalidResponse`: the agent's `fs.readFile` reply breaks the version
/// contract (no version on a part before the end, or a malformed one).
fn invalid_response(why: &str) -> ScriptFault {
    ScriptFault::new(
        "invalidResponse",
        format!("malformed reply to fs.readFile: {why}"),
    )
}

fn malformed(op: &str) -> ScriptFault {
    ScriptFault::new("hostError", format!("malformed reply to {op}"))
}

/// `readFile`: the chunks, in order, of one version of the file. The
/// first reply's `version` and `size` are the read's; every later chunk
/// names that version (`ifVersion`), and the agent refuses it with
/// `fileChanged` when the file was replaced or rewritten meanwhile. Only
/// a reply that reaches the end may omit the version (a file read in one
/// chunk); one before the end without it, or a malformed version, is
/// `invalidResponse` and nothing more is sent. A reply with another
/// version or size, a chunk that runs past the size or is
/// longer than asked, or `eof` anywhere but exactly at the end, means the
/// file changed in between: the read fails with `fileChanged` and returns
/// nothing. A chunk may be shorter than asked (the agent may shorten
/// one); the next starts where it ended.
pub fn read(session: &mut Session, args: Value, flavor: Flavor) -> Result<Value, ScriptFault> {
    let a = read_args(args, flavor)?;
    let max = a.max_bytes.unwrap_or(MAX_FILE);
    let mut bytes: Vec<u8> = Vec::new();
    let mut size: Option<u64> = None;
    let mut version: Option<String> = None;
    loop {
        let offset = bytes.len() as u64;
        let length = match size {
            None => READ_CHUNK.min(max),
            Some(size) => READ_CHUNK.min(size - offset),
        };
        let request = ReadChunk {
            path: &a.path,
            encoding: Encoding::Base64,
            max_bytes: max,
            offset,
            length,
            if_version: version.as_deref(),
        };
        let reply =
            session
                .call("fs.readFile", to_value(&request)?)
                .map_err(|fault| match fault.code.as_str() {
                    "fileChanged" => host_changed(fault, &a.path),
                    _ => fault,
                })?;
        let reply: ReadReply =
            serde_json::from_value(reply).map_err(|_| malformed("fs.readFile"))?;
        let named = match reply.version {
            None | Some(Value::Null) => None,
            Some(Value::String(v)) if valid_read_version(&v) => Some(v),
            Some(_) => {
                return Err(invalid_response("its version is not one the agent issues"));
            }
        };
        match (&version, named) {
            // Without a version the agent can serve no later chunk: only a
            // reply that reaches the end may omit it (a file read in one
            // chunk).
            (_, None) if !reply.eof => {
                return Err(invalid_response(
                    "a part before the end of the file names no version",
                ));
            }
            (_, None) => {}
            (None, Some(v)) => version = Some(v),
            (Some(first), Some(v)) if *first != v => return Err(changed(&a.path)),
            (Some(_), Some(_)) => {}
        }
        let chunk = base64::decode(&reply.content).ok_or_else(|| malformed("fs.readFile"))?;
        if reply.bytes.is_some_and(|n| n != chunk.len() as u64) {
            return Err(malformed("fs.readFile"));
        }
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
                // The later chunks must fit the run's call budget.
                let chunks = reply.size.saturating_sub(length).div_ceil(READ_CHUNK);
                session.reserve(u32::try_from(chunks).unwrap_or(u32::MAX))?;
                size = Some(reply.size);
                reply.size
            }
            Some(size) if size != reply.size => return Err(changed(&a.path)),
            Some(size) => size,
        };
        let end = offset + chunk.len() as u64;
        if chunk.len() as u64 > length || end > total || reply.eof != (end == total) {
            return Err(changed(&a.path));
        }
        bytes.extend_from_slice(&chunk);
        if reply.eof {
            break;
        }
        if chunk.is_empty() {
            return Err(ScriptFault::new(
                "hostError",
                "fs.readFile: the agent sent an empty chunk before the end of the file",
            ));
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
    create_only: bool,
}

/// An agent's handle, opaque: only its shape is checked (the host's
/// `MAX_WRITE_HANDLE`).
pub fn handle_of(reply: &Value) -> Option<String> {
    let h = reply.get("handle")?.as_str()?;
    (!h.is_empty() && h.len() <= MAX_HANDLE && h.bytes().all(|b| b.is_ascii_graphic()))
        .then(|| h.to_owned())
}

/// The longest write handle.
pub const MAX_HANDLE: usize = 64;

/// `writeFile`. A file of at most [`WRITE_CHUNK`] bytes whose request fits
/// is one `fs.writeFile` in the script's encoding. A larger one (or text
/// that JSON escapes past the request cap) is open, base64 chunks,
/// commit: it starts only when every call it may need fits the run's
/// budget (open, the chunks, commit and an abort), and once the agent
/// holds a handle, any failure aborts it.
pub fn write(session: &mut Session, args: Value, flavor: Flavor) -> Result<Value, ScriptFault> {
    let (a, bytes) = write_args(args, flavor)?;
    if bytes.len() <= WRITE_CHUNK {
        let content = match a.encoding {
            Encoding::Utf8 => a.content.clone(),
            Encoding::Base64 => base64::encode(&bytes),
        };
        let single = serde_json::json!({
            "path": a.path, "content": content, "encoding": a.encoding, "createOnly": a.create_only,
        });
        // The envelope adds `{"args":…,"op":"fs.writeFile"}`.
        if single.to_string().len() + 32 <= crate::bridge::limits::REQUEST {
            session.call("fs.writeFile", single)?;
            return Ok(serde_json::json!({}));
        }
    }
    let chunks = bytes.len().div_ceil(WRITE_CHUNK);
    session.reserve(u32::try_from(chunks + 3).unwrap_or(u32::MAX))?;
    let open = WriteOpen {
        path: &a.path,
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
                serde_json::json!({ "handle": handle, "content": base64::encode(chunk), "encoding": "base64" }),
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
