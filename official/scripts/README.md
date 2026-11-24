# deckpilot.scripts (Scripts / Skrypty)

DeckPilot's Scripts plugin: user JavaScript, run on keys, gestures, Multi
Action steps and workflow nodes, against the repositories of the current
project. DeckPilot `design-projects-scripts.md` §4, §5 and PR **P5**.

> **Status: held, not released.** The manifest needs plugin API **1.13**
> (the `scripts` permission, the `script` parameter kind, the `scripts`
> view), which no DeckPilot has yet. `official/plugins.json` lists it as
> `held` with `hostApi: "1.13"` (`tools/check-official.py`, "Ahead of the
> pin"). It is built, tested and checked here, but never packaged, signed,
> released or given a Marketplace entry. It runs nothing until the agent's
> host bridge (P6) exists. Without the bridge, `run` fails with `unsupported`.

## Runtime

QuickJS-NG through `rquickjs = "=0.14.0"` (MIT), compiled into this
plugin's own `wasm32-unknown-unknown` module (design §4.1, option A). The
module runs in the agent's out-of-process wasmtime host like every plugin,
with its fuel, epoch and memory limits.

- **The only import is `deckpilot_v1.host_call`.** `rquickjs-sys` builds
  QuickJS against a vendored wasi-libc archive plus a shim whose only
  import is `env.__rquickjs_host_now_us`. `src/clock.rs` defines that
  symbol, so the linker resolves it inside the module. There is no WASI
  import and no `env` import. `tools/official.sh package` runs
  `tools/check-wasm-imports.py` on every build.
- **Nothing ambient.** `quickjs-libc` is not linked, so there is no `std`,
  `os`, files or timers. There is no module loader: a static `import` is a
  syntax error and `import()` is rejected. `eval` and the `Function`,
  generator and async function constructors throw. The `deckpilot`,
  `ScriptError` and `console` globals are frozen and non-writable.
- **Limits** (§5.4):
  - memory: QuickJS's own limit, the script's `memoryMiB` (16–128,
    default 64) less 8 MiB of headroom;
  - a 256 KiB stack;
  - an interrupt handler that polls `script.poll` every 256 interrupts and
    stops on Stop or at the end of the run's time, plus a guest-side
    instruction budget;
  - 4,096 bridge calls;
  - logs: 1,000 lines or 256 KiB, then one "…truncated" line.

  A stop is final: QuickJS interrupts cannot be caught, and every op after
  a cancellation fails in the guest.

## Files

| File | What |
| --- | --- |
| `src/lib.rs` | the actions `run {script, inputs?}` and `check {source}` (compile only, `JS_EVAL_FLAG_COMPILE_ONLY`, never evaluated) |
| `src/runtime.rs` | runtime, context, limits, interrupt handler, natives, error and result mapping |
| `src/prelude.js` | the `deckpilot` object (§4.4): paths and repositories, the confirmation round, processes, `forEachRepository`, errors naming the repository and command |
| `src/bridge.rs` | the wire envelope, the per-run budget, the `host_call` transport |
| `src/api/` | one marshaller per op group: `fs`, `process` (and `shell`), `projects`, `confirm` (and `sleep`), `log`, and `path` (pure, per host OS) |
| `src/mock.rs`, `src/tests.rs`, `src/api/tests.rs`, `src/api/path/tests.rs` | native tests against a mock bridge |
| `deckpilot.d.ts` | the API's types, for the editor's completion (P7) |

## The bridge contract (for P6)

Each request is one `host_call` with `{"op": "<op>", "args": {…}}`, the
envelope of `CapabilityRequest`. A reply is `{"ok": <value>}` or
`{"error": {"code": <CallErrorCode>, "message": "…", "script"?: {…}}}`.
The optional `script` object (`code`, `message`, `repository`,
`command`, `path`, `confirmation`) carries the script-level detail; its
`code` is an `ErrorCode` of `deckpilot.d.ts`. Without it, the guest maps
the call code: `not-permitted` to `notGranted`, `bad-request` to
`invalidArgument`, `throttled` to `limit` and `unsupported-op` to
`unsupported`. The guest validates every argument (`src/api/`, with the
unknown fields refused). The agent must check everything again: the guest
is not trusted.

| Op | Args | Ok value |
| --- | --- | --- |
| `script.load` | `{script}`: the invocation's own parameter only | `{source, platform: "macos"\|"windows"\|"linux", settings: {timeoutMs, stopOnError, memoryMiB}, grants: {shell?: string[]\|null, …}, run: {id, origin, userInitiated}, nowUs?}`; refuse untrusted with `script.code` `scriptNeedsReview`, and missing with `scriptMissing` |
| `script.poll` | `{}` | `{cancelled, remainingMs, nowUs?}`; not counted in the 4,096 calls (the guest caps it at 8,192 per run) |
| `script.log` | `{level: debug\|info\|warn\|error, message}` | `null` |
| `script.sleep` | `{ms}` (≤ 600,000) | `null`; `timedOut` / `cancelled` on the deadline or Stop |
| `script.confirm` | `{token}` | `{confirmed}`: the agent shows **its own stored plan** for the token (dialogs 1.5), only in user-started runs |
| `project.list` | `{}` | `{projects: [{id, key, name}]}` (active profile) |
| `project.repositories` | `{project: id\|null}` (`null`: the current one) | `{project: {id, key, name}\|null, repositories: [{id, key, name, description, isDefault, path\|null, exists, remote}]}` |
| `fs.exists`, `fs.stat` | `{path}` | `{exists}`, `{stat\|null}` |
| `fs.list` | `{path}` | `{entries: [{name, kind}]}` |
| `fs.mkdir` | `{path, recursive}` | `{}` |
| `fs.readFile` | `{path, encoding, maxBytes}` | `{content}` |
| `fs.writeFile` | `{path, content, encoding, createOnly}` | `{}` |
| `fs.copy`, `fs.move` | `{src, dst, recursive?, overwrite, confirmation?}` | `{}` |
| `fs.remove` | `{path, recursive, dryRun, confirmation?}` | a `RemovalPlan` |
| `fs.removeContents` | `{path, dryRun, keep, confirmation?}` | a `RemovalPlan` |
| `process.run` | `{command, args, cwd?, env, timeoutMs?, stdin?}` | `{ok, exitCode, signal, stdout, stderr, truncated, durationMs, timedOut, cancelled}`; not found: `commandNotFound`; Stop: `cancelled` |
| `shell.run` | `{shell, script, cwd?, env, timeoutMs?}` | as `process.run` |
| `shell.available` | `{}` | `{shells}` |

A **path** is `{"path": "<absolute>"}` (fully qualified on Windows) or
`{"repository": "<key>", "relative": "a/b"}`. The guest normalizes the
relative part to `/` separators, with no `.` segments, never empty segments
and never escaping the root. The agent resolves both forms against the
grants (§5.3), with cap-std.

**Destructive operations** with `destructive: "ask"`: the agent answers
the first request with `script.code` `needsConfirmation` and
`confirmation: {token}`. The guest sends `script.confirm {token}`, and only
on `{confirmed: true}` repeats the op with `confirmation: token`. The agent
recomputes the plan then. The guest never supplies the text the user sees.

**What P6 must also provide:**

- the request and reply caps raised for this plugin: up to about 6 MiB for
  `fs.writeFile` and `fs.readFile`, and about 1.3 MiB for `process.run`
  with stdin (`bridge::max_request`);
- the 4,096-call counter in place of the 256 per invocation
  (`runtime.hostCalls`);
- the 2 s continuous deadline (`runtime.limits`);
- `nowUs` in the replies, for `Date.now()`.

The manifest's `scripts` permission, `script` parameter kind, `code`
parameter kind, `scripts` view, `runtime.limits`, `runtime.hostCalls` and
`actions[].contexts` follow the design. P6 defines their exact schema, and
this manifest follows it before P9.

## S1, the spike

The PR body (knittingcode/deckpilot-plugins, P5) records the numbers:

- the module's one import;
- the package size against the 4 MiB cap;
- two clean builds byte-identical.

## Building

`tools/official.sh` builds it with the other members. QuickJS's C code
needs a clang with the wasm32 backend and the Rust toolchain's `llvm-ar`
(`rustup component add llvm-tools`). CI installs clang 19 from Ubuntu's
archive (`tools/wasm-c-toolchain.sh --install`). Locally, set
`DECKPILOT_WASM_CC=clang` to use another clang; those modules are not CI's
bytes.

## License

The plugin's code is Apache-2.0. The module statically includes QuickJS-NG
and rquickjs (MIT) and wasi-libc (MIT, with the musl and cloudlibc
notices). `LICENSE` carries all of them.
