//! The marshallers alone: what each op accepts, refuses and sends.

use serde_json::json;

use super::*;

const L: Flavor = Flavor::Linux;

#[test]
fn only_known_ops_are_dispatched() {
    for op in OPS {
        // Each known op is recognised (its arguments may still be refused).
        let r = dispatch(op, json!({}), L);
        if let Err(f) = &r {
            assert!(
                !f.message
                    .as_deref()
                    .unwrap_or("")
                    .contains("is not an operation"),
                "{op}"
            );
        }
    }
    for op in [
        "script.load",
        "script.poll",
        "tool.run",
        "credentials.has",
        "http.fetch",
        "fs.chmod",
        "",
    ] {
        let f = dispatch(op, json!({}), L).unwrap_err();
        assert_eq!(f.code, "invalidArgument", "{op}");
    }
}

#[test]
fn unknown_fields_are_refused() {
    let f = dispatch(
        "fs.exists",
        json!({ "path": { "path": "/a" }, "follow": true }),
        L,
    )
    .unwrap_err();
    assert_eq!(f.code, "invalidArgument");
    let f = dispatch("process.run", json!({ "command": "git", "shell": true }), L).unwrap_err();
    assert_eq!(f.code, "invalidArgument");
    let f = dispatch(
        "fs.exists",
        json!({ "path": { "path": "/a", "repository": "cms" } }),
        L,
    )
    .unwrap_err();
    assert_eq!(f.code, "invalidArgument");
}

#[test]
fn repository_keys_follow_the_core_grammar() {
    for ok in ["cms", "a", "web_2", "a234567890123456789012345678901"] {
        assert!(valid_repository_key(ok), "{ok}");
    }
    for bad in [
        "",
        "Cms",
        "2web",
        "a-b",
        "a.b",
        "../x",
        "a234567890123456789012345678901234",
    ] {
        assert!(!valid_repository_key(bad), "{bad}");
    }
}

#[test]
fn repository_relative_paths_are_normalized_and_contained() {
    use fs::repository_relative as rel;
    assert_eq!(rel("p", "cms", "a\\b/./c/").unwrap(), "a/b/c");
    assert_eq!(rel("p", "cms", "a/../b").unwrap(), "b");
    assert_eq!(rel("p", "cms", "").unwrap(), "");
    for bad in [
        "..",
        "a/../../b",
        "/etc",
        "\\x",
        "C:\\x",
        "file.txt:stream",
        "a/../..",
    ] {
        let f = rel("p", "cms", bad).unwrap_err();
        assert_eq!(f.repository.as_deref(), Some("cms"), "{bad}");
    }
}

#[test]
fn process_limits() {
    let many: Vec<String> = (0..=process::MAX_ARGS).map(|i| i.to_string()).collect();
    assert!(dispatch("process.run", json!({ "command": "git", "args": many }), L).is_err());
    let long = "x".repeat(process::MAX_ARG + 1);
    assert!(
        dispatch(
            "process.run",
            json!({ "command": "git", "args": [long] }),
            L
        )
        .is_err()
    );
    let stdin = "y".repeat(process::MAX_STDIN + 1);
    assert!(
        dispatch(
            "process.run",
            json!({ "command": "git", "stdin": stdin }),
            L
        )
        .is_err()
    );
    assert!(dispatch("process.run", json!({ "command": "-c" }), L).is_err());
    assert!(dispatch("process.run", json!({ "command": "/usr/bin/git" }), L).is_ok());
    assert!(
        dispatch(
            "process.run",
            json!({ "command": "C:\\Git\\git.exe" }),
            Flavor::Windows
        )
        .is_ok()
    );
    for name in [
        "LD_PRELOAD",
        "ld_preload",
        "DYLD_LIBRARY_PATH",
        "DECKPILOT_TOKEN",
        "GIT_CONFIG_GLOBAL",
        "GIT_SSH_COMMAND",
        "GIT_EXEC_PATH",
        "DOCKER_HOST",
        "NODE_OPTIONS",
        "PYTHONPATH",
        "ComSpec",
    ] {
        let f = dispatch(
            "process.run",
            json!({ "command": "git", "env": { name: "x" } }),
            L,
        )
        .unwrap_err();
        assert_eq!(f.code, "notGranted", "{name}");
    }
    assert!(
        dispatch(
            "process.run",
            json!({ "command": "git", "env": { "CI": "1", "_X1": "y" } }),
            L
        )
        .is_ok()
    );
}

#[test]
fn shells_are_a_closed_list() {
    assert!(
        dispatch(
            "shell.run",
            json!({ "shell": "bash", "script": "echo hi" }),
            L
        )
        .is_ok()
    );
    for bad in ["fish", "/bin/sh", "sh -c", ""] {
        assert!(
            dispatch("shell.run", json!({ "shell": bad, "script": "x" }), L).is_err(),
            "{bad}"
        );
    }
}

#[test]
fn read_and_write_bounds() {
    let p = json!({ "path": "/a" });
    assert!(dispatch("fs.readFile", json!({ "path": p, "maxBytes": 0 }), L).is_err());
    assert!(
        dispatch(
            "fs.readFile",
            json!({ "path": p, "maxBytes": fs::MAX_FILE + 1 }),
            L
        )
        .is_err()
    );
    assert_eq!(
        dispatch("fs.readFile", json!({ "path": p }), L).unwrap(),
        json!({ "path": { "path": "/a" }, "encoding": "utf8", "maxBytes": fs::MAX_FILE })
    );
    assert!(
        dispatch(
            "fs.writeFile",
            json!({ "path": p, "content": "not base64!", "encoding": "base64" }),
            L
        )
        .is_err()
    );
    assert!(
        dispatch(
            "fs.writeFile",
            json!({ "path": p, "content": "aGk=", "encoding": "base64" }),
            L
        )
        .is_ok()
    );
    assert!(dispatch("fs.list", json!({ "path": p, "recursive": true }), L).is_err());
}

#[test]
fn confirmation_tokens_and_sleep() {
    assert!(dispatch("script.confirm", json!({ "token": "tok-1" }), L).is_ok());
    assert!(dispatch("script.confirm", json!({ "token": "" }), L).is_err());
    assert!(dispatch("script.confirm", json!({ "token": "a b" }), L).is_err());
    assert!(
        dispatch(
            "fs.remove",
            json!({ "path": { "path": "/a" }, "confirmation": "x\ny" }),
            L
        )
        .is_err()
    );
    assert!(
        dispatch(
            "script.sleep",
            json!({ "ms": confirm::MAX_SLEEP_MS + 1 }),
            L
        )
        .is_err()
    );
    assert!(dispatch("project.repositories", json!({ "project": "../x" }), L).is_err());
    assert!(dispatch("project.repositories", json!({ "project": null }), L).is_ok());
}

/// The path argument of fs and process ops on each host OS: what passes
/// to the agent (which resolves it against the grants) and what is
/// refused before any host call.
#[test]
fn path_arguments_per_platform() {
    let arg = |f: Flavor, p: &str| dispatch("fs.exists", json!({ "path": { "path": p } }), f);
    for f in [Flavor::Linux, Flavor::MacOs] {
        for ok in ["/", "/home/dev/repo", "/home/dev/repo/", "/x/CON", "/x\\y"] {
            assert!(arg(f, ok).is_ok(), "{f:?} {ok}");
        }
        // The host's canonical form: no `.`, `..` or empty names, no
        // stream, no control or bidirectional character.
        for bad in [
            "relative",
            "C:\\x",
            "./x",
            "",
            "~/x",
            "/a\0b",
            "/a/../b",
            "/a/./b",
            "/a//b",
            "/a:b",
            "/x\\..\\y",
            "/tmp/a\u{1b}b",
            "/tmp/\u{202e}txt.exe",
            "/tmp/a\u{200b}b",
        ] {
            assert_eq!(
                arg(f, bad).unwrap_err().code,
                "invalidArgument",
                "{f:?} {bad}"
            );
        }
    }
    let w = Flavor::Windows;
    for ok in [
        "C:\\work\\repo",
        "c:/work/repo",
        "D:\\",
        "\\\\srv\\share\\repo",
        "C:\\work\\cms\\",
        "C:\\console\\nulx",
    ] {
        assert!(arg(w, ok).is_ok(), "{ok}");
    }
    for bad in [
        "/etc/hosts",
        "\\rooted",
        "C:relative",
        "relative",
        "\\\\srv",
        "\\\\?\\C:\\x",
        "\\\\.\\pipe\\x",
        "//?/C:/x",
        "\\\\.\\C:",
        "C:\\x\\file.txt:stream",
        "C:\\x\\file.txt::$DATA",
        "\\\\srv\\share\\a:b",
        "C:\\dir\\CON",
        "C:\\dir\\nul.txt",
        "C:\\dir\\Com1",
        "C:\\dir\\lpt9.log",
        "C:\\dir\\AUX \\x",
        "C:\\CONOUT$",
        "C:\\a\\..\\b",
        "C:\\a\\\\b",
        // UNC only with backslashes, as the host takes it.
        "//srv/share/repo",
    ] {
        assert_eq!(arg(w, bad).unwrap_err().code, "invalidArgument", "{bad}");
    }
    // Repository-relative parts: the same on every host, plus the reserved
    // device names on Windows.
    let rel = |f: Flavor, r: &str| {
        dispatch(
            "fs.exists",
            json!({ "path": { "repository": "cms", "relative": r } }),
            f,
        )
    };
    for f in [Flavor::Linux, Flavor::MacOs, w] {
        assert_eq!(
            rel(f, "src\\lib/./a.js").unwrap(),
            json!({ "path": { "repository": "cms", "relative": "src/lib/a.js" } })
        );
        for bad in ["..", "a/../../b", "/x", "\\x", "C:\\x", "C:x", "a:b"] {
            let e = rel(f, bad).unwrap_err();
            assert_eq!(e.repository.as_deref(), Some("cms"), "{f:?} {bad}");
        }
    }
    assert!(rel(Flavor::Linux, "dir/CON").is_ok());
    assert!(rel(Flavor::MacOs, "nul.txt").is_ok());
    let e = rel(w, "dir/CON").unwrap_err();
    assert_eq!(
        (e.code.as_str(), e.repository.as_deref()),
        ("invalidArgument", Some("cms"))
    );
    assert!(rel(w, "lpt1.txt").is_err());
    // A process's cwd is a path like any other.
    assert!(
        dispatch(
            "process.run",
            json!({ "command": "git", "cwd": { "path": "\\\\?\\C:\\x" } }),
            w
        )
        .is_err()
    );
}
