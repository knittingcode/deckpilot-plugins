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

#[test]
fn dot_dot_never_climbs_above_a_root() {
    for f in [Linux, MacOs] {
        assert_eq!(normalize(f, "/../../.."), "/");
        assert_eq!(normalize(f, "/a/../../b/./"), "/b/");
        // A relative path keeps the `..` it cannot resolve.
        assert_eq!(normalize(f, "../../x"), "../../x");
        assert_eq!(resolve(f, "/srv", &s(&["../../../etc"])).unwrap(), "/etc");
        assert_eq!(relative(f, "/a/b", "/").unwrap(), "../..");
    }
    let f = Windows;
    assert_eq!(normalize(f, "C:\\..\\..\\x"), "C:\\x");
    assert_eq!(
        normalize(f, "\\\\srv\\share\\..\\..\\x"),
        "\\\\srv\\share\\x"
    );
    assert_eq!(
        normalize(f, "C:..\\x"),
        "C:..\\x",
        "drive-relative keeps it"
    );
    assert_eq!(
        resolve(f, "C:\\work", &s(&["..\\..\\..\\Windows"])).unwrap(),
        "C:\\Windows"
    );
    assert_eq!(
        resolve(f, "\\\\srv\\share", &s(&["..\\other"])).unwrap(),
        "\\\\srv\\share\\other",
        "a UNC path never leaves its share"
    );
}

#[test]
fn drive_letters_and_unc_roots() {
    let f = Windows;
    // A drive letter is one root whatever its case.
    assert_eq!(normalize(f, "c:\\x"), "C:\\x");
    assert_eq!(relative(f, "c:\\a", "C:\\a\\b").unwrap(), "b");
    assert_eq!(dirname(f, "d:/x/y"), "D:\\x");
    assert!(is_fully_qualified(f, "z:\\"));
    assert!(!is_fully_qualified(f, "1:\\x"), "not a drive letter");
    assert!(!is_fully_qualified(f, "C:"), "drive-relative");
    // UNC needs a server and a share; `\\server` alone is only rooted.
    assert!(is_fully_qualified(f, "\\\\srv\\share\\x"));
    assert!(is_fully_qualified(f, "//srv/share"));
    assert!(!is_fully_qualified(f, "\\\\srv"));
    assert_eq!(normalize(f, "\\\\srv"), "\\srv");
    // Another share is another root, like another drive.
    assert_eq!(
        relative(f, "\\\\srv\\a\\x", "\\\\srv\\b\\y").unwrap(),
        "\\\\srv\\b\\y"
    );
    assert_eq!(
        relative(f, "\\\\SRV\\Share\\x", "\\\\srv\\share\\y").unwrap(),
        "..\\y"
    );
    // The verbatim and device forms are only syntax here (a server `?` or
    // `.`); fs and process ops refuse them (api::fs::windows_problem).
    assert!(is_fully_qualified(f, "\\\\?\\C:\\x"));
    // Drive syntax means nothing on POSIX.
    for p in [Linux, MacOs] {
        assert!(!is_fully_qualified(p, "C:\\x"));
        assert_eq!(normalize(p, "C:\\x\\..\\y"), "C:\\x\\..\\y");
        assert_eq!(normalize(p, "//srv/share"), "/srv/share");
    }
}

#[test]
fn separators_and_conversion_per_platform() {
    assert_eq!(
        join(Windows, &s(&["C:/a", "b\\c", "d/e"])),
        "C:\\a\\b\\c\\d\\e"
    );
    assert_eq!(join(Linux, &s(&["/a", "b\\c"])), "/a/b\\c");
    assert_eq!(basename(Windows, "C:\\a\\b/c.txt", None), "c.txt");
    assert_eq!(basename(Linux, "/a/b\\c.txt", None), "b\\c.txt");
    assert_eq!(from_posix(Windows, "/a/b"), "\\a\\b");
    assert_eq!(from_posix(MacOs, "/a/b"), "/a/b");
    assert_eq!(to_posix("\\\\srv\\share\\x"), "//srv/share/x");
    assert_eq!(
        call(Windows, "fromPosix", &s(&["C:/x/y"])).unwrap(),
        serde_json::json!("C:\\x\\y")
    );
}

#[test]
fn case_follows_the_host() {
    // Linux compares exactly; macOS (default volumes) and Windows do not.
    assert_eq!(relative(Linux, "/A/b", "/a/b/c").unwrap(), "../../a/b/c");
    assert_eq!(relative(MacOs, "/A/b", "/a/b/c").unwrap(), "c");
    assert_eq!(relative(Windows, "C:\\A\\B", "c:\\a\\b\\c").unwrap(), "c");
    // Case is kept as written: only comparisons ignore it.
    assert_eq!(normalize(MacOs, "/Users/Dev/./Repo"), "/Users/Dev/Repo");
    assert_eq!(join(Windows, &s(&["C:\\Work", "Repo"])), "C:\\Work\\Repo");
    // basename's extension is matched as written on every host, like Node.
    assert_eq!(basename(MacOs, "/a/b.TXT", Some(".txt")), "b.TXT");
}
