// The `deckpilot` object of a script (DeckPilot design-projects-scripts.md
// §4.4; the types: deckpilot.d.ts). Evaluated once per run, before the
// script, in the same QuickJS context.
//
// It takes the natives from globalThis.__dp and deletes that, so the
// script only reaches them through this API. Every privileged effect is
// one bridge op, decided by the agent against the script's current grants;
// nothing here is a security boundary (the script could rebuild any of it),
// it is the API's ergonomics: PathLike handling, structured results,
// errors that name the repository and the command, the confirmation round
// of destructive operations, and forEachRepository.
//
// Then `deckpilot`, `ScriptError` and `console` are frozen (deeply) and
// non-writable, and `eval` and the Function constructors are disabled: a
// script runs its own source, never code built from strings.
(function (dp) {
  "use strict";
  const setup = JSON.parse(dp.setup);
  const nativeCall = dp.call;
  const nativeLog = dp.log;
  const nativePath = dp.path;
  const nativeAborted = dp.aborted;
  const nativeRecord = dp.record;
  const stringify = JSON.stringify;
  const parse = JSON.parse;
  delete globalThis.__dp;

  // The built-ins the prelude uses, taken before the script runs: a script
  // that replaces a prototype method (String.prototype.includes,
  // Array.prototype.map, Object.assign, …) changes its own code, not the
  // API's. Called as uncurried functions: sSlice(text, 0, 3).
  const { assign, defineProperty, freeze, getOwnPropertyDescriptor, getPrototypeOf, isFrozen, keys } = Object;
  const ownKeys = Reflect.ownKeys;
  const isArray = Array.isArray;
  const uncurry = Function.prototype.bind.bind(Function.prototype.call);
  const sIncludes = uncurry(String.prototype.includes);
  const sSlice = uncurry(String.prototype.slice);
  const sSplit = uncurry(String.prototype.split);
  const sTrim = uncurry(String.prototype.trim);
  const reTest = uncurry(RegExp.prototype.test);
  const reExec = uncurry(RegExp.prototype.exec);
  const aMap = uncurry(Array.prototype.map);
  const aFilter = uncurry(Array.prototype.filter);
  const aFind = uncurry(Array.prototype.find);
  const aJoin = uncurry(Array.prototype.join);
  const aPush = uncurry(Array.prototype.push);
  const aPop = uncurry(Array.prototype.pop);
  const aSlice = uncurry(Array.prototype.slice);
  const aSome = uncurry(Array.prototype.some);
  const aConcat = uncurry(Array.prototype.concat);
  const wsHas = uncurry(WeakSet.prototype.has);
  const wsAdd = uncurry(WeakSet.prototype.add);

  const FIELDS = ["repository", "command", "path"];

  class ScriptError extends Error {
    constructor(code, message, detail) {
      super(message);
      this.name = "ScriptError";
      this.code = typeof code === "string" ? code : "scriptError";
      for (const k of FIELDS) {
        if (detail && typeof detail[k] === "string") this[k] = detail[k];
      }
    }

    // The error of a failed ProcessResult: names the command and, when it
    // ran in one, the repository.
    static fromResult(result, repository) {
      if (!result || typeof result !== "object") {
        return new ScriptError("invalidArgument", "fromResult takes a ProcessResult");
      }
      const command = result.command || "the command";
      const repo = repositoryKey(repository) || result.repository || null;
      const where = repo ? ` in repository "${repo}"` : "";
      let code, message;
      if (result.timedOut) {
        code = "timedOut";
        message = `${command} timed out${where} after ${result.durationMs} ms`;
      } else if (result.cancelled) {
        code = "cancelled";
        message = `${command} was stopped${where}`;
      } else {
        code = result.errorCode || "nonZeroExit";
        const how = result.exitCode === null ? `signal ${result.signal || "?"}` : `exit ${result.exitCode}`;
        const text = firstLine(result.stderr) || firstLine(result.stdout);
        message = `${command} failed${where} (${how})${text ? ": " + text : ""}`;
      }
      return new ScriptError(code, message, { repository: repo || undefined, command });
    }

    // Returns `result` when it succeeded; throws its error otherwise.
    static check(result, repository) {
      if (result && result.ok === true) return result;
      throw ScriptError.fromResult(result, repository);
    }
  }

  function firstLine(text) {
    if (typeof text !== "string") return "";
    const lines = aFilter(aMap(sSplit(text, /\r?\n/), (l) => sTrim(l)), (l) => l.length > 0);
    // git and docker put the reason on the last line ("fatal: …").
    const line = aFind(lines, (l) => reTest(/^(fatal|error)\b/i, l)) || lines[lines.length - 1] || "";
    return line.length > 300 ? sSlice(line, 0, 300) + "…" : line;
  }

  function fault(f, context) {
    const detail = {
      repository: f.repository || (context && context.repository) || undefined,
      command: f.command || (context && context.command) || undefined,
      path: f.path || undefined,
    };
    let message = f.message || f.code;
    if (detail.repository && !sIncludes(message, `"${detail.repository}"`)) {
      message = `${message} (repository "${detail.repository}")`;
    }
    const e = new ScriptError(f.code, message, detail);
    if (f.confirmation) defineProperty(e, "confirmation", { value: f.confirmation });
    return e;
  }

  // One bridge op; a refusal or failure is thrown as a ScriptError.
  function host(op, args, context) {
    const reply = parse(nativeCall(op, stringify(args)));
    if (reply.error) throw fault(reply.error, context);
    return reply.ok;
  }

  // ---- paths and repositories -------------------------------------------

  const repositories = new WeakSet();

  function relativeJoin(key, parts) {
    const out = [];
    for (const part of parts) {
      if (typeof part !== "string") {
        throw new ScriptError("invalidArgument", "repo.join takes strings", { repository: key });
      }
      if (reTest(/^[\\/]/, part) || sIncludes(part, ":") || sIncludes(part, "\0")) {
        throw new ScriptError("outsideScope", `\`${part}\` is not a path inside repository "${key}"`, { repository: key });
      }
      for (const seg of sSplit(part, /[\\/]/)) {
        if (seg === "" || seg === ".") continue;
        if (seg === "..") {
          if (out.length === 0) {
            throw new ScriptError("outsideScope", `\`${aJoin(parts, "/")}\` leaves repository "${key}"`, { repository: key });
          }
          aPop(out);
        } else aPush(out, seg);
      }
    }
    return aJoin(out, "/");
  }

  function repositoryKey(r) {
    if (typeof r === "string") return r;
    if (r && wsHas(repositories, r)) return r.key;
    return null;
  }

  // PathLike -> the bridge's path argument, and the repository it is in.
  function pathArg(p, what) {
    if (typeof p === "string") return [{ path: p }, null];
    if (p && typeof p === "object") {
      if (wsHas(repositories, p)) return [{ repository: p.key, relative: "" }, p.key];
      if (typeof p.repository === "string" && typeof p.relative === "string") {
        return [{ repository: p.repository, relative: p.relative }, p.repository];
      }
    }
    throw new ScriptError("invalidArgument", `${what} is a path: an absolute string, a repository or repo.join(...)`);
  }

  function makeRepository(r) {
    const key = String(r.key);
    const repo = {
      id: String(r.id),
      key,
      name: String(r.name),
      description: r.description == null ? null : String(r.description),
      isDefault: r.isDefault === true,
      path: typeof r.path === "string" ? r.path : null,
      exists: r.exists === true,
      remote: r.remote
        ? { url: r.remote.url == null ? null : String(r.remote.url), defaultBranch: r.remote.defaultBranch == null ? null : String(r.remote.defaultBranch) }
        : null,
      join(...parts) {
        return freeze({ repository: key, relative: relativeJoin(key, parts) });
      },
      toString() {
        return `repository "${key}"`;
      },
    };
    wsAdd(repositories, repo);
    return deepFreeze(repo);
  }

  function makeProject(p, repos) {
    let cache = repos ? aMap(repos, makeRepository) : null;
    const id = String(p.id);
    return deepFreeze({
      id,
      key: String(p.key),
      name: String(p.name),
      repositories() {
        if (cache === null) {
          const r = host("project.repositories", { project: id });
          cache = aMap(r.repositories || [], makeRepository);
        }
        return aSlice(cache);
      },
    });
  }

  const projects = {
    current() {
      const r = host("project.repositories", { project: null });
      return r.project ? makeProject(r.project, r.repositories || []) : null;
    },
    list() {
      return aMap(host("project.list", {}).projects || [], (p) => makeProject(p, null));
    },
  };

  const repositoriesApi = {
    current() {
      const p = projects.current();
      return p ? p.repositories() : [];
    },
    // OD-3: the marked default; else the only repository; else none.
    default() {
      const all = repositoriesApi.current();
      return aFind(all, (r) => r.isDefault) || (all.length === 1 ? all[0] : null);
    },
    get(key) {
      return aFind(repositoriesApi.current(), (r) => r.key === key) || null;
    },
  };

  const path = {
    sep: setup.sep,
  };
  for (const op of ["join", "normalize", "resolve", "dirname", "basename", "extname", "relative", "isAbsolute", "toPosix", "fromPosix"]) {
    path[op] = function (...args) {
      for (const a of args) {
        if (a !== undefined && typeof a !== "string") {
          throw new ScriptError("invalidArgument", `path.${op} takes strings`);
        }
      }
      const reply = parse(nativePath(op, stringify(aFilter(args, (a) => a !== undefined))));
      if (reply.error) throw fault(reply.error);
      return reply.ok;
    };
  }

  // ---- files --------------------------------------------------------------

  function opts(o) {
    return o && typeof o === "object" ? o : {};
  }

  // A destructive op: when the agent asks for a confirmation, the user sees
  // the agent's own plan (script.confirm); only then is the op repeated
  // with the agent's token. One round: a plan that grew is refused again.
  function destructive(op, args, context) {
    try {
      return host(op, args, context);
    } catch (e) {
      if (!(e instanceof ScriptError) || e.code !== "needsConfirmation" || !e.confirmation || typeof e.confirmation.token !== "string") throw e;
      const token = e.confirmation.token;
      const answer = host("script.confirm", { token }, context);
      if (!answer || answer.confirmed !== true) {
        throw fault({ code: "needsConfirmation", message: `${sSlice(op, 3)} was not confirmed` }, context);
      }
      return host(op, assign({}, args, { confirmation: token }), context);
    }
  }

  const fs = {
    exists(p) {
      const [arg, repository] = pathArg(p, "path");
      return host("fs.exists", { path: arg }, { repository }).exists === true;
    },
    stat(p) {
      const [arg, repository] = pathArg(p, "path");
      return host("fs.stat", { path: arg }, { repository }).stat || null;
    },
    list(p, o) {
      const [arg, repository] = pathArg(p, "path");
      return host("fs.list", { path: arg, recursive: opts(o).recursive === true }, { repository }).entries || [];
    },
    mkdir(p, o) {
      const [arg, repository] = pathArg(p, "path");
      host("fs.mkdir", { path: arg, recursive: opts(o).recursive === true }, { repository });
    },
    readFile(p, o) {
      const [arg, repository] = pathArg(p, "path");
      const a = { path: arg, encoding: opts(o).encoding || "utf8" };
      if (opts(o).maxBytes !== undefined) a.maxBytes = opts(o).maxBytes;
      return host("fs.readFile", a, { repository }).content;
    },
    writeFile(p, content, o) {
      const [arg, repository] = pathArg(p, "path");
      if (typeof content !== "string") throw new ScriptError("invalidArgument", "writeFile takes a string", { repository });
      host("fs.writeFile", { path: arg, content, encoding: opts(o).encoding || "utf8", createOnly: opts(o).createOnly === true }, { repository });
    },
    copy(src, dst, o) {
      const [s, repository] = pathArg(src, "src");
      const [d, dstRepo] = pathArg(dst, "dst");
      destructive("fs.copy", { src: s, dst: d, recursive: opts(o).recursive === true, overwrite: opts(o).overwrite === true }, { repository: dstRepo || repository });
    },
    move(src, dst, o) {
      const [s, repository] = pathArg(src, "src");
      const [d, dstRepo] = pathArg(dst, "dst");
      destructive("fs.move", { src: s, dst: d, overwrite: opts(o).overwrite === true }, { repository: repository || dstRepo });
    },
    remove(p, o) {
      const [arg, repository] = pathArg(p, "path");
      return destructive("fs.remove", { path: arg, recursive: opts(o).recursive === true, dryRun: opts(o).dryRun === true }, { repository });
    },
    removeContents(p, o) {
      const [arg, repository] = pathArg(p, "path");
      const keep = opts(o).keep === undefined ? [] : opts(o).keep;
      return destructive("fs.removeContents", { path: arg, dryRun: opts(o).dryRun === true, keep }, { repository });
    },
  };

  // ---- processes ----------------------------------------------------------

  function display(command, args) {
    return aJoin(aMap(aConcat([command], args), (a) => (reTest(/^[A-Za-z0-9_@%+=:,./-]+$/, a) ? a : stringify(a))), " ");
  }

  function strings(v, what) {
    if (v === undefined) return [];
    if (!isArray(v) || aSome(v, (a) => typeof a !== "string")) {
      throw new ScriptError("invalidArgument", `${what} is an array of strings (there is no shell)`);
    }
    return aSlice(v);
  }

  function processRun(req, unavailable) {
    const r = opts(req);
    if (typeof r.command !== "string") throw new ScriptError("invalidArgument", "process.run needs a `command`");
    const args = strings(r.args, "args");
    const a = { command: r.command, args, env: {} };
    let repository = null;
    if (r.cwd !== undefined) [a.cwd, repository] = pathArg(r.cwd, "cwd");
    if (r.env !== undefined) {
      if (typeof r.env !== "object" || r.env === null) throw new ScriptError("invalidArgument", "env is an object of strings");
      for (const k of keys(r.env)) {
        if (typeof r.env[k] !== "string") throw new ScriptError("invalidArgument", `env.${k} is not a string`);
        a.env[k] = r.env[k];
      }
    }
    if (r.timeoutMs !== undefined) a.timeoutMs = r.timeoutMs;
    if (r.stdin !== undefined) a.stdin = r.stdin;
    const command = display(r.command, args);
    let result;
    try {
      result = host("process.run", a, { repository, command });
    } catch (e) {
      if (unavailable && e instanceof ScriptError && e.code === "commandNotFound") {
        throw new ScriptError(unavailable, e.message, { repository, command });
      }
      throw e;
    }
    return finish(result, command, repository);
  }

  function finish(result, command, repository) {
    const out = {
      ok: result.ok === true,
      exitCode: typeof result.exitCode === "number" ? result.exitCode : null,
      signal: typeof result.signal === "string" ? result.signal : null,
      stdout: typeof result.stdout === "string" ? result.stdout : "",
      stderr: typeof result.stderr === "string" ? result.stderr : "",
      truncated: result.truncated === true,
      durationMs: typeof result.durationMs === "number" ? result.durationMs : 0,
      timedOut: result.timedOut === true,
      cancelled: result.cancelled === true,
      command,
      repository: repository || null,
      errorCode: null,
    };
    if (!out.ok) {
      out.errorCode = out.timedOut ? "timedOut" : out.cancelled ? "cancelled" : "nonZeroExit";
    }
    return freeze(out);
  }

  // What available() never turns into { ok: false }: a stop, and a refusal
  // of the grant (the script must not read "no git" when it lacks the
  // permission to run it).
  const isFinal = (code) => code === "cancelled" || code === "timedOut" || code === "notGranted";

  const DAEMON_DOWN = /Cannot connect to the Docker daemon|Is the docker daemon running|error during connect|docker daemon is not running/i;

  function dockerResult(r) {
    if (!r.ok && !r.timedOut && !r.cancelled && reTest(DAEMON_DOWN, r.stderr)) {
      return freeze(assign({}, r, { errorCode: "dockerDaemonDown" }));
    }
    return r;
  }

  function needCwd(o, what) {
    if (!o || o.cwd === undefined) throw new ScriptError("invalidArgument", `${what} needs { cwd }: a repository or a path`);
  }

  const git = {
    available() {
      try {
        const r = processRun({ command: "git", args: ["--version"] }, "gitUnavailable");
        if (!r.ok) return { ok: false, error: ScriptError.fromResult(r).message };
        const m = reExec(/git version (\S+)/, r.stdout);
        return { ok: true, version: m ? m[1] : sTrim(r.stdout) };
      } catch (e) {
        if (e instanceof ScriptError && isFinal(e.code)) throw e;
        return { ok: false, error: e.message };
      }
    },
    run(args, o) {
      needCwd(o, "git.run");
      return processRun({ command: "git", args: strings(args, "args"), cwd: o.cwd, timeoutMs: o.timeoutMs }, "gitUnavailable");
    },
  };

  const docker = {
    available() {
      try {
        const client = processRun({ command: "docker", args: ["version", "--format", "{{.Client.Version}}"] }, "dockerUnavailable");
        const version = sSplit(sTrim(client.stdout), /\r?\n/)[0] || undefined;
        const server = dockerResult(processRun({ command: "docker", args: ["info", "--format", "{{.ServerVersion}}"] }, "dockerUnavailable"));
        if (!server.ok) {
          const e = ScriptError.fromResult(server);
          return { ok: false, version, error: server.errorCode === "dockerDaemonDown" ? `the Docker daemon is not running: ${e.message}` : e.message };
        }
        const compose = processRun({ command: "docker", args: ["compose", "version", "--short"] }, "dockerUnavailable");
        return { ok: true, version, composeVersion: compose.ok ? sTrim(compose.stdout) : undefined };
      } catch (e) {
        if (e instanceof ScriptError && isFinal(e.code)) throw e;
        return { ok: false, error: e.message };
      }
    },
    run(args, o) {
      const p = opts(o);
      return dockerResult(processRun({ command: "docker", args: strings(args, "args"), cwd: p.cwd, timeoutMs: p.timeoutMs }, "dockerUnavailable"));
    },
    compose(args, o) {
      needCwd(o, "docker.compose");
      return docker.run(aConcat(["compose"], strings(args, "args")), o);
    },
  };

  const shellGrant = setup.grants && isArray(setup.grants.shell) ? setup.grants.shell : null;
  const shell = shellGrant
    ? {
        run(req) {
          const r = opts(req);
          const a = { shell: r.shell, script: r.script, env: {} };
          let repository = null;
          if (r.cwd !== undefined) [a.cwd, repository] = pathArg(r.cwd, "cwd");
          if (r.env !== undefined) assign(a.env, r.env);
          if (r.timeoutMs !== undefined) a.timeoutMs = r.timeoutMs;
          const command = `${r.shell} script`;
          return finish(host("shell.run", a, { repository, command }), command, repository);
        },
        available() {
          return aSlice(host("shell.available", {}).shells || []);
        },
      }
    : undefined;

  // ---- runs, logs, repositories -------------------------------------------

  function show(v) {
    if (typeof v === "string") return v;
    if (v instanceof Error) return v.stack ? `${v.name}: ${v.message}\n${v.stack}` : `${v.name}: ${v.message}`;
    if (v === undefined) return "undefined";
    try {
      const s = stringify(v);
      return s === undefined ? String(v) : s;
    } catch (_) {
      return String(v);
    }
  }

  const log = {};
  for (const level of ["debug", "info", "warn", "error"]) {
    log[level] = (...a) => nativeLog(level, aJoin(aMap(a, show), " "));
  }

  function faultOf(e) {
    if (e instanceof ScriptError) {
      const f = { code: e.code, message: e.message };
      for (const k of FIELDS) if (typeof e[k] === "string") f[k] = e[k];
      return f;
    }
    return { code: "scriptError", message: e instanceof Error ? `${e.name}: ${e.message}` : show(e) };
  }

  function inRepository(e, repo) {
    if (e instanceof ScriptError) {
      if (e.repository) return e;
      const named = new ScriptError(e.code, `${e.message} (repository "${repo.key}")`, { repository: repo.key, command: e.command, path: e.path });
      return named;
    }
    const what = e instanceof Error ? `${e.name}: ${e.message}` : show(e);
    return new ScriptError("scriptError", `in repository "${repo.key}": ${what}`, { repository: repo.key });
  }

  function isResult(v) {
    return v !== null && typeof v === "object" && typeof v.ok === "boolean" && "exitCode" in v && "command" in v;
  }

  function forEachRepository(fn, o) {
    if (typeof fn !== "function") throw new ScriptError("invalidArgument", "forEachRepository takes a function");
    const project = projects.current();
    if (!project) throw new ScriptError("noProject", "there is no current project");
    const stopOnError = typeof opts(o).stopOnError === "boolean" ? opts(o).stopOnError : setup.stopOnError !== false;
    const outcomes = [];
    for (const repo of project.repositories()) {
      let outcome;
      try {
        const value = fn(repo);
        outcome = isResult(value) && !value.ok
          ? { repository: repo.key, ok: false, error: ScriptError.fromResult(value, repo) }
          : { repository: repo.key, ok: true, value };
      } catch (e) {
        if (e instanceof ScriptError && (e.code === "cancelled" || e.code === "timedOut")) {
          nativeRecord(stringify({ repository: repo.key, ok: false, error: faultOf(e) }));
          throw e;
        }
        outcome = { repository: repo.key, ok: false, error: inRepository(e, repo) };
      }
      nativeRecord(stringify(outcome.ok ? { repository: repo.key, ok: true } : { repository: repo.key, ok: false, error: faultOf(outcome.error) }));
      aPush(outcomes, freeze(outcome));
      if (!outcome.ok && stopOnError) throw outcome.error;
    }
    return outcomes;
  }

  function sleep(ms) {
    if (typeof ms !== "number" || !(ms >= 0)) throw new ScriptError("invalidArgument", "sleep takes milliseconds");
    host("script.sleep", { ms: Math.min(Math.floor(ms), 600000) });
  }

  const run = setup.run || {};
  const deckpilot = {
    platform: setup.platform,
    run: {
      id: String(run.id || ""),
      origin: String(run.origin || "editor"),
      userInitiated: run.userInitiated === true,
      inputs: run.inputs === undefined ? null : run.inputs,
      signal: defineProperty({}, "aborted", { get: () => nativeAborted(), enumerable: true }),
    },
    sleep,
    log,
    projects,
    repositories: repositoriesApi,
    path,
    fs,
    process: { run: (req) => processRun(req, null) },
    git,
    docker,
    forEachRepository,
  };
  if (shell) deckpilot.shell = shell;

  function deepFreeze(o) {
    if (o === null || (typeof o !== "object" && typeof o !== "function") || isFrozen(o)) return o;
    freeze(o);
    for (const k of ownKeys(o)) {
      const d = getOwnPropertyDescriptor(o, k);
      if (d && "value" in d) deepFreeze(d.value);
    }
    return o;
  }

  const console = {
    log: log.info,
    info: log.info,
    debug: log.debug,
    warn: log.warn,
    error: log.error,
  };

  deepFreeze(ScriptError.prototype);
  for (const [name, value] of [["deckpilot", deepFreeze(deckpilot)], ["ScriptError", freeze(ScriptError)], ["console", deepFreeze(console)]]) {
    defineProperty(globalThis, name, { value, writable: false, enumerable: false, configurable: false });
  }

  // No code from strings: eval, Function and the generator and async
  // function constructors throw, and every path to the originals (the
  // globals and the prototypes' `constructor`) is replaced and frozen.
  const refuse = function () {
    throw new EvalError("code from strings is disabled in DeckPilot scripts");
  };
  freeze(refuse);
  const constructors = [
    Function,
    getPrototypeOf(function* () {}).constructor,
    getPrototypeOf(async function () {}).constructor,
    getPrototypeOf(async function* () {}).constructor,
  ];
  for (const C of constructors) {
    defineProperty(C.prototype, "constructor", { value: refuse, writable: false, configurable: false });
    freeze(C.prototype);
  }
  for (const name of ["eval", "Function"]) {
    defineProperty(globalThis, name, { value: refuse, writable: false, configurable: false });
  }
})(globalThis.__dp);
