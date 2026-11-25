//! The shared request fixture, `tests/fixtures/bridge-requests.json`: the
//! guest's proposed ABI for P6, which DeckPilot's ABI crate parses too.
//!
//! Each driver below runs a script through the real prelude and
//! marshallers against the mock bridge, answering with the fixture's
//! replies, and the requests it emits must equal the fixture's, in order,
//! byte for byte in canonical JSON. Every fixture request is emitted by a
//! driver, every wire op has requests, every request fits the host's
//! 64 KiB and every reply its 1 MiB, and the guest accepts every reply.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use serde_json::{Value, json};

use crate::api::{WIRE_OPS, base64};
use crate::bridge::{Session, Transport, limits};
use crate::mock::Mock;
use crate::tests::run_mock;

fn fixture() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/bridge-requests.json"
    );
    let text = std::fs::read_to_string(path).expect("the fixture");
    serde_json::from_str(&text).expect("the fixture is JSON")
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

fn request<'f>(f: &'f Value, case: &str) -> &'f Value {
    let (op, name) = case.split_once(':').unwrap();
    f["ops"][op]["requests"]
        .get(name)
        .unwrap_or_else(|| panic!("no request {case} in the fixture"))
}

fn reply(f: &Value, op: &str, name: &str) -> Value {
    f["ops"][op]["replies"]
        .get(name)
        .unwrap_or_else(|| panic!("no reply {op}:{name} in the fixture"))
        .clone()
}

/// One script and the requests it must emit after `script.load:load`.
struct Driver {
    name: &'static str,
    platform: &'static str,
    source: &'static str,
    /// Per op, the replies of its calls in order (then `ok`).
    answers: &'static [(&'static str, &'static [&'static str])],
    /// `fs.readFile` served from a file of this many bytes instead.
    file: Option<usize>,
    cancel_at_poll: Option<u32>,
    expect: &'static [&'static str],
}

const fn d(name: &'static str, source: &'static str, expect: &'static [&'static str]) -> Driver {
    Driver {
        name,
        platform: "macos",
        source,
        answers: &[],
        file: None,
        cancel_at_poll: None,
        expect,
    }
}

fn drivers() -> Vec<Driver> {
    vec![
        Driver {
            cancel_at_poll: Some(1),
            ..d("a poll", "for (;;) {}", &["script.poll:poll"])
        },
        d(
            "a log line",
            r#"deckpilot.log.info("Updating", "CMS")"#,
            &["script.log:info"],
        ),
        d("a sleep", "deckpilot.sleep(250.7)", &["script.sleep:ms"]),
        Driver {
            answers: &[("fs.remove", &["needsConfirmation", "ok"])],
            ..d(
                "a confirmed removal",
                r#"deckpilot.fs.remove({ repository: "cms", relative: "build/" }, { recursive: true })"#,
                &[
                    "fs.remove:recursive",
                    "script.confirm:token",
                    "fs.remove:confirmed",
                ],
            )
        },
        Driver {
            answers: &[("fs.remove", &["dryRun"])],
            ..d(
                "a dry run",
                r#"deckpilot.fs.remove("/Users/dev/shop/cms/build", { recursive: true, dryRun: true })"#,
                &["fs.remove:dryRun"],
            )
        },
        d(
            "projects and repositories",
            "deckpilot.projects.list()[0].repositories(); deckpilot.repositories.current()",
            &[
                "project.list:list",
                "project.repositories:byId",
                "project.repositories:current",
            ],
        ),
        d(
            "both path forms",
            r#"deckpilot.fs.exists("/Users/dev/shop/cms/package.json");
               deckpilot.fs.exists({ repository: "cms", relative: "src/./main.js" })"#,
            &["fs.exists:absolute", "fs.exists:repository"],
        ),
        Driver {
            platform: "windows",
            ..d(
                "Windows paths",
                r#"deckpilot.fs.exists("C:\\work\\shop\\cms\\package.json");
                   deckpilot.fs.exists("\\\\fileserver\\projects\\shop")"#,
                &["fs.exists:windowsDrive", "fs.exists:windowsUnc"],
            )
        },
        d(
            "stat, list and mkdir",
            r#"deckpilot.fs.stat({ repository: "cms", relative: "" });
               deckpilot.fs.list({ repository: "cms", relative: "src" });
               deckpilot.fs.mkdir({ repository: "cms", relative: "dist\\assets" }, { recursive: true })"#,
            &[
                "fs.stat:repositoryRoot",
                "fs.list:repository",
                "fs.mkdir:recursive",
            ],
        ),
        d(
            "a small read",
            r#"deckpilot.fs.readFile("/Users/dev/shop/cms/README.md")"#,
            &["fs.readFile:oneChunk"],
        ),
        d(
            "a read with maxBytes",
            r#"deckpilot.fs.readFile({ repository: "cms", relative: "VERSION" }, { maxBytes: 100 })"#,
            &["fs.readFile:maxBytes"],
        ),
        Driver {
            file: Some(600_000),
            ..d(
                "a read of two chunks",
                r#"deckpilot.fs.readFile({ repository: "cms", relative: "data/catalog.json" }, { encoding: "base64" }).length"#,
                &["fs.readFile:firstOfTwo", "fs.readFile:secondOfTwo"],
            )
        },
        d(
            "a write of two chunks",
            r#"deckpilot.fs.writeFile("/Users/dev/shop/cms/dist/app.js", "x".repeat(32773))"#,
            &[
                "fs.write.open:replace",
                "fs.write.chunk:largest",
                "fs.write.chunk:last",
                "fs.write.commit:commit",
            ],
        ),
        Driver {
            answers: &[("fs.write.chunk", &["failed"])],
            ..d(
                "a failed write",
                r#"try { deckpilot.fs.writeFile({ repository: "cms", relative: "notes.txt" }, "hi", { createOnly: true }) } catch (e) { e.code }"#,
                &[
                    "fs.write.open:createOnly",
                    "fs.write.chunk:small",
                    "fs.write.abort:abort",
                ],
            )
        },
        d(
            "copy, move and removeContents",
            r#"deckpilot.fs.copy("/Users/dev/shop/cms/dist", { repository: "web", relative: "public" }, { recursive: true, overwrite: true });
               deckpilot.fs.move({ repository: "cms", relative: "a.txt" }, { repository: "cms", relative: "b.txt" });
               deckpilot.fs.removeContents({ repository: "cms", relative: "cache" }, { keep: [".gitkeep"] })"#,
            &[
                "fs.copy:recursiveOverwrite",
                "fs.move:rename",
                "fs.removeContents:keep",
            ],
        ),
        d(
            "processes",
            r#"const cms = { repository: "cms", relative: "" };
               deckpilot.git.run(["pull", "--ff-only"], { cwd: cms });
               // 120 s asked, 58.5 s left (60 s at the load, 1.5 s spent by git).
               deckpilot.docker.compose(["up", "-d"], { cwd: cms, timeoutMs: 120000 });
               deckpilot.process.run({ command: "make", args: ["test"], cwd: "/Users/dev/shop/cms", env: { CI: "1" }, stdin: "y\n", timeoutMs: 5000 })"#,
            &[
                "process.run:gitPull",
                "process.run:composeClamped",
                "process.run:makeWithStdin",
            ],
        ),
        d(
            "a shell",
            r#"deckpilot.shell.available();
               deckpilot.shell.run({ shell: "sh", script: "make -j4 && make install", cwd: { repository: "cms", relative: "" }, timeoutMs: 30000 })"#,
            &["shell.available:available", "shell.run:sh"],
        ),
    ]
}

/// Runs a driver; returns the requests it emitted, as bytes.
fn emitted(f: &Value, driver: &Driver) -> Vec<Vec<u8>> {
    let f2 = f.clone();
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
                return json!({ "ok": { "content": base64::encode(&bytes[offset..end]), "size": n, "eof": end == n } });
            }
            let mut calls = calls.borrow_mut();
            let k = calls.entry(op.to_owned()).or_default();
            let name = answers
                .iter()
                .find(|(o, _)| *o == op)
                .and_then(|(_, names)| names.get(*k).copied())
                .unwrap_or("ok");
            *k += 1;
            reply(&f2, op, name)
        }),
    );
    let mut load = reply(f, "script.load", "ok")["ok"].clone();
    load["source"] = json!(driver.source);
    load["platform"] = json!(driver.platform);
    mock.set(|s| {
        s.load = load;
        s.cancel_at_poll = driver.cancel_at_poll;
    });
    let _ = run_mock(&mock);
    mock.0.borrow().raw.clone()
}

#[test]
fn every_request_the_guest_emits_is_the_fixtures_byte_for_byte() {
    let f = fixture();
    let load = canonical(request(&f, "script.load:load"));
    let mut covered: BTreeSet<String> = BTreeSet::from(["script.load:load".to_owned()]);
    for driver in drivers() {
        let raw = emitted(&f, &driver);
        let got: Vec<String> = raw
            .iter()
            .map(|r| String::from_utf8(r.clone()).unwrap())
            .collect();
        let mut want = vec![load.clone()];
        for case in driver.expect {
            want.push(canonical(request(&f, case)));
            covered.insert((*case).to_owned());
        }
        assert_eq!(got, want, "{}: the emitted requests", driver.name);
    }
    let all: BTreeSet<String> = f["ops"]
        .as_object()
        .unwrap()
        .iter()
        .flat_map(|(op, o)| {
            o["requests"]
                .as_object()
                .unwrap()
                .keys()
                .map(move |name| format!("{op}:{name}"))
        })
        .collect();
    assert_eq!(
        all.difference(&covered).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "fixture requests no driver emits"
    );
}

#[test]
fn the_fixture_has_every_wire_op_and_nothing_else() {
    let f = fixture();
    let ops: BTreeSet<&str> = f["ops"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let wire: BTreeSet<&str> = WIRE_OPS.iter().copied().collect();
    assert_eq!(ops, wire);
    for (op, o) in f["ops"].as_object().unwrap() {
        assert!(
            !o["requests"].as_object().unwrap().is_empty(),
            "{op}: no request"
        );
        assert!(o["replies"].get("ok").is_some(), "{op}: no ok reply");
        for (name, r) in o["requests"].as_object().unwrap() {
            assert_eq!(r["op"], json!(op), "{op}:{name}");
            assert_eq!(r.as_object().unwrap().len(), 2, "{op}:{name}: op and args");
            assert!(r["args"].is_object(), "{op}:{name}");
        }
    }
    assert!(
        f["$comment"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c.as_str().unwrap().contains("proposed ABI for P6"))
    );
}

#[test]
fn every_request_and_reply_fits_the_host_caps() {
    let f = fixture();
    assert_eq!(f["limits"]["maxRequestBytes"], json!(limits::REQUEST));
    assert_eq!(f["limits"]["maxReplyBytes"], json!(limits::REPLY));
    assert_eq!(
        f["limits"]["readChunkBytes"],
        json!(crate::api::fs::READ_CHUNK)
    );
    assert_eq!(
        f["limits"]["writeChunkBytes"],
        json!(crate::api::fs::WRITE_CHUNK)
    );
    assert_eq!(
        f["limits"]["processArgsBytes"],
        json!(crate::api::process::MAX_ARGS_TOTAL)
    );
    assert_eq!(
        f["limits"]["processStdinBytes"],
        json!(crate::api::process::MAX_STDIN)
    );
    assert_eq!(
        f["limits"]["shellScriptBytes"],
        json!(crate::api::process::MAX_SHELL_SCRIPT)
    );
    assert_eq!(f["limits"]["hostCalls"], json!(limits::HOST_CALLS));
    assert_eq!(f["limits"]["polls"], json!(limits::POLLS));
    let mut largest = 0;
    for (op, o) in f["ops"].as_object().unwrap() {
        for (name, r) in o["requests"].as_object().unwrap() {
            let n = canonical(r).len();
            largest = largest.max(n);
            assert!(n < limits::REQUEST, "{op}:{name}: {n} bytes");
        }
        for (name, r) in o["replies"].as_object().unwrap() {
            let n = canonical(r).len();
            assert!(n < limits::REPLY, "{op}:{name}: {n} bytes");
        }
    }
    // The largest chunk is in the fixture: 32 KiB of data, as base64.
    let chunk = request(&f, "fs.write.chunk:largest")["args"]["data"]
        .as_str()
        .unwrap();
    assert_eq!(
        base64::decode(chunk).unwrap().len(),
        crate::api::fs::WRITE_CHUNK
    );
    assert!(largest > 43_000, "{largest}");
    // And the largest read reply the guest can ask for fits too.
    let read = json!({ "ok": { "content": base64::encode(&vec![0xffu8; crate::api::fs::READ_CHUNK as usize]), "size": 4194304, "eof": false, "nowUs": 1.7e15 } });
    assert!(canonical(&read).len() < limits::REPLY);
}

/// Answers every request with one reply.
struct Fixed(Value);

impl Transport for Fixed {
    fn call(&mut self, _: &[u8]) -> Option<Vec<u8>> {
        Some(serde_json::to_vec(&self.0).unwrap())
    }
}

#[test]
fn the_guest_accepts_every_reply_of_the_fixture() {
    let f = fixture();
    for (op, o) in f["ops"].as_object().unwrap() {
        for (name, r) in o["replies"].as_object().unwrap() {
            let mut s = Session::new(Box::new(Fixed(r.clone())));
            if op == "script.poll" {
                let stop = s.poll();
                assert_eq!(stop, name != "ok", "{op}:{name}");
                assert!(s.budget.remaining_ms.is_some(), "{op}:{name}");
                continue;
            }
            let got = s.call(op, json!({}));
            match r.get("error") {
                None => {
                    assert!(got.is_ok(), "{op}:{name}: {got:?}");
                }
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
