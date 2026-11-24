//! `deckpilot.path` on all three host OSes. Pure and lexical, so every
//! flavor runs on every CI runner; the expected values follow Node's
//! `path.posix` and `path.win32` where `deckpilot.path` keeps their
//! semantics.

use super::Flavor::{Linux, MacOs, Windows};
use super::*;

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| (*x).to_owned()).collect()
}

#[test]
fn separators_follow_the_host() {
    assert_eq!(Linux.sep(), '/');
    assert_eq!(MacOs.sep(), '/');
    assert_eq!(Windows.sep(), '\\');
    assert_eq!(Flavor::from_platform("windows"), Some(Windows));
    assert_eq!(Flavor::from_platform("freebsd"), None);
}

#[test]
fn posix_normalize_and_join() {
    for f in [Linux, MacOs] {
        assert_eq!(normalize(f, "/a//b/./c/../d/"), "/a/b/d/");
        assert_eq!(normalize(f, "a/../../b"), "../b");
        assert_eq!(normalize(f, "/../x"), "/x");
        assert_eq!(normalize(f, ""), ".");
        assert_eq!(normalize(f, "./"), ".");
        assert_eq!(normalize(f, "/"), "/");
        // A backslash is an ordinary character on POSIX.
        assert_eq!(normalize(f, "a\\b/../c"), "c");
        assert_eq!(
            join(f, &s(&["/srv", "app", "../web", "index.js"])),
            "/srv/web/index.js"
        );
        assert_eq!(join(f, &s(&["", ""])), ".");
        assert_eq!(join(f, &s(&["a", "", "b/"])), "a/b/");
    }
}

#[test]
fn windows_normalize_and_join() {
    let f = Windows;
    assert_eq!(
        normalize(f, "C:/Users//dev/./repo/../app"),
        "C:\\Users\\dev\\app"
    );
    assert_eq!(normalize(f, "c:\\a\\..\\..\\b"), "C:\\b");
    assert_eq!(normalize(f, "C:"), "C:.");
    assert_eq!(normalize(f, "C:a\\..\\..\\b"), "C:..\\b");
    assert_eq!(
        normalize(f, "\\\\server\\share\\x\\..\\y"),
        "\\\\server\\share\\y"
    );
    assert_eq!(normalize(f, "//server/share/"), "\\\\server\\share\\");
    assert_eq!(normalize(f, "\\a/b\\c"), "\\a\\b\\c");
    // Mixed separators are one separator kind on Windows.
    assert_eq!(
        join(f, &s(&["C:\\work", "repo/src", "..\\lib"])),
        "C:\\work\\repo\\lib"
    );
}

#[test]
fn absolute_and_fully_qualified() {
    for f in [Linux, MacOs] {
        assert!(is_absolute(f, "/x"));
        assert!(!is_absolute(f, "x/y"));
        assert!(!is_absolute(f, "C:\\x"), "a drive is not POSIX syntax");
        assert!(is_fully_qualified(f, "/"));
    }
    let f = Windows;
    assert!(is_absolute(f, "C:\\x"));
    assert!(is_absolute(f, "c:/x"));
    assert!(is_absolute(f, "\\\\srv\\share\\x"));
    assert!(is_absolute(f, "\\x"), "rooted, like Node");
    assert!(!is_fully_qualified(f, "\\x"), "but its drive is implicit");
    assert!(!is_absolute(f, "C:x"), "drive-relative");
    assert!(!is_absolute(f, "x\\y"));
    assert!(is_fully_qualified(f, "C:\\"));
    assert!(is_fully_qualified(f, "\\\\srv\\share"));
}

#[test]
fn resolve_needs_an_absolute_base() {
    assert_eq!(
        resolve(Linux, "/home/dev/repo", &s(&["src", "../lib", "a.js"])).unwrap(),
        "/home/dev/repo/lib/a.js"
    );
    assert_eq!(resolve(Linux, "/a", &s(&["/b", "c/"])).unwrap(), "/b/c");
    assert_eq!(resolve(Linux, "/", &s(&[])).unwrap(), "/");
    assert!(resolve(Linux, "relative", &s(&["x"])).is_err());
    assert!(resolve(MacOs, "", &s(&["/x"])).is_err());

    let f = Windows;
    assert_eq!(
        resolve(f, "C:\\work", &s(&["repo", "..\\x"])).unwrap(),
        "C:\\work\\x"
    );
    assert_eq!(
        resolve(f, "C:\\work", &s(&["D:\\other"])).unwrap(),
        "D:\\other"
    );
    assert_eq!(resolve(f, "C:\\work", &s(&["\\root"])).unwrap(), "C:\\root");
    assert_eq!(
        resolve(f, "c:\\work", &s(&["C:sub"])).unwrap(),
        "C:\\work\\sub"
    );
    assert!(
        resolve(f, "C:\\work", &s(&["D:sub"])).is_err(),
        "no per-drive cwd"
    );
    assert!(resolve(f, "\\rooted", &s(&["x"])).is_err());
    assert_eq!(
        resolve(f, "\\\\srv\\share\\a", &s(&["..", "..", "b"])).unwrap(),
        "\\\\srv\\share\\b"
    );
}

#[test]
fn dirname_basename_extname() {
    for f in [Linux, MacOs] {
        assert_eq!(dirname(f, "/a/b/c.txt"), "/a/b");
        assert_eq!(dirname(f, "/a/b/"), "/a");
        assert_eq!(dirname(f, "/a"), "/");
        assert_eq!(dirname(f, "/"), "/");
        assert_eq!(dirname(f, "a"), ".");
        assert_eq!(basename(f, "/a/b/c.txt", None), "c.txt");
        assert_eq!(basename(f, "/a/b/c.txt", Some(".txt")), "c");
        assert_eq!(basename(f, "/a/b/", None), "b");
        assert_eq!(basename(f, ".txt", Some(".txt")), ".txt");
        assert_eq!(extname(f, "index.js"), ".js");
        assert_eq!(extname(f, "archive.tar.gz"), ".gz");
        assert_eq!(extname(f, ".env"), "");
        assert_eq!(extname(f, "Makefile"), "");
        assert_eq!(extname(f, "name."), ".");
    }
    let f = Windows;
    assert_eq!(dirname(f, "C:\\a\\b.txt"), "C:\\a");
    assert_eq!(dirname(f, "C:\\a"), "C:\\");
    assert_eq!(dirname(f, "C:a"), "C:");
    assert_eq!(dirname(f, "\\\\srv\\share\\x"), "\\\\srv\\share\\");
    assert_eq!(basename(f, "C:/a/b.TXT", Some(".TXT")), "b");
    assert_eq!(basename(f, "C:\\", None), "");
    assert_eq!(extname(f, "C:\\dir.d\\file"), "");
}

#[test]
fn relative_compares_case_like_the_host() {
    assert_eq!(relative(Linux, "/a/b/c", "/a/d").unwrap(), "../../d");
    assert_eq!(relative(Linux, "/a", "/a").unwrap(), "");
    assert_eq!(
        relative(Linux, "/Users/Dev", "/users/dev/x").unwrap(),
        "../../users/dev/x"
    );
    // macOS default volumes and Windows are case-insensitive.
    assert_eq!(relative(MacOs, "/Users/Dev", "/users/dev/x").unwrap(), "x");
    assert_eq!(
        relative(Windows, "C:\\Work\\Repo", "c:\\work\\repo\\src").unwrap(),
        "src"
    );
    assert_eq!(
        relative(Windows, "C:\\a\\b", "C:\\a\\c\\d").unwrap(),
        "..\\c\\d"
    );
    // Another drive has no relative path: the target itself.
    assert_eq!(relative(Windows, "C:\\a", "D:\\b\\").unwrap(), "D:\\b");
    assert!(relative(Linux, "a", "/b").is_err());
}

#[test]
fn posix_conversion() {
    assert_eq!(to_posix("C:\\a\\b"), "C:/a/b");
    assert_eq!(from_posix(Windows, "C:/a/b"), "C:\\a\\b");
    assert_eq!(from_posix(Linux, "a\\b/c"), "a\\b/c");
}

#[test]
fn the_call_table_names_every_function_and_nothing_else() {
    for op in [
        "join",
        "normalize",
        "resolve",
        "dirname",
        "basename",
        "extname",
        "relative",
        "isAbsolute",
        "toPosix",
        "fromPosix",
    ] {
        let r = call(Linux, op, &s(&["/a/b.c", "/a"]));
        assert!(r.is_ok(), "{op}: {r:?}");
    }
    assert!(call(Linux, "readFile", &s(&["/etc/passwd"])).is_err());
    assert_eq!(
        call(Windows, "isAbsolute", &s(&["C:\\x"])).unwrap(),
        serde_json::json!(true)
    );
}
