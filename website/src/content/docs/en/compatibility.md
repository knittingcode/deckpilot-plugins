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
| Public DeckPilot versions | [v0.10.0](https://github.com/knittingcode/deckpilot/releases/tag/v0.10.0) (2026-11-21), the current stable release, with plugin API 1.9. [v0.9.0](https://github.com/knittingcode/deckpilot/releases/tag/v0.9.0) (2026-11-20), with plugin API 1.4. [v0.8.1](https://github.com/knittingcode/deckpilot/releases/tag/v0.8.1) and [v0.8.0](https://github.com/knittingcode/deckpilot/releases/tag/v0.8.0) (2026-11-19), with plugin API 1.3. [v0.7.0](https://github.com/knittingcode/deckpilot/releases/tag/v0.7.0) (2026-11-18), the first release with plugin support, with plugin API 1.2. Before them, v0.6.0 (2026-11-17), the first stable release, without plugin support, and the pre-releases v0.6.0-alpha.3 and v0.6.0-alpha.4 |
| Rust SDK | `deckpilot-plugin-sdk` 0.1.0, not published to crates.io |
| JavaScript SDK | Type declarations only; cannot build or run a plugin |
| Registry | Live at `https://knittingcode.github.io/deckpilot-plugins/v1/`, signed with production key set 1. Publishes DeckPilot's own plugins; not open to other publishers yet |
| Bundled plugins | Since DeckPilot 0.10.0, Git, Docker, Workspace, Projects, GitHub, GitLab, Jira and Workflows are DeckPilot's own plugins bundled with the app, together with notifications (`deckpilot.git`, `deckpilot.docker`, `deckpilot.workspace`, `deckpilot.projects`, `deckpilot.github`, `deckpilot.gitlab`, `deckpilot.jira`, `deckpilot.workflows`, `deckpilot.notifications`). They come only from the application bundle, never from a registry |

Plugin API 1.x (1.9 in DeckPilot 0.10.0), guest ABI 1 and manifest version 1 are what DeckPilot implements.

## The version numbers

| Number | Where | Today | Rule |
| --- | --- | --- | --- |
| Manifest version | `manifestVersion` | `1` | The only version the agent reads |
| Guest ABI | `runtime.abi`; the module's `dp_abi_version` export | `1` | The host loads only ABI 1 modules |
| Plugin API level | `engines.pluginApi` | DeckPilot 0.7.0 provides `1.2`, 0.8.x `1.3`, 0.9.0 `1.4`, 0.10.0 `1.9` | `^MAJOR.MINOR`: same major, at least this minor |
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

The plugin API level the module needs, written `^MAJOR.MINOR`: the same major level, and at least this minor level. An agent below it does not install the plugin and reports the version as incompatible.

| DeckPilot | Plugin API it provides | `pluginApi` values that install |
| --- | --- | --- |
| 0.6.0 | none: no plugin support | none |
| 0.7.0 | 1.2 | `^1.0` to `^1.2` |
| 0.8.0, 0.8.1 | 1.3 | `^1.0` to `^1.3` |
| 0.9.0 | 1.4 | `^1.0` to `^1.4` |
| 0.10.0 | 1.9 | `^1.0` to `^1.9` |

Each minor level adds something a plugin can use:

| Level | Adds |
| --- | --- |
| 1.1 | The time (`Context::now`) and random bytes (`Context::random_bytes`) |
| 1.2 | Key state: an action's state handler draws its key ([Actions](../actions/)) |
| 1.3 | Per action, the controls and inputs it works on (`controls`), and up to four toolbar entries (`navigation`) that open views DeckPilot draws itself (`views`); see [Manifest format](../manifest/#controls) |
| 1.4 | A view's `context` (the device and profile selectors shown with it), and `dependencies`: up to 16 other plugins the plugin needs, which DeckPilot resolves and installs with it before download. An action's `requiresDependencies` names the optional ones it cannot run without. A dependency grants nothing |
| 1.5 | The `dialogs` permission and `Context::dialog`: standard confirmation, alert, form and selection dialogs that DeckPilot draws, only in a run the user started, without secret fields |
| 1.6 | Translations: `locales/<tag>.json` files in the package, `locales` and `defaultLocale` in the manifest, `t:` keys in manifest texts, `Context::locale` and `Context::t` |
| 1.7 | An action's `editorNavigation`: what running it does to the page or layout a device shows, for the Device Editor's preview |
| 1.8 | An action's `title` (a suggested and a dynamic title, its position), and `contributes.visualAssets`: icons and backgrounds for DeckPilot's Icon and Background Libraries |
| 1.9 | Mostly for DeckPilot's own bundled plugins; see [below](#plugin-api-19) |

Declare `^1.0` unless the plugin cannot work without a later addition. A call an agent does not know is answered as unsupported, so a plugin that can do without it keeps `^1.0` and runs on every DeckPilot with plugin support. A plugin that declares a higher level raises it in code too, with `Plugin::requires_api`; `testing::manifest_mismatches` compares the two.

A manifest that uses `controls`, `navigation` or `views` must declare `"pluginApi": "^1.3"`; the manifest validator refuses a lower level, and agents below 1.3 refuse such a manifest anyway. Since DeckPilot 0.8.0 is the first release that provides 1.3, such a plugin should also declare `"deckpilot": ">=0.8.0"` (for example `">=0.8.0 <1.0.0"`), so that DeckPilot 0.7.0 does not offer it. Plugins that declare `^1.0` to `^1.2` keep working on DeckPilot 0.8.0.

The same holds for later levels: a manifest that uses a 1.4 to 1.8 addition must declare that level, and should name the first release that provides it as the lower bound of `engines.deckpilot`: `">=0.9.0"` for 1.4, `">=0.10.0"` for 1.5 to 1.8. Every plugin that declares a lower level keeps working on DeckPilot 0.10.0.

### Plugin API 1.9

Plugin API 1.9 is the platform on which DeckPilot 0.10.0 moved Git, Docker, Workspace, Projects and Workflows out of its core into its own bundled plugins. Its additions are the permissions `tools` (brokered `git` and `docker` runs), `resources`, `projectContext` and `hostViews`, `openUrl` with `fromParams`, `resource` parameters, an action's `requiresPermissions`, and variables whose value DeckPilot computes from the active project. All of these are accepted **only** from DeckPilot's own plugins shipped inside the app, whose every file matches the pins compiled into the agent. A third-party manifest, a package from a registry (a `deckpilot.*` one included), and a sideloaded or developer package that declares one are refused (`first-party-only`), and the registry's tooling refuses them too. Process execution in general, the shell, the file system and plugin UI code stay never grantable for every publisher.

The one 1.9 addition open to every publisher is the `profile` permission (`Context::profile`: the active profile's name and, with `project`, the active project); see [Permissions](../permissions/). A third-party plugin that does not use it has no reason to declare `^1.9`.

The full rules are in DeckPilot's [permissions reference](https://github.com/knittingcode/deckpilot/blob/v0.10.0/docs/marketplace/sdk/permissions.md#plugin-api-19-first-party-bundled-plugins-only).

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
