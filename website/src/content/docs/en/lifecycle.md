---
title: "Lifecycle"
description: "What runs when: init and shutdown hooks, the host process, invocation limits, cancellation, crashes and restarts, and what ABI 1 lacks."
group: build
order: 11
---

A plugin is not a long-running program. It has two optional hooks, init and shutdown, neither of which can use capabilities, and otherwise its code runs only while one of its actions is being invoked. This page describes the host process around the module, the limits of an invocation, and what happens when things go wrong.

## The host process and the module

- The agent starts one sandboxed host process per enabled plugin **on demand**, at the first invocation, not when the plugin is enabled.
- Before every start the agent verifies the installed files against their integrity record and validates `plugin.wasm` again.
- The host instantiates the module once and keeps it for the life of the process. Statics, and the `Plugin` built by the `plugin!` macro, persist between invocations of that host until it stops.
- Do not rely on in-memory state for anything that must survive. Use [storage](../storage/).
- Invocations of one plugin run one at a time, in order. Up to 8 may wait in the queue; more fail at once as busy. Different plugins run in parallel.

## Hooks

Register the hooks on the plugin definition with `.on_init(init)` and `.on_shutdown(shutdown)`:

```rust
/// Runs once when the host loads the module. No capabilities here.
fn init() -> Result<(), PluginError> {
    if API.starts_with("https://") {
        Ok(())
    } else {
        Err(PluginError::new(codes::PLUGIN_ERROR, "the API origin must be https"))
    }
}

/// Runs on a clean stop only. No capabilities, best effort.
fn shutdown() {}
```

| Hook | Signature | When | What it can do |
| --- | --- | --- | --- |
| init | `fn() -> Result<(), PluginError>` | Once, when the host loads the module, before the first invocation | Check static tables, prepare in-memory state |
| shutdown | `fn()` | On a clean stop: disable, update, rollback, uninstall, quarantine, agent lock and stop | Best effort cleanup of in-memory state |

Rules for both:

- **No capabilities.** A hook has no `Context`. There is no invocation yet, so a hook cannot fetch, store, publish or log.
- They run under the same fuel and CPU limits as an invocation.
- A failing init refuses the load: the invocation that triggered it fails, and the next one tries a fresh host.
- Shutdown is not run after a crash, a timeout or a kill. After the host is told to shut down, it is killed after 1 second.

Because hooks have no capabilities, typical start-up work such as "load my state" belongs at the start of an action, read from storage.

## One invocation

1. The agent checks the plugin's state, the platform and the parameters. Bad parameters fail with `invalid-parameters` before the host runs.
2. The host passes the input to the module. The SDK runs init if needed, finds the action, checks the parameters, calls the handler and checks the result.
3. Every capability call goes from the host to the agent's broker and executor and back, synchronously. The module waits.

### Limits of one invocation

| Limit | Value | When exceeded |
| --- | --- | --- |
| Wall clock | The action's `node.timeoutMs`: default 30 s, at most 10 min | The host is killed; the action fails |
| CPU between two host calls | 250 ms | Trap; the host exits |
| Fuel | 5,000,000,000 per invocation, roughly instructions | Trap; the host exits |
| Linear memory | 64 MiB | Allocation fails, or a trap |
| WebAssembly stack | 512 KiB | Trap |
| Capability calls | 256 | The SDK refuses further calls as throttled |
| Input and output | 1 MiB and 192 KiB of JSON | Refused |

### Cancellation

A cancelled run, for example a cancelled workflow, or one past its deadline stops the capability call in progress at once. Later calls are refused, and the invocation's result is discarded: the action fails as cancelled. ABI 1 has no abort message for the module, so the host gets 2 seconds to finish and is then killed. This does not count as a crash.

Write handlers that make their capability calls early and return promptly after a failed call.

### Revocation during a run

Grants are read on every capability call. Revoking a permission, disabling or uninstalling makes the next call of a running invocation fail with `not-permitted`. A call that is already running is not interrupted.

## Crashes and restarts

A panic aborts the module, because the `guest` build profile sets `panic = "abort"`. The host process exits and the action fails with `plugin-host-failed`.

- The next invocation starts a fresh host, with a backoff of 1, 2, 4 and up to 60 seconds.
- Five crashes within ten minutes disable the plugin. This is the crash-loop breaker.

Return a `PluginError` instead of panicking, and avoid `unwrap` on anything an input can influence.

## State across events

| Event | Host | Storage | Settings and credentials | Published variables | Grants |
| --- | --- | --- | --- | --- | --- |
| Agent restart, lock and unlock | stopped | kept | kept | dropped | kept |
| Disable and enable | stopped | kept | kept | dropped | kept, inactive while disabled |
| Revoke or grant | running | kept | kept | kept | applies to the next call |
| Update, rollback | stopped | kept | kept | dropped | escalations need consent |
| Host crash | restarted on demand | kept | kept | kept | kept |
| Uninstall | stopped | deleted | deleted unless the user keeps them | dropped | deleted |

## What ABI 1 does not have

The original design and the JavaScript type declarations describe more than ABI 1 implements. Stated plainly:

- **No background work.** No timers and no intervals. The `background` permission exists in the manifest schema but has no effect. A plugin runs only while one of its actions runs.
- **No activation hooks with capabilities.** Init and shutdown are the only hooks, and they cannot make capability calls.
- **No settings-changed notification.** Read `Context::settings()` in each invocation.
- **Button state only through key state.** From plugin API 1.2 the agent asks an action's state handler what its key shows ([key state](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/actions.md#key-state)); a plugin has no other device access. From plugin API 1.3 an action can declare the dials and touch zones it works on; see [Manifest format](../manifest/#controls).
- **No sleep.** A module cannot wait. It reads the time only from the agent, with `Context::now` (plugin API 1.1).
- **No progress reports** from a running action.

One thing a handler can learn about its invocation is what started it: `Context::run_context()` returns whether a key, a gesture, the app or a workflow started the run, and for a workflow whether the user, a schedule or an event did. It does not expose a deadline or an attempt number.

```rust
    // Pages open only in a run the user started; otherwise report the link.
    if ctx.run_context()?.is_user_initiated() {
        ctx.open_url(url.clone())?;
        Ok(Output::message("opened the status page").with("url", url))
    } else {
        Ok(Output::message("not opened: this run was not started by the user").with("url", url))
    }
```

## Canonical reference

[docs/marketplace/sdk/lifecycle.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/lifecycle.md). Its closing list still says a handler does not learn what started it; the run context described in [docs/marketplace/sdk/capabilities.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/capabilities.md) was added later and is implemented in the SDK and the agent.
