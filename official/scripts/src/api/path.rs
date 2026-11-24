//! `deckpilot.path` (§4.4): pure, lexical path functions with the **host
//! OS's** semantics, like Node's `path.posix` / `path.win32`, but without a
//! working directory (`resolve` needs an absolute base). No grant; nothing
//! here touches the file system, and nothing here decides access: the
//! agent resolves every path an fs or process op receives itself (§5.3).
//!
//! The flavor comes from the agent (`script.load`'s `platform`), never from
//! the guest's own build target, so the same module gives Windows
//! semantics on Windows. macOS is POSIX with case-insensitive comparison
//! (`relative`), like its default volumes; Windows compares
//! case-insensitively and accepts both separators.

/// The path semantics of one host OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    Linux,
    MacOs,
    Windows,
}

impl Flavor {
    pub fn from_platform(platform: &str) -> Option<Self> {
        match platform {
            "linux" => Some(Self::Linux),
            "macos" => Some(Self::MacOs),
            "windows" => Some(Self::Windows),
            _ => None,
        }
    }

    pub fn platform(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::MacOs => "macos",
            Self::Windows => "windows",
        }
    }

    pub fn sep(self) -> char {
        if self == Self::Windows { '\\' } else { '/' }
    }

    fn is_sep(self, c: char) -> bool {
        c == '/' || (self == Self::Windows && c == '\\')
    }

    fn case_insensitive(self) -> bool {
        self != Self::Linux
    }
}

/// The root of a path: `""` (relative), `/` (POSIX), and on Windows `\`
/// (rooted, no drive), `C:` (drive-relative), `C:\` or `\\server\share\`.
/// `absolute` is whether the root ends in a separator.
#[derive(Debug, Clone, PartialEq)]
struct Root {
    root: String,
    absolute: bool,
}

fn split_root(f: Flavor, p: &str) -> (Root, &str) {
    let chars: Vec<char> = p.chars().take(3).collect();
    if f != Flavor::Windows {
        if p.starts_with('/') {
            return (
                Root {
                    root: "/".into(),
                    absolute: true,
                },
                p.trim_start_matches('/'),
            );
        }
        return (
            Root {
                root: String::new(),
                absolute: false,
            },
            p,
        );
    }
    // UNC: two separators, a server, a separator, a share.
    if chars.len() >= 2 && f.is_sep(chars[0]) && f.is_sep(chars[1]) {
        let rest = &p[2..];
        let mut parts = rest.splitn(3, |c| f.is_sep(c));
        let server = parts.next().unwrap_or("");
        let share = parts.next().unwrap_or("");
        if !server.is_empty() && !share.is_empty() {
            let tail = parts.next().unwrap_or("");
            return (
                Root {
                    root: format!("\\\\{server}\\{share}\\"),
                    absolute: true,
                },
                tail.trim_start_matches(|c| f.is_sep(c)),
            );
        }
        return (
            Root {
                root: "\\".into(),
                absolute: true,
            },
            p.trim_start_matches(|c| f.is_sep(c)),
        );
    }
    if chars.len() >= 2 && chars[0].is_ascii_alphabetic() && chars[1] == ':' {
        let drive = format!("{}:", chars[0].to_ascii_uppercase());
        let rest = &p[2..];
        if rest.starts_with(|c| f.is_sep(c)) {
            return (
                Root {
                    root: format!("{drive}\\"),
                    absolute: true,
                },
                rest.trim_start_matches(|c| f.is_sep(c)),
            );
        }
        return (
            Root {
                root: drive,
                absolute: false,
            },
            rest,
        );
    }
    if p.starts_with(|c| f.is_sep(c)) {
        return (
            Root {
                root: "\\".into(),
                absolute: true,
            },
            p.trim_start_matches(|c| f.is_sep(c)),
        );
    }
    (
        Root {
            root: String::new(),
            absolute: false,
        },
        p,
    )
}

/// Like Node's `isAbsolute`: on Windows a rooted path (`\x`) counts.
pub fn is_absolute(f: Flavor, p: &str) -> bool {
    split_root(f, p).0.absolute
}

/// An absolute path that names one location on its own: on Windows a
/// drive (`C:\`) or UNC root, not a rooted `\x` (whose drive is implicit).
pub fn is_fully_qualified(f: Flavor, p: &str) -> bool {
    let (root, _) = split_root(f, p);
    root.absolute && (f != Flavor::Windows || root.root.len() > 1)
}

fn segments(f: Flavor, rest: &str, absolute: bool) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in rest.split(|c| f.is_sep(c)) {
        match s {
            "" | "." => {}
            ".." => {
                if out.last().is_some_and(|l| l != "..") {
                    out.pop();
                } else if !absolute {
                    out.push("..".into());
                }
            }
            s => out.push(s.to_owned()),
        }
    }
    out
}

pub fn normalize(f: Flavor, p: &str) -> String {
    if p.is_empty() {
        return ".".into();
    }
    let (root, rest) = split_root(f, p);
    let trailing = p.ends_with(|c| f.is_sep(c));
    let segs = segments(f, rest, root.absolute);
    let sep = f.sep().to_string();
    let mut out = root.root.clone();
    out.push_str(&segs.join(&sep));
    if segs.is_empty() {
        if root.root.is_empty() {
            return ".".into();
        }
        if !root.absolute {
            out.push('.');
        }
        return out;
    }
    if trailing {
        out.push(f.sep());
    }
    out
}

pub fn join(f: Flavor, parts: &[String]) -> String {
    let joined: Vec<&str> = parts
        .iter()
        .map(String::as_str)
        .filter(|s| !s.is_empty())
        .collect();
    if joined.is_empty() {
        return ".".into();
    }
    normalize(f, &joined.join(&f.sep().to_string()))
}

/// `resolve(base, ...parts)`: the parts applied right to left onto an
/// absolute base, normalized, without a trailing separator. A relative
/// base is refused (there is no working directory).
pub fn resolve(f: Flavor, base: &str, parts: &[String]) -> Result<String, String> {
    if !is_fully_qualified(f, base) {
        return Err(format!(
            "resolve: the base `{base}` is not an absolute path"
        ));
    }
    let mut current = base.to_owned();
    for p in parts.iter().filter(|p| !p.is_empty()) {
        let (root, _) = split_root(f, p);
        let (cur_root, _) = split_root(f, &current);
        if is_fully_qualified(f, p) {
            current = p.clone();
        } else if root.absolute {
            // Rooted without a drive (`\x` on Windows): on the base's drive.
            let drive = cur_root.root.trim_end_matches('\\').to_owned();
            current = format!("{drive}{p}");
        } else if !root.root.is_empty() {
            // Drive-relative `D:x`: only onto the same drive.
            if !eq(f, &cur_root.root[..2.min(cur_root.root.len())], &root.root) {
                return Err(format!(
                    "resolve: `{p}` is relative to another drive's working directory"
                ));
            }
            current = format!("{current}{}{}", f.sep(), &p[2..]);
        } else {
            current = format!("{current}{}{p}", f.sep());
        }
    }
    let out = normalize(f, &current);
    let (root, _) = split_root(f, &out);
    Ok(if out.len() > root.root.len() {
        out.trim_end_matches(|c| f.is_sep(c)).to_owned()
    } else {
        out
    })
}

fn trim_trailing<'a>(f: Flavor, rest: &'a str) -> &'a str {
    rest.trim_end_matches(|c| f.is_sep(c))
}

pub fn dirname(f: Flavor, p: &str) -> String {
    let (root, rest) = split_root(f, p);
    let rest = trim_trailing(f, rest);
    match rest.rfind(|c| f.is_sep(c)) {
        None => {
            if root.root.is_empty() {
                ".".into()
            } else {
                root.root
            }
        }
        Some(i) => {
            let dir = trim_trailing(f, &rest[..i]);
            format!("{}{dir}", root.root)
        }
    }
}

pub fn basename(f: Flavor, p: &str, ext: Option<&str>) -> String {
    let (_, rest) = split_root(f, p);
    let rest = trim_trailing(f, rest);
    let base = rest.rsplit(|c| f.is_sep(c)).next().unwrap_or("");
    match ext {
        Some(e) if !e.is_empty() && base != e && base.ends_with(e) => {
            base[..base.len() - e.len()].to_owned()
        }
        _ => base.to_owned(),
    }
}

pub fn extname(f: Flavor, p: &str) -> String {
    let base = basename(f, p, None);
    match base.rfind('.') {
        Some(0) | None => String::new(),
        Some(i) => base[i..].to_owned(),
    }
}

fn eq(f: Flavor, a: &str, b: &str) -> bool {
    if f.case_insensitive() {
        a.to_lowercase() == b.to_lowercase()
    } else {
        a == b
    }
}

/// The relative path from `from` to `to`, both absolute. Different roots
/// (drives, shares) have none: `to` itself.
pub fn relative(f: Flavor, from: &str, to: &str) -> Result<String, String> {
    if !is_fully_qualified(f, from) || !is_fully_qualified(f, to) {
        return Err("relative: both paths must be absolute".into());
    }
    let (from_root, from_rest) = split_root(f, from);
    let (to_root, to_rest) = split_root(f, to);
    let a = segments(f, from_rest, true);
    let b = segments(f, to_rest, true);
    if !eq(f, &from_root.root, &to_root.root) {
        return Ok(normalize(f, to)
            .trim_end_matches(|c| f.is_sep(c))
            .to_owned());
    }
    let common = a.iter().zip(&b).take_while(|(x, y)| eq(f, x, y)).count();
    let mut out: Vec<String> = vec!["..".into(); a.len() - common];
    out.extend(b[common..].iter().cloned());
    Ok(out.join(&f.sep().to_string()))
}

pub fn to_posix(p: &str) -> String {
    p.replace('\\', "/")
}

pub fn from_posix(f: Flavor, p: &str) -> String {
    if f == Flavor::Windows {
        p.replace('/', "\\")
    } else {
        p.to_owned()
    }
}

/// One `deckpilot.path` call from the prelude: `op` and its string
/// arguments; the result is a string or a boolean.
pub fn call(f: Flavor, op: &str, args: &[String]) -> Result<serde_json::Value, String> {
    use serde_json::Value;
    let one = || args.first().map(String::as_str).unwrap_or("");
    Ok(match op {
        "join" => Value::String(join(f, args)),
        "normalize" => Value::String(normalize(f, one())),
        "resolve" => Value::String(resolve(f, one(), args.get(1..).unwrap_or(&[]))?),
        "dirname" => Value::String(dirname(f, one())),
        "basename" => Value::String(basename(f, one(), args.get(1).map(String::as_str))),
        "extname" => Value::String(extname(f, one())),
        "relative" => Value::String(relative(
            f,
            one(),
            args.get(1).map(String::as_str).unwrap_or(""),
        )?),
        "isAbsolute" => Value::Bool(is_absolute(f, one())),
        "toPosix" => Value::String(to_posix(one())),
        "fromPosix" => Value::String(from_posix(f, one())),
        _ => return Err(format!("path.{op} does not exist")),
    })
}

#[cfg(test)]
mod tests;
