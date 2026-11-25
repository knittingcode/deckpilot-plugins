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
//
// Nothing here goes through a lookup the script can replace. The built-ins
// it uses are taken before the script runs; it loops by index (no for-of,
// no destructuring, no spread: no iterator protocol); it never calls a
// method that consults Symbol.species, Symbol.split or RegExp.prototype.exec
// (Array map/filter/slice/concat, String split, RegExp test); it adds
// properties with defineProperty, never by assignment (an accessor on
// Object.prototype could take them); and it serializes requests itself
// (JSON.stringify consults toJSON on objects).
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

  const O = Object;
  const defineProperty = O.defineProperty;
  const freeze = O.freeze;
  const getOwnPropertyDescriptor = O.getOwnPropertyDescriptor;
  const getPrototypeOf = O.getPrototypeOf;
  const isFrozen = O.isFrozen;
  const keys = O.keys;
  const ownKeys = Reflect.ownKeys;
  const isArray = Array.isArray;
  const toStr = String;
  const mMin = Math.min;
  const mFloor = Math.floor;
  const ErrorBase = Error;
  const EvalErrorBase = EvalError;
  const uncurry = Function.prototype.bind.bind(Function.prototype.call);
  const sIncludes = uncurry(String.prototype.includes);
  const sSlice = uncurry(String.prototype.slice);
  const sTrim = uncurry(String.prototype.trim);
  const reExec = uncurry(RegExp.prototype.exec);
  const wsHas = uncurry(WeakSet.prototype.has);
  const wsAdd = uncurry(WeakSet.prototype.add);

  // ---- helpers that use nothing the script can replace ---------------------

  // An own data property's value, or undefined: never a getter of the
  // object's prototypes (a reply's absent `error` must not be the script's
  // Object.prototype.error).
  function ownValue(o, k) {
    if (o === null || typeof o !== "object") return undefined;
    const d = getOwnPropertyDescriptor(o, k);
    return d && "value" in d ? d.value : undefined;
  }

  // An own data property, as an object literal makes it.
  function own(o, k, v) {
    defineProperty(o, k, { value: v, writable: true, enumerable: true, configurable: true });
    return o;
  }
  function put(arr, v) {
    return own(arr, arr.length, v);
  }
  function map(arr, f) {
    const out = [];
    for (let i = 0; i < arr.length; i++) put(out, f(arr[i]));
    return out;
  }
  function find(arr, f) {
    for (let i = 0; i < arr.length; i++) if (f(arr[i])) return arr[i];
    return undefined;
  }
  function copy(arr) {
    return map(arr, (x) => x);
  }
  function join(arr, sep) {
    let s = "";
    for (let i = 0; i < arr.length; i++) s += (i === 0 ? "" : sep) + arr[i];
    return s;
  }
  // `o` with its own enumerable properties copied, and `k: v` added.
  function withField(o, k, v) {
    const out = {};
    const ks = keys(o);
    for (let i = 0; i < ks.length; i++) own(out, ks[i], o[ks[i]]);
    return own(out, k, v);
  }
  // Splits at every character for which `isSep` is true.
  function splitAt(text, isSep) {
    const out = [];
    let start = 0;
    for (let i = 0; i < text.length; i++) {
      if (isSep(text[i])) {
        put(out, sSlice(text, start, i));
        start = i + 1;
      }
    }
    return put(out, sSlice(text, start));
  }
  function lines(text) {
    return map(splitAt(text, (c) => c === "\n"), (l) => (l.length > 0 && l[l.length - 1] === "\r" ? sSlice(l, 0, -1) : l));
  }
  const isPathSep = (c) => c === "/" || c === "\\";
  function matches(re, s) {
    return reExec(re, s) !== null;
  }

  // The JSON of a request: plain objects, arrays and primitives, without
  // toJSON (JSON.stringify consults it on objects; on primitives it does
  // not, so strings are escaped by it).
  function toJson(v, depth) {
    const d = depth || 0;
    if (d > 32) throw new ScriptError("invalidArgument", "the value is nested too deeply");
    if (typeof v === "bigint") throw new ScriptError("invalidArgument", "a BigInt is not a JSON value");
    if (v === null || typeof v !== "object") {
      const s = stringify(v);
      return s === undefined ? "null" : s;
    }
    let s;
    if (isArray(v)) {
      s = "[";
      for (let i = 0; i < v.length; i++) s += (i === 0 ? "" : ",") + toJson(v[i], d + 1);
      return s + "]";
    }
    const ks = keys(v);
    s = "{";
    let first = true;
    for (let i = 0; i < ks.length; i++) {
      const x = v[ks[i]];
      if (x === undefined || typeof x === "function") continue;
      s += (first ? "" : ",") + stringify(ks[i]) + ":" + toJson(x, d + 1);
      first = false;
    }
    return s + "}";
  }

  const FIELDS = ["repository", "command", "path"];

  class ScriptError extends ErrorBase {
    constructor(code, message, detail) {
      super(message);
      own(this, "name", "ScriptError");
      own(this, "code", typeof code === "string" ? code : "scriptError");
      for (let i = 0; i < FIELDS.length; i++) {
        const k = FIELDS[i];
        if (detail && typeof detail[k] === "string") own(this, k, detail[k]);
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
    const all = lines(text);
    const kept = [];
    for (let i = 0; i < all.length; i++) {
      const l = sTrim(all[i]);
      if (l.length > 0) put(kept, l);
    }
    // git and docker put the reason on the last line ("fatal: …").
    const line = find(kept, (l) => matches(/^(fatal|error)\b/i, l)) || kept[kept.length - 1] || "";
    return line.length > 300 ? sSlice(line, 0, 300) + "…" : line;
  }

  function fault(f, context) {
    const code = ownValue(f, "code");
    const detail = {
      repository: ownValue(f, "repository") || (context && context.repository) || undefined,
      command: ownValue(f, "command") || (context && context.command) || undefined,
      path: ownValue(f, "path") || undefined,
    };
    let message = ownValue(f, "message") || code;
    if (detail.repository && !sIncludes(message, `"${detail.repository}"`)) {
      message = `${message} (repository "${detail.repository}")`;
    }
    const e = new ScriptError(code, message, detail);
    const confirmation = ownValue(f, "confirmation");
    if (confirmation) defineProperty(e, "confirmation", { value: confirmation });
    return e;
  }

  // One bridge op; a refusal or failure is thrown as a ScriptError.
  function host(op, args, context) {
    const reply = parse(nativeCall(op, toJson(args)));
    const error = ownValue(reply, "error");
    if (error) throw fault(error, context);
    return ownValue(reply, "ok");
  }

  // ---- paths and repositories -------------------------------------------

  const repositories = new WeakSet();

  function relativeJoin(key, parts) {
    const out = [];
    for (let i = 0; i < parts.length; i++) {
      const part = parts[i];
      if (typeof part !== "string") {
        throw new ScriptError("invalidArgument", "repo.join takes strings", { repository: key });
      }
      if (isPathSep(part[0]) || sIncludes(part, ":") || sIncludes(part, "\0")) {
        throw new ScriptError("outsideScope", `\`${part}\` is not a path inside repository "${key}"`, { repository: key });
      }
      const segs = splitAt(part, isPathSep);
      for (let j = 0; j < segs.length; j++) {
        const seg = segs[j];
        if (seg === "" || seg === ".") continue;
        if (seg === "..") {
          if (out.length === 0) {
            throw new ScriptError("outsideScope", `\`${join(parts, "/")}\` leaves repository "${key}"`, { repository: key });
          }
          out.length = out.length - 1;
        } else put(out, seg);
      }
    }
    return join(out, "/");
  }

  function repositoryKey(r) {
    if (typeof r === "string") return r;
    if (r && wsHas(repositories, r)) return r.key;
    return null;
  }

  // PathLike -> { arg: the bridge's path argument, repository: the
  // repository it is in, or null }.
  function pathArg(p, what) {
    if (typeof p === "string") return { arg: { path: p }, repository: null };
    if (p && typeof p === "object") {
      if (wsHas(repositories, p)) return { arg: { repository: p.key, relative: "" }, repository: p.key };
      // Each read once: a getter answers the same for the check and the use.
      const repository = p.repository;
      const relative = p.relative;
      if (typeof repository === "string" && typeof relative === "string") {
        return { arg: { repository, relative }, repository };
      }
    }
    throw new ScriptError("invalidArgument", `${what} is a path: an absolute string, a repository or repo.join(...)`);
  }

  function makeRepository(r) {
    const key = toStr(r.key);
    const repo = {
      id: toStr(r.id),
      key,
      name: toStr(r.name),
      description: r.description == null ? null : toStr(r.description),
      isDefault: r.isDefault === true,
      path: typeof r.path === "string" ? r.path : null,
      exists: r.exists === true,
      remote: r.remote
        ? { url: r.remote.url == null ? null : toStr(r.remote.url), defaultBranch: r.remote.defaultBranch == null ? null : toStr(r.remote.defaultBranch) }
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
    let cache = repos ? map(repos, makeRepository) : null;
    const id = toStr(p.id);
    return deepFreeze({
      id,
      key: toStr(p.key),
      name: toStr(p.name),
      repositories() {
        if (cache === null) {
          const r = host("project.repositories", { project: id });
          cache = map(r.repositories || [], makeRepository);
        }
        return copy(cache);
      },
    });
  }

  const projects = {
    current() {
      const r = host("project.repositories", { project: null });
      return r.project ? makeProject(r.project, r.repositories || []) : null;
    },
    list() {
      return map(host("project.list", {}).projects || [], (p) => makeProject(p, null));
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
      return find(all, (r) => r.isDefault) || (all.length === 1 ? all[0] : null);
    },
    get(key) {
      return find(repositoriesApi.current(), (r) => r.key === key) || null;
    },
  };

  const path = {
    sep: setup.sep,
  };
  const PATH_OPS = ["join", "normalize", "resolve", "dirname", "basename", "extname", "relative", "isAbsolute", "toPosix", "fromPosix"];
  for (let i = 0; i < PATH_OPS.length; i++) {
    const op = PATH_OPS[i];
    own(path, op, function (...args) {
      const given = [];
      for (let j = 0; j < args.length; j++) {
        const a = args[j];
        if (a === undefined) continue;
        if (typeof a !== "string") throw new ScriptError("invalidArgument", `path.${op} takes strings`);
        put(given, a);
      }
      const reply = parse(nativePath(op, toJson(given)));
      const error = ownValue(reply, "error");
      if (error) throw fault(error);
      return ownValue(reply, "ok");
    });
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
      if (!(e instanceof ScriptError) || ownValue(e, "code") !== "needsConfirmation") throw e;
      const token = ownValue(ownValue(e, "confirmation"), "token");
      if (typeof token !== "string") throw e;
      const answer = host("script.confirm", { token }, context);
      if (ownValue(answer, "confirmed") !== true) {
        throw fault({ code: "needsConfirmation", message: `${sSlice(op, 3)} was not confirmed` }, context);
      }
      return host(op, withField(args, "confirmation", token), context);
    }
  }

  const fs = {
    exists(p) {
      const a = pathArg(p, "path");
      return host("fs.exists", { path: a.arg }, a).exists === true;
    },
    stat(p) {
      const a = pathArg(p, "path");
      return host("fs.stat", { path: a.arg }, a).stat || null;
    },
    list(p, o) {
      const a = pathArg(p, "path");
      return host("fs.list", { path: a.arg, recursive: opts(o).recursive === true }, a).entries || [];
    },
    mkdir(p, o) {
      const a = pathArg(p, "path");
      host("fs.mkdir", { path: a.arg, recursive: opts(o).recursive === true }, a);
    },
    // Chunked by the guest (512 KiB per request); see api/fs.rs.
    readFile(p, o) {
      const a = pathArg(p, "path");
      const opt = opts(o);
      const encoding = opt.encoding;
      const maxBytes = opt.maxBytes;
      const args = { path: a.arg, encoding: encoding || "utf8" };
      if (maxBytes !== undefined) own(args, "maxBytes", maxBytes);
      return host("fs.readFile", args, a).content;
    },
    // Chunked by the guest (32 KiB per request), into a temporary file the
    // agent renames into place at the end; see api/fs.rs.
    writeFile(p, content, o) {
      const a = pathArg(p, "path");
      if (typeof content !== "string") throw new ScriptError("invalidArgument", "writeFile takes a string", a);
      host("fs.writeFile", { path: a.arg, content, encoding: opts(o).encoding || "utf8", createOnly: opts(o).createOnly === true }, a);
    },
    copy(src, dst, o) {
      const s = pathArg(src, "src");
      const d = pathArg(dst, "dst");
      destructive("fs.copy", { src: s.arg, dst: d.arg, recursive: opts(o).recursive === true, overwrite: opts(o).overwrite === true }, { repository: d.repository || s.repository });
    },
    move(src, dst, o) {
      const s = pathArg(src, "src");
      const d = pathArg(dst, "dst");
      destructive("fs.move", { src: s.arg, dst: d.arg, overwrite: opts(o).overwrite === true }, { repository: s.repository || d.repository });
    },
    remove(p, o) {
      const a = pathArg(p, "path");
      return destructive("fs.remove", { path: a.arg, recursive: opts(o).recursive === true, dryRun: opts(o).dryRun === true }, a);
    },
    removeContents(p, o) {
      const a = pathArg(p, "path");
      const keep = opts(o).keep === undefined ? [] : opts(o).keep;
      return destructive("fs.removeContents", { path: a.arg, dryRun: opts(o).dryRun === true, keep }, a);
    },
  };

  // ---- processes ----------------------------------------------------------

  const PLAIN_ARG = /^[A-Za-z0-9_@%+=:,./-]+$/;

  function display(command, args) {
    let s = matches(PLAIN_ARG, command) ? command : stringify(command);
    for (let i = 0; i < args.length; i++) s += " " + (matches(PLAIN_ARG, args[i]) ? args[i] : stringify(args[i]));
    return s;
  }

  function strings(v, what) {
    if (v === undefined) return [];
    if (!isArray(v)) throw new ScriptError("invalidArgument", `${what} is an array of strings (there is no shell)`);
    const out = [];
    const n = v.length;
    for (let i = 0; i < n; i++) {
      // Each element read once: what is checked is what is sent.
      const a = v[i];
      if (typeof a !== "string") throw new ScriptError("invalidArgument", `${what} is an array of strings (there is no shell)`);
      put(out, a);
    }
    return out;
  }

  function envOf(env) {
    const out = {};
    if (env === undefined) return out;
    if (typeof env !== "object" || env === null) throw new ScriptError("invalidArgument", "env is an object of strings");
    const ks = keys(env);
    for (let i = 0; i < ks.length; i++) {
      const v = env[ks[i]];
      if (typeof v !== "string") throw new ScriptError("invalidArgument", `env.${ks[i]} is not a string`);
      own(out, ks[i], v);
    }
    return out;
  }

  function processRun(req, unavailable) {
    // Each field read once: what is checked is what is sent.
    const r = opts(req);
    const name = r.command;
    const rawArgs = r.args;
    const env = r.env;
    const cwd = r.cwd;
    const timeoutMs = r.timeoutMs;
    const stdin = r.stdin;
    if (typeof name !== "string") throw new ScriptError("invalidArgument", "process.run needs a `command`");
    const args = strings(rawArgs, "args");
    const a = { command: name, args, env: envOf(env) };
    let repository = null;
    if (cwd !== undefined) {
      const c = pathArg(cwd, "cwd");
      own(a, "cwd", c.arg);
      repository = c.repository;
    }
    if (timeoutMs !== undefined) own(a, "timeoutMs", timeoutMs);
    if (stdin !== undefined) own(a, "stdin", stdin);
    const command = display(name, args);
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
    if (!r.ok && !r.timedOut && !r.cancelled && matches(DAEMON_DOWN, r.stderr)) {
      return freeze(withField(r, "errorCode", "dockerDaemonDown"));
    }
    return r;
  }

  // The `cwd` of `o`, read once; refused when there is none.
  function needCwd(o, what) {
    const cwd = o ? o.cwd : undefined;
    if (cwd === undefined) throw new ScriptError("invalidArgument", `${what} needs { cwd }: a repository or a path`);
    return cwd;
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
      const cwd = needCwd(o, "git.run");
      return processRun({ command: "git", args: strings(args, "args"), cwd, timeoutMs: o.timeoutMs }, "gitUnavailable");
    },
  };

  const docker = {
    available() {
      try {
        const client = processRun({ command: "docker", args: ["version", "--format", "{{.Client.Version}}"] }, "dockerUnavailable");
        const version = lines(sTrim(client.stdout))[0] || undefined;
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
      const cwd = needCwd(o, "docker.compose");
      const all = ["compose"];
      const rest = strings(args, "args");
      for (let i = 0; i < rest.length; i++) put(all, rest[i]);
      return docker.run(all, { cwd, timeoutMs: o.timeoutMs });
    },
  };

  const shellGrant = setup.grants && isArray(setup.grants.shell) ? setup.grants.shell : null;
  const shell = shellGrant
    ? {
        run(req) {
          // Each field read once: what is checked is what is sent.
          const r = opts(req);
          const name = r.shell;
          const script = r.script;
          const env = r.env;
          const cwd = r.cwd;
          const timeoutMs = r.timeoutMs;
          const a = { shell: name, script, env: envOf(env) };
          let repository = null;
          if (cwd !== undefined) {
            const c = pathArg(cwd, "cwd");
            own(a, "cwd", c.arg);
            repository = c.repository;
          }
          if (timeoutMs !== undefined) own(a, "timeoutMs", timeoutMs);
          const command = `${name} script`;
          return finish(host("shell.run", a, { repository, command }), command, repository);
        },
        available() {
          return copy(host("shell.available", {}).shells || []);
        },
      }
    : undefined;

  // ---- runs, logs, repositories -------------------------------------------

  function show(v) {
    if (typeof v === "string") return v;
    if (v instanceof ErrorBase) return v.stack ? `${v.name}: ${v.message}\n${v.stack}` : `${v.name}: ${v.message}`;
    if (v === undefined) return "undefined";
    try {
      const s = stringify(v);
      return s === undefined ? toStr(v) : s;
    } catch (_) {
      return toStr(v);
    }
  }

  const log = {};
  const LEVELS = ["debug", "info", "warn", "error"];
  for (let i = 0; i < LEVELS.length; i++) {
    const level = LEVELS[i];
    own(log, level, (...a) => nativeLog(level, join(map(a, show), " ")));
  }

  function faultOf(e) {
    if (e instanceof ScriptError) {
      const f = { code: e.code, message: e.message };
      for (let i = 0; i < FIELDS.length; i++) if (typeof e[FIELDS[i]] === "string") own(f, FIELDS[i], e[FIELDS[i]]);
      return f;
    }
    return { code: "scriptError", message: e instanceof ErrorBase ? `${e.name}: ${e.message}` : show(e) };
  }

  function inRepository(e, repo) {
    if (e instanceof ScriptError) {
      if (e.repository) return e;
      return new ScriptError(e.code, `${e.message} (repository "${repo.key}")`, { repository: repo.key, command: e.command, path: e.path });
    }
    const what = e instanceof ErrorBase ? `${e.name}: ${e.message}` : show(e);
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
    const repos = project.repositories();
    for (let i = 0; i < repos.length; i++) {
      const repo = repos[i];
      let outcome;
      try {
        const value = fn(repo);
        outcome = isResult(value) && !value.ok
          ? { repository: repo.key, ok: false, error: ScriptError.fromResult(value, repo) }
          : { repository: repo.key, ok: true, value };
      } catch (e) {
        if (e instanceof ScriptError && (e.code === "cancelled" || e.code === "timedOut")) {
          nativeRecord(toJson({ repository: repo.key, ok: false, error: faultOf(e) }));
          throw e;
        }
        outcome = { repository: repo.key, ok: false, error: inRepository(e, repo) };
      }
      nativeRecord(toJson(outcome.ok ? { repository: repo.key, ok: true } : { repository: repo.key, ok: false, error: faultOf(outcome.error) }));
      put(outcomes, freeze(outcome));
      if (!outcome.ok && stopOnError) throw outcome.error;
    }
    return outcomes;
  }

  function sleep(ms) {
    if (typeof ms !== "number" || !(ms >= 0)) throw new ScriptError("invalidArgument", "sleep takes milliseconds");
    host("script.sleep", { ms: mMin(mFloor(ms), 600000) });
  }

  const run = setup.run || {};
  const deckpilot = {
    platform: setup.platform,
    run: {
      id: toStr(run.id || ""),
      origin: toStr(run.origin || "editor"),
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
  if (shell) own(deckpilot, "shell", shell);

  function deepFreeze(o) {
    if (o === null || (typeof o !== "object" && typeof o !== "function") || isFrozen(o)) return o;
    freeze(o);
    const ks = ownKeys(o);
    for (let i = 0; i < ks.length; i++) {
      const d = getOwnPropertyDescriptor(o, ks[i]);
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

  // The stack-trace hooks of QuickJS-NG (V8's API) would run script code
  // whenever any error's stack is made, the API's own included: fixed
  // before the script runs.
  defineProperty(ErrorBase, "prepareStackTrace", { value: undefined, writable: false, enumerable: false, configurable: false });
  defineProperty(ErrorBase, "stackTraceLimit", { value: ErrorBase.stackTraceLimit, writable: false, enumerable: false, configurable: false });

  deepFreeze(ScriptError.prototype);
  const GLOBALS = [
    ["deckpilot", deepFreeze(deckpilot)],
    ["ScriptError", freeze(ScriptError)],
    ["console", deepFreeze(console)],
  ];
  for (let i = 0; i < GLOBALS.length; i++) {
    defineProperty(globalThis, GLOBALS[i][0], { value: GLOBALS[i][1], writable: false, enumerable: false, configurable: false });
  }

  // No code from strings: eval, Function and the generator and async
  // function constructors throw, and every path to the originals (the
  // globals and the prototypes' `constructor`) is replaced and frozen.
  const refuse = function () {
    throw new EvalErrorBase("code from strings is disabled in DeckPilot scripts");
  };
  freeze(refuse);
  const constructors = [
    Function,
    getPrototypeOf(function* () {}).constructor,
    getPrototypeOf(async function () {}).constructor,
    getPrototypeOf(async function* () {}).constructor,
  ];
  for (let i = 0; i < constructors.length; i++) {
    defineProperty(constructors[i].prototype, "constructor", { value: refuse, writable: false, configurable: false });
    freeze(constructors[i].prototype);
  }
  const DISABLED = ["eval", "Function"];
  for (let i = 0; i < DISABLED.length; i++) {
    defineProperty(globalThis, DISABLED[i], { value: refuse, writable: false, configurable: false });
  }
})(globalThis.__dp);
