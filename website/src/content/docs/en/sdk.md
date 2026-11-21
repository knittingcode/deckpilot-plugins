---
title: "SDK overview"
description: "The types of the Rust crate deckpilot-plugin-sdk: Plugin, Action, Param, Context, Args, Output, the plugin! macro and the MockHost for tests."
group: start
order: 3
---

`deckpilot-plugin-sdk` is the Rust crate you write a plugin with. It declares the plugin and its actions, checks parameters before your code runs, wraps every capability of guest ABI 1 in a typed method, and generates the WebAssembly exports. This page is a map of the crate; the other pages go into each part.

> **Status:** The crate is version 0.1.0, implements plugin API 1.0 to 1.9 (DeckPilot 0.10.0 provides 1.9; the 1.9 additions except `Context::profile` are only for DeckPilot's own bundled plugins) and guest ABI 1, and is not published to crates.io. See [Getting started](../getting-started/#how-you-get-the-sdk) for how to depend on it. While it is 0.x, a minor version may change the Rust API.

## A whole plugin

This example from the crate's own documentation shows most of the surface: a definition, two typed parameters, a handler, a log line and a structured result.

```rust
use deckpilot_plugin_sdk::prelude::*;

fn plugin() -> Plugin {
    Plugin::new("acme.hello", "0.1.0").action(
        Action::new("greet", greet)
            .param(Param::text("name").required().max_length(64))
            .param(Param::choice("style", &["plain", "excited"]).default("plain")),
    )
}

fn greet(ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let name: String = args.get("name")?;
    let style: String = args.get("style")?;
    ctx.info("greeting someone");
    let mark = if style == "excited" { "!" } else { "." };
    Ok(Output::message(format!("Hello, {name}{mark}")).with("name", name))
}
```

In a real plugin crate one more line, `deckpilot_plugin_sdk::plugin!(plugin);`, generates the exports.

## The prelude

`use deckpilot_plugin_sdk::prelude::*;` brings in everything a plugin usually needs:

| Type | What it is |
| --- | --- |
| `Plugin` | The definition: id, version, actions and the two lifecycle hooks |
| `Action` | One action: an id from the manifest, its parameters and a handler |
| `Param` | One typed parameter with bounds, options and a default |
| `Context` | The handle for capability calls inside a handler |
| `Args` | The checked parameters of one invocation |
| `Output` | A successful or skipped result with a message and named outputs |
| `ActionResult` | `Result<Output, PluginError>`, what a handler returns |
| `PluginError` | A typed failure: a code, a message, optionally retryable |
| `codes` | The shared error codes as constants |
| `Request`, `Response`, `HttpMethod` | Brokered HTTP |
| `SecretHandle`, `AuthScheme` | Credential slots, by name only |
| `Settings`, `StorageUsage` | What `Context::settings` and `Context::storage_usage` return |
| `CapabilityError`, `CapabilityErrorKind`, `NetworkErrorKind` | Why a capability call failed |
| `RunContext`, `RunTrigger`, `RunCause` | What started the current run |
| `LogLevel`, `NotifySeverity`, `ActionOutcome` | Log levels, notification looks, the wire form of a result |
| `Controls`, `Control`, `Input` | The controls and inputs an action works on (plugin API 1.3) |

## Defining a plugin

`Plugin::new(id, version)` takes the same id and version as `manifest.json`. Chain `.action(Action)` for each action and optionally `.on_init(fn)` and `.on_shutdown(fn)`.

`Action::new(id, handler)` takes a handler of type `fn(&mut Context<'_>, &Args<'_>) -> ActionResult`: a plain function or a non-capturing closure. `.param(Param)` adds a parameter.

`Plugin::validate()` checks ids, keys, uniqueness, kinds and defaults against the manifest's rules. Call it in a unit test. An invalid definition fails every invocation with the code `plugin-error`.

`Plugin::requires_api(ApiLevel)` declares the plugin API level the plugin cannot work without; the manifest declares the same level in `engines.pluginApi`. Without it the plugin needs 1.0. See [Compatibility](../compatibility/#enginespluginapi).

Details: [Actions](../actions/) and [Parameters](../parameters/).

## Controls

Plugin API 1.3, since DeckPilot 0.8.0, lets an action say which controls it works on: keys, dials (encoders) or touch zones, and which inputs of each. `Action::controls(Controls)` declares them; `Controls::new()` chained with `.key(..)`, `.encoder(..)` or `.touch_zone(..)` lists the `Input`s per kind. This example from the crate's documentation binds an action to a key press or click and to a dial turned clockwise:

```rust
use deckpilot_plugin_sdk::prelude::*;
use deckpilot_plugin_sdk::ApiLevel;

fn next(_: &mut Context<'_>, _: &Args<'_>) -> ActionResult {
    Ok(Output::ok())
}

fn plugin() -> Plugin {
    Plugin::new("acme.pager", "1.0.0")
        .requires_api(ApiLevel { major: 1, minor: 3 })
        .action(Action::new("next", next).controls(
            Controls::new()
                .key([Input::Press, Input::Click])
                .encoder([Input::RotateClockwise]),
        ))
}
```

Declaring controls needs `requires_api` 1.3: `Plugin::validate` refuses them below it. The manifest entry of the action declares the same `controls` (`Controls::to_json()` gives that form), and `testing::manifest_mismatches` compares the two. An action without controls works on keys only. See [Manifest format](../manifest/#controls) and the [canonical description](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/actions.md#controls).

## The `plugin!` macro

`deckpilot_plugin_sdk::plugin!(build_fn)` generates the guest ABI 1 exports: `dp_abi_version`, `dp_alloc`, `dp_invoke`, `dp_describe`, `dp_init`, `dp_shutdown` and `dp_features`. Use it once, at the root of the `cdylib` crate, with a function of type `fn() -> Plugin`. The plugin is built on first use and kept for the life of the module.

Your crate needs no `unsafe` of its own. Use `#![deny(unsafe_code)]` rather than `forbid`, because the macro's export attributes need an `allow`.

## Capabilities on `Context`

Every method below is one capability request that the agent's permission broker decides. At most 256 calls are allowed per invocation.

| Capability | Methods | Permission |
| --- | --- | --- |
| HTTP | `fetch(Request)`, `http_get(url)` | `network` for the exact host and method |
| Credentials | `has_credential(&SecretHandle)`; `Request::bearer`, `Request::credential` | `credentials` for the slot |
| Settings | `settings()` | none |
| Storage | `storage_get`, `storage_get_as::<T>`, `storage_set`, `storage_delete`, `storage_keys`, `storage_usage` | none (a `storage` permission raises the quota) |
| Variables | `publish_variable`, `clear_variable`, `clear_variables` | none, for the plugin's own declared variables |
| Notifications | `notify(title, body)`, `notify_with(severity, title, body)` | `notifications` |
| Open URL | `open_url(url)` | `openUrl` for the exact host |
| Run context | `run_context()` | none |
| Log | `log(level, message)`, `debug`, `info`, `warn`, `error` | none |

Each returns `Result<_, CapabilityError>`, except the log methods, which are best effort and return nothing. In a handler, `?` converts a `CapabilityError` into a `PluginError` with a shared code such as `not-permitted` or `request-failed`.

Details: [Permissions](../permissions/), [Credentials](../credentials/), [Storage](../storage/), [Variables](../variables/).

## Results and errors

A handler returns `Ok(Output)` or `Err(PluginError)`. `Output::ok()`, `Output::message(text)` and `Output::skipped(text)` create a result, and `.with(key, value)` adds a named output. `PluginError::new(code, message)` creates a failure, and `.retryable()` marks it transient. See [Actions](../actions/#results).

## Testing support

The `testing` module runs actions natively, without WebAssembly and without an agent:

- `testing::MockHost` answers capability requests from a closure and records them. `MockHost::allow_all()` and `MockHost::deny_all()` are ready-made hosts.
- `testing::manifest_mismatches(&plugin, &manifest)` lists every difference between the code and `manifest.json`.

Outside WebAssembly, a capability call that does not go through a `MockHost` fails with `CapabilityErrorKind::Unavailable`. See [Testing](../testing/).

## Version constants

| Constant | Value | Meaning |
| --- | --- | --- |
| `deckpilot_plugin_sdk::SDK_VERSION` | `0.1.0` | The crate's own version |
| `deckpilot_plugin_sdk::API_LEVEL` | 1.0 | The plugin API level a plugin needs unless it calls `Plugin::requires_api`; the manifest then declares `"pluginApi": "^1.0"` |
| `deckpilot_plugin_sdk::LATEST_API_LEVEL` | 1.9 | The highest plugin API level the SDK implements (at the DeckPilot v0.10.0 commit) |
| `deckpilot_plugin_sdk::CONTROLS_API_LEVEL` | 1.3 | The level that introduced `Action::controls` |
| `deckpilot_plugin_sdk::DIALOGS_API_LEVEL` | 1.5 | The level that introduced `Context::dialog` |
| `deckpilot_plugin_sdk::LOCALIZATION_API_LEVEL` | 1.6 | The level that introduced `Context::locale` and `Context::t` |
| `deckpilot_plugin_sdk::PRIVILEGED_API_LEVEL` | 1.9 | The level of the operations of DeckPilot's own bundled plugins and of `Context::profile` |
| `deckpilot_plugin_sdk::ABI_VERSION` | 1 | The guest ABI, asserted at compile time |

See [Compatibility](../compatibility/).

## What the SDK does not offer

ABI 1 has no file system, process, clipboard or shell access. The time and random bytes come only from the agent, through `Context::now` and `Context::random_bytes` (plugin API 1.1); `std::time::SystemTime::now()` and random number generators fail or panic in the module. There are no timers, no background work, no streaming HTTP and no WebSockets, and a plugin cannot read variables, the active profile or the project.

The SDK also declares `Param::number` and `Param::secret`. Manifest version 1 has no parameter kind for them, so they work under `MockHost` but cannot be reached through the agent; see [Parameters](../parameters/#kinds-the-manifest-does-not-have).

## JavaScript

The repository also contains `plugins/sdk-js`, the package `@deckpilot/plugin-sdk` at version 0.1.0. It is private, has never been published, and holds type declarations only. No DeckPilot host can run a JavaScript plugin: compiling JavaScript into an ABI 1 module is not available. The declarations describe an earlier design and differ from ABI 1 in several places; [its README](https://github.com/knittingcode/deckpilot/blob/main/plugins/sdk-js/README.md) lists the differences. Write plugins in Rust.

## Canonical reference

The full SDK documentation, including the coverage table of every capability with its tests, is [docs/marketplace/sdk/README.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/README.md). The crate's source is under [plugins/runtime/guest/plugin-sdk](https://github.com/knittingcode/deckpilot/blob/main/plugins/runtime/guest/plugin-sdk/src/lib.rs).
