//! Chunked I/O (P6 decision D1), the 64 KiB request cap, timeouts clamped
//! to the run's remaining time, and the audit of cancellation: what the
//! guest sends, and stops sending, when a run is stopped or out of time.

use std::cell::RefCell;
use std::rc::Rc;

use serde_json::{Value, json};

use crate::api::base64;
use crate::api::fs::{READ_CHUNK, WRITE_CHUNK};
use crate::bridge::limits;
use crate::mock::{Mock, exit, ok, refuse};
use crate::tests::{failed, result, run_mock};

/// A file the mock serves by offset and length, as the agent would; `edit`
/// may change it between chunks (it gets the chunk's index).
fn serving(source: &str, file: Vec<u8>, edit: impl FnMut(usize, &mut Vec<u8>) + 'static) -> Mock {
    let file = Rc::new(RefCell::new(file));
    let mut edit = edit;
    let mut chunk = 0;
    Mock::with(
        source,
        json!({}),
        Box::new(move |op, args| match op {
            "fs.readFile" => {
                edit(chunk, &mut file.borrow_mut());
                chunk += 1;
                let f = file.borrow();
                let offset = args["offset"].as_u64().unwrap() as usize;
                let length = args["length"].as_u64().unwrap() as usize;
                let start = offset.min(f.len());
                let end = (offset + length).min(f.len());
                ok(json!({
                    "content": base64::encode(&f[start..end]),
                    "bytes": end - start,
                    "size": f.len(),
                    "eof": end == f.len(),
                }))
            }
            "script.log" => ok(json!(null)),
            other => panic!("{other} {args}"),
        }),
    )
}

fn pattern(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i % 251) as u8).collect()
}

fn reads(mock: &Mock) -> Vec<(u64, u64)> {
    mock.0
        .borrow()
        .of("fs.readFile")
        .iter()
        .map(|a| (a["offset"].as_u64().unwrap(), a["length"].as_u64().unwrap()))
        .collect()
}

// ---- reads ---------------------------------------------------------------------

#[test]
fn a_small_file_is_one_chunk() {
    let mock = serving(
        "deckpilot.fs.readFile('/w/a.txt')",
        "zażółć\n".as_bytes().to_vec(),
        |_, _| {},
    );
    assert_eq!(result(run_mock(&mock)), json!("zażółć\n"));
    assert_eq!(reads(&mock), vec![(0, READ_CHUNK)]);
    assert_eq!(
        mock.0.borrow().of("fs.readFile")[0]["encoding"],
        "base64",
        "the wire is always base64"
    );
}

#[test]
fn a_large_file_is_read_in_chunks_and_joined() {
    let file = pattern(1_200_000);
    let mock = serving(
        "const c = deckpilot.fs.readFile('/w/big.bin', { encoding: 'base64' }); \
         [c.length, c.slice(0, 64), c.slice(699000, 699100), c.slice(1398050, 1398150), c.slice(-64)]",
        file.clone(),
        |_, _| {},
    );
    let b = base64::encode(&file);
    assert_eq!(
        result(run_mock(&mock)),
        json!([
            b.len(),
            &b[..64],
            &b[699000..699100],
            &b[1398050..1398150],
            &b[b.len() - 64..]
        ])
    );
    assert_eq!(
        reads(&mock),
        vec![
            (0, READ_CHUNK),
            (READ_CHUNK, READ_CHUNK),
            (2 * READ_CHUNK, 1_200_000 - 2 * READ_CHUNK)
        ]
    );
}

#[test]
fn a_file_of_exactly_the_limit_reads_and_one_byte_more_is_too_large() {
    let max = crate::api::fs::MAX_FILE as usize;
    let mock = serving(
        "deckpilot.fs.readFile('/w/f', { encoding: 'base64' }).length",
        pattern(max),
        |_, _| {},
    );
    assert_eq!(result(run_mock(&mock)), json!(base64::encoded_len(max)));
    assert_eq!(reads(&mock).len(), 8);
    let mock = serving(
        "try { deckpilot.fs.readFile('/w/f') } catch (e) { e.code }",
        pattern(max + 1),
        |_, _| {},
    );
    assert_eq!(result(run_mock(&mock)), json!("tooLarge"));
    assert_eq!(reads(&mock).len(), 1, "refused at the first chunk");
    let mock = serving(
        "try { deckpilot.fs.readFile('/w/f', { maxBytes: 100 }) } catch (e) { e.code }",
        pattern(101),
        |_, _| {},
    );
    assert_eq!(result(run_mock(&mock)), json!("tooLarge"));
    assert_eq!(reads(&mock), vec![(0, 100)]);
}

#[test]
fn a_file_that_changes_size_between_chunks_is_never_spliced() {
    for (what, edit) in [
        (
            "grew",
            Box::new(|i: usize, f: &mut Vec<u8>| {
                if i == 1 {
                    f.push(1)
                }
            }) as Box<dyn FnMut(usize, &mut Vec<u8>)>,
        ),
        (
            "shrank",
            Box::new(|i: usize, f: &mut Vec<u8>| {
                if i == 1 {
                    f.truncate(600_000)
                }
            }),
        ),
        (
            "replaced",
            Box::new(|i: usize, f: &mut Vec<u8>| {
                if i == 2 {
                    *f = pattern(10)
                }
            }),
        ),
    ] {
        let mut edit = edit;
        let mock = serving(
            "try { deckpilot.fs.readFile('/w/log.txt', { encoding: 'base64' }) } catch (e) { [e.code, e.path] }",
            pattern(1_100_000),
            move |i, f| edit(i, f),
        );
        assert_eq!(
            result(run_mock(&mock)),
            json!(["fileChanged", "/w/log.txt"]),
            "{what}"
        );
    }
}

#[test]
fn a_chunk_that_does_not_match_its_request_fails() {
    // The agent says eof before the size it reported, or sends a chunk of
    // another length: either way the bytes are not one version.
    for reply in [
        json!({ "content": base64::encode(&[1; 10]), "size": 20, "eof": true }),
        json!({ "content": base64::encode(&[1; 10]), "size": 10, "eof": false }),
        json!({ "content": base64::encode(&[1; 12]), "size": 10, "eof": true }),
    ] {
        let mock = Mock::with(
            "try { deckpilot.fs.readFile('/w/a') } catch (e) { e.code }",
            json!({}),
            Box::new(move |_, _| ok(reply.clone())),
        );
        assert_eq!(result(run_mock(&mock)), json!("fileChanged"));
    }
    let mock = Mock::with(
        "try { deckpilot.fs.readFile('/w/a') } catch (e) { e.code }",
        json!({}),
        Box::new(|_, _| ok(json!({ "content": "not base64!", "size": 3, "eof": true }))),
    );
    assert_eq!(result(run_mock(&mock)), json!("hostError"));
}

#[test]
fn text_that_is_not_utf8_is_refused_and_base64_reads_it() {
    let src = "const out = {}; try { deckpilot.fs.readFile('/w/x') } catch (e) { out.utf8 = e.code } out.b64 = deckpilot.fs.readFile('/w/x', { encoding: 'base64' }); out";
    let mock = serving(src, vec![0xff, 0xfe, 0x00], |_, _| {});
    assert_eq!(
        result(run_mock(&mock)),
        json!({ "utf8": "invalidArgument", "b64": "//4A" })
    );
}

#[test]
fn an_empty_file_is_one_empty_chunk() {
    let mock = serving("deckpilot.fs.readFile('/w/empty')", Vec::new(), |_, _| {});
    assert_eq!(result(run_mock(&mock)), json!(""));
    assert_eq!(reads(&mock), vec![(0, READ_CHUNK)]);
}

// ---- writes --------------------------------------------------------------------

/// A bridge that accepts writes and records the bytes of each handle;
/// `fail(op, chunk index)` may answer instead.
fn writing(source: &str, fail: impl Fn(&str, usize) -> Option<Value> + 'static) -> Mock {
    let mut chunks = 0;
    Mock::with(
        source,
        json!({}),
        Box::new(move |op, args| {
            let index = chunks;
            if op == "fs.write.chunk" {
                chunks += 1;
            }
            if let Some(reply) = fail(op, index) {
                return reply;
            }
            match op {
                "fs.write.open" => ok(json!({ "handle": "wh-7f3a" })),
                "fs.writeFile" => ok(json!({})),
                "fs.write.chunk" | "fs.write.commit" | "fs.write.abort" => {
                    assert_eq!(args["handle"], "wh-7f3a");
                    ok(json!({}))
                }
                "script.log" => ok(json!(null)),
                other => panic!("{other} {args}"),
            }
        }),
    )
}

fn written(mock: &Mock) -> Vec<u8> {
    mock.0
        .borrow()
        .of("fs.write.chunk")
        .iter()
        .flat_map(|a| {
            assert_eq!(a["encoding"], "base64");
            base64::decode(a["content"].as_str().unwrap()).unwrap()
        })
        .collect()
}

#[test]
fn a_write_is_open_chunks_commit() {
    let mock = writing(
        "deckpilot.fs.writeFile({ repository: 'cms', relative: 'dist/app.js' }, 'é'.repeat(50000))",
        |_, _| None,
    );
    run_mock(&mock).unwrap();
    let s = mock.0.borrow();
    assert_eq!(
        s.ops(),
        vec![
            "fs.write.open",
            "fs.write.chunk",
            "fs.write.chunk",
            "fs.write.chunk",
            "fs.write.chunk",
            "fs.write.commit"
        ]
    );
    assert_eq!(
        s.requests[0].1,
        json!({ "path": { "repository": "cms", "relative": "dist/app.js" }, "createOnly": false })
    );
    drop(s);
    assert_eq!(written(&mock), "é".repeat(50000).into_bytes());
    for a in mock.0.borrow().of("fs.write.chunk") {
        assert!(
            base64::decode(a["content"].as_str().unwrap())
                .unwrap()
                .len()
                <= WRITE_CHUNK
        );
    }
}

#[test]
fn a_small_write_is_one_request_in_the_scripts_encoding() {
    let mock = writing("deckpilot.fs.writeFile('/w/e', '')", |_, _| None);
    run_mock(&mock).unwrap();
    assert_eq!(
        mock.0.borrow().requests,
        vec![(
            "fs.writeFile".to_owned(),
            json!({ "path": { "path": "/w/e" }, "content": "", "encoding": "utf8", "createOnly": false })
        )]
    );
    let mock = writing(
        "deckpilot.fs.writeFile('/w/b', 'AAEC/w==', { encoding: 'base64', createOnly: true })",
        |_, _| None,
    );
    run_mock(&mock).unwrap();
    assert_eq!(
        mock.0.borrow().of("fs.writeFile"),
        vec![
            &json!({ "path": { "path": "/w/b" }, "content": "AAEC/w==", "encoding": "base64", "createOnly": true })
        ]
    );
    // Exactly 32 KiB is still one request; one byte more is chunked.
    let mock = writing(
        "deckpilot.fs.writeFile('/w/c', 'x'.repeat(32768))",
        |_, _| None,
    );
    run_mock(&mock).unwrap();
    assert_eq!(mock.0.borrow().ops(), vec!["fs.writeFile"]);
    let mock = writing(
        "deckpilot.fs.writeFile('/w/c', 'x'.repeat(32769))",
        |_, _| None,
    );
    run_mock(&mock).unwrap();
    assert_eq!(
        mock.0.borrow().ops(),
        vec![
            "fs.write.open",
            "fs.write.chunk",
            "fs.write.chunk",
            "fs.write.commit"
        ]
    );
    assert_eq!(written(&mock), vec![b'x'; 32769]);
}

#[test]
fn a_short_chunk_is_read_on_from_where_it_ended() {
    let file = pattern(1000);
    let served = file.clone();
    let mock = Mock::with(
        "deckpilot.fs.readFile('/w/a', { encoding: 'base64' })",
        json!({}),
        Box::new(move |_, args| {
            // The agent sends at most 300 bytes per chunk.
            let offset = args["offset"].as_u64().unwrap() as usize;
            let end = (offset + 300).min(served.len());
            ok(
                json!({ "content": base64::encode(&served[offset..end]), "bytes": end - offset, "size": served.len(), "eof": end == served.len() }),
            )
        }),
    );
    assert_eq!(result(run_mock(&mock)), json!(base64::encode(&file)));
    assert_eq!(
        reads(&mock),
        vec![(0, READ_CHUNK), (300, 700), (600, 400), (900, 100)]
    );
    // `bytes` must say what `content` holds.
    let mock = Mock::with(
        "try { deckpilot.fs.readFile('/w/a') } catch (e) { e.code }",
        json!({}),
        Box::new(|_, _| ok(json!({ "content": "aGk=", "bytes": 3, "size": 2, "eof": true }))),
    );
    assert_eq!(result(run_mock(&mock)), json!("hostError"));
}

#[test]
fn heavily_escaped_small_text_goes_in_base64_chunks() {
    let mock = writing(
        "deckpilot.fs.writeFile('/w/x', '\\u0001'.repeat(20000))",
        |_, _| None,
    );
    run_mock(&mock).unwrap();
    assert_eq!(
        mock.0.borrow().ops(),
        vec!["fs.write.open", "fs.write.chunk", "fs.write.commit"]
    );
    assert_eq!(written(&mock), vec![1; 20000]);
}

#[test]
fn a_failed_chunk_or_commit_aborts_the_handle() {
    // The counter is the chunk index: the commit comes after chunk 3.
    for (op, at) in [("fs.write.chunk", 1), ("fs.write.commit", 3)] {
        let mock = writing(
            "try { deckpilot.fs.writeFile('/w/x', 'x'.repeat(70000)) } catch (e) { e.code }",
            move |o, i| {
                (o == op && i == at)
                    .then(|| refuse("failed", "notGranted", "the grant was revoked"))
            },
        );
        assert_eq!(result(run_mock(&mock)), json!("notGranted"), "{op}");
        let ops = mock
            .0
            .borrow()
            .ops()
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        assert_eq!(ops.last().unwrap(), "fs.write.abort", "{op}: {ops:?}");
        assert_eq!(ops.iter().filter(|o| *o == "fs.write.abort").count(), 1);
        assert!(!ops.contains(&"fs.write.commit".to_owned()) || op == "fs.write.commit");
    }
}

#[test]
fn a_stop_during_a_write_aborts_the_handle_and_ends_the_run() {
    let src = r#"
        try { deckpilot.fs.writeFile('/w/x', 'x'.repeat(100000)) } catch (e) { globalThis.code = e.code }
        deckpilot.log.info("after the stop");
        'finished'
    "#;
    for (code, outcome) in [("cancelled", "cancelled"), ("timedOut", "timed-out")] {
        let mock = writing(src, move |o, i| {
            (o == "fs.write.chunk" && i == 1).then(|| refuse("failed", code, "the run was stopped"))
        });
        let (c, _) = failed(run_mock(&mock));
        assert_eq!(c, outcome);
        let s = mock.0.borrow();
        assert_eq!(
            s.ops(),
            vec![
                "fs.write.open",
                "fs.write.chunk",
                "fs.write.chunk",
                "fs.write.abort"
            ],
            "{code}: the abort, and nothing after it"
        );
    }
}

#[test]
fn a_write_starts_only_when_all_its_calls_fit_the_budget() {
    // script.load and 4,090 exists leave 5 calls: a write of 5 chunks needs
    // 8 (open, 5 chunks, commit, a possible abort).
    let src = "for (let i = 0; i < 4090; i++) deckpilot.fs.exists('/w/x'); \
               try { deckpilot.fs.writeFile('/w/x', 'x'.repeat(5 * 32768)) } catch (e) { e.code }";
    let mock = Mock::with(
        src,
        json!({}),
        Box::new(|op, _| match op {
            "fs.exists" => ok(json!({ "exists": true })),
            other => panic!("{other} sent"),
        }),
    );
    assert_eq!(result(run_mock(&mock)), json!("limit"));
    assert_eq!(mock.0.borrow().requests.len(), 4090);
}

#[test]
fn an_open_without_a_handle_is_a_host_error_and_nothing_is_aborted() {
    let mock = writing(
        "try { deckpilot.fs.writeFile('/w/x', 'x'.repeat(40000)) } catch (e) { e.code }",
        |op, _| (op == "fs.write.open").then(|| ok(json!({ "handle": "a b" }))),
    );
    assert_eq!(result(run_mock(&mock)), json!("hostError"));
    assert_eq!(mock.0.borrow().ops(), vec!["fs.write.open"]);
}

#[test]
fn the_largest_chunk_fits_one_request() {
    let mock = writing(
        "deckpilot.fs.writeFile('/w/x', '\\u0001'.repeat(32768))",
        |_, _| None,
    );
    run_mock(&mock).unwrap();
    let s = mock.0.borrow();
    let biggest = s.raw.iter().map(Vec::len).max().unwrap();
    assert!(biggest < limits::REQUEST, "{biggest}");
    assert!(biggest > 43_000, "a whole 32 KiB chunk: {biggest}");
}

// ---- processes: the request cap and the timeout --------------------------------

#[test]
fn process_and_shell_values_over_48_kib_never_reach_the_agent() {
    let big = 48 * 1024 + 1;
    for src in [
        format!(
            "deckpilot.process.run({{ command: 'git', args: ['a'.repeat({}), 'b'.repeat(16385)] }})",
            32 * 1024
        ),
        format!("deckpilot.process.run({{ command: 'git', stdin: 'y'.repeat({big}) }})"),
        format!(
            "deckpilot.shell.run({{ shell: 'sh', script: 'echo'.repeat({}) }})",
            big / 4 + 1
        ),
    ] {
        let mock = Mock::with(
            &format!("try {{ {src}; 'sent' }} catch (e) {{ e.code }}"),
            json!({ "shell": ["sh"] }),
            Box::new(|op, _| panic!("{op} reached the agent")),
        );
        assert_eq!(result(run_mock(&mock)), json!("invalidArgument"), "{src}");
    }
    // Each value within its limit, but the request over 64 KiB (two 32 KiB
    // environment values): refused as a limit, before any host call.
    let mock = Mock::with(
        "try { deckpilot.process.run({ command: 'git', env: { A: 'a'.repeat(32768), B: 'b'.repeat(32768) } }); 'sent' } catch (e) { e.code }",
        json!({}),
        Box::new(|op, _| panic!("{op} reached the agent")),
    );
    assert_eq!(result(run_mock(&mock)), json!("limit"));
}

#[test]
fn a_timeout_never_outlasts_the_run() {
    // script.load: 5 s left at 1,700,000,000 s; the first process takes the
    // agent's clock 2 s further.
    let mock = Mock::with(
        r#"
        deckpilot.process.run({ command: "make", timeoutMs: 60000 });
        deckpilot.process.run({ command: "make", timeoutMs: 60000 });
        deckpilot.process.run({ command: "make", timeoutMs: 1000 });
        deckpilot.process.run({ command: "make" });
        deckpilot.shell.run({ shell: "sh", script: "true", timeoutMs: 600000 });
        "#,
        json!({ "shell": ["sh"] }),
        Box::new(|_, _| {
            let mut r = exit(0, "", "");
            r["ok"]["nowUs"] = json!(1_700_000_002_000_000.0_f64);
            r
        }),
    );
    mock.set(|s| s.load["remainingMs"] = json!(5000));
    run_mock(&mock).unwrap();
    let s = mock.0.borrow();
    let timeouts: Vec<Value> = s
        .requests
        .iter()
        .map(|(_, a)| a.get("timeoutMs").cloned().unwrap_or(Value::Null))
        .collect();
    assert_eq!(
        timeouts,
        vec![
            json!(5000),
            json!(3000),
            json!(1000),
            Value::Null,
            json!(3000)
        ]
    );
}

#[test]
fn the_remaining_time_of_the_last_poll_clamps_too() {
    let mock = Mock::with(
        "let n = 0; for (let i = 0; i < 4e6; i++) n += i; deckpilot.process.run({ command: 'make', timeoutMs: 60000 })",
        json!({}),
        Box::new(|_, _| exit(0, "", "")),
    );
    mock.set(|s| s.poll_remaining_ms = 1234);
    run_mock(&mock).unwrap();
    let s = mock.0.borrow();
    assert!(s.polls >= 1);
    assert_eq!(s.of("process.run")[0]["timeoutMs"], 1234);
}

#[test]
fn no_time_left_sends_no_process() {
    let mock = Mock::with(
        "try { deckpilot.process.run({ command: 'make' }) } catch (e) { globalThis.code = e.code } 'done'",
        json!({}),
        Box::new(|op, _| panic!("{op} sent")),
    );
    mock.set(|s| s.load["remainingMs"] = json!(0));
    let (code, _) = failed(run_mock(&mock));
    assert_eq!(code, "timed-out");
}

// ---- cancellation ------------------------------------------------------------------

#[test]
fn a_stop_seen_at_a_poll_runs_no_finally_and_sends_nothing() {
    let mock = Mock::with(
        "try { while (true) {} } finally { deckpilot.fs.exists('/w/x'); deckpilot.log.info('finally') }",
        json!({}),
        Box::new(|op, _| panic!("{op} sent after the stop")),
    );
    mock.set(|s| s.cancel_at_poll = Some(1));
    let (code, _) = failed(run_mock(&mock));
    assert_eq!(code, "cancelled");
    let s = mock.0.borrow();
    assert!(s.requests.is_empty());
    assert_eq!(s.polls, 1, "no poll after the stop either");
}

#[test]
fn a_caught_stop_still_ends_the_run_and_nothing_is_sent_after_it() {
    let src = r#"
        const out = {};
        try { deckpilot.process.run({ command: "make" }) } catch (e) { out.first = e.code }
        const attempt = (name, f) => { try { f(); out[name] = "ran" } catch (e) { out[name] = e.code } };
        attempt("exists", () => deckpilot.fs.exists("/w/x"));
        attempt("read", () => deckpilot.fs.readFile("/w/x"));
        attempt("write", () => deckpilot.fs.writeFile("/w/x", "y"));
        attempt("remove", () => deckpilot.fs.remove("/w/x"));
        attempt("sleep", () => deckpilot.sleep(1));
        attempt("git", () => deckpilot.git.run(["status"], { cwd: "/w" }));
        attempt("projects", () => deckpilot.projects.list());
        deckpilot.log.info("dropped");
        out.aborted = deckpilot.run.signal.aborted;
        out.path = deckpilot.path.join("/a", "b");
        Promise.resolve().then(() => deckpilot.fs.exists("/w/later"));
        out
    "#;
    let mock = Mock::with(
        src,
        json!({}),
        Box::new(|op, _| match op {
            "process.run" => refuse("failed", "cancelled", "the run was stopped"),
            other => panic!("{other} sent after the stop"),
        }),
    );
    // The script caught the stop and finished: the run is still stopped.
    let (code, _) = failed(run_mock(&mock));
    assert_eq!(code, "cancelled");
    assert_eq!(mock.0.borrow().ops(), vec!["process.run"]);
}

#[test]
fn a_stop_is_reported_as_one_and_the_ops_fail_with_it() {
    let src = r#"
        const out = {};
        try { deckpilot.process.run({ command: "make" }) } catch (e) { out.first = e.code }
        try { deckpilot.fs.exists("/w/x") } catch (e) { out.later = e.code }
        out.aborted = deckpilot.run.signal.aborted;
        throw new ScriptError("noProject", JSON.stringify(out));
    "#;
    let mock = Mock::with(
        src,
        json!({}),
        Box::new(|_, _| refuse("failed", "cancelled", "stopped")),
    );
    let (code, _) = failed(run_mock(&mock));
    assert_eq!(
        code, "cancelled",
        "the stop overrides what the script threw"
    );
}

#[test]
fn a_cancelled_or_timed_out_sleep_ends_the_run() {
    for (code, outcome) in [("cancelled", "cancelled"), ("timedOut", "timed-out")] {
        let mock = Mock::with(
            "try { deckpilot.sleep(60000) } catch (e) { globalThis.c = e.code } deckpilot.fs.exists('/w/x'); 'done'",
            json!({}),
            Box::new(move |op, _| match op {
                "script.sleep" => refuse("failed", code, "stopped while sleeping"),
                other => panic!("{other} sent after the stop"),
            }),
        );
        let (c, _) = failed(run_mock(&mock));
        assert_eq!(c, outcome);
        assert_eq!(mock.0.borrow().ops(), vec!["script.sleep"]);
    }
}

#[test]
fn a_stop_during_the_confirmation_removes_nothing() {
    let src = "try { deckpilot.fs.remove({ repository: 'cms', relative: 'build' }, { recursive: true }) } catch (e) { globalThis.c = e.code } 'done'";
    let mock = Mock::with(
        src,
        json!({}),
        Box::new(|op, args| match op {
            "fs.remove" if args.get("confirmation").is_none() => {
                json!({ "error": { "code": "not-permitted", "message": "confirm",
                "script": { "code": "needsConfirmation", "message": "remove 3 files", "confirmation": { "token": "tok-1" } } } })
            }
            "script.confirm" => refuse(
                "failed",
                "cancelled",
                "the run was stopped during the confirmation",
            ),
            other => panic!("{other} {args} sent after the stop"),
        }),
    );
    let (code, _) = failed(run_mock(&mock));
    assert_eq!(code, "cancelled");
    assert_eq!(mock.0.borrow().ops(), vec!["fs.remove", "script.confirm"]);
}

#[test]
fn a_stop_seen_at_a_poll_during_promise_jobs_ends_them() {
    let src = "(async () => { for (;;) { await null; for (let i = 0; i < 1e5; i++); } })()";
    let mock = Mock::with(src, json!({}), Box::new(|op, _| panic!("{op} sent")));
    mock.set(|s| s.cancel_at_poll = Some(2));
    let (code, _) = failed(run_mock(&mock));
    assert_eq!(code, "cancelled");
    assert_eq!(mock.0.borrow().polls, 2);
}

// ---- promises: the job budget and unhandled rejections ---------------------------

#[test]
fn a_promise_that_runs_out_of_jobs_says_so() {
    let (code, message) = failed(run_mock(&Mock::new(
        "(function f() { return Promise.resolve().then(f) })()",
    )));
    assert_eq!(code, "limit");
    assert!(message.contains("job budget ran out"), "{message}");
    // Jobs queued by a script that finished: the same budget.
    let (code, message) = failed(run_mock(&Mock::new(
        "Promise.resolve().then(function f() { Promise.resolve().then(f) }); 'done'",
    )));
    assert_eq!(code, "limit");
    assert!(message.contains("job budget ran out"), "{message}");
}

#[test]
fn the_unhandled_rejection_named_is_the_one_still_unhandled() {
    let src = "const a = Promise.reject(new Error('first')); Promise.reject(new TypeError('second')); a.catch(() => {}); 'done'";
    let (code, message) = failed(run_mock(&Mock::new(src)));
    assert_eq!(code, "script-error");
    assert!(message.contains("TypeError: second"), "{message}");
    assert!(!message.contains("first"), "{message}");
    // Handled later, in a job: none left.
    let src = "const a = Promise.reject(new Error('x')); Promise.resolve().then(() => a.catch(() => {})); 'done'";
    assert_eq!(result(run_mock(&Mock::new(src))), json!("done"));
}

#[test]
fn a_confirmed_getter_on_object_prototype_does_not_confirm() {
    let src = r#"
        Object.defineProperty(Object.prototype, "confirmed", { get: () => true, configurable: true });
        try { deckpilot.fs.remove({ repository: "cms", relative: "build" }, { recursive: true }); "removed" } catch (e) { e.code }
    "#;
    let mock = Mock::with(
        src,
        json!({}),
        Box::new(|op, args| match op {
            "fs.remove" if args.get("confirmation").is_none() => {
                json!({ "error": { "code": "not-permitted", "message": "confirm",
                "script": { "code": "needsConfirmation", "message": "remove 3 files", "confirmation": { "token": "tok-1" } } } })
            }
            "script.confirm" => ok(json!({})),
            other => panic!("{other} {args}: the op was repeated"),
        }),
    );
    assert_eq!(result(run_mock(&mock)), json!("needsConfirmation"));
    assert_eq!(mock.0.borrow().ops(), vec!["fs.remove", "script.confirm"]);
}

#[test]
fn a_sleep_never_outlasts_the_run() {
    let mock = Mock::with(
        "deckpilot.sleep(5000); deckpilot.sleep(50)",
        json!({}),
        Box::new(|_, _| ok(json!({ "nowUs": 1_700_000_000_000_000.0_f64 }))),
    );
    mock.set(|s| s.load["remainingMs"] = json!(100));
    run_mock(&mock).unwrap();
    assert_eq!(
        mock.0.borrow().of("script.sleep"),
        vec![&json!({ "ms": 100 }), &json!({ "ms": 50 })]
    );
    let mock = Mock::with(
        "try { deckpilot.sleep(10) } catch (e) { globalThis.c = e.code } 'done'",
        json!({}),
        Box::new(|op, _| panic!("{op} sent")),
    );
    mock.set(|s| s.load["remainingMs"] = json!(0));
    assert_eq!(failed(run_mock(&mock)).0, "timed-out");
}

#[test]
fn a_bigint_or_a_changing_value_never_reaches_the_agent() {
    let src = r#"
        const out = {};
        try { deckpilot.fs.removeContents("/w/x", { keep: [1n] }) } catch (e) { out.bigint = e.code }
        // A getter that answers a string once, then something else.
        let n = 0;
        const args = []; Object.defineProperty(args, 0, { get: () => (n++ === 0 ? "status" : 42), enumerable: true });
        args.length = 1;
        const r = deckpilot.process.run({ command: "git", args });
        let k = 0;
        const where = { get repository() { return k++ === 0 ? "cms" : 7 }, relative: "" };
        deckpilot.fs.exists(where);
        out
    "#;
    let mock = Mock::with(
        src,
        json!({}),
        Box::new(|op, _| match op {
            "process.run" => exit(0, "", ""),
            "fs.exists" => ok(json!({ "exists": true })),
            other => panic!("{other} sent"),
        }),
    );
    assert_eq!(
        result(run_mock(&mock)),
        json!({ "bigint": "invalidArgument" })
    );
    let s = mock.0.borrow();
    assert_eq!(s.of("process.run")[0]["args"], json!(["status"]));
    assert_eq!(
        s.of("fs.exists")[0],
        &json!({ "path": { "repository": "cms", "relative": "" } })
    );
}
