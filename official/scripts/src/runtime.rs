//! The QuickJS runtime of one run (§4.1 option A): QuickJS-NG through
//! rquickjs, inside this plugin's wasm module, inside the out-of-process
//! plugin host.
//!
//! * **Limits.** QuickJS's own memory limit (the script's `memoryMiB` less
//!   [`limits::MEMORY_HEADROOM_MIB`], under the wasm store's), its stack
//!   limit ([`limits::STACK`]), and an interrupt handler that stops the
//!   script on a Stop or the end of its time (both learned from
//!   `script.poll`) and on a fuel-independent instruction budget. The
//!   host's fuel, epoch deadline and memory caps stay underneath.
//! * **Nothing ambient.** No `quickjs-libc` (no `std`, `os`, files, timers),
//!   no module loader (static `import` is a syntax error in a script,
//!   dynamic `import()` fails), and `eval` and the `Function`
//!   constructors are disabled by the prelude before the script runs.
//!   Every effect is a bridge op ([`crate::api::dispatch`]).
//! * **Stopping is final.** An interrupt is uncatchable in QuickJS, and once
//!   the run is cancelled every op fails locally, so a `try` cannot keep a
//!   stopped script going.

use std::cell::RefCell;
use std::rc::Rc;

use rquickjs::context::EvalOptions;
use rquickjs::{CatchResultExt, CaughtError, Ctx, Function, Object, Value};
use serde::Serialize;
use serde_json::json;

use crate::api::{self, Flavor};
use crate::bridge::{ScriptFault, Session, limits};

const PRELUDE: &str = include_str!("prelude.js");
/// The name scripts' stack traces and syntax errors show.
pub const SCRIPT_FILE: &str = "script.js";

/// What the agent's `script.load` says about the run.
#[derive(Debug, Clone)]
pub struct RunSetup {
    pub source: String,
    pub flavor: Flavor,
    pub memory_mib: u32,
    pub stop_on_error: bool,
    /// The JSON the prelude exposes as `deckpilot.run` (id, origin,
    /// userInitiated, inputs) and the grant summary.
    pub run: serde_json::Value,
    pub grants: serde_json::Value,
    /// The instruction budget, in interrupts ([`limits::INTERRUPTS`]).
    pub max_interrupts: u64,
}

/// A repository outcome of `forEachRepository`, kept for the run's result.
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct Outcome {
    pub repository: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ScriptFault>,
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Ending {
    /// The script finished; its completion value as JSON (`null` when it
    /// had none or it could not be serialized).
    Done(serde_json::Value),
    /// The script threw (or was stopped, or hit a limit).
    Failed(ScriptFault),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub ending: Ending,
    pub outcomes: Vec<Outcome>,
    pub host_calls: u32,
    pub log_lines: u32,
    pub log_truncated: bool,
}

/// Why the interrupt handler stopped the script.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Stop {
    Cancelled,
    TimedOut,
    Instructions,
}

struct Shared {
    session: Session,
    outcomes: Vec<Outcome>,
    stop: Option<Stop>,
    since_poll: u32,
    max_interrupts: u64,
}

/// A syntax error, with its place in the script.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SyntaxError {
    pub message: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

fn runtime(memory_mib: u32) -> Result<(rquickjs::Runtime, rquickjs::Context), ScriptFault> {
    let rt = rquickjs::Runtime::new()
        .map_err(|e| ScriptFault::new("hostError", format!("the JavaScript runtime: {e}")))?;
    let mib = memory_mib.clamp(limits::MEMORY_MIB_MIN, limits::MEMORY_MIB_MAX)
        - limits::MEMORY_HEADROOM_MIB;
    rt.set_memory_limit(mib as usize * 1024 * 1024);
    rt.set_max_stack_size(limits::STACK);
    let ctx = rquickjs::Context::full(&rt)
        .map_err(|e| ScriptFault::new("hostError", format!("the JavaScript context: {e}")))?;
    Ok((rt, ctx))
}

/// Compiles `source` as a script without running anything of it (the
/// `check` action, the editor's diagnostics).
pub fn check(source: &str) -> Result<(), SyntaxError> {
    let (_rt, ctx) = runtime(32).map_err(|f| SyntaxError {
        message: f.message.unwrap_or_default(),
        line: None,
        column: None,
    })?;
    ctx.with(|ctx| match compile_only(&ctx, source) {
        Ok(()) => Ok(()),
        Err(e) => Err(syntax_error(&ctx, e)),
    })
}

/// `JS_Eval` with `JS_EVAL_FLAG_COMPILE_ONLY`: parses and compiles, and
/// frees the compiled function unrun.
#[allow(unsafe_code)]
fn compile_only<'js>(ctx: &Ctx<'js>, source: &str) -> Result<(), CaughtError<'js>> {
    use rquickjs::qjs;
    let mut src = source.as_bytes().to_vec();
    let len = src.len();
    src.push(0);
    let file = c"script.js";
    let flags = (qjs::JS_EVAL_TYPE_GLOBAL | qjs::JS_EVAL_FLAG_COMPILE_ONLY) as i32;
    // SAFETY: `src` is NUL-terminated with `len` bytes before the NUL, as
    // JS_Eval requires; `file` is a C string; the context is live for 'js.
    // The returned value is owned and freed here (it is never called).
    let value = unsafe {
        qjs::JS_Eval(
            ctx.as_raw().as_ptr(),
            src.as_ptr().cast(),
            len as _,
            file.as_ptr(),
            flags,
        )
    };
    // SAFETY: as above; a JS_EXCEPTION value carries no reference.
    let failed = unsafe { qjs::JS_IsException(value) };
    // SAFETY: freeing the value JS_Eval returned, once.
    unsafe { qjs::JS_FreeValue(ctx.as_raw().as_ptr(), value) };
    if failed {
        let e: rquickjs::Result<()> = Err(rquickjs::Error::Exception);
        return e.catch(ctx).map_err(|e| e);
    }
    Ok(())
}

fn syntax_error(ctx: &Ctx<'_>, e: CaughtError<'_>) -> SyntaxError {
    let _ = ctx;
    match e {
        CaughtError::Exception(ex) => {
            let message = ex.message().unwrap_or_else(|| "syntax error".into());
            let (line, column) = place(ex.stack().as_deref().unwrap_or(""), ex.as_object());
            SyntaxError {
                message,
                line,
                column,
            }
        }
        other => SyntaxError {
            message: other.to_string(),
            line: None,
            column: None,
        },
    }
}

/// The line and column of an error: QuickJS-NG gives `lineNumber` and
/// `columnNumber` on errors, and `script.js:L:C` in the stack.
fn place(stack: &str, obj: &Object<'_>) -> (Option<u32>, Option<u32>) {
    let line = obj.get::<_, Option<u32>>("lineNumber").ok().flatten();
    let column = obj.get::<_, Option<u32>>("columnNumber").ok().flatten();
    if line.is_some() {
        return (line, column);
    }
    let needle = format!("{SCRIPT_FILE}:");
    stack
        .lines()
        .find_map(|l| {
            let at = l.find(&needle)? + needle.len();
            let mut it = l[at..].split(|c: char| !c.is_ascii_digit());
            let line = it.next()?.parse().ok()?;
            let column = it.next().and_then(|c| c.parse().ok());
            Some((Some(line), column))
        })
        .unwrap_or((None, None))
}

/// Runs one script to its end.
pub fn run(session: Session, setup: RunSetup) -> Report {
    let shared = Rc::new(RefCell::new(Shared {
        session,
        outcomes: Vec::new(),
        stop: None,
        since_poll: 0,
        max_interrupts: setup.max_interrupts,
    }));
    let ending = match runtime(setup.memory_mib) {
        Ok((rt, ctx)) => {
            install_interrupt(&rt, &shared);
            let ending = ctx.with(|ctx| execute(&ctx, &shared, &setup));
            // Settle what the script queued (promise reactions), under the
            // same interrupt handler; their failures do not change a
            // finished run's result.
            let mut jobs = 0;
            while rt.is_job_pending() && jobs < 10_000 && shared.borrow().stop.is_none() {
                jobs += 1;
                if rt.execute_pending_job().is_err() {
                    break;
                }
            }
            ending
        }
        Err(f) => Ending::Failed(f),
    };
    let ending = match (ending, shared.borrow().stop) {
        // A stop overrides whatever the interrupted code reported.
        (Ending::Failed(_), Some(stop)) | (Ending::Done(_), Some(stop)) => {
            Ending::Failed(stop_fault(stop))
        }
        (e, None) => e,
    };
    let s = shared.borrow();
    Report {
        ending,
        outcomes: s.outcomes.clone(),
        host_calls: s.session.budget.calls,
        log_lines: s.session.budget.log_lines,
        log_truncated: s.session.budget.log_truncated,
    }
}

fn stop_fault(stop: Stop) -> ScriptFault {
    match stop {
        Stop::Cancelled => ScriptFault::new("cancelled", "the run was stopped"),
        Stop::TimedOut => ScriptFault::new("timedOut", "the script ran out of time"),
        Stop::Instructions => ScriptFault::new(
            "limit",
            "the script used its whole instruction budget (an endless loop?)",
        ),
    }
}

fn install_interrupt(rt: &rquickjs::Runtime, shared: &Rc<RefCell<Shared>>) {
    let shared = Rc::clone(shared);
    rt.set_interrupt_handler(Some(Box::new(move || {
        let Ok(mut s) = shared.try_borrow_mut() else {
            return false;
        };
        if s.stop.is_some() {
            return true;
        }
        s.session.budget.interrupts += 1;
        if s.session.budget.interrupts >= s.max_interrupts {
            s.stop = Some(Stop::Instructions);
            return true;
        }
        s.since_poll += 1;
        if s.since_poll >= limits::INTERRUPTS_PER_POLL || s.session.budget.cancelled {
            s.since_poll = 0;
            if s.session.poll() {
                s.stop = Some(if s.session.budget.timed_out {
                    Stop::TimedOut
                } else {
                    Stop::Cancelled
                });
                return true;
            }
        }
        false
    })));
}

fn execute(ctx: &Ctx<'_>, shared: &Rc<RefCell<Shared>>, setup: &RunSetup) -> Ending {
    if let Err(f) = install_natives(ctx, shared, setup) {
        return Ending::Failed(f);
    }
    let prelude: rquickjs::Result<Value> = ctx.eval_with_options(PRELUDE, options("prelude.js"));
    if let Err(e) = prelude.catch(ctx) {
        return Ending::Failed(ScriptFault::new(
            "hostError",
            format!("the prelude failed: {e}"),
        ));
    }
    let result: rquickjs::Result<Value> =
        ctx.eval_with_options(setup.source.as_str(), options(SCRIPT_FILE));
    match result.catch(ctx) {
        Ok(v) => Ending::Done(result_json(ctx, v)),
        Err(e) => Ending::Failed(fault_of(e)),
    }
}

fn options(file: &str) -> EvalOptions {
    let mut o = EvalOptions::default();
    o.global = true;
    o.strict = false;
    o.backtrace_barrier = true;
    o.promise = false;
    o.filename = Some(file.to_owned());
    o
}

fn result_json<'js>(ctx: &Ctx<'js>, v: Value<'js>) -> serde_json::Value {
    if v.is_undefined() || v.is_function() {
        return serde_json::Value::Null;
    }
    match ctx.json_stringify(v) {
        Ok(Some(s)) => {
            let s = s.to_string().unwrap_or_default();
            if s.len() > limits::RESULT {
                json!({ "truncated": true, "bytes": s.len() })
            } else {
                serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)
            }
        }
        _ => serde_json::Value::Null,
    }
}

/// The fault of what a script threw: a `ScriptError` keeps its code and
/// detail; another error becomes `scriptError` with its name and message;
/// QuickJS's own limits are `limit`.
fn fault_of(e: CaughtError<'_>) -> ScriptFault {
    match e {
        CaughtError::Exception(ex) => {
            let obj = ex.as_object();
            let message = ex.message().unwrap_or_default();
            let name: String = obj.get("name").unwrap_or_else(|_| "Error".into());
            let code: Option<String> = obj.get("code").ok().flatten();
            let text = |key: &str| obj.get::<_, Option<String>>(key).ok().flatten();
            let (line, _) = place(ex.stack().as_deref().unwrap_or(""), obj);
            let at = line
                .map(|l| format!(" (script.js line {l})"))
                .unwrap_or_default();
            let (code, message) = match code {
                Some(c) if name == "ScriptError" => (c, message),
                _ if name == "SyntaxError" => (
                    "syntaxError".to_owned(),
                    format!("SyntaxError: {message}{at}"),
                ),
                _ if name == "InternalError" && message.contains("out of memory") => (
                    "limit".to_owned(),
                    "the script used more memory than its limit".to_owned(),
                ),
                _ if name == "RangeError" && message.contains("stack") => (
                    "limit".to_owned(),
                    format!("the script recursed too deeply: {message}{at}"),
                ),
                _ => ("scriptError".to_owned(), format!("{name}: {message}{at}")),
            };
            ScriptFault {
                code,
                message: Some(message),
                repository: text("repository"),
                command: text("command"),
                path: text("path"),
                confirmation: None,
            }
        }
        CaughtError::Value(v) => ScriptFault::new(
            "scriptError",
            format!(
                "the script threw a value that is not an Error: {}",
                v.as_string()
                    .and_then(|s| s.to_string().ok())
                    .unwrap_or_else(|| format!("{:?}", v.type_of()))
            ),
        ),
        CaughtError::Error(e) => ScriptFault::new("hostError", e.to_string()),
    }
}

/// The natives the prelude takes from `globalThis.__dp` (and deletes from
/// it before the script runs).
fn install_natives(
    ctx: &Ctx<'_>,
    shared: &Rc<RefCell<Shared>>,
    setup: &RunSetup,
) -> Result<(), ScriptFault> {
    let err = |e: rquickjs::Error| ScriptFault::new("hostError", e.to_string());
    let dp = Object::new(ctx.clone()).map_err(err)?;
    let flavor = setup.flavor;

    let s = Rc::clone(shared);
    let call = Function::new(ctx.clone(), move |op: String, args: String| -> String {
        let reply = match serde_json::from_str(&args) {
            Err(_) => Err(api::invalid("the arguments are not JSON")),
            Ok(args) => api::dispatch(&op, args, flavor).and_then(|args| {
                let mut s = s.borrow_mut();
                s.session.call(&op, args)
            }),
        };
        envelope(reply)
    })
    .map_err(err)?;
    dp.set("call", call).map_err(err)?;

    let s = Rc::clone(shared);
    let log = Function::new(ctx.clone(), move |level: String, message: String| {
        let mut s = s.borrow_mut();
        api::log::line(&mut s.session, &level, &message);
    })
    .map_err(err)?;
    dp.set("log", log).map_err(err)?;

    let path = Function::new(ctx.clone(), move |op: String, args: String| -> String {
        let reply = serde_json::from_str::<Vec<String>>(&args)
            .map_err(|_| api::invalid("path: the arguments are strings"))
            .and_then(|a| api::path::call(flavor, &op, &a).map_err(api::invalid));
        envelope(reply)
    })
    .map_err(err)?;
    dp.set("path", path).map_err(err)?;

    let s = Rc::clone(shared);
    let aborted = Function::new(ctx.clone(), move || -> bool {
        s.try_borrow()
            .map(|s| s.session.budget.cancelled)
            .unwrap_or(false)
    })
    .map_err(err)?;
    dp.set("aborted", aborted).map_err(err)?;

    let s = Rc::clone(shared);
    let record = Function::new(ctx.clone(), move |outcome: String| {
        if let Ok(o) = serde_json::from_str::<Outcome>(&outcome) {
            let mut s = s.borrow_mut();
            if s.outcomes.len() < 4096 {
                s.outcomes.push(o);
            }
        }
    })
    .map_err(err)?;
    dp.set("record", record).map_err(err)?;

    let setup_json = json!({
        "platform": flavor.platform(),
        "sep": flavor.sep().to_string(),
        "run": setup.run,
        "grants": setup.grants,
        "stopOnError": setup.stop_on_error,
        "ops": api::OPS,
    });
    dp.set("setup", setup_json.to_string()).map_err(err)?;
    ctx.globals().set("__dp", dp).map_err(err)?;
    Ok(())
}

fn envelope(reply: Result<serde_json::Value, ScriptFault>) -> String {
    match reply {
        Ok(v) => json!({ "ok": v }).to_string(),
        Err(f) => json!({ "error": f }).to_string(),
    }
}
