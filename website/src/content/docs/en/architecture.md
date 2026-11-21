---
title: "Architecture"
description: "How a DeckPilot plugin runs: one WebAssembly module in a sandboxed host, a permission broker for every call, and a signed static registry."
group: start
order: 2
---

A plugin is untrusted code. DeckPilot never runs it inside the application or the background agent: it runs in a separate, sandboxed process, and everything it wants from the outside world is a request that the agent checks against what the user approved. This page describes the pieces and how they fit.

> **Status:** Everything here describes the `main` branch of DeckPilot. DeckPilot 0.7.0 is the first release with plugin support; 0.10.0, the current release, provides plugin API 1.9. The signed registry is live. See [Compatibility](../compatibility/#current-status).

## The pieces

```text
 configuration app
        |  IPC
        v
 +---------------------- agent ------------------------+
 |  plugin store      registry client     package      |
 |  (encrypted        (verifies           verification |
 |   database)         signatures)                     |
 |                                                     |
 |  permission broker  <----  executor                 |
 |  (grants, limits)          (HTTP, storage, notify)  |
 +---------|-------------------------------------------+
           |  one supervisor per running plugin
           v
 +---- deckpilot-plugin-host (OS sandbox) ----+
 |  wasmtime, no WASI                          |
 |  plugin.wasm  --host_call-->  capability    |
 +---------------------------------------------+
```

| Piece | Role |
| --- | --- |
| `plugin.wasm` | Your code: one WebAssembly module implementing guest ABI 1 |
| `manifest.json` | The only thing the agent reads to learn what the plugin is, asks for and offers. The agent never runs plugin code to discover metadata |
| Plugin host | A separate process, `deckpilot-plugin-host`, confined by the operating system's sandbox, that runs the module with wasmtime |
| Supervisor | Starts one host per running plugin, enforces limits and restarts it after a crash |
| Permission broker | Decides every capability request against the user's current grants |
| Executor | Carries out an allowed request in the agent: the HTTP call, the storage write, the notification |
| Registry | Signed static metadata plus plugin packages, published from a separate repository |

## The module

The executable artifact is exactly one WebAssembly module, shipped as `plugin.wasm` and declared in the manifest as `"runtime": { "kind": "wasm", "abi": 1 }`. The package check validates it statically, without compiling or running it:

- it is a core WebAssembly 2.0 module (no components, threads, 64-bit or multiple memories, GC or exceptions);
- its only import is `deckpilot_v1.host_call`;
- it exports `memory`, `dp_abi_version`, `dp_alloc` and `dp_invoke`; the optional exports `dp_describe`, `dp_init`, `dp_shutdown` and `dp_features` must have their exact types;
- it is at most 4 MiB.

There is no WASI. The module has no file system, network, clock, random source, environment or process of its own. The Rust SDK generates all exports with one macro, so plugin code never deals with the ABI directly; see [SDK overview](../sdk/).

The contract is language-neutral on purpose, but today the Rust SDK is the only way to build such a module. JavaScript plugins cannot be built or run.

## The sandboxed host

The agent starts a host process for a plugin on demand, at its first invocation. Before every start it verifies the installed files against their integrity record and validates `plugin.wasm` again.

The host confines itself before it reads a byte of plugin code. The agent accepts only a host that reports full sandbox enforcement:

| Platform | Mechanism |
| --- | --- |
| macOS | Seatbelt, with a deny-by-default profile |
| Linux | seccomp together with Landlock |
| Windows | An AppContainer without capabilities plus a Job Object |

A host that reports only partial enforcement is stopped before it receives the module. Windows execution has been verified on a CI runner only, not on desktop Windows editions.

Inside the host, each invocation runs under limits: a wall-clock timeout, a CPU deadline between two host calls, a fuel budget, 64 MiB of linear memory and at most 256 capability calls. The exact values are on the [Lifecycle](../lifecycle/#limits-of-one-invocation) page.

## The capability broker

When plugin code calls, for example, `Context::fetch`, the SDK sends one capability request through `host_call`. The host forwards it to the agent. The request never contains the plugin's id: the agent knows which plugin's host is asking.

The broker then checks the request against the plugin's current grants. Permissions are deny by default, the manifest lists everything a plugin may ever ask for, and the user approves each entry. Because the broker reads the grants on every call, revoking a permission takes effect at the next call, even in the middle of a running action.

Secrets follow the same path in reverse. A plugin names a credential slot, and the agent injects the stored secret into the outgoing request. The secret never enters the module's memory. See [Permissions](../permissions/) and [Credentials](../credentials/).

## The registry

Plugins are distributed separately from the application, from the repository [knittingcode/deckpilot-plugins](https://github.com/knittingcode/deckpilot-plugins):

- **Registry metadata** is a set of static, signed JSON documents served by GitHub Pages: root-signed key sets, a signed index of every plugin, content-addressed plugin details, and a timestamp.
- **Plugin packages** (`<id>-<version>.dpplugin`) are assets of that repository's GitHub Releases, one release per plugin version.

Plugin authors submit by pull request. The registry rebuilds each package from the author's tagged source, and the owner reviews and publishes. The application repository never publishes a plugin, and the registry never publishes an application release.

## The trust chain

DeckPilot trusts the registry only through signatures it verifies itself. Hosting and TLS are not part of the trust.

1. Root public keys are compiled into the application.
2. The roots sign key sets, which name the index key and the timestamp key.
3. The timestamp key signs a short-lived timestamp that pins one index by hash.
4. The index key signs the index, which pins each plugin's detail document by hash.
5. A detail pins each package version by sha256 and size.
6. A downloaded package is installed only if its bytes match those pins and it passes the package checks.

A compromised host can therefore withhold updates, which clients detect as a stale registry, but cannot make a client accept a package. Plugin authors hold no keys and sign nothing; see [Signing and trust](../signing/).

## Further reading

- [Artifact contract](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/artifact-contract.md): the module and package rules.
- [Plugins in the agent](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/agent-integration.md): the backend in detail.
- [Plugin runtime](https://github.com/knittingcode/deckpilot/blob/main/plugins/runtime/README.md): ABI, host, sandbox and supervisor.
- [Registry architecture](https://github.com/knittingcode/deckpilot-plugins/blob/main/docs/architecture.md) and [trust model](https://github.com/knittingcode/deckpilot-plugins/blob/main/docs/trust-model.md).
