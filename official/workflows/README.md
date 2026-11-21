# deckpilot.workflows

The first-party Workflows plugin (DeckPilot
`docs/marketplace/module-migration.md` §7 X8, §11 D6, WP-B5), moved here
from DeckPilot's `plugins/first-party/workflows` (0.1.0, shipped with
DeckPilot 0.10.0). DeckPilot bundles it with class `default`: seeded and
enabled on every computer.

**Manifest only.** It declares no actions and no settings. Its manifest
declares the host view of kind `workflows` (context `profile`) and its
navigation entry `workflows`, under the `hostViews` permission; DeckPilot
shows its own Workflows screen and that entry only while the plugin is
enabled. The screen is drawn by the app from core data: the plugin ships
no UI code and is given no data.

## What stays in DeckPilot's core

The boundary is DeckPilot's decision D1/D6, and none of it is here:

- the workflow engine and the workflow-execution primitives;
- the workflow definitions (stored in the profiles, in the core's format)
  and their bindings: keys run workflows through the core's
  `BindingTarget::Workflow`, with or without this plugin;
- the execution IPC and the run monitoring views;
- the React code of the Workflows screen (plugins cannot ship UI code).

With the plugin disabled or removed, keys that run workflows still run
them and stored workflows are untouched; only the screen and its
navigation entry are hidden (DeckPilot's
`apps/agent/tests/plugins_workflows.rs`).

## The contract

Unchanged since the move, and tested in `src/tests.rs`:

| | |
| --- | --- |
| id | `deckpilot.workflows` |
| actions | none |
| settings | none |
| view | `workflows`, kind `workflows`, context `profile` |
| navigation | `workflows`, view `workflows`, icon `assets/icon-128.png` |
| permission | `hostViews` (no other) |
| texts | `locales/en.json`, `locales/pl.json`, the same keys |

Only `version` (0.1.1, a new patch version: a build here is not
byte-identical to DeckPilot's 0.1.0) and `repository` changed.
`tools/check-official.py` compares the rest of the manifest, the texts
and the icon with DeckPilot's at the pinned commit while DeckPilot still
has them.

## Release: exactly as reviewed

`hostViews` and the `workflows` view are plugin API 1.9 privileged kinds.
The security review (DeckPilot `docs/marketplace/publisher-review-request.md`)
opened the privileged-kinds gate for exactly `../privileged-policy.json`,
so 0.1.1 is a released plugin (no longer `held` in `../plugins.json`,
2026-11-21): `package --first-party` packages it, its release tag is
`deckpilot.workflows--v0.1.1`, and any manifest that is not exactly its
reviewed entry (a new construct, another permission, another version) is
refused by `tools/check-official.py` and the pinned DeckPilot's packager
and publisher verifier. It never has a Marketplace entry (bundled in
DeckPilot only). Until its release is in DeckPilot's `artifacts.json`,
DeckPilot keeps building it from its own `plugins/first-party/workflows`.
