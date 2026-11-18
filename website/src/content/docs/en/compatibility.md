---
title: "Compatibility"
description: "The version numbers of a plugin: engines.deckpilot, pluginApi, sdk, guest ABI, manifestVersion, platforms, plugin semver, and how updates work."
group: reference
order: 16
---

A plugin carries several version numbers, each answering a different question: which DeckPilot versions it runs on, which plugin API it needs, which binary interface its module speaks. This page lists them, what they are today, and how DeckPilot uses them when it installs and updates a plugin.

## Current status

| Fact | Today |
| --- | --- |
| Plugin support | Included since DeckPilot 0.7.0, with plugins on by default. DeckPilot 0.6.0 does not include it. An agent built from source needs the `plugins` cargo feature; `--disable-plugins` or `DECKPILOT_ENABLE_PLUGINS=0` turns plugins off |
| Public DeckPilot versions | [v0.7.0](https://github.com/knittingcode/deckpilot/releases/tag/v0.7.0) (2026-11-18), the current stable release and the first with plugin support. Before it, v0.6.0 (2026-11-17), the first stable release, without plugin support, and the pre-releases v0.6.0-alpha.3 and v0.6.0-alpha.4 |
| Rust SDK | `deckpilot-plugin-sdk` 0.1.0, not published to crates.io |
| JavaScript SDK | Type declarations only; cannot build or run a plugin |
| Registry | Live at `https://knittingcode.github.io/deckpilot-plugins/v1/`, signed with production key set 1. Publishes DeckPilot's own plugins; not open to other publishers yet |

Plugin API 1.x, guest ABI 1 and manifest version 1 are what the `main` branch of DeckPilot implements.

## The version numbers

| Number | Where | Today | Rule |
| --- | --- | --- | --- |
| Manifest version | `manifestVersion` | `1` | The only version the agent reads |
| Guest ABI | `runtime.abi`; the module's `dp_abi_version` export | `1` | The host loads only ABI 1 modules |
| Plugin API level | `engines.pluginApi` | `1.0` | `^MAJOR.MINOR`: same major, at least this minor |
| DeckPilot version | `engines.deckpilot` | `>=0.5.0 <1.0.0` in the examples | A range matched against the agent's version |
| SDK version | `engines.sdk` | `0.1.0` | Informational |
| Plugin version | `version` | yours | Semantic versioning with a restricted pre-release form |

## Engines

```json
"engines": { "deckpilot": ">=0.5.0 <1.0.0", "pluginApi": "^1.0", "sdk": "0.1.0" }
```

### `engines.deckpilot`

The DeckPilot versions the plugin runs on. It is one comparator, or a lower and an upper bound separated by one space. A comparator is `>=`, `>`, `<=`, `<` or `=` (or none, which means `=`) followed by `MAJOR.MINOR.PATCH` without leading zeros and without a pre-release part. The range must be satisfiable and is at most 64 characters.

Matching uses semantic versioning precedence and includes DeckPilot pre-releases:

- `0.6.0-alpha.3` satisfies `>=0.5.0 <1.0.0`.
- `0.8.0-beta.1` does not satisfy `>=0.8.0`, because a pre-release precedes its release.

The registry client checks the range before an install, and the agent checks it against its own version. An incompatible version is not offered for installation.

### `engines.pluginApi`

The plugin API level the module needs, written `^MAJOR.MINOR`. The agent provides level 1.0, so `^1.0` is the only value that installs today.

### `engines.sdk`

The version of the SDK the plugin was built with. It is informational. The only thing that reads it is the unit-test helper `testing::manifest_mismatches`, which compares it with the SDK the test is compiled against.

## The guest ABI

`"runtime": { "kind": "wasm", "abi": 1 }` declares the binary interface. `wasm` is the only kind and `1` the only ABI. A manifest without `runtime` describes an older JavaScript design, and packages of it are refused.

ABI 1 has grown compatibly, and the rules for that growth protect existing modules:

- Additions are optional exports and new capability operations. A module that does not use them is unchanged.
- An operation an older host or agent does not know is refused with an error, never misread. In the SDK this is the error kind `Unsupported`.
- A field added to what the host sends reaches only modules that declare, through the optional `dp_features` export, that they read it. Every module built with the current SDK does.

For plugin code this means: a call such as `Context::run_context` or `Context::notify_with` can fail on an agent older than the operation. Treat that as "unknown" and fall back.

## The SDK crate

`deckpilot-plugin-sdk` has its own semantic version, independent of DeckPilot's. While it is 0.x, a minor version may break the Rust API. It is not published, so pin the DeckPilot commit you build against; see [Getting started](../getting-started/#how-you-get-the-sdk).

## Platforms

`platforms` is an optional list of `macos`, `windows` and `linux`. Omitted means all three. A plugin is not offered on a platform it does not list.

The agent runs plugins only where the host has a full operating system sandbox, which exists for macOS, Linux and Windows. Windows execution has been verified on a CI runner only.

## Plugin versions

`version` is `X.Y.Z` with an optional pre-release of exactly the form `-alpha.N`, `-beta.N` or `-rc.N`, where N is at least 1. Build metadata and leading zeros are not allowed, and the whole string is at most 32 characters. It must equal the version passed to `Plugin::new`.

In the registry, versions are append-only:

- A new version must be greater than every recorded one.
- A recorded version is immutable. It cannot be edited or deleted.
- A version can be withdrawn by the owner with a revocation; see [Signing and trust](../signing/#revocation).

DeckPilot does not prescribe what a major, minor or patch change means for your plugin. A sensible convention is to treat action ids, parameter keys, output keys and variable ids as your public interface, because users' keys and workflows refer to them. To retire an action, mark it `deprecated` and name its successor in `replacedBy` rather than removing it at once.

## Updates and permission escalation

Updates compare versions by semantic versioning. When an update is available, what happens depends on its permissions:

- **Same or fewer permissions.** The update can be applied.
- **Added or broadened permissions.** This is an escalation. The agent stages the update and shows the escalated permissions in a consent dialog. Approving applies the update; declining keeps the old version, without the new permission.

Examples of an escalation: a new permission kind, another network host, another HTTP method, a credential slot bound to a new host, a larger storage quota.

What an update keeps and drops:

| State | After an update |
| --- | --- |
| Storage | Kept |
| Settings and credentials | Kept. A saved setting that no longer fits its field falls back to the default |
| Published variables | Dropped, until an action publishes them again |
| Grants | Kept; escalated keys need new consent |

The user can also roll back to the previous version. Plan stored data so that an older version of your plugin does not fail on values a newer one wrote.

## Registry tooling and the DeckPilot pin

The registry repository builds its tools and copies its schemas from one pinned DeckPilot commit. Manifest features added to DeckPilot after that commit do not validate in the registry until the pin is moved.

## Canonical reference

The Versions section of [docs/marketplace/sdk/errors-and-versions.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/errors-and-versions.md) and the compatibility table in [docs/marketplace/sdk/manifest.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/manifest.md).
