//! Native tests of the runtime and the guest side of the bridge, against
//! [`crate::mock`]: limits (CPU loop, memory, recursion, log flood, host
//! calls), cancellation and timeouts, syntax errors, the sandbox (no
//! `eval`, `Function`, `import`, `__dp`), permission refusals, and the
//! structured `process.run` requests of the owner's examples (Docker
//! Compose and `git pull --ff-only`), which are only built and recorded,
//! never executed.

use serde_json::{Value, json};

use super::*;
use crate::mock::{Mock, exit, ok, refuse, repository};
use deckpilot_plugin_sdk::ParamValue;

fn run_mock(mock: &Mock) -> ActionResult {
    run(mock.boxed(), "11111111-2222-4333-8444-555555555555", None)
}

fn failed(r: ActionResult) -> (String, String) {
    match r {
        Err(e) => (e.code().to_owned(), e.message().to_owned()),
        Ok(o) => panic!("expected a failure, got {o:?}"),
    }
}

fn result(r: ActionResult) -> Value {
    let out = r.unwrap_or_else(|e| panic!("expected success, got {}: {}", e.code(), e.message()));
    match out.data().get("result") {
        Some(ParamValue::String(s)) => serde_json::from_str(s).unwrap(),
        other => panic!("no result: {other:?}"),
    }
}

/// A bridge that answers projects with three repositories and records
/// processes, answering each from `proc`.
fn three_repositories(source: &str, proc: impl FnMut(&Value) -> Value + 'static) -> Mock {
    let mut proc = proc;
    Mock::with(
        source,
        json!({ "projectsRead": true }),
        Box::new(move |op, args| match op {
            "script.log" => ok(json!(null)),
            "project.repositories" => ok(json!({
                "project": { "id": "p1", "key": "shop", "name": "Shop" },
                "repositories": [repository("cms", true), repository("api", false), repository("web", false)],
            })),
            "process.run" => proc(args),
            other => refuse("unsupported-op", "unsupported", other),
        }),
    )
}

// ---- results, inputs, check ---------------------------------------------

#[test]
fn a_script_returns_its_completion_value_and_sees_its_inputs() {
    let mock = Mock::new(
        "({ sum: 1 + 2, inputs: deckpilot.run.inputs, platform: deckpilot.platform, user: deckpilot.run.userInitiated })",
    );
    let r = run(mock.boxed(), "s1", Some(r#"{"name": "cms"}"#));
    assert_eq!(
        result(r),
        json!({ "sum": 3, "inputs": { "name": "cms" }, "platform": "linux", "user": true })
    );
    assert!(
        mock.0.borrow().requests.is_empty(),
        "no op beyond script.load"
    );
}

#[test]
fn inputs_that_are_not_json_are_refused_before_anything_runs() {
    let mock = Mock::new("1");
    let (code, _) = failed(run(mock.boxed(), "s1", Some("{nope")));
    assert_eq!(code, "invalid-parameters");
    assert_eq!(mock.0.borrow().polls, 0);
}

#[test]
fn check_compiles_without_running() {
    assert!(check("let x = 1;\nfunction f() { return x }").is_ok());
    // An endless loop compiles, and check returns at once: nothing runs.
    assert!(check("while (true) {}").is_ok());
    // Not even with a deckpilot call: there is no deckpilot in check.
    assert!(check("deckpilot.fs.remove('/', { recursive: true })").is_ok());
    let e = check("let a = 1;\nlet b = ;\n").unwrap_err();
    assert_eq!(e.code(), "syntax-error");
    assert!(e.message().contains("line 2"), "{}", e.message());
    // A trick to escape a wrapper cannot run anything: it is just invalid.
    assert_eq!(
        check("}); deckpilot.log.info('x'); (function(){")
            .unwrap_err()
            .code(),
        "syntax-error"
    );
    assert_eq!(
        check(&"x".repeat(limits::SOURCE + 1)).unwrap_err().code(),
        "too-large"
    );
}

#[test]
fn a_syntax_error_in_a_run_names_its_line() {
    let mock = Mock::new("const ok = 1;\nconst broken = {;\n");
    let (code, message) = failed(run_mock(&mock));
    assert_eq!(code, "syntax-error");
    assert!(message.contains("line 2"), "{message}");
    assert!(mock.0.borrow().requests.is_empty());
}

#[test]
fn a_thrown_error_fails_the_run_with_its_message_and_line() {
    let (code, message) = failed(run_mock(&Mock::new("\n\nnull.boom;")));
    assert_eq!(code, "script-error");
    assert!(message.starts_with("TypeError"), "{message}");
    assert!(message.contains("line 3"), "{message}");
    let (code, message) = failed(run_mock(&Mock::new(
        "throw new ScriptError('noProject', 'no project here')",
    )));
    assert_eq!(
        (code.as_str(), message.as_str()),
        ("no-project", "no project here")
    );
    let (code, _) = failed(run_mock(&Mock::new("throw 42")));
    assert_eq!(code, "script-error");
}

#[test]
fn outcome_codes_are_kebab_case() {
    assert_eq!(outcome_code("notGranted"), "not-granted");
    assert_eq!(outcome_code("scriptNeedsReview"), "script-needs-review");
    assert_eq!(outcome_code("limit"), "limit");
    assert_eq!(outcome_code("Weird Code!"), "weird-code");
    assert_eq!(outcome_code("???"), "script-error");
}

// ---- script.load ------------------------------------------------------------

#[test]
fn an_untrusted_or_missing_script_never_runs() {
    for (code, outcome) in [
        ("scriptNeedsReview", "script-needs-review"),
        ("scriptMissing", "script-missing"),
    ] {
        let mock = Mock::new("deckpilot.log.info('ran')");
        mock.set(|s| s.load_error = Some(refuse("not-permitted", code, "review the script first")));
        let (c, m) = failed(run_mock(&mock));
        assert_eq!(
            (c.as_str(), m.as_str()),
            (outcome, "review the script first")
        );
        assert!(
            mock.0.borrow().requests.is_empty(),
            "nothing after a refused load"
        );
    }
}

#[test]
fn an_agent_without_the_bridge_runs_nothing() {
    let mock = Mock::new("1");
    mock.set(|s| {
        s.load_error = Some(
            json!({ "error": { "code": "unsupported-op", "message": "unknown op script.load" } }),
        )
    });
    let (code, _) = failed(run_mock(&mock));
    assert_eq!(code, "unsupported");
}

#[test]
fn the_load_request_names_only_the_invocations_script() {
    struct Spy(std::rc::Rc<std::cell::RefCell<Vec<Value>>>);
    impl Transport for Spy {
        fn call(&mut self, request: &[u8]) -> Option<Vec<u8>> {
            self.0
                .borrow_mut()
                .push(serde_json::from_slice(request).unwrap());
            Some(br#"{"error":{"code":"not-permitted","message":"no"}}"#.to_vec())
        }
    }
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let _ = run(Box::new(Spy(seen.clone())), "abc", None);
    assert_eq!(
        *seen.borrow(),
        vec![json!({ "op": "script.load", "args": { "script": "abc" } })]
    );
}

// ---- the sandbox ------------------------------------------------------------

#[test]
fn there_is_no_code_from_strings_no_import_and_no_natives() {
    let probe = r#"
        const out = {};
        const attempt = (name, f) => { try { f(); out[name] = "ran"; } catch (e) { out[name] = e.name; } };
        attempt("eval", () => eval("1"));
        attempt("indirectEval", () => (0, globalThis.eval)("1"));
        attempt("Function", () => Function("return 1")());
        attempt("newFunction", () => new Function("return 1"));
        attempt("ctor", () => (() => 1).constructor("return 1")());
        attempt("generator", () => (function* () {}).constructor("yield 1"));
        attempt("async", () => (async function () {}).constructor("return 1"));
        attempt("asyncGenerator", () => (async function* () {}).constructor("yield 1"));
        attempt("reflect", () => Reflect.construct(Object.getPrototypeOf(() => 1).constructor, ["return 1"]));
        attempt("restoreFunction", () => { Function.prototype.constructor = null; if (Function.prototype.constructor === null) throw 0; });
        const imported = import("x");
        imported.then(() => { globalThis.importLoaded = true }, () => {});
        out.dynamicImport = typeof imported;
        out.natives = typeof globalThis.__dp;
        out.std = typeof globalThis.std + "," + typeof globalThis.os + "," + typeof globalThis.require + "," + typeof globalThis.process;
        out.frozen = Object.isFrozen(deckpilot) && Object.isFrozen(deckpilot.fs) && Object.isFrozen(deckpilot.run);
        attempt("replace", () => { "use strict"; globalThis.deckpilot = {}; });
        attempt("redefine", () => Object.defineProperty(globalThis, "deckpilot", { value: 1 }));
        out
    "#;
    let r = result(run_mock(&Mock::new(probe)));
    for name in [
        "eval",
        "indirectEval",
        "Function",
        "newFunction",
        "ctor",
        "generator",
        "async",
        "asyncGenerator",
        "reflect",
    ] {
        assert_eq!(r[name], "EvalError", "{name}: {r}");
    }
    assert_ne!(r["restoreFunction"], "ran", "{r}");
    assert_eq!(
        r["dynamicImport"], "object",
        "import() gives a rejected promise"
    );
    assert_eq!(r["natives"], "undefined");
    assert_eq!(r["std"], "undefined,undefined,undefined,undefined");
    assert_eq!(r["frozen"], true);
    assert_eq!(r["replace"], "TypeError");
    assert_eq!(r["redefine"], "TypeError");
}

#[test]
fn a_static_import_is_a_syntax_error() {
    let (code, _) = failed(run_mock(&Mock::new("import fs from 'fs';\nfs")));
    assert_eq!(code, "syntax-error");
}

#[test]
fn shell_exists_only_with_its_grant() {
    let r = result(run_mock(&Mock::new("typeof deckpilot.shell")));
    assert_eq!(r, "undefined");
    let mock = Mock::with(
        "deckpilot.shell.available()",
        json!({ "shell": ["sh"] }),
        Box::new(|op, _| {
            assert_eq!(op, "shell.available");
            ok(json!({ "shells": ["sh"] }))
        }),
    );
    assert_eq!(result(run_mock(&mock)), json!(["sh"]));
}

// ---- limits -----------------------------------------------------------------

#[test]
fn an_endless_loop_stops_when_the_agent_says_time_is_up() {
    let mock = Mock::new("let n = 0; while (true) { n++ }");
    mock.set(|s| s.timeout_at_poll = Some(3));
    let (code, _) = failed(run_mock(&mock));
    assert_eq!(code, "timed-out");
    assert_eq!(mock.0.borrow().polls, 3);
}

#[test]
fn an_endless_loop_stops_at_the_instruction_budget_without_the_agent() {
    let mock = Mock::new("for (;;) {}");
    let (code, message) = failed(run_with(mock.boxed(), "s1", None, 50));
    assert_eq!(code, "limit");
    assert!(message.contains("instruction budget"), "{message}");
}

#[test]
fn a_stop_cannot_be_caught() {
    let src = r#"
        try { while (true) {} } catch (e) { deckpilot.log.info("caught " + e) }
        deckpilot.log.info("after");
    "#;
    let mock = Mock::new(src);
    mock.set(|s| s.cancel_at_poll = Some(2));
    let (code, _) = failed(run_mock(&mock));
    assert_eq!(code, "cancelled");
    assert!(
        mock.0.borrow().logs().is_empty(),
        "nothing ran after the stop: {:?}",
        mock.0.borrow().logs()
    );
}

#[test]
fn a_stop_during_a_process_ends_the_run_even_if_the_script_catches_it() {
    let src = r#"
        try { deckpilot.process.run({ command: "sleep", args: ["100"] }) } catch (e) { globalThis.first = e.code }
        // The script tries to go on: every op is refused from now on.
        try { deckpilot.fs.exists("/tmp/x") } catch (e) { globalThis.second = e.code }
        while (true) {}
    "#;
    let mock = Mock::with(
        src,
        json!({}),
        Box::new(|op, _| match op {
            "process.run" => refuse("failed", "cancelled", "the run was stopped"),
            other => panic!("{other} must not reach the agent after a stop"),
        }),
    );
    let (code, _) = failed(run_mock(&mock));
    assert_eq!(code, "cancelled");
    assert_eq!(mock.0.borrow().ops(), vec!["process.run"]);
}

#[test]
fn sleep_is_a_host_call_and_its_timeout_ends_the_run() {
    let mock = Mock::with(
        "deckpilot.sleep(250.7); deckpilot.sleep(5000); 'not reached'",
        json!({}),
        Box::new(|_, args| {
            if args["ms"] == 250 {
                ok(json!(null))
            } else {
                refuse("failed", "timedOut", "the script ran out of time")
            }
        }),
    );
    let (code, _) = failed(run_mock(&mock));
    assert_eq!(code, "timed-out");
    let s = mock.0.borrow();
    assert_eq!(
        s.of("script.sleep"),
        vec![&json!({ "ms": 250 }), &json!({ "ms": 5000 })]
    );
}

#[test]
fn a_memory_bomb_hits_the_memory_limit() {
    let mock = Mock::new("const a = []; while (true) a.push(new Array(65536).fill(7));");
    mock.set(|s| s.load["settings"]["memoryMiB"] = json!(16));
    let (code, message) = failed(run_mock(&mock));
    assert_eq!(code, "limit", "{message}");
    assert!(message.contains("memory"), "{message}");
}

#[test]
fn deep_recursion_hits_the_stack_limit() {
    let (code, message) = failed(run_mock(&Mock::new(
        "function f(n) { return f(n + 1) + 1 }\nf(0)",
    )));
    assert_eq!(code, "limit", "{message}");
    assert!(message.contains("recursed"), "{message}");
}

#[test]
fn a_log_flood_is_cut_at_the_line_limit_with_one_marker() {
    let mock = Mock::with(
        "for (let i = 0; i < 5000; i++) deckpilot.log.info('line', i); 'done'",
        json!({}),
        Box::new(|_, _| ok(json!(null))),
    );
    let r = run_mock(&mock).unwrap();
    assert_eq!(r.data().get("logTruncated"), Some(&ParamValue::Bool(true)));
    let logs = mock.0.borrow().logs();
    assert_eq!(logs.len(), limits::LOG_LINES as usize + 1);
    assert_eq!(logs[0], "line 0");
    assert_eq!(logs.last().unwrap(), crate::api::log::TRUNCATED);
}

#[test]
fn a_log_flood_is_cut_at_the_byte_limit() {
    let mock = Mock::with(
        "const big = 'x'.repeat(100000); for (let i = 0; i < 100; i++) console.log(big)",
        json!({}),
        Box::new(|_, _| ok(json!(null))),
    );
    run_mock(&mock).unwrap();
    let logs = mock.0.borrow().logs();
    let bytes: usize = logs.iter().map(String::len).sum();
    assert!(bytes <= limits::LOG_BYTES + 200, "{bytes}");
    assert!(
        logs.iter()
            .all(|l| l.len() <= limits::LOG_LINE.max(crate::api::log::TRUNCATED.len()))
    );
    assert_eq!(logs.last().unwrap(), crate::api::log::TRUNCATED);
}

#[test]
fn the_bridge_call_budget_is_enforced_before_the_agent() {
    let src = "let n = 0; try { for (;;) { deckpilot.fs.exists('/tmp/x'); n++ } } catch (e) { ({ n, code: e.code }) }";
    let mock = Mock::with(
        src,
        json!({}),
        Box::new(|_, _| ok(json!({ "exists": false }))),
    );
    // script.load is the first of the budget.
    assert_eq!(
        result(run_mock(&mock)),
        json!({ "n": limits::HOST_CALLS - 1, "code": "limit" })
    );
    assert_eq!(
        mock.0.borrow().requests.len() as u32,
        limits::HOST_CALLS - 1
    );
}

// ---- permission refusals ----------------------------------------------------

#[test]
fn a_refusal_reaches_the_script_as_a_script_error() {
    let src = r#"
        let seen;
        try { deckpilot.fs.readFile("/etc/hosts") } catch (e) { seen = { name: e.name, code: e.code, path: e.path, message: e.message, isError: e instanceof ScriptError } }
        seen
    "#;
    let mock = Mock::with(
        src,
        json!({}),
        Box::new(|_, _| {
            json!({ "error": { "code": "not-permitted", "message": "no fs grant",
        "script": { "code": "notGranted", "message": "fs: the script has no read access to /etc/hosts", "path": "/etc/hosts" } } })
        }),
    );
    assert_eq!(
        result(run_mock(&mock)),
        json!({ "name": "ScriptError", "code": "notGranted", "path": "/etc/hosts", "message": "fs: the script has no read access to /etc/hosts", "isError": true })
    );
}

#[test]
fn an_uncaught_refusal_fails_the_run_with_its_code() {
    let mock = Mock::with(
        "deckpilot.process.run({ command: 'make' })",
        json!({}),
        Box::new(
            |_, _| json!({ "error": { "code": "not-permitted", "message": "process: make is not granted to this script" } }),
        ),
    );
    let (code, message) = failed(run_mock(&mock));
    assert_eq!(
        (code.as_str(), message.as_str()),
        ("not-granted", "process: make is not granted to this script")
    );
}

#[test]
fn a_refused_repository_is_named() {
    let mock = Mock::with(
        "deckpilot.fs.readFile({ repository: 'cms', relative: 'package.json' })",
        json!({}),
        Box::new(|_, _| {
            refuse(
                "not-permitted",
                "outsideScope",
                "fs: package.json is outside the script's scopes",
            )
        }),
    );
    let (code, message) = failed(run_mock(&mock));
    assert_eq!(code, "outside-scope");
    assert!(message.contains("repository \"cms\""), "{message}");
}

// ---- arguments refused before any host call ---------------------------------

#[test]
fn invalid_arguments_never_reach_the_agent() {
    let cases = [
        ("deckpilot.fs.readFile('relative/file')", "invalidArgument"),
        (
            "deckpilot.fs.readFile({ repository: 'cms', relative: '../../etc/passwd' })",
            "invalidArgument",
        ),
        (
            "deckpilot.fs.readFile({ repository: 'Not A Key', relative: '' })",
            "invalidArgument",
        ),
        ("deckpilot.fs.readFile('/tmp/a\\u0000b')", "invalidArgument"),
        (
            "deckpilot.fs.writeFile('/tmp/a', 'x'.repeat(4 * 1024 * 1024 + 1))",
            "tooLarge",
        ),
        (
            "deckpilot.fs.removeContents('/tmp/a', { keep: ['../x'] })",
            "invalidArgument",
        ),
        (
            "deckpilot.process.run({ command: 'git pull --ff-only' })",
            "invalidArgument",
        ),
        (
            "deckpilot.process.run({ command: 'git', args: 'pull --ff-only' })",
            "invalidArgument",
        ),
        (
            "deckpilot.process.run({ command: 'git', args: ['pull'], env: { DYLD_INSERT_LIBRARIES: '/x.dylib' } })",
            "notGranted",
        ),
        (
            "deckpilot.process.run({ command: 'git', env: { GIT_SSH_COMMAND: 'x' } })",
            "notGranted",
        ),
        (
            "deckpilot.process.run({ command: 'docker', env: { DOCKER_HOST: 'tcp://x' } })",
            "notGranted",
        ),
        (
            "deckpilot.process.run({ command: 'git', env: { 'BAD-NAME': 'x' } })",
            "invalidArgument",
        ),
        (
            "deckpilot.process.run({ command: 'git', timeoutMs: 700000 })",
            "invalidArgument",
        ),
        (
            "deckpilot.process.run({ command: 'git', cwd: 'relative' })",
            "invalidArgument",
        ),
        ("deckpilot.git.run(['status'], {})", "invalidArgument"),
        ("deckpilot.docker.compose(['up', '-d'])", "invalidArgument"),
        ("deckpilot.sleep(-1)", "invalidArgument"),
        ("deckpilot.path.join('/a', 3)", "invalidArgument"),
        ("deckpilot.path.resolve('relative', 'x')", "invalidArgument"),
    ];
    for (src, want) in cases {
        let mock = Mock::with(
            &format!("try {{ {src}; 'no error' }} catch (e) {{ e.code }}"),
            json!({}),
            Box::new(|op, a| panic!("{op} {a} reached the agent")),
        );
        assert_eq!(result(run_mock(&mock)), json!(want), "{src}");
        assert!(mock.0.borrow().requests.is_empty(), "{src}");
    }
}

#[test]
fn a_repository_join_cannot_leave_the_repository() {
    let src = r#"
        const repo = deckpilot.repositories.default();
        const out = { ref: repo.join("src", "./lib/../app", "main.js"), root: repo.join() };
        try { repo.join("..", "other") } catch (e) { out.escape = e.code }
        try { repo.join("/etc") } catch (e) { out.absolute = e.code }
        try { repo.join("C:\\x") } catch (e) { out.drive = e.code }
        out
    "#;
    let mock = three_repositories(src, |_| panic!("no process"));
    assert_eq!(
        result(run_mock(&mock)),
        json!({ "ref": { "repository": "cms", "relative": "src/app/main.js" }, "root": { "repository": "cms", "relative": "" },
                "escape": "outsideScope", "absolute": "outsideScope", "drive": "outsideScope" })
    );
}

// ---- process.run: the owner's examples, built and recorded only -------------

const UPDATE_ALL: &str = r#"
    deckpilot.forEachRepository(repo => {
      deckpilot.log.info(`Updating ${repo.name}`);
      const r = deckpilot.git.run(["pull", "--ff-only"], { cwd: repo });
      if (!r.ok) throw ScriptError.fromResult(r, repo);
      return deckpilot.docker.compose(["up", "-d"], { cwd: repo, timeoutMs: 120000 });
    });
"#;

#[test]
fn git_pull_and_docker_compose_are_structured_requests() {
    let mock = three_repositories(UPDATE_ALL, |_| exit(0, "", ""));
    let r = run_mock(&mock).unwrap();
    let outcomes: Value = match r.data().get("outcomes") {
        Some(ParamValue::String(s)) => serde_json::from_str(s).unwrap(),
        other => panic!("{other:?}"),
    };
    assert_eq!(
        outcomes,
        json!([{ "repository": "cms", "ok": true }, { "repository": "api", "ok": true }, { "repository": "web", "ok": true }])
    );
    let s = mock.0.borrow();
    let runs = s.of("process.run");
    assert_eq!(runs.len(), 6);
    for (i, key) in ["cms", "api", "web"].iter().enumerate() {
        assert_eq!(
            runs[2 * i],
            &json!({ "command": "git", "args": ["pull", "--ff-only"], "cwd": { "repository": key, "relative": "" }, "env": {} })
        );
        assert_eq!(
            runs[2 * i + 1],
            &json!({ "command": "docker", "args": ["compose", "up", "-d"], "cwd": { "repository": key, "relative": "" }, "env": {}, "timeoutMs": 120000 })
        );
    }
    assert_eq!(
        s.logs(),
        vec!["Updating CMS", "Updating API", "Updating WEB"]
    );
}

fn pull_fails_in_cms(args: &Value) -> Value {
    if args["command"] == "git" && args["cwd"]["repository"] == "cms" {
        exit(
            1,
            "",
            "hint: Diverging branches can't be fast-forwarded.\nfatal: Not possible to fast-forward, aborting.\n",
        )
    } else {
        exit(0, "", "")
    }
}

#[test]
fn a_failed_pull_names_the_repository_and_the_command_and_stops() {
    let mock = three_repositories(UPDATE_ALL, pull_fails_in_cms);
    let (code, message) = failed(run_mock(&mock));
    assert_eq!(code, "non-zero-exit");
    assert_eq!(
        message,
        "git pull --ff-only failed in repository \"cms\" (exit 1): fatal: Not possible to fast-forward, aborting."
    );
    // stopOnError (the default): nothing after cms.
    assert_eq!(mock.0.borrow().of("process.run").len(), 1);
}

#[test]
fn without_stop_on_error_every_repository_runs_and_the_failures_are_named() {
    let mock = three_repositories(UPDATE_ALL, pull_fails_in_cms);
    mock.set(|s| s.load["settings"]["stopOnError"] = json!(false));
    let (code, message) = failed(run_mock(&mock));
    assert_eq!(code, "repository-failed");
    assert!(
        message.starts_with(
            "1 of 3 repositories failed: git pull --ff-only failed in repository \"cms\""
        ),
        "{message}"
    );
    // cms: the pull only; api and web: pull and compose.
    assert_eq!(mock.0.borrow().of("process.run").len(), 5);
}

#[test]
fn a_returned_failed_result_fails_its_repository_too() {
    let src = r#"
        deckpilot.forEachRepository(repo => deckpilot.git.run(["pull", "--ff-only"], { cwd: repo }), { stopOnError: false })
          .map(o => ({ repository: o.repository, ok: o.ok, code: o.error ? o.error.code : null, msg: o.error ? o.error.message : null }))
    "#;
    let mock = three_repositories(
        &format!("try {{ {src} }} catch (e) {{ e.message }}"),
        pull_fails_in_cms,
    );
    // The run fails (a repository failed) but the script saw every outcome.
    let (code, _) = failed(run_mock(&mock));
    assert_eq!(code, "repository-failed");
}

#[test]
fn processes_do_not_throw_on_a_non_zero_exit() {
    let src = r#"
        const r = deckpilot.process.run({ command: "make", args: ["test"], cwd: "/work/app", env: { CI: "1" }, stdin: "y\n", timeoutMs: 5000 });
        ({ ok: r.ok, exitCode: r.exitCode, errorCode: r.errorCode, command: r.command, err: ScriptError.fromResult(r).message })
    "#;
    let mock = Mock::with(
        src,
        json!({}),
        Box::new(|_, _| exit(2, "", "make: *** [test] Error 2\n")),
    );
    assert_eq!(
        result(run_mock(&mock)),
        json!({ "ok": false, "exitCode": 2, "errorCode": "nonZeroExit", "command": "make test", "err": "make test failed (exit 2): make: *** [test] Error 2" })
    );
    assert_eq!(
        mock.0.borrow().of("process.run")[0],
        &json!({ "command": "make", "args": ["test"], "cwd": { "path": "/work/app" }, "env": { "CI": "1" }, "timeoutMs": 5000, "stdin": "y\n" })
    );
}

#[test]
fn missing_git_or_docker_and_a_stopped_daemon_are_named() {
    let src = r#"
        const out = { git: deckpilot.git.available(), docker: deckpilot.docker.available() };
        try { deckpilot.git.run(["status"], { cwd: "/w" }) } catch (e) { out.gitRun = e.code }
        out
    "#;
    let mock = Mock::with(
        src,
        json!({}),
        Box::new(
            |_, args| match (args["command"].as_str(), args["args"][0].as_str()) {
                (Some("git"), _) => refuse(
                    "failed",
                    "commandNotFound",
                    "git was not found at the configured path or on PATH",
                ),
                (Some("docker"), Some("version")) => exit(0, "27.3.1\n", ""),
                (Some("docker"), Some("info")) => exit(
                    1,
                    "",
                    "Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?\n",
                ),
                other => panic!("{other:?}"),
            },
        ),
    );
    let r = result(run_mock(&mock));
    assert_eq!(r["git"]["ok"], false);
    assert_eq!(r["gitRun"], "gitUnavailable");
    assert_eq!(r["docker"]["ok"], false);
    assert_eq!(r["docker"]["version"], "27.3.1");
    assert!(
        r["docker"]["error"]
            .as_str()
            .unwrap()
            .starts_with("the Docker daemon is not running"),
        "{r}"
    );
}

// ---- destructive operations -------------------------------------------------

fn confirming(answer: bool) -> Mock {
    let src = "deckpilot.fs.remove({ repository: 'cms', relative: 'build' }, { recursive: true })";
    Mock::with(
        src,
        json!({}),
        Box::new(move |op, args| match op {
            "fs.remove" if args.get("confirmation").is_none() => {
                json!({ "error": { "code": "not-permitted", "message": "confirm",
            "script": { "code": "needsConfirmation", "message": "remove 3 files", "confirmation": { "token": "tok-1" } } } })
            }
            "fs.remove" => ok(
                json!({ "dryRun": false, "root": "/w/cms/build", "files": 3, "directories": 1, "bytes": 10, "sample": [], "removed": true }),
            ),
            "script.confirm" => ok(json!({ "confirmed": answer })),
            other => panic!("{other}"),
        }),
    )
}

#[test]
fn a_destructive_operation_runs_only_after_the_users_confirmation() {
    let mock = confirming(true);
    assert_eq!(result(run_mock(&mock))["removed"], true);
    let s = mock.0.borrow();
    assert_eq!(s.ops(), vec!["fs.remove", "script.confirm", "fs.remove"]);
    assert_eq!(
        s.requests[1].1,
        json!({ "token": "tok-1" }),
        "the guest sends the agent's token, no text of its own"
    );
    assert_eq!(s.requests[2].1["confirmation"], "tok-1");
}

#[test]
fn a_declined_confirmation_removes_nothing() {
    let mock = confirming(false);
    let (code, message) = failed(run_mock(&mock));
    assert_eq!(code, "needs-confirmation");
    assert!(message.contains("repository \"cms\""), "{message}");
    assert_eq!(mock.0.borrow().ops(), vec!["fs.remove", "script.confirm"]);
}

// ---- projects and wire shapes -----------------------------------------------

#[test]
fn projects_and_repositories() {
    let src = r#"
        const p = deckpilot.projects.current();
        ({ project: p.key, keys: p.repositories().map(r => r.key), def: deckpilot.repositories.default().key,
           api: deckpilot.repositories.get("api").name, none: deckpilot.repositories.get("nope"), path: deckpilot.repositories.get("web").path })
    "#;
    assert_eq!(
        result(run_mock(&three_repositories(src, |_| panic!()))),
        json!({ "project": "shop", "keys": ["cms", "api", "web"], "def": "cms", "api": "API", "none": null, "path": null })
    );
}

#[test]
fn no_project_means_no_repositories() {
    let src = "const out = { current: deckpilot.projects.current(), repos: deckpilot.repositories.current() }; try { deckpilot.forEachRepository(r => r) } catch (e) { out.each = e.code } out";
    let mock = Mock::with(
        src,
        json!({}),
        Box::new(|_, _| ok(json!({ "project": null, "repositories": [] }))),
    );
    assert_eq!(
        result(run_mock(&mock)),
        json!({ "current": null, "repos": [], "each": "noProject" })
    );
}

#[test]
fn fs_requests_have_their_canonical_shape() {
    let src = r#"
        const repo = { repository: "cms", relative: "a\\b/./c" };
        deckpilot.fs.mkdir(repo, { recursive: true });
        deckpilot.fs.writeFile("/w/x.txt", "hi", { createOnly: true });
        deckpilot.fs.readFile("/w/x.txt", { maxBytes: 10 });
        deckpilot.fs.copy("/w/a", "/w/b", { recursive: true });
        deckpilot.fs.removeContents("/w/tmp", { dryRun: true, keep: [".gitkeep"] });
    "#;
    let mock = Mock::with(
        src,
        json!({}),
        Box::new(|op, _| {
            ok(match op {
                "fs.readFile" => json!({ "content": "hi" }),
                _ => json!({}),
            })
        }),
    );
    run_mock(&mock).unwrap();
    let s = mock.0.borrow();
    let shapes: Vec<Value> = s.requests.iter().map(|(op, a)| json!([op, a])).collect();
    assert_eq!(
        shapes,
        vec![
            json!(["fs.mkdir", { "path": { "repository": "cms", "relative": "a/b/c" }, "recursive": true }]),
            json!(["fs.writeFile", { "path": { "path": "/w/x.txt" }, "content": "hi", "encoding": "utf8", "createOnly": true }]),
            json!(["fs.readFile", { "path": { "path": "/w/x.txt" }, "encoding": "utf8", "maxBytes": 10 }]),
            json!(["fs.copy", { "src": { "path": "/w/a" }, "dst": { "path": "/w/b" }, "recursive": true, "overwrite": false }]),
            json!(["fs.removeContents", { "path": { "path": "/w/tmp" }, "dryRun": true, "keep": [".gitkeep"] }]),
        ]
    );
}

#[test]
fn windows_paths_follow_the_agents_platform() {
    let src = r#"({ sep: deckpilot.path.sep, j: deckpilot.path.join("C:\\work", "repo/src"), abs: deckpilot.path.isAbsolute("C:/x"),
                  rel: deckpilot.path.relative("C:\\Work", "c:\\work\\a") })"#;
    let mock = Mock::new(src);
    mock.set(|s| s.load["platform"] = json!("windows"));
    assert_eq!(
        result(run_mock(&mock)),
        json!({ "sep": "\\", "j": "C:\\work\\repo\\src", "abs": true, "rel": "a" })
    );
    // On Windows a POSIX absolute path is not fully qualified: refused.
    let mock = Mock::with(
        "try { deckpilot.fs.exists('/etc/hosts') } catch (e) { e.code }",
        json!({}),
        Box::new(|_, _| panic!()),
    );
    mock.set(|s| s.load["platform"] = json!("windows"));
    assert_eq!(result(run_mock(&mock)), json!("invalidArgument"));
}

#[test]
fn the_clock_moves_with_the_agents_replies() {
    let mock = Mock::new("Date.now() > 0");
    assert_eq!(result(run_mock(&mock)), json!(true));
    assert!(clock::now_us() >= 1_700_000_000_000_000.0);
}

#[test]
fn a_script_cannot_throw_the_runs_own_codes_or_unbounded_text() {
    let (code, _) = failed(run_mock(&Mock::new(
        "throw new ScriptError('scriptNeedsReview', 'looks like a review')",
    )));
    assert_eq!(code, "script-error");
    let (code, message) = failed(run_mock(&Mock::new(
        "throw new ScriptError('limit', 'x'.repeat(100000))",
    )));
    assert_eq!(code, "limit");
    assert!(
        message.len() <= runtime::MAX_ERROR_TEXT,
        "{}",
        message.len()
    );
    // Outcomes recorded by forEachRepository are bounded and cleaned too.
    let src = "deckpilot.forEachRepository(r => { throw new ScriptError('scriptMissing', 'y'.repeat(50000)) }, { stopOnError: false })";
    let mock = three_repositories(src, |_| panic!());
    let (code, message) = failed(run_mock(&mock));
    assert_eq!(code, "repository-failed");
    assert!(
        message.len() <= 4 * runtime::MAX_ERROR_TEXT,
        "{}",
        message.len()
    );
}

// ---- async scripts ----------------------------------------------------------

#[test]
fn an_async_script_ends_with_its_settled_value() {
    let src = "(async () => { const x = await Promise.resolve(20); return { x: x + 1 }; })()";
    assert_eq!(result(run_mock(&Mock::new(src))), json!({ "x": 21 }));
    let src = "Promise.resolve(1).then((v) => v + 1)";
    assert_eq!(result(run_mock(&Mock::new(src))), json!(2));
}

#[test]
fn a_rejected_async_script_fails_the_run() {
    let src =
        "(async () => { await null; throw new ScriptError('noProject', 'async failure'); })()";
    let (code, message) = failed(run_mock(&Mock::new(src)));
    assert_eq!(
        (code.as_str(), message.as_str()),
        ("no-project", "async failure")
    );
    let (code, message) = failed(run_mock(&Mock::new(
        "Promise.reject(new TypeError('nope'))",
    )));
    assert_eq!(code, "script-error");
    assert!(message.contains("TypeError: nope"), "{message}");
}

#[test]
fn an_unhandled_rejection_fails_a_run_that_otherwise_finished() {
    let src = "Promise.reject(new Error('lost')); 'done'";
    let (code, message) = failed(run_mock(&Mock::new(src)));
    assert_eq!(code, "script-error");
    assert!(
        message.contains("unhandled promise rejection: Error: lost"),
        "{message}"
    );
    // A rejection that is handled is not one.
    let src = "Promise.reject(new Error('caught')).catch(() => {}); 'done'";
    assert_eq!(result(run_mock(&Mock::new(src))), json!("done"));
}

#[test]
fn a_promise_that_never_settles_fails() {
    let (code, message) = failed(run_mock(&Mock::new("new Promise(() => {})")));
    assert_eq!(code, "script-error");
    assert!(message.contains("never settled"), "{message}");
}

// ---- the prelude does not depend on what the script changes ------------------

#[test]
fn replaced_built_ins_do_not_change_the_api() {
    let src = r#"
        String.prototype.includes = () => { throw new Error("polluted includes") };
        String.prototype.split = () => { throw new Error("polluted split") };
        String.prototype.trim = () => { throw new Error("polluted trim") };
        Array.prototype.map = () => { throw new Error("polluted map") };
        Array.prototype.join = () => { throw new Error("polluted join") };
        Array.prototype.find = () => { throw new Error("polluted find") };
        Object.assign = () => { throw new Error("polluted assign") };
        Array.isArray = () => false;
        const repo = deckpilot.repositories.default();
        const r = deckpilot.git.run(["pull", "--ff-only"], { cwd: repo });
        let error;
        try { repo.join("..") } catch (e) { error = e.code }
        ({ key: repo.key, ref: repo.join("a", "b"), command: r.command, msg: ScriptError.fromResult(r).message, error })
    "#;
    let mock = three_repositories(src, |_| {
        exit(1, "", "fatal: Not possible to fast-forward, aborting.\n")
    });
    assert_eq!(
        result(run_mock(&mock)),
        json!({ "key": "cms", "ref": { "repository": "cms", "relative": "a/b" }, "command": "git pull --ff-only",
                "msg": "git pull --ff-only failed in repository \"cms\" (exit 1): fatal: Not possible to fast-forward, aborting.",
                "error": "outsideScope" })
    );
}

// ---- available() and grants ---------------------------------------------------

#[test]
fn available_rethrows_a_refused_grant() {
    let src = "const out = {}; for (const t of ['git', 'docker']) { try { out[t] = deckpilot[t].available() } catch (e) { out[t] = e.code } } out";
    let mock = Mock::with(
        src,
        json!({}),
        Box::new(|_, _| refuse("not-permitted", "notGranted", "process: not granted")),
    );
    assert_eq!(
        result(run_mock(&mock)),
        json!({ "git": "notGranted", "docker": "notGranted" })
    );
}

// ---- outcome size ---------------------------------------------------------------

#[test]
fn the_outcomes_output_is_capped() {
    let many: Vec<runtime::Outcome> = (0..4096)
        .map(|i| runtime::Outcome {
            repository: format!("r{i}"),
            ok: false,
            error: Some(bridge::ScriptFault::new("scriptError", "e".repeat(2048))),
        })
        .collect();
    let text = outcomes_json(&many);
    assert!(text.len() <= MAX_OUTCOMES_JSON, "{}", text.len());
    let parsed: Value = serde_json::from_str(&text).unwrap();
    let items = parsed.as_array().unwrap();
    assert_eq!(
        items.last().unwrap(),
        &json!({ "truncated": true, "total": 4096 })
    );
    assert!(items.len() > 10);
    // Few outcomes are kept whole, without a marker.
    let few = &many[..3];
    let parsed: Value = serde_json::from_str(&outcomes_json(few)).unwrap();
    assert_eq!(parsed.as_array().unwrap().len(), 3);
    assert_eq!(outcomes_json(&[]), "[]");
}
