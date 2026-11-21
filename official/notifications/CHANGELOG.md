# Changelog: deckpilot.notifications

Not packaged (the manifest names no changelog); the release notes of the
GitHub Release `deckpilot.notifications--v<version>` are taken from here.
Versions 0.1.0 and 0.1.1 were built and shipped by DeckPilot from its
`plugins/first-party/notifications`.

## 0.1.2 (unreleased)

Release notes:

> The Notifications plugin is now built and released from
> knittingcode/deckpilot-plugins (`official/notifications`) as a signed
> artifact. Nothing changes for users: the same actions (`show`,
> `workflow-summary`), settings (`desktop`, `statusBar`,
> `minimumSeverity`) and permission (`notifications`, routed by those
> settings).

- Moved from DeckPilot's `plugins/first-party/notifications` at 0.1.1.
  The source (`src/lib.rs`), the icon, the licence and the manifest are
  unchanged but for `version` and `repository` (now this repository,
  `official/notifications`).
- A new patch version because the module's bytes change: built here
  against the SDK as a git dependency on the pinned DeckPilot commit, Cargo
  hashes the SDK's git source into the build, so the module differs from
  DeckPilot's 0.1.1 of the same source. A shipped version with other bytes
  fails the agent's bundled pins and shows the plugin as Broken.
- New tests: the manifest equals DeckPilot's shipped 0.1.1 (by its pinned
  sha256) but for `version` and `repository`; the version is above every
  version DeckPilot shipped.

## 0.1.1

Shipped in DeckPilot 0.10.0. Rebuilt with the current plugin SDK; the
manifest and the actions are unchanged.

## 0.1.0

Shipped in DeckPilot 0.7.0 to 0.9.0. The first release, the replacement of
the built-in `notifications` module.
