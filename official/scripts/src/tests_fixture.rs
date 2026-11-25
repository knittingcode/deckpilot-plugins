//! The shared request fixture: `tests/fixtures/script-requests.json` is a
//! byte-for-byte copy of DeckPilot's
//! `plugins/runtime/crates/abi/tests/fixtures/script-requests.json`
//! (version 2: knittingcode/deckpilot PR #481, commit
//! 33957b3f325fb1f345871fadd6ac56cb47176262, sha256
//! e09772aa197beb668fd8aa0d33db8daf3dc8a4bab7bddfe52019264e5dd55d30,
//! held by [`the_shared_fixture_is_the_recorded_copy`]). It is the one
//! contract of the two repositories: never edit it here; change it in
//! DeckPilot and copy it again.
//!
//! * **Its `valid` requests.** Each driver below runs a script through the
//!   real prelude and marshallers against the mock bridge, answered with
//!   `tests/fixtures/bridge-replies.json`. Every request a driver emits is
//!   byte-equal (canonical JSON) to a `valid` entry, or, where the
//!   fixture's one example has other values (the first chunk of a read,
//!   the request a confirmation repeats), of exactly the shape of one.
//!   Every chunk of a read after the first carries the first reply's
//!   `version` as `ifVersion`, a version the host accepts.
//!   Every `valid` entry is emitted byte for byte by some driver, but
//!   those in [`NOT_SENT`], which say why the guest never sends them.
//! * **Its `invalid` requests.** The guest never emits one: each is
//!   refused by its own validation (or normalized to another request)
//!   on every host OS, or is one the guest cannot produce at all, and the
//!   test says which. A chunk of a read (`offset`, `ifVersion`) is never a
//!   script's argument: the guest builds it, and the test drives reads
//!   against agents that answer without a version or with a malformed
//!   one, and holds that no later chunk ever leaves without a valid one.
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

/// The copy's source and digest: DeckPilot #481 at this commit.
const FIXTURE_COMMIT: &str = "33957b3f325fb1f345871fadd6ac56cb47176262";
const FIXTURE_SHA256: &str = "e09772aa197beb668fd8aa0d33db8daf3dc8a4bab7bddfe52019264e5dd55d30";
const FIXTURE_VERSION: u64 = 2;

/// The version the drivers' agent gives a read (the fixture's own).
const READ_VERSION: &str = "0123456789abcdef0123456789abcdef";

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
                if offset > 0 {
                    assert_eq!(args["ifVersion"], READ_VERSION, "{args}");
                }
                // The host's form: a version on every reply before the
                // end, none on the one that reaches it.
                let mut reply = json!({ "content": base64::encode(&bytes[offset..end]), "bytes": end - offset, "size": n, "eof": end == n });
                if end < n {
                    reply["version"] = json!(READ_VERSION);
                }
                return json!({ "ok": reply });
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
            // A later chunk of a read names the read's version.
            let v: Value = serde_json::from_str(request).unwrap();
            if v["op"] == "fs.readFile" && v["args"]["offset"].as_u64() != Some(0) {
                assert_eq!(
                    v["args"]["ifVersion"], READ_VERSION,
                    "{}: {request}",
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
        if op == "fs.readFile" && (args.get("offset").is_some() || args.get("ifVersion").is_some())
        {
            // A chunk: the guest's own, never a script's argument.
            for flavor in flavors {
                assert!(api::dispatch(op, args.clone(), flavor).is_err(), "{why}");
            }
            for reply_version in [None, args.get("ifVersion").cloned()] {
                for sent in chunks_against(reply_version.clone()) {
                    assert_ne!(canonical(&sent), input, "{why}");
                    let offset = sent["offset"].as_u64().unwrap();
                    match sent.get("ifVersion").and_then(Value::as_str) {
                        Some(v) => assert!(api::fs::valid_read_version(v), "{why}: {sent}"),
                        None => assert_eq!(offset, 0, "{why}: {sent}"),
                    }
                }
            }
            continue;
        }
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

/// The `fs.readFile` chunks a two-chunk read sends to an agent whose
/// first reply has this `version` member (`None`: none at all).
fn chunks_against(version: Option<Value>) -> Vec<Value> {
    let mock = Mock::with(
        "try { deckpilot.fs.readFile('/tmp/a') } catch (e) { e.code }",
        json!({}),
        Box::new(move |_, args| {
            let offset = args["offset"].as_u64().unwrap();
            let mut reply = json!({ "content": base64::encode(&[b'a'; 524_288]), "bytes": 524_288, "size": 600_000, "eof": false });
            match (&version, offset) {
                (Some(v), 0) => reply["version"] = v.clone(),
                (None, 0) => {}
                _ => reply["version"] = json!(READ_VERSION),
            }
            json!({ "ok": reply })
        }),
    );
    let _ = crate::run(mock.boxed(), SCRIPT, None);
    mock.0
        .borrow()
        .of("fs.readFile")
        .into_iter()
        .cloned()
        .collect()
}

#[test]
fn the_shared_fixture_is_the_recorded_copy() {
    let path = format!(
        "{}/tests/fixtures/script-requests.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(
        sha256_hex(&bytes),
        FIXTURE_SHA256,
        "{path} is not the byte-for-byte copy of deckpilot {FIXTURE_COMMIT}"
    );
    assert_eq!(requests()["version"], FIXTURE_VERSION);
    // The digest itself (FIPS 180-4's examples).
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(&[b'a'; 1_000_000]),
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
    );
}

/// SHA-256 (FIPS 180-4), for the one check above: no dependency for it.
fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in msg.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
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
    let read = json!({ "ok": { "content": base64::encode(&vec![0xffu8; api::fs::READ_CHUNK as usize]), "bytes": api::fs::READ_CHUNK, "size": 4194304, "eof": false, "version": "v".repeat(api::fs::MAX_READ_VERSION), "nowUs": 1.7e15 } });
    assert!(canonical(&read).len() < limits::REPLY);
    let later = json!({ "op": "fs.readFile", "args": { "path": { "path": format!("/{}", "a".repeat(api::fs::MAX_PATH - 1)) }, "encoding": "base64", "maxBytes": 4194304, "offset": 3670016, "length": 524288, "ifVersion": "v".repeat(api::fs::MAX_READ_VERSION) } });
    assert!(canonical(&later).len() < limits::REQUEST);
    assert_eq!(
        r["limits"]["processOutputBytes"],
        json!(api::process::HOST_OUTPUT_TOTAL)
    );
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
