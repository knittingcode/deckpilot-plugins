# Contributing to the DeckPilot plugin registry

Thank you for building a plugin for DeckPilot. This repository accepts
plugin submissions, plugin updates, and fixes to its documentation. Changes
to tooling, workflows, schemas and keys are maintained by the owner (see
[What you may change](#what-you-may-change)).

> **Status:** the signed production registry is live and serves DeckPilot's
> own plugins, but it is not open to other publishers yet: the signing step
> refuses anything outside DeckPilot's namespace until the trust model for
> other publishers is complete
> ([docs/third-party-readiness.md](docs/third-party-readiness.md)). You can
> already prepare and validate a submission; it will be reviewed and
> published once the registry opens.

By participating you agree to the [Code of Conduct](CODE_OF_CONDUCT.md).

## Before you start

- Plugins are **WebAssembly modules implementing plugin ABI 1**
  (`plugin.wasm`), built with DeckPilot's Rust SDK
  (`deckpilot-plugin-sdk`). See DeckPilot's
  [artifact contract](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/artifact-contract.md)
  and the sample
  [`hello-weather`](https://github.com/knittingcode/deckpilot/tree/main/plugins/examples/hello-weather).
- Your source lives in **your own public GitHub repository**. This
  repository records which tag and commit to build; it never holds your
  source.
- Your plugin id is `<namespace>.<name>`. The namespace is yours once your
  publisher record is merged, and it is bound to your numeric GitHub user id.
- The publisher ids DeckPilot reserves cannot be your namespace:
  `deckpilot` (DeckPilot's own first-party plugins), `knittingcode`, generic
  names such as `core`, `system`, `plugin(s)`, `builtin`, `dev`, `local`,
  `test`, `user`, and the built-in module ids (`general`, `projects`, `git`,
  `docker`, `http`, `notifications`, `github`, `gitlab`, `jira`,
  `workspace`, `work-session`). *Submission rules* refuses them, and
  DeckPilot refuses manifests that use them.
- Choose a license for your plugin and declare it in `manifest.json`
  (`license`, SPDX). Ship the `LICENSE` file in the plugin directory. Your
  plugin keeps its own license; this repository's Apache-2.0 license does
  not apply to it.

## Submit a new plugin

1. **Tag your source.** Commit `Cargo.lock`, pin the Rust toolchain
   (`rust-toolchain.toml` with an exact version) and push a tag, e.g.
   `v1.0.0`. Never move or delete a tag that a registry record names.
2. **Fork this repository** and add:
   - `registry/publishers/<your-login>.json` (first plugin only):
     ```json
     { "namespace": "acme", "github_id": 1001, "login": "acme", "verified": false }
     ```
     `github_id` is your numeric id (`gh api users/<login> -q .id`). Leave
     `verified` as `false`; only the owner changes it.
   - `registry/plugins/<id>/plugin.json`: the listing, see
     [`schemas/inputs/plugin-info.schema.json`](schemas/inputs/plugin-info.schema.json)
     and the [example](plugins/examples/hello-weather/registry/plugins/acme.hello-weather/plugin.json).
   - `plugins/<id>/build.json`: how to build your module, see
     [`schemas/inputs/build-recipe.schema.json`](schemas/inputs/build-recipe.schema.json)
     and the [example](plugins/examples/hello-weather/build.json).
   - `plugins/<id>/manifest.json`: a copy of your version's `manifest.json`.
   - `registry/plugins/<id>/versions/<version>.json`: the version record.
     Generate it with the pinned tools as described in
     [docs/publishing.md](docs/publishing.md#1-submission-contributor). If
     your local build differs from CI's reference build, the
     *Validate registry* job prints the correct record to copy.
   - Optional: `registry/plugins/<id>/listing.json` and
     `registry/plugins/<id>/media/`, your storefront listing (promo text,
     icon, screenshots, links, release notes). See
     [docs/storefront.md](docs/storefront.md) for the format and the asset
     rules (PNG, WebP or JPEG; no SVG; no metadata).
3. **Run the checks** locally if you can: `tools/build-cli.sh && tools/validate.sh`.
4. **Open a pull request** using the template, and fill in the plugin
   submission checklist.

## Submit an update

Add a new `registry/plugins/<id>/versions/<new-version>.json` (the version
must be greater than every recorded one), replace
`plugins/<id>/manifest.json` with the new manifest, and change
`plugin.json` or `build.json` if needed. Recorded versions are immutable:
never edit or delete one. If an update adds permissions, say why in the pull
request. Users are asked to approve added permissions before the update
applies.

## Storefront listing

Your listing describes your plugin; it never places it. There is no
"featured" flag, order or collection in `listing.json`, and the schema
refuses any field it does not know. Where plugins appear on the Discover
page (banners, collection rows, spotlights) is decided by the owner in
`editorial/`, which contributors cannot change. Badges such as official or
verified come from the client, never from listing text; text that claims
one is flagged for review. Check your listing with
`python3 -I tools/check-storefront.py` before you open the pull request.

## Docs and fixes

Typos, clarifications and broken links in the documentation are welcome
as ordinary pull requests. For a problem with the tooling or a workflow,
open an issue (*Bug report*) rather than a pull request. Those paths are
owner-maintained.

## What you may change

The *Submission rules* check enforces this on every pull request:

| Path | Who |
| --- | --- |
| `registry/publishers/<your-login>.json` | you (with your own `github_id` and `login`) |
| `registry/plugins/<your-namespace>.*/**`, `plugins/<your-namespace>.*/**` | you |
| images (`.png`, `.webp`, `.jpg`) | you, only in `registry/plugins/<your-namespace>.*/media/`; never SVG |
| `docs/*.md`, `README.md`, `plugins/README.md`, `registry/README.md` | anyone |
| `editorial/**` (storefront placements), `.github/**`, `tools/**`, `schemas/**`, `keys/**`, `website/**`, `registry/revocations.json`, other publishers' files, `SECURITY.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `LICENSE` | owner only |

Dependabot (the `dependabot[bot]` account) may only modify
`website/package.json` (the versions of existing dependencies) and
`website/package-lock.json`. Its other updates are owner-only like
everything else.

Everything is reviewed by the owner (CODEOWNERS) before it is merged.

## What CI checks

All checks run without secrets and with a read-only token, also for forks:

| Check | What |
| --- | --- |
| *Repository hygiene* | no private keys, key files, packages, Python bytecode or generated `site/` output; storefront images only in `media/` directories, never SVG |
| *Submission rules* | the path guard above, publisher identity, no reserved publisher id, append-only versions, your `listing.json` against its schema; the permission diff and a source compare link go to the job summary |
| *Schemas match DeckPilot* | `schemas/` equal the pinned DeckPilot commit |
| *Validate registry* | schemas of every input; your source fetched at the recorded commit (the tag must point at it); `cargo build --locked`; `deckpilot-registry package`; the version record re-derived and compared byte for byte; the registry signed with throwaway keys and accepted by DeckPilot's own client (`verify-site --require-packages`); your listing and its media ([docs/storefront.md](docs/storefront.md#asset-rules)) |
| *Build examples* | the same for the example plugins |
| *actionlint and zizmor* | workflow lint |

CI builds your code, so its build scripts and proc macros run in the CI
job. That job has no secrets. Keep builds hermetic: no network access
beyond Cargo's locked dependencies, and no build-time downloads.

## Review

Every submission is reviewed by the owner before it is merged; see
[docs/review-policy.md](docs/review-policy.md) for the checklist. Nothing is
merged automatically, and a green CI run is not an approval. Expect
questions about permissions, network hosts and anything in the source diff
that is not obviously needed.

**Merging does not publish.** The owner publishes a version by creating its
release tag, `<id>--v<version>` ([docs/publishing.md](docs/publishing.md)).

## Publish rights

Contributors never get publish rights automatically: not by having a plugin
merged, not by being a verified publisher, and not by contributing
regularly. Tags, releases, environments, keys and secrets belong to the
owner. A verified publisher badge (`"verified": true`) is a review
decision of the owner, not a permission.

## Sign-off and licensing of contributions

A [DCO](https://developercertificate.org/) sign-off (`git commit -s`) is
welcome but optional. There is no CLA. Contributions to this repository's
own files (docs, tooling) are under its Apache-2.0 license. Your plugin
stays under the license you chose.

## Reporting problems

- A bug in the tooling or a workflow: *Bug report* issue.
- A plugin that misbehaves, misleads or abuses permissions: *Plugin report*
  issue, or, if it is malicious or a vulnerability, **privately** as
  described in [SECURITY.md](SECURITY.md).
