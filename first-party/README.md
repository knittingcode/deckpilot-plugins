# First-party plugin sources

The sources of DeckPilot's own **independently installable** plugins
(`deckpilot.<name>`): the ones a user adds from the Marketplace, as opposed
to the plugins bundled with the application, whose sources stay in
`knittingcode/deckpilot` under `plugins/first-party/`.

What each plugin can and cannot do today, and why, is in
[docs/plugin-factory.md](../docs/plugin-factory.md). Read it first: plugin
API 1.0 has no timers, no clock, no random source and no access to keys;
1.1 adds the time and random bytes, 1.2 key state (a plugin describes
what its keys show), and 1.3 the controls an action can be bound to (keys,
dials, touch zones; without `controls`, keys only) and toolbar entries that
open views DeckPilot draws (see `template/`). DeckPilot 0.9.0 provides 1.4
(manifest `dependencies`), 0.10.0 provides 1.9: 1.5 dialogs, 1.6
translations, 1.7 editor navigation, 1.8 title suggestions; the 1.9
permissions (`tools`, `resources`, `projectContext`, `hostViews`) are
accepted only from the plugins bundled with the application, never from a
registry. There are still no timers. A plugin that needs a later level
declares it in `engines.pluginApi` and with `Plugin::requires_api` (see
`memory/` and `template/`).

> **Status.** Nothing here is published yet. Since DeckPilot B6 (pinned) a
> registry may serve `deckpilot.*` plugins when its root-signed key set
> authorizes the namespace and it lists `deckpilot` as a verified
> publisher; there are no production registry keys yet (B1). CI builds,
> tests and packages every plugin as `deckpilot.<name>` (`--first-party`)
> into a registry signed with throwaway keys whose key set authorizes the
> namespace, and checks it with the client's rules.

## Layout

```text
first-party/
  Cargo.toml            one workspace, one Cargo.lock; the SDK as a git
  Cargo.lock            dependency on the commit of tools/deckpilot.env
  template/             deckpilot.template: copy it to start a plugin
  <name>/               one plugin, id deckpilot.<name>
    Cargo.toml          package deckpilot-<name>, crate types cdylib and rlib
    manifest.json       the manifest; English texts
    locales/pl.json     the same texts in Polish (checked against the manifest)
    README.md           user documentation, English
    README.pl.md        user documentation, Polish
    LICENSE             Apache-2.0
    assets/icon-128.png 128x128 PNG, at most 256 KiB
    src/lib.rs          plugin() and deckpilot_plugin_sdk::plugin!(plugin)
    src/tests.rs        native unit tests against the SDK's MockHost
```

This directory is not a registry submission. Each plugin to be published
has `registry/plugins/deckpilot.<name>/plugin.json` and
`plugins/deckpilot.<name>/build.json` (`"workspace": "first-party"`); a
published version adds a version record whose `source` names this
repository, a source tag, the commit and `first-party/<name>`, made with
`--first-party` by the owner ([docs/publishing.md](../docs/publishing.md),
"First-party plugins"). The plugins the application bundles
(`deckpilot.docker`, `deckpilot.git`, `deckpilot.github`,
`deckpilot.gitlab`, `deckpilot.jira`, `deckpilot.notifications`,
`deckpilot.projects`, `deckpilot.workflows`, `deckpilot.workspace`) never
come here: `tools/first-party.sh check` refuses their ids.

## Commands

```sh
tools/first-party.sh check     # layout, SDK pin, Polish texts (python3 only)
tools/first-party.sh test      # cargo test, fmt --check, clippy -D warnings

tools/build-cli.sh             # once: the pinned deckpilot-registry and schema-check
tools/first-party.sh package   # schema, wasm build, package, throwaway-key registry, verify-site
tools/first-party.sh           # all of it
```

One plugin while working on it, from `first-party/`:

```sh
cargo test --locked -p deckpilot-<name>
cargo build --locked -p deckpilot-<name> --target wasm32-unknown-unknown --profile guest
```

The toolchain is the repository's `rust-toolchain.toml` (the one DeckPilot
pins); `rustup target add wasm32-unknown-unknown` once. Output goes to
`.work/first-party/` (ignored by git). The workflow *First-party plugins*
(`.github/workflows/first-party.yml`) runs the same script.

## Adding a plugin

1. Copy `template/` to `<name>/` (`[a-z][a-z0-9]*(-[a-z][a-z0-9]*)*`, 2-32
   characters). Set the package name `deckpilot-<name>` in `Cargo.toml`,
   and the id `deckpilot.<name>`, the name, the description and
   `repository.directory` in `manifest.json` and in `src/lib.rs`.
2. Add `"<name>"` to `members` in `first-party/Cargo.toml`. Add
   dependencies to `[workspace.dependencies]` only if the plugin needs
   them, and run `cargo update -p <crate> --precise <version>` or a plain
   build once without `--locked` so that `Cargo.lock` changes with it.
   Dependencies must build for `wasm32-unknown-unknown` without WASI or
   JavaScript glue: the package check refuses every import except
   `deckpilot_v1.host_call`.
3. Declare every permission the code uses, each with a justification a
   user understands, and nothing more: exact hosts, only the methods used.
4. Write `locales/pl.json` and `README.pl.md`. `tools/first-party.sh check`
   lists every Polish text that is missing or left over.
5. Draw your own icon; ship no third-party art without its licence.
6. Unit tests: the definition against the manifest
   (`manifest_mismatches`), every action with a scripted `MockHost`,
   every error code, a denied host. Use only capabilities the SDK has; do
   not work around a missing one.

## Texts in two languages

Plugin API 1.0 and the registry format have no localized metadata: the
manifest's texts are what DeckPilot and the Marketplace show, in one
language, English. `locales/pl.json` and `README.pl.md` keep the Polish
texts next to the source so that they are reviewed with it and ready when
the manifest can carry them; they are **not packaged** and DeckPilot does
not read them. `pl.json` mirrors the manifest:

```json
{
  "name": "…", "description": "…",
  "permissions": { "<kind>": "<justification>" },
  "actions": { "<id>": { "name": "…", "description": "…", "category": "…",
    "parameters": { "<key>": { "label": "…", "description": "…" } },
    "outputs": { "<key>": "<label>" } } },
  "variables": { "<id>": { "name": "…", "description": "…" } },
  "settings": { "<key>": { "label": "…", "description": "…" } },
  "credentials": { "<key>": { "label": "…", "description": "…" } },
  "errorCodes": { "<code>": "…" }
}
```

Only the sections the manifest has, and a `description` only where the
manifest has one. Every text keeps the manifest's length limit.

## The SDK pin

`deckpilot-plugin-sdk` is not published to crates.io. The workspace
depends on it by git at the commit of `tools/deckpilot.env`
(`DECKPILOT_COMMIT`), the same commit the registry tooling and `schemas/`
come from; `tools/first-party.sh check` refuses any other `rev`. When the
pin moves, change the `rev` in `Cargo.toml` and refresh `Cargo.lock` in
the same pull request.
