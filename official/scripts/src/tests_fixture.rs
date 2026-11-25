//! The shared request fixture: `tests/fixtures/script-requests.json` is a
//! byte-for-byte copy of DeckPilot's
//! `plugins/runtime/crates/abi/tests/fixtures/script-requests.json`
//! (knittingcode/deckpilot PR #465, commit
//! 5c1c4c8460b096427dc599812b2713875293fb88, sha256
//! 75b079ff820f00301e581b5906d90e59fbdba76dc73bad22c3b04dd0f6e70af3). It is
//! the one contract of the two repositories: never edit it here; change it
//! in DeckPilot and copy it again.
//!
//! * **Its `valid` requests.** Each driver below runs a script through the
//!   real prelude and marshallers against the mock bridge, answered with
//!   `tests/fixtures/bridge-replies.json`. Every request a driver emits is
//!   byte-equal (canonical JSON) to a `valid` entry, or, where the
//!   fixture's one example has other values (the first chunk of a read,
//!   the request a confirmation repeats), of exactly the shape of one.
//!   Every `valid` entry is emitted byte for byte by some driver, but
//!   those in [`NOT_SENT`], which say why the guest never sends them.
//! * **Its `invalid` requests.** The guest never emits one: each is
//!   refused by its own validation (or normalized to another request)
//!   on every host OS, or is one the guest cannot produce at all, and the
//!   test says which.
//! * Every request is under 64 KiB and every reply of
//!   `bridge-replies.json` under 1 MiB, and the guest accepts each reply.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use serde_json::{Value, json};

use crate::api::{self, Flavor, WIRE_OPS, base64};
use crate::bridge::{Session, Transport, limits};
use crate::mock::Mock;

/// The script id of every driver's run (the fixture's `script.load`).
const SCRIPT: &str = "6f1c2a8e-4b1d-4c2e-9a77-0d5b1e3f9c10";

/// `valid` requests the guest never sends, and why.
const NOT_SENT: &[(&str, &str)] = &[(
    r#"{"args":{"encoding":"utf8","maxBytes":4194304,"path":{"relative":"README.md","repository":"cms"}},"op":"fs.readFile"}"#,
    "the whole-file form (no offset and length): the guest always reads in chunks of \
     `offset` and `length`, so a file of any size takes the same path",
)];

fn load(name: &str) -> Value {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn requests() -> Value {
    load("script-requests.json")
}

fn replies() -> Value {
    load("bridge-replies.json")
}

/// Canonical JSON: compact, object keys sorted bytewise (serde_json's
/// default map), so equal values have equal bytes.
fn canonical(v: &Value) -> String {
    match v {
        Value::Object(m) => {
            let sorted: BTreeMap<&String, &Value> = m.iter().collect();
            let items: Vec<String> = sorted
                .iter()
                .map(|(k, v)| format!("{}:{}", Value::String((*k).clone()), canonical(v)))
                .collect();
            format!("{{{}}}", items.join(","))
        }
        Value::Array(a) => format!(
            "[{}]",
            a.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        other => other.to_string(),
    }
}

/// A request's shape: its keys, recursively, and the kinds of its values.
fn shape(v: &Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(m.iter().map(|(k, v)| (k.clone(), shape(v))).collect()),
        Value::Array(_) => json!("array"),
        Value::String(_) => json!("string"),
        Value::Number(_) => json!("number"),
        Value::Bool(_) => json!("bool"),
        Value::Null => json!("null"),
    }
}

fn reply(f: &Value, op: &str, name: &str) -> Value {
    f["ops"][op]["replies"]
        .get(name)
        .unwrap_or_else(|| panic!("no reply {op}:{name} in bridge-replies.json"))
        .clone()
}

/// One script; what it emits is checked against the `valid` requests.
struct Driver {
    name: &'static str,
    platform: &'static str,
    source: &'static str,
    /// Per op, the replies of its calls in order (then `ok`).
    answers: &'static [(&'static str, &'static [&'static str])],
    /// `fs.readFile` served from a file of this many bytes instead.
    file: Option<usize>,
    cancel_at_poll: Option<u32>,
}

const fn d(name: &'static str, source: &'static str) -> Driver {
    Driver {
        name,
        platform: "macos",
        source,
        answers: &[],
        file: None,
        cancel_at_poll: None,
    }
}

fn drivers() -> Vec<Driver> {
    vec![
        Driver {
            cancel_at_poll: Some(1),
            ..d("a poll", "for (;;) {}")
        },
        d("a log line", r#"deckpilot.log.info("Updating cms")"#),
        d("a sleep", "deckpilot.sleep(250)"),
        Driver {
            answers: &[("fs.remove", &["needsConfirmation", "ok"])],
            ..d(
                "a confirmed removal",
                r#"deckpilot.fs.remove({ repository: "cms", relative: "node_modules" }, { recursive: true })"#,
            )
        },
        Driver {
            answers: &[("fs.remove", &["dryRun"])],
            ..d(
                "a dry run",
                r#"deckpilot.fs.remove({ repository: "cms", relative: "node_modules" }, { recursive: true, dryRun: true })"#,
            )
        },
        d(
            "projects and repositories",
            "deckpilot.projects.list()[0].repositories(); deckpilot.repositories.current()",
        ),
        d(
            "exists, stat, list and mkdir",
            r#"deckpilot.fs.exists({ repository: "cms", relative: "package.json" });
               deckpilot.fs.stat("/Users/me/work/cms/src");
               deckpilot.fs.list({ repository: "cms", relative: "" });
               deckpilot.fs.mkdir({ repository: "cms", relative: "build/cache" }, { recursive: true })"#,
        ),
        Driver {
            platform: "windows",
            ..d("a Windows path", r#"deckpilot.fs.stat("C:\\work\\cms\\")"#)
        },
        Driver {
            file: Some(1_200_000),
            ..d(
                "a read in chunks",
                r#"deckpilot.fs.readFile({ repository: "cms", relative: "logo.png" }, { encoding: "base64" }).length"#,
            )
        },
        d(
            "a small write",
            r#"deckpilot.fs.writeFile({ repository: "cms", relative: ".env.local" }, "PORT=3000\n")"#,
        ),
        d(
            "a chunked write",
            r#"deckpilot.fs.writeFile({ repository: "cms", relative: "dist/bundle.js" }, "x".repeat(32768) + "hello")"#,
        ),
        Driver {
            answers: &[("fs.write.commit", &["exists"])],
            ..d(
                "an aborted write",
                r#"try { deckpilot.fs.writeFile({ repository: "cms", relative: "dist/bundle.js" }, "x".repeat(32768) + "hello") } catch (e) { e.code }"#,
            )
        },
        d(
            "a copy",
            r#"deckpilot.fs.copy({ repository: "cms", relative: "config/example.env" }, { repository: "cms", relative: ".env" })"#,
        ),
        Driver {
            answers: &[("fs.copy", &["needsConfirmation", "ok"])],
            ..d(
                "a confirmed copy",
                r#"deckpilot.fs.copy({ repository: "cms", relative: "templates" }, { repository: "web", relative: "templates" }, { recursive: true, overwrite: true })"#,
            )
        },
        d(
            "a move and removeContents",
            r#"deckpilot.fs.move({ repository: "cms", relative: "tmp/out" }, { repository: "cms", relative: "out" });
               deckpilot.fs.removeContents({ repository: "cms", relative: "var/cache" }, { keep: [".gitkeep"] })"#,
        ),
        d(
            "processes",
            r#"deckpilot.git.run(["pull", "--ff-only"], { cwd: { repository: "cms", relative: "" }, timeoutMs: 120000 });
               deckpilot.process.run({ command: "/usr/local/bin/make", env: { CI: "1" }, stdin: "input\n" });
               deckpilot.process.run({ command: "docker", args: ["compose", "up", "-d"], env: { COMPOSE_PROJECT_NAME: "cms", GIT_AUTHOR_NAME: "Me" } })"#,
        ),
        d(
            "a shell",
            r#"deckpilot.shell.available();
               deckpilot.shell.run({ shell: "sh", script: 'echo "$HOME"', cwd: { repository: "cms", relative: "" }, timeoutMs: 10000 })"#,
        ),
    ]
}

/// Runs a driver; returns the requests it emitted, as text.
fn emitted(replies: &Value, driver: &Driver) -> Vec<String> {
    let r2 = replies.clone();
    let answers = driver.answers;
    let file = driver.file;
    let calls: Rc<RefCell<BTreeMap<String, usize>>> = Rc::default();
    let mock = Mock::with(
        driver.source,
        json!({}),
        Box::new(move |op, args| {
            if op == "fs.readFile"
                && let Some(n) = file
            {
                let bytes: Vec<u8> = (0..n).map(|i| (i % 251) as u8).collect();
                let offset = args["offset"].as_u64().unwrap() as usize;
                let end = (offset + args["length"].as_u64().unwrap() as usize).min(n);
                return json!({ "ok": { "content": base64::encode(&bytes[offset..end]), "bytes": end - offset, "size": n, "eof": end == n } });
            }
            let mut calls = calls.borrow_mut();
            let k = calls.entry(op.to_owned()).or_default();
            let name = answers
                .iter()
                .find(|(o, _)| *o == op)
                .and_then(|(_, names)| names.get(*k).copied())
                .unwrap_or("ok");
            *k += 1;
            reply(&r2, op, name)
        }),
    );
    let mut loaded = reply(replies, "script.load", "ok")["ok"].clone();
    loaded["source"] = json!(driver.source);
    loaded["platform"] = json!(driver.platform);
    mock.set(|s| {
        s.load = loaded;
        s.cancel_at_poll = driver.cancel_at_poll;
    });
    let _ = crate::run(mock.boxed(), SCRIPT, None);
    mock.0
        .borrow()
        .raw
        .iter()
        .map(|r| String::from_utf8(r.clone()).unwrap())
        .collect()
}

#[test]
fn every_request_the_guest_emits_is_a_valid_one_of_the_shared_fixture() {
    let f = requests();
    let r = replies();
    let valid: Vec<String> = f["valid"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| canonical(&v["request"]))
        .collect();
    let shapes: BTreeSet<String> = f["valid"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| canonical(&shape(&v["request"])))
        .collect();
    let mut sent: BTreeSet<String> = BTreeSet::new();
    for driver in drivers() {
        let got = emitted(&r, &driver);
        assert!(
            got.len() >= 2,
            "{}: nothing after script.load: {got:?}",
            driver.name
        );
        for request in &got {
            if valid.contains(request) {
                sent.insert(request.clone());
            } else {
                let v: Value = serde_json::from_str(request).unwrap();
                assert!(
                    shapes.contains(&canonical(&shape(&v))),
                    "{}: {request} is no valid request of the shared fixture, not even in shape",
                    driver.name
                );
            }
            // The guest's own bytes are canonical.
            assert_eq!(
                request,
                &canonical(&serde_json::from_str(request).unwrap()),
                "{}",
                driver.name
            );
        }
    }
    let not_sent: BTreeSet<String> = NOT_SENT.iter().map(|(r, _)| (*r).to_owned()).collect();
    for n in &not_sent {
        assert!(
            valid.contains(n),
            "NOT_SENT names a request the fixture lacks: {n}"
        );
    }
    let missing: Vec<&String> = valid
        .iter()
        .filter(|v| !sent.contains(*v) && !not_sent.contains(*v))
        .collect();
    assert!(
        missing.is_empty(),
        "valid requests no driver emits: {missing:#?}"
    );
    for n in &not_sent {
        assert!(
            !sent.contains(n),
            "{n} is sent after all: drop it from NOT_SENT"
        );
    }
}

/// Answers every request with one reply and records what it was sent.
struct Fixed(Value, Rc<RefCell<Vec<Value>>>);

impl Transport for Fixed {
    fn call(&mut self, request: &[u8]) -> Option<Vec<u8>> {
        self.1
            .borrow_mut()
            .push(serde_json::from_slice(request).unwrap());
        Some(serde_json::to_vec(&self.0).unwrap())
    }
}

#[test]
fn the_guest_never_emits_an_invalid_request_of_the_shared_fixture() {
    let f = requests();
    let flavors = [Flavor::Linux, Flavor::MacOs, Flavor::Windows];
    for entry in f["invalid"].as_array().unwrap() {
        let why = entry["why"].as_str().unwrap();
        let request = &entry["request"];
        let op = request["op"].as_str().unwrap();
        let args = request["args"].clone();
        let input = canonical(&args);
        if api::OPS.contains(&op) {
            // What a script can make the guest send: refused, or sent only
            // as another, normalized request.
            for flavor in flavors {
                if let Ok(out) = api::dispatch(op, args.clone(), flavor) {
                    assert_ne!(canonical(&out), input, "{why}: sent as is on {flavor:?}");
                }
            }
            continue;
        }
        match op {
            "script.load" => {
                let only_script = args.as_object().unwrap().keys().eq(["script"].iter());
                if only_script {
                    // The invocation's parameter: refused before any call.
                    let seen = Rc::new(RefCell::new(Vec::new()));
                    let t = Fixed(json!({ "ok": {} }), Rc::clone(&seen));
                    let e = crate::run(Box::new(t), args["script"].as_str().unwrap(), None)
                        .unwrap_err();
                    assert_eq!(e.code(), "invalid-parameters", "{why}");
                    assert!(seen.borrow().is_empty(), "{why}");
                } else {
                    // The guest sends `{script}` and nothing else.
                    assert!(args.get("script").is_some(), "{why}");
                }
            }
            // The guest sends `{}`, a constant.
            "script.poll" => assert_ne!(input, "{}", "{why}"),
            "script.log" => {
                // An unknown level is sent as `info`.
                let seen = Rc::new(RefCell::new(Vec::new()));
                let t = Fixed(json!({ "ok": null }), Rc::clone(&seen));
                let mut s = Session::new(Box::new(t));
                api::log::line(
                    &mut s,
                    args["level"].as_str().unwrap(),
                    args["message"].as_str().unwrap(),
                );
                assert_eq!(seen.borrow()[0]["args"]["level"], "info", "{why}");
            }
            // Chunks are the guest's own canonical base64.
            "fs.write.chunk" => {
                assert!(
                    base64::decode(args["content"].as_str().unwrap()).is_none(),
                    "{why}"
                );
            }
            // A handle comes from the agent's reply and is checked there.
            "fs.write.commit" | "fs.write.abort" => {
                assert!(api::fs::handle_of(&args).is_none(), "{why}");
            }
            other => panic!("{why}: an invalid {other} request this test does not classify"),
        }
    }
}

#[test]
fn the_shared_fixture_covers_every_wire_op() {
    let f = requests();
    let ops: BTreeSet<&str> = f["valid"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["request"]["op"].as_str().unwrap())
        .collect();
    let wire: BTreeSet<&str> = WIRE_OPS.iter().copied().collect();
    assert_eq!(ops, wire);
    for v in f["valid"].as_array().unwrap() {
        assert_eq!(v["op"], v["request"]["op"]);
        assert_eq!(v["request"].as_object().unwrap().len(), 2, "op and args");
    }
    let r = replies();
    let replied: BTreeSet<&str> = r["ops"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(replied, wire, "bridge-replies.json has replies of every op");
}

#[test]
fn every_request_and_reply_fits_the_host_caps() {
    let f = requests();
    let r = replies();
    assert_eq!(r["limits"]["maxRequestBytes"], json!(limits::REQUEST));
    assert_eq!(r["limits"]["maxReplyBytes"], json!(limits::REPLY));
    assert_eq!(r["limits"]["readChunkBytes"], json!(api::fs::READ_CHUNK));
    assert_eq!(r["limits"]["writeChunkBytes"], json!(api::fs::WRITE_CHUNK));
    assert_eq!(
        r["limits"]["processArgsBytes"],
        json!(api::process::MAX_ARGS_TOTAL)
    );
    assert_eq!(
        r["limits"]["processStdinBytes"],
        json!(api::process::MAX_STDIN)
    );
    assert_eq!(
        r["limits"]["shellScriptBytes"],
        json!(api::process::MAX_SHELL_SCRIPT)
    );
    assert_eq!(r["limits"]["hostCalls"], json!(limits::HOST_CALLS));
    assert_eq!(r["limits"]["polls"], json!(limits::POLLS));
    for v in f["valid"].as_array().unwrap() {
        let n = canonical(&v["request"]).len();
        assert!(n < limits::REQUEST, "{n} bytes");
    }
    for (op, o) in r["ops"].as_object().unwrap() {
        for (name, reply) in o["replies"].as_object().unwrap() {
            let n = canonical(reply).len();
            assert!(n < limits::REPLY, "{op}:{name}: {n} bytes");
        }
    }
    // The largest requests and replies the guest makes fit too: a whole
    // write chunk, and a whole read chunk's reply.
    let chunk = json!({ "op": "fs.write.chunk", "args": { "handle": "f".repeat(api::fs::MAX_HANDLE), "content": base64::encode(&[0xff; api::fs::WRITE_CHUNK]), "encoding": "base64" } });
    assert!(canonical(&chunk).len() < limits::REQUEST);
    let read = json!({ "ok": { "content": base64::encode(&vec![0xffu8; api::fs::READ_CHUNK as usize]), "bytes": api::fs::READ_CHUNK, "size": 4194304, "eof": false, "nowUs": 1.7e15 } });
    assert!(canonical(&read).len() < limits::REPLY);
}

#[test]
fn the_guest_accepts_every_reply() {
    let r = replies();
    for (op, o) in r["ops"].as_object().unwrap() {
        for (name, reply) in o["replies"].as_object().unwrap() {
            let mut s = Session::new(Box::new(Fixed(reply.clone(), Rc::default())));
            if op == "script.poll" {
                let stop = s.poll();
                assert_eq!(stop, name != "ok", "{op}:{name}");
                assert!(s.budget.remaining_ms.is_some(), "{op}:{name}");
                continue;
            }
            let got = s.call(op, json!({}));
            match reply.get("error") {
                None => assert!(got.is_ok(), "{op}:{name}: {got:?}"),
                Some(e) => {
                    let fault = got.unwrap_err();
                    assert_eq!(json!(fault.code), e["script"]["code"], "{op}:{name}");
                    assert_eq!(
                        fault.confirmation.is_some(),
                        e["script"].get("confirmation").is_some(),
                        "{op}:{name}"
                    );
                }
            }
        }
    }
}
