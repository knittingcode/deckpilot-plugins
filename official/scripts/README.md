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
  - memory: QuickJS's own limit, the script's `memoryMib` (16–128,
    default 64) less 8 MiB of headroom;
  - a 256 KiB stack;
  - an interrupt handler that polls `script.poll` every 256 interrupts and
    stops on Stop or at the end of the run's time, plus a guest-side
    instruction budget;
  - 4,096 bridge calls and 8,192 polls;
  - logs: 1,000 lines or 256 KiB, then one "…truncated" line.

  A stop is final: QuickJS interrupts cannot be caught (no `catch` or
  `finally` runs), every op after a cancellation fails in the guest, and a
  run whose script caught a `cancelled` or `timedOut` refusal still ends
  cancelled or timed out.
- **Async scripts.** When a script's completion value is a promise (an
  `async` main or a `.then` chain), the run waits for it, under the same
  limits, and ends with its value or its rejection. A promise that never
  settles fails the run, and so does a rejection no handler took. Top-level
  `await` is not supported; a script is not a module.
- **Built-ins are pinned.** The prelude takes the built-ins it uses before
  the script runs (`Object.*`, `Array.isArray`, the `String`, `Array`,
  `RegExp` and `WeakSet` methods), so a script that replaces a prototype
  method changes only its own code. Nothing in the prelude is a security
  boundary; the agent decides every op.

## Files

| File | What |
| --- | --- |
| `src/lib.rs` | the actions `run {script, inputs?}` and `check {source}` (compile only, `JS_EVAL_FLAG_COMPILE_ONLY`, never evaluated) |
| `src/runtime.rs` | runtime, context, limits, interrupt handler, natives, error and result mapping |
| `src/prelude.js` | the `deckpilot` object (§4.4): paths and repositories, the confirmation round, processes, `forEachRepository`, errors naming the repository and command |
| `src/bridge.rs` | the wire envelope, the per-run budget, the `host_call` transport |
| `src/api/` | one marshaller per op group: `fs`, `process` (and `shell`), `projects`, `confirm` (and `sleep`), `log`, and `path` (pure, per host OS) |
| `src/mock.rs`, `src/tests.rs`, `src/tests_io.rs`, `src/bridge/tests.rs`, `src/api/tests.rs`, `src/api/path/tests.rs` | native tests against a mock bridge |
| `tests/fixtures/script-requests.json`, `tests/fixtures/bridge-replies.json`, `src/tests_fixture.rs` | the shared request fixture (DeckPilot's, copied byte for byte), the guest's replies fixture, and the test that the guest emits only the fixture's requests |
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
| `script.load` | `{script}`: the invocation's own parameter only | `{source, platform: "macos"\|"windows"\|"linux", settings: {timeoutMs, stopOnError, memoryMib}, grants: {shell?: string[]\|null, …}, run: {id, origin, userInitiated}, remainingMs, nowUs}`: `remainingMs` (the run's time left, always sent since deckpilot #481) is the run's deadline; only an agent without it gets the fallback, `settings.timeoutMs`, else 60,000; refuse untrusted with `script.code` `scriptNeedsReview`, and missing with `scriptMissing` |
| `script.poll` | `{}` | `{cancelled, remainingMs, nowUs?}`; not counted in the 4,096 calls (the guest caps it at 8,192 per run); **must not reset the epoch deadline** (see below) |
| `script.log` | `{level: debug\|info\|warn\|error, message}` | `null`; **must not reset the epoch deadline** |
| `script.sleep` | `{ms}` (≤ 600,000) | `{nowUs}`; `timedOut` / `cancelled` on the deadline or Stop |
| `script.confirm` | `{token}` | `{confirmed}`: the agent shows **its own stored plan** for the token (dialogs 1.5), only in user-started runs |
| `project.list` | `{}` | `{projects: [{id, key, name}]}` (active profile) |
| `project.repositories` | `{project: id\|null}` (`null`: the current one) | `{project: {id, key, name}\|null, repositories: [{id, key, name, description, isDefault, path\|null, exists, remote}]}` |
| `fs.exists`, `fs.stat` | `{path}` | `{exists}`, `{stat\|null}` |
| `fs.list` | `{path}` | `{entries: [{name, kind}]}` |
| `fs.mkdir` | `{path, recursive}` | `{}` |
| `fs.readFile` | `{path, encoding: "base64", maxBytes, offset, length, ifVersion?}` (`length` ≤ 524,288; `ifVersion` on every chunk after the first, never on the first; the guest never sends the whole-file form without `offset` and `length`) | `{content, bytes, size, eof, version?}`: `bytes` bytes from `offset` (at most `length`, maybe fewer), the file's `size`, `eof` when they reach it, and the read's `version` (absent only when the first chunk already reaches the end); a later chunk of a file that changed: `script.code` `fileChanged` |
| `fs.writeFile` | `{path, content, encoding, createOnly}` (content ≤ 32,768 bytes) | `{}`: a small file in one request |
| `fs.write.open` | `{path, createOnly}` | `{handle}`: a temporary file in the target's directory |
| `fs.write.chunk` | `{handle, content, encoding: "base64"}` (≤ 32,768 bytes) | `{}`; appended in order |
| `fs.write.commit` | `{handle}` | `{}`: renamed atomically onto the target (`createOnly`: only if it does not exist) |
| `fs.write.abort` | `{handle}` | `{}`: the temporary file removed |
| `fs.copy`, `fs.move` | `{src, dst, recursive?, overwrite, confirmation?}` | `{}` |
| `fs.remove` | `{path, recursive, dryRun, confirmation?}` | a `RemovalPlan` |
| `fs.removeContents` | `{path, dryRun, keep, confirmation?}` | a `RemovalPlan` |
| `process.run` | `{command, args, cwd?, env, timeoutMs?, stdin?}` (`args` ≤ 48 KiB in total, `stdin` ≤ 48 KiB) | `{ok, exitCode, signal, stdout, stderr, truncated, durationMs, timedOut, cancelled, nowUs?}`: `stdout` and `stderr` together at most 960 KiB (`truncated` when cut); not found: `commandNotFound`; Stop: `cancelled` |
| `shell.run` | `{shell, script, cwd?, env, timeoutMs?}` (`script` ≤ 48 KiB) | as `process.run` |
| `shell.available` | `{}` | `{shells}` |

**Sizes (P6 decision D1).** The host's caps stay as they are: a request is
at most 64 KiB (`MAX_HOST_CALL_REQUEST`) and a reply at most 1 MiB
(`MAX_HOST_CALL_REPLY`), for every op (`bridge::limits::REQUEST` and
`REPLY`). The script's API keeps its limits (4 MiB files), and the guest
chunks:

- `readFile` is a loop of `fs.readFile`, 512 KiB at a time, under the
  **version contract** (deckpilot #481, under review; fixture version 2):
  - a reply names the read's `version` (1 to 64 printable ASCII bytes;
    the agent's are 32 hex digits), except when the first chunk already
    reaches the end (a file read in one chunk). The first reply's
    `version` and `size` are the read's;
  - every later chunk sends that version back as `ifVersion`, exactly as
    the fixture's later chunk shows. The agent serves the later chunks
    from the state of the file the first chunk saw, and refuses with
    `fileChanged` when the file was replaced, or its size or
    modification time changed, in between;
  - the guest checks too: a later reply with another `version` or
    `size`, a chunk longer than asked or past that size, or `eof`
    anywhere but exactly at the end, fails the read with `fileChanged`.
    A reply before the end without a version, or with a version the host
    would not accept back, fails it with `invalidResponse`, and nothing
    more is sent;
  - a shorter chunk is read on from where it ended.

  So a script gets the bytes of **one version** of the file, or a
  `fileChanged` `ScriptError` naming the path ("… changed while it was
  being read; read it again"); never a splice of two versions.
- `writeFile` of at most 32 KiB is one `fs.writeFile` in the script's
  encoding (unless JSON escaping would take it past 64 KiB). A larger one
  is `fs.write.open`, then `fs.write.chunk` per 32 KiB, then
  `fs.write.commit`. The agent writes a temporary file in the same
  directory and renames it onto the target on commit. Any failure after
  the open (a refusal, an error, a Stop, the end of the run's time) sends
  `fs.write.abort`, the one request the guest sends after a Stop; the
  agent drops the temporary file at the run's end in any case. A write
  starts only when all its calls (and the abort) fit the run's budget.
- Chunks (read and write) travel as base64: a chunk of text may end
  inside a character, and JSON escaping could grow 32 KiB of text past
  the request cap.
- `process.run` args and stdin, and `shell.run` scripts, are at most
  48 KiB each (a larger input goes through a file the script writes), and
  the whole request is checked against the 64 KiB before it is sent.
- A process's output: the agent keeps `stdout` and `stderr` **together**
  at most 960 KiB, then shortens them until the result's JSON fits the
  1 MiB reply, with `truncated: true` whenever it cut. The guest takes
  the result as it comes (any reply under 1 MiB); there is no separate
  per-stream size.

Every over-limit value is refused in the guest (`invalidArgument`,
`tooLarge` for a file over 4 MiB, `limit` for a request over 64 KiB)
before any host call.

**Timeouts.** A `process.run` or `shell.run` `timeoutMs`, and a
`script.sleep`'s `ms`, are sent as `min(value, remaining)`, with the
remaining time of the last `script.load` (`remainingMs`; only when an
agent omits it, `settings.timeoutMs`, else 60,000) or `script.poll`,
less the agent time (`nowUs`) since. With none left, nothing is sent and
the run ends `timedOut`. The agent applies the same rule with its own
clock.

**The shared fixture.** `tests/fixtures/script-requests.json` is a
byte-for-byte copy of DeckPilot's
`plugins/runtime/crates/abi/tests/fixtures/script-requests.json`
(knittingcode/deckpilot #465 at
`2fc870dad2e6438413a200c751f142134cae7517`, sha256
`e2e12b2ec6db4e207114f9134f0fe3f3c885429d7025d30db45f5c8842becfe4`, which
a test holds), the one contract of both repositories: never edit it
here. Version 2 (deckpilot #481) replaces it once approved; until then a
later chunk is compared with the version 1 entries without its
`ifVersion`. Its `valid` requests are what the host
parses and serializes back unchanged, its `invalid` ones what it refuses.
`src/tests_fixture.rs` drives the prelude and the marshallers against the
mock bridge (answered from `tests/fixtures/bridge-replies.json`) and holds
every emitted request byte-equal (canonical JSON: compact, keys sorted)
to a `valid` one, or of exactly its shape where the fixture's example
has other values; every `valid` request but the whole-file read is
emitted byte for byte (the first chunk, and the later one with its
`ifVersion`); every chunk after the first carries a valid `ifVersion`;
and the guest never emits an `invalid` one: it refuses or normalizes
each it could be given, and it never sends a later chunk without a
version or with a malformed one.

A **path** is `{"path": "<absolute>"}` (fully qualified on Windows) or
`{"repository": "<key>", "relative": "a/b"}`, in the host's canonical
form. The guest normalizes the relative part to `/` separators, with no
`.` segments, never empty segments and never escaping the root. It
refuses an absolute path with `.`, `..` or empty names (a trailing
separator is fine), a stream, a control or bidirectional character, a
verbatim or device form, or (on Windows) a reserved device name. A
command is a bare name (`^[A-Za-z0-9][A-Za-z0-9._+-]*$`) or a canonical
absolute path, never UNC or a batch file, and the environment denylist
is the host's `scripts::env_denied`. The agent resolves both forms against the
grants (§5.3), with cap-std.

**Destructive operations** with `destructive: "ask"`: the agent answers
the first request with `script.code` `needsConfirmation` and
`confirmation: {token}`. The guest sends `script.confirm {token}`, and only
on `{confirmed: true}` repeats the op with `confirmation: token`. The agent
recomputes the plan then. The guest never supplies the text the user sees.

The **token** is the agent's and is only a reference; the agent alone gives
it meaning:

- it is **single-use**: consumed by the op it confirms, refused after that;
- it is **bound to one op and its exact args**: the op, the resolved paths
  and the options it was issued for (another path, another op, another run
  or another script is refused);
- it is **valid only after `script.confirm` answered `confirmed: true`** in
  this run, and expires with the run or after a short time;
- the plan is recomputed when the op runs, and a plan larger than the one
  confirmed is refused and asked again.

**The CPU deadline.** The host resets its continuous-CPU (epoch) deadline
after each host call, so long `process.run` calls never trip it. The guest
polls `script.poll` about every 2.5 million bytecode operations, and logs
freely. If those calls reset the deadline too, a busy loop that polls, or
logs every iteration, would never reach `continuousMs` (`runtime.limits`).
So P6 **must not reset the epoch deadline for `script.poll` and
`script.log`**, only for calls that wait on the host (`process.run`,
`shell.run`, `script.sleep`, `script.confirm`, the `fs.*` ops). The fuel
budget, the guest's instruction budget and the run's wall time still bound
every loop.

**The grant summary** (`script.load`'s `grants`) tells the guest what to
expose, not what is allowed. It is never trusted for a decision; the agent
checks every op. The guest reads only one member today:

- `shell`: `string[]` of the granted shells (`"sh"`, `"bash"`, `"zsh"`,
  `"pwsh"`, `"powershell"`, `"cmd"`), or `null`/absent without the shell
  grant. Only with an array is `deckpilot.shell` defined; `shell.run` is
  refused by the agent in any case without the grant.

Other members (`projectsRead`, `repositoriesPaths`, `fs`, `process`,
`destructive`) are P6's to define; the guest ignores them.

**The rest of P6's side** (p6-plan): the 4,096-call budget in place of
the 256 per invocation (`runtime.hostCalls`), polls counted apart, the
2 s continuous deadline (`runtime.limits`), and `nowUs` in the replies of
`script.load`, `script.poll`, `script.sleep`, `process.run` and
`shell.run`, for `Date.now()`.

The manifest's `scripts` permission, `script` parameter kind, `code`
parameter kind, `scripts` view, `runtime.limits`, `runtime.hostCalls` and
`actions[].contexts` follow the design. P6 defines their exact schema, and
this manifest follows it before P9.

## Before the release (P9): blockers

- **The vendored C code is reviewed code.** `rquickjs-sys` 0.14.0 vendors
  QuickJS-NG's sources and a prebuilt `vendor/wasi-libc/lib/libc.a` (from
  wasi-sdk 24) that is linked into this module. Neither is built from
  source here, so their bytes must be recorded as reviewed when this plugin
  joins the reviewed privileged policy (`official/reviewed-sources.json`,
  which today admits only the policy's plugins):
  - `rquickjs-sys` 0.14.0, crate checksum
    `cee271d0eeba64f0915b846cb7ae02e16faf3dfdffdca91731101d9d30fe3423`
    (`official/Cargo.lock`);
  - `vendor/wasi-libc/lib/libc.a`, sha256
    `566b9d6322cb3f339963cb1e6719b0bb0c932724fafa3418cf6aebe763dc2036`;
  - `rquickjs-core` `6657979a…bbf24` and `rquickjs` `a0a22bf7…eb34e` with
    them.

  Removing `held` without that record is not allowed (the `held` reason in
  `official/plugins.json` says so).
- The privileged policy entry, the contract at the pin, `PLUGIN_API` 1.13
  (P8), and the security review sign-off. The version contract of chunked
  reads must ship with the agent at the pin: this guest refuses a read
  whose replies name no version (deckpilot #481 is a draft today), so the
  shared fixture copied here must be the one of the DeckPilot it pins.

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
