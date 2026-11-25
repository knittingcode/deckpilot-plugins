// The JavaScript API of DeckPilot scripts (deckpilot.scripts 0.1.0), for
// editor completion. The runtime is src/prelude.js; every call that reaches
// files, programs or a shell is checked by DeckPilot against the grants of
// the script (its Permissions tab) at the time of the call. A new or imported
// script has none.
//
// The API is synchronous: each call waits for DeckPilot's answer. There is no
// `eval`, no `Function` from strings, no `import`, no timers but
// `deckpilot.sleep`, and no Node.js or browser API.

declare const deckpilot: {
  /** The operating system the script runs on; `deckpilot.path` follows it. */
  readonly platform: "macos" | "windows" | "linux";
  readonly run: {
    readonly id: string;
    readonly origin: "editor" | "key" | "gesture" | "sequence" | "workflow" | "trigger";
    /** Started by the user (a key, the editor), not by a trigger. Confirmations need it. */
    readonly userInitiated: boolean;
    /** The run's `inputs` parameter, parsed JSON (`null` without one). */
    readonly inputs: unknown;
    /** True once the run was stopped. A stopped script ends at its next step. */
    readonly signal: { readonly aborted: boolean };
  };
  /** Waits `ms` milliseconds (at most the run's remaining time); Stop ends it at once. */
  sleep(ms: number): void;

  /** Lines in the run's log: at most 1,000 lines or 256 KiB per run. */
  log: {
    debug(...a: unknown[]): void;
    info(...a: unknown[]): void;
    warn(...a: unknown[]): void;
    error(...a: unknown[]): void;
  };

  /** Grant: `projects.read`. The active profile's projects only. */
  projects: {
    current(): Project | null;
    list(): Project[];
  };
  /** Grant: `projects.read` (and `repositories.paths` for `Repository.path`). */
  repositories: {
    /** The current project's repositories, `[]` without a project. */
    current(): Repository[];
    /** The default repository; or the only one; otherwise `null`. */
    default(): Repository | null;
    get(key: string): Repository | null;
  };
  /** Pure path functions with the host OS's semantics. No grant; no file system. */
  path: {
    readonly sep: "/" | "\\";
    join(...p: string[]): string;
    normalize(p: string): string;
    /** `base` must be absolute: there is no working directory. */
    resolve(base: string, ...p: string[]): string;
    dirname(p: string): string;
    basename(p: string, ext?: string): string;
    extname(p: string): string;
    relative(from: string, to: string): string;
    isAbsolute(p: string): boolean;
    toPosix(p: string): string;
    fromPosix(p: string): string;
  };
  /**
   * Grant: file system scopes (a repository, every repository of the current
   * project, or a folder you picked), each read, write or delete. Relative
   * strings are refused: use `repo.join(...)` or an absolute path.
   */
  fs: {
    exists(path: PathLike): boolean;
    stat(path: PathLike): { kind: "file" | "directory" | "symlink" | "other"; size: number; modifiedMs: number } | null;
    /** At most 10,000 entries. */
    list(path: PathLike, opts?: { recursive?: false }): { name: string; kind: "file" | "directory" | "symlink" | "other" }[];
    mkdir(path: PathLike, opts?: { recursive?: boolean }): void;
    /**
     * At most 4 MiB (`maxBytes`, 1 to 4 MiB, lowers it: a larger file is
     * `tooLarge`). Read in chunks of 512 KiB; a file whose size changes
     * meanwhile fails with `fileChanged` (read it again). A change that
     * keeps the size is not detected yet. `utf8` (the default) refuses
     * bytes that are not UTF-8.
     */
    readFile(path: PathLike, opts?: { encoding?: "utf8" | "base64"; maxBytes?: number }): string;
    /**
     * At most 4 MiB. Written to a temporary file next to the target and
     * renamed onto it at the end, so a failure or a Stop leaves the target
     * as it was. `createOnly` fails with `exists` if the target exists.
     */
    writeFile(path: PathLike, content: string, opts?: { encoding?: "utf8" | "base64"; createOnly?: boolean }): void;
    copy(src: PathLike, dst: PathLike, opts?: { recursive?: boolean; overwrite?: boolean }): void;
    move(src: PathLike, dst: PathLike, opts?: { overwrite?: boolean }): void;
    /**
     * `dryRun` returns the plan and removes nothing. A real removal may ask
     * you to confirm (in runs you started); symbolic links are never followed;
     * a repository's `.git` and protected folders are never removed.
     */
    remove(path: PathLike, opts?: { recursive?: boolean; dryRun?: boolean }): RemovalPlan;
    removeContents(path: PathLike, opts?: { dryRun?: boolean; keep?: string[] }): RemovalPlan;
  };
  /**
   * Grant: `process` (each command, its working directories, environment).
   * Arguments are an array: there is never a shell. A non-zero exit does not
   * throw; the result says `ok: false`. `args` are at most 48 KiB in total
   * and `stdin` at most 48 KiB (write a larger input to a file).
   * `timeoutMs` (at most 600,000) never outlasts the run's remaining time.
   */
  process: {
    run(req: {
      command: string;
      args?: string[];
      cwd?: PathLike;
      env?: Record<string, string>;
      timeoutMs?: number;
      stdin?: string;
    }): ProcessResult;
  };
  /** Present only with the `shell` grant (off by default). Not portable. `script` is at most 48 KiB. */
  shell?: {
    run(req: {
      shell: "sh" | "bash" | "zsh" | "pwsh" | "powershell" | "cmd";
      script: string;
      cwd?: PathLike;
      env?: Record<string, string>;
      timeoutMs?: number;
    }): ProcessResult;
    /** The granted shells that exist on this computer. */
    available(): string[];
  };
  /** `process.run` with command `git` (your configured Git). */
  git: {
    available(): { ok: boolean; version?: string; error?: string };
    run(args: string[], opts: { cwd: PathLike; timeoutMs?: number }): ProcessResult;
  };
  /** `process.run` with command `docker` (your configured Docker). */
  docker: {
    available(): { ok: boolean; version?: string; composeVersion?: string; error?: string };
    run(args: string[], opts?: { cwd?: PathLike; timeoutMs?: number }): ProcessResult;
    /** `docker compose ...` */
    compose(args: string[], opts: { cwd: PathLike; timeoutMs?: number }): ProcessResult;
  };
  /**
   * Calls `fn` for each repository of the current project, in order. A
   * repository fails when `fn` throws or returns a failed ProcessResult; its
   * error names the repository. With `stopOnError` (default: the script's
   * setting, on) the first failure is thrown; without it every repository
   * runs and the run reports each failure.
   */
  forEachRepository<T>(fn: (repo: Repository) => T, opts?: { stopOnError?: boolean }): RepoOutcome<T>[];
};

/** `console.log` and friends write to the run's log (`log` is `info`). */
declare const console: {
  log(...a: unknown[]): void;
  info(...a: unknown[]): void;
  debug(...a: unknown[]): void;
  warn(...a: unknown[]): void;
  error(...a: unknown[]): void;
};

/** A path: an absolute string, a repository (its root), or `repo.join(...)`. */
type PathLike = string | Repository | PathRef;

interface Project {
  id: string;
  key: string;
  name: string;
  repositories(): Repository[];
}

interface Repository {
  id: string;
  key: string;
  name: string;
  description: string | null;
  isDefault: boolean;
  /** `null` without the `repositories.paths` grant. */
  path: string | null;
  exists: boolean;
  remote: { url: string | null; defaultBranch: string | null } | null;
  /** A path inside the repository; refuses `..` that would leave it. Works without `repositories.paths`. */
  join(...p: string[]): PathRef;
}

interface PathRef {
  readonly repository: string;
  readonly relative: string;
}

interface ProcessResult {
  ok: boolean;
  exitCode: number | null;
  signal: string | null;
  stdout: string;
  stderr: string;
  /** The output was longer than the limit and was cut. */
  truncated: boolean;
  durationMs: number;
  timedOut: boolean;
  cancelled: boolean;
  /** The command as shown in errors, e.g. `git pull --ff-only`. */
  command: string;
  /** The repository it ran in (by its `cwd`), or `null`. */
  repository: string | null;
  /** Why it failed: `nonZeroExit`, `timedOut`, `cancelled`, `dockerDaemonDown`; `null` when `ok`. */
  errorCode: ErrorCode | null;
}

interface RemovalPlan {
  dryRun: boolean;
  root: string;
  files: number;
  directories: number;
  bytes: number;
  /** At most 50 paths. */
  sample: string[];
  removed: boolean;
}

interface RepoOutcome<T> {
  repository: string;
  ok: boolean;
  value?: T;
  error?: ScriptError;
}

declare class ScriptError extends Error {
  constructor(code: ErrorCode, message: string, detail?: { repository?: string; command?: string; path?: string });
  readonly code: ErrorCode;
  readonly repository?: string;
  readonly command?: string;
  readonly path?: string;
  /** The error of a failed result, naming the command and the repository:
   *  `git pull --ff-only failed in repository "cms" (exit 1): fatal: …` */
  static fromResult(result: ProcessResult, repository?: Repository | string): ScriptError;
  /** `result` when it succeeded; throws `fromResult(result)` otherwise. */
  static check(result: ProcessResult, repository?: Repository | string): ProcessResult;
}

type ErrorCode =
  | "notGranted" | "outsideScope" | "protectedPath" | "symlinkEscape" | "notFound" | "exists" | "tooLarge"
  | "commandNotFound" | "gitUnavailable" | "dockerUnavailable" | "dockerDaemonDown" | "nonZeroExit" | "timedOut" | "cancelled"
  | "needsConfirmation" | "notUserInitiated" | "limit" | "invalidArgument" | "noProject" | "noDefaultRepository"
  | "scriptError" | "syntaxError" | "unsupported" | "unavailable" | "hostError" | "fileChanged";
