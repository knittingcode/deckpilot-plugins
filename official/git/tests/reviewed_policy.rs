//! The reviewed process-execution policy of `deckpilot.git`, frozen.
//!
//! `deckpilot.git` is a privileged plugin: it asks the agent to run the
//! user's own `git` (`tools`, plugin API 1.9). The agent, not the plugin,
//! enforces the execution model (DeckPilot's process permission review):
//! the executable `git` only, arguments as an array (never a shell
//! string), the working directory only a bound project resource folder,
//! time and output limits, consent on first use and the audit log. What
//! the plugin controls is what its manifest declares, so this test freezes
//! that declaration byte for byte as it was reviewed (DeckPilot's
//! `plugins/first-party/git/manifest.json`, now the contract of the release
//! DeckPilot bundles, `plugins/first-party/contracts/deckpilot.git/`),
//! refuses any other executable or a shell anywhere in the manifest, refuses
//! a process the plugin would start itself, and, where the pinned DeckPilot
//! checkout exists (`tools/build-cli.sh`; always in CI), compares the
//! plugin with that contract: the manifest (only `version`, never older,
//! and `repository` may differ), the locales and the icon. A pinned
//! DeckPilot without the contract fails. The module is not in the contract:
//! the bundled release's module is pinned by DeckPilot (artifacts.json,
//! released-pins.json), and `tools/official.sh sign-test` runs DeckPilot's
//! released-pins guard on a build of this source, so a source that no longer
//! builds the released module under its version fails there.
//!
//! Changing anything here is a change of a reviewed security policy: it
//! needs the security review, here and in DeckPilot, never a test update.

use std::path::{Path, PathBuf};

use serde_json::Value;

const MANIFEST: &str = include_str!("../manifest.json");
const EN: &[u8] = include_bytes!("../locales/en.json");
const PL: &[u8] = include_bytes!("../locales/pl.json");
const ICON: &[u8] = include_bytes!("../assets/icon-128.png");
const SOURCES: &[(&str, &str)] = &[
    ("lib.rs", include_str!("../src/lib.rs")),
    ("porcelain.rs", include_str!("../src/porcelain.rs")),
    ("state.rs", include_str!("../src/state.rs")),
    ("tests.rs", include_str!("../src/tests.rs")),
];

/// The `permissions` of the manifest, byte for byte as reviewed.
const REVIEWED_PERMISSIONS: &str = r#"  "permissions": [
    {
      "kind": "tools",
      "tools": [
        {
          "executable": "git",
          "subcommands": [
            {
              "name": "status",
              "options": ["--porcelain=v2", "--branch", "-z", "--untracked-files"]
            },
            {
              "name": "log",
              "options": ["-1", "--no-show-signature", "--format"]
            },
            {
              "name": "config",
              "options": ["--get"]
            },
            {
              "name": "remote"
            },
            {
              "name": "rev-parse",
              "options": ["--verify", "-q"]
            },
            {
              "name": "ls-remote"
            }
          ],
          "network": true
        }
      ],
      "justification": "t:permissions.tools"
    },
    {
      "kind": "resources",
      "paths": true,
      "open": true,
      "justification": "t:permissions.resources"
    },
    {
      "kind": "profile",
      "read": ["project"],
      "justification": "t:permissions.profile"
    },
    {
      "kind": "device",
      "state": true,
      "justification": "t:permissions.device"
    }
  ],"#;

/// Executables and interpreters that must never appear in the manifest.
const FORBIDDEN: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "fish",
    "dash",
    "ksh",
    "csh",
    "tcsh",
    "cmd",
    "cmd.exe",
    "powershell",
    "powershell.exe",
    "pwsh",
    "env",
    "sudo",
    "su",
    "osascript",
    "python",
    "python3",
    "node",
    "perl",
    "ruby",
    "docker",
    "/bin/sh",
    "/bin/bash",
    "/usr/bin/env",
];

fn manifest() -> Value {
    serde_json::from_str(MANIFEST).expect("manifest.json is JSON")
}

#[test]
fn the_permissions_are_byte_for_byte_the_reviewed_ones() {
    let start = MANIFEST
        .find("  \"permissions\": [\n")
        .expect("manifest.json declares permissions");
    let end = MANIFEST[start..]
        .find("\n  ],\n")
        .map(|i| start + i + "\n  ],".len())
        .expect("the permissions end");
    assert_eq!(
        &MANIFEST[start..end],
        REVIEWED_PERMISSIONS,
        "the permissions of deckpilot.git differ from the reviewed ones"
    );
    assert_eq!(
        MANIFEST.matches("\"permissions\"").count(),
        1,
        "one permissions list"
    );
    let reviewed: Value = serde_json::from_str(&format!(
        "{{{}}}",
        REVIEWED_PERMISSIONS.strip_suffix(',').unwrap()
    ))
    .expect("reviewed JSON");
    assert_eq!(manifest()["permissions"], reviewed["permissions"]);
}

#[test]
fn only_git_runs_and_never_a_shell() {
    let manifest = manifest();
    assert_eq!(manifest["id"], "deckpilot.git");
    let permissions = manifest["permissions"].as_array().unwrap();
    let tools: Vec<&Value> = permissions
        .iter()
        .filter(|p| p["kind"] == "tools")
        .collect();
    assert_eq!(tools.len(), 1, "one tools permission");
    let tools = tools[0]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1, "one executable");
    assert_eq!(tools[0]["executable"], "git");
    // Every `executable` anywhere in the manifest, and every string, is
    // checked: no shell, interpreter or other tool can hide elsewhere.
    fn walk(value: &Value, executables: &mut Vec<String>, strings: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (k, v) in map {
                    if k == "executable" {
                        executables.push(v.as_str().unwrap_or("?").to_owned());
                    }
                    assert!(
                        !matches!(
                            k.as_str(),
                            "shell" | "command" | "commandLine" | "script" | "env"
                        ),
                        "the manifest has a `{k}` key"
                    );
                    walk(v, executables, strings);
                }
            }
            Value::Array(items) => items.iter().for_each(|v| walk(v, executables, strings)),
            Value::String(s) => strings.push(s.clone()),
            _ => {}
        }
    }
    let (mut executables, mut strings) = (Vec::new(), Vec::new());
    let mut rest = manifest.clone();
    // `repository.type` is the version control system of the source ("git").
    assert_eq!(rest["repository"]["type"], "git");
    rest.as_object_mut().unwrap().remove("repository");
    walk(&rest, &mut executables, &mut strings);
    assert_eq!(executables, ["git"]);
    for s in &strings {
        assert!(!FORBIDDEN.contains(&s.as_str()), "the manifest names `{s}`");
    }
    for sub in tools[0]["subcommands"].as_array().unwrap() {
        let name = sub["name"].as_str().unwrap();
        assert!(
            name.chars()
                .all(|c| c.is_ascii_lowercase() || c == '-' || c == ' '),
            "subcommand `{name}` is a name, not a command line"
        );
        for option in sub["options"].as_array().into_iter().flatten() {
            let option = option.as_str().unwrap();
            assert!(
                option.starts_with('-')
                    && option
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '=')),
                "option `{option}` of `git {name}`"
            );
        }
    }
}

#[test]
fn the_plugin_starts_no_process_itself() {
    for (name, text) in SOURCES {
        for forbidden in [
            "std::process",
            "Command::new",
            "Tool::Docker",
            "extern \"C\"",
            "#[link",
        ] {
            assert!(
                !text.contains(forbidden),
                "src/{name} contains `{forbidden}`"
            );
        }
        // Every tool run names the reviewed tool.
        for (at, _) in text.match_indices("run_tool") {
            let call = text[at..]
                .split_once('(')
                .map(|(_, rest)| rest.trim_start());
            assert!(
                call.is_some_and(|rest| rest.starts_with("Tool::Git")),
                "src/{name}: a run_tool call that is not Tool::Git"
            );
        }
    }
}

/// The pinned DeckPilot checkout of `tools/build-cli.sh`, if it exists.
fn pinned_deckpilot() -> Option<PathBuf> {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo = crate_dir.join("../..");
    let work = std::env::var_os("DECKPILOT_PLUGINS_WORK")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.join(".work"));
    let checkout = work.join("deckpilot");
    let env = std::fs::read_to_string(repo.join("tools/deckpilot.env")).unwrap();
    let commit = env
        .lines()
        .find_map(|l| l.strip_prefix("DECKPILOT_COMMIT="))
        .expect("DECKPILOT_COMMIT")
        .trim()
        .to_owned();
    let head = std::fs::read_to_string(checkout.join(".git/HEAD")).ok();
    match head {
        Some(head) => {
            assert_eq!(
                head.trim(),
                commit,
                "{} is not DeckPilot at {commit}",
                checkout.display()
            );
            Some(checkout)
        }
        None if std::env::var_os("CI").is_some() => {
            panic!(
                "no pinned DeckPilot checkout at {} (tools/build-cli.sh)",
                checkout.display()
            )
        }
        None => {
            eprintln!("no pinned DeckPilot checkout: not compared with DeckPilot's deckpilot.git");
            None
        }
    }
}

#[test]
fn ids_actions_settings_and_permissions_are_deckpilots() {
    let Some(checkout) = pinned_deckpilot() else {
        return;
    };
    let theirs = checkout.join("plugins/first-party/contracts/deckpilot.git");
    assert!(
        theirs.join("manifest.json").is_file(),
        "the pinned DeckPilot has no contract of deckpilot.git at {}",
        theirs.display()
    );
    let read = |rel: &str| std::fs::read(theirs.join(rel)).unwrap();
    let mut ours = manifest();
    let mut deckpilot: Value = serde_json::from_slice(&read("manifest.json")).unwrap();
    assert_eq!(deckpilot["id"], "deckpilot.git");
    // The bundled release or a newer one, from this repository: nothing
    // else differs.
    assert!(
        version_of(&ours) >= version_of(&deckpilot),
        "{} is older than DeckPilot's contract {}",
        ours["version"],
        deckpilot["version"]
    );
    for m in [&mut ours, &mut deckpilot] {
        let m = m.as_object_mut().unwrap();
        m.remove("version");
        m.remove("repository");
    }
    assert_eq!(
        ours, deckpilot,
        "manifest.json differs from DeckPilot's contract beyond version and repository"
    );
    let text = String::from_utf8(read("manifest.json")).unwrap();
    assert!(
        text.contains(REVIEWED_PERMISSIONS),
        "the reviewed permissions are not those of DeckPilot's contract at the pinned commit"
    );
    assert_eq!(EN, read("locales/en.json").as_slice(), "locales/en.json");
    assert_eq!(PL, read("locales/pl.json").as_slice(), "locales/pl.json");
    assert_eq!(
        ICON,
        read("assets/icon-128.png").as_slice(),
        "assets/icon-128.png"
    );
}

/// `x.y.z` as numbers.
fn version_of(manifest: &Value) -> (u64, u64, u64) {
    let text = manifest["version"].as_str().unwrap();
    let parts: Vec<u64> = text.split('.').map(|p| p.parse().unwrap()).collect();
    assert_eq!(parts.len(), 3, "{text} is not x.y.z");
    (parts[0], parts[1], parts[2])
}
