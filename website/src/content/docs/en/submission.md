---
title: "Submission"
description: "The pull-request flow for the plugin registry: publisher record, listing, build recipe, version record, CI checks, review and publication."
group: ship
order: 15
---

Plugins are submitted to the registry repository, [knittingcode/deckpilot-plugins](https://github.com/knittingcode/deckpilot-plugins), by pull request. The registry never holds your source: it records which tag and commit of your own repository to build, rebuilds the package itself, and the owner reviews and publishes.

> **Status:** The signed registry is live and publishes DeckPilot's own plugins. It is not open to other publishers yet. You can already prepare and validate a submission; according to the registry's contributing guide it will be reviewed and published once the registry opens.

## Before you start

- Your plugin is a WebAssembly module implementing plugin ABI 1, built with the Rust SDK.
- Your source lives in your own public GitHub repository.
- Your plugin has a license, declared as an SPDX expression in `manifest.json`, and ships a `LICENSE` file. The plugin keeps its own license; the registry repository's Apache-2.0 license does not apply to it.

### Namespaces

A plugin id is `<namespace>.<name>`. The namespace is yours once your publisher record is merged, and it is bound to your numeric GitHub user id.

Some publisher ids are reserved and cannot be a namespace: `deckpilot` (DeckPilot's own first-party plugins), `knittingcode`, generic names (`core`, `system`, `plugin`, `plugins`, `builtin`, `dev`, `local`, `test`, `user`) and the ids of the built-in modules (`general`, `projects`, `git`, `docker`, `http`, `notifications`, `github`, `gitlab`, `jira`, `workspace`, `work-session`). The registry's checks refuse them, and DeckPilot refuses manifests that use them.

## Submitting a new plugin

### 1. Tag your source

Commit `Cargo.lock`, pin the Rust toolchain in `rust-toolchain.toml` with an exact version, and push a tag such as `v1.0.0`. Never move or delete a tag that a registry record names.

### 2. Add the submission files

Fork the registry repository and add these files:

| File | When | What |
| --- | --- | --- |
| `registry/publishers/<your-login>.json` | First plugin only | Your publisher record |
| `registry/plugins/<id>/plugin.json` | First version; later when the listing changes | The listing shown in the app |
| `plugins/<id>/build.json` | First version; later when the build changes | The build recipe; see [Packaging](../packaging/#the-build-recipe) |
| `plugins/<id>/manifest.json` | Every version | A copy of this version's `manifest.json`, so that the permissions are visible in the pull request diff |
| `registry/plugins/<id>/versions/<version>.json` | Every version | The version record |

The publisher record:

```json
{ "namespace": "acme", "github_id": 1001, "login": "acme", "verified": false }
```

`github_id` is your numeric GitHub id; `gh api users/<login> -q .id` prints it. Leave `verified` as `false`. Only the owner changes it.

The listing has the required fields `id`, `name`, `summary`, `description` and `license`, and the optional fields `homepage`, `source_repository`, `categories` and `deprecated`. This is the listing of the registry's own CI fixture:

```json
{
  "id": "acme.hello-weather",
  "name": "Hello Weather",
  "summary": "Example plugin: a greeting and the current temperature of a city.",
  "description": "DeckPilot's sample plugin, built from the DeckPilot repository. It is a CI fixture of this registry and is never published.",
  "license": "Apache-2.0",
  "homepage": "https://github.com/knittingcode/deckpilot/tree/main/plugins/examples/hello-weather",
  "source_repository": "knittingcode/deckpilot",
  "categories": ["other"]
}
```

### 3. Generate the version record

The version record is the output of the registry tool for the package built from your tagged source. It records the version, the package's sha256 and size, the manifest digest, the permissions, the engines and the source repository, tag and commit. Produce it with the pinned tools in your fork of the registry repository:

```sh
tools/build-cli.sh
tools/build-plugin.sh --recipe plugins/<id>/build.json \
  --repository <owner>/<repo> --commit <40-hex sha> --tag <tag> \
  --out .work/<id>.dpplugin
.work/bin/deckpilot-registry add-version --dir . --package .work/<id>.dpplugin \
  --plugin-detail registry/plugins/<id>/plugin.json \
  --source-repository <owner>/<repo> --tag <tag> --commit <sha> \
  --builder 1.0.0
rm -rf releases   # add-version also stores the asset here; never commit it
```

If the plugin is not at the root of your repository, add `--path <dir>` to `build-plugin.sh` and `--source-path <dir>` to `add-version`.

The reference build is the registry's CI build on `ubuntu-24.04`, x86_64. A build on another operating system or architecture can produce a different module and so a different sha256. If your record does not match, the *Validate registry* job fails and prints the record derived from its own build. Commit that record.

### 4. Run the checks and open the pull request

```sh
tools/build-cli.sh
tools/validate.sh
```

`tools/validate.sh` builds plugin code from source, so run it only for plugins you are willing to build on your machine. Then open a pull request with the repository's template and fill in the submission checklist.

## Submitting an update

Add a new `registry/plugins/<id>/versions/<new-version>.json`, replace `plugins/<id>/manifest.json` with the new manifest, and change `plugin.json` or `build.json` if needed.

- The new version must be greater than every recorded one.
- Recorded versions are immutable. Never edit or delete one.
- If the update adds permissions, say why in the pull request. Users are asked to approve added permissions before the update applies.

## What you may change

| Path | Who |
| --- | --- |
| `registry/publishers/<your-login>.json` | You, with your own `github_id` and `login` |
| `registry/plugins/<your-namespace>.*/**`, `plugins/<your-namespace>.*/**` | You |
| `docs/*.md`, `README.md`, `plugins/README.md`, `registry/README.md` | Anyone |
| Workflows, `tools/`, `schemas/`, `keys/`, `registry/revocations.json`, other publishers' files, the policy documents | Owner only |

## What CI checks

All checks run without secrets and with a read-only token, also for forks.

| Check | What it does |
| --- | --- |
| *Repository hygiene* | No private keys, key files, packages, Python bytecode or generated site output |
| *Submission rules* | The path guard above, the publisher identity, no reserved publisher id, append-only versions. The permission diff against the previous version and a source compare link go to the job summary |
| *Schemas match DeckPilot* | The registry's schemas equal those of the DeckPilot commit it pins |
| *Validate registry* | Schemas of every input; your source fetched at the recorded commit, which the tag must point at; `cargo build --locked`; `deckpilot-registry package`; the version record derived again and compared byte for byte; the whole registry signed with throwaway keys and accepted by DeckPilot's own client |
| *Build examples* | The same for the registry's example fixtures |
| *actionlint and zizmor* | Workflow lint |

CI builds your code, so your build scripts and procedural macros run in the CI job. That job has no secrets. Keep the build hermetic.

> **Note:** The registry builds its tools from one pinned DeckPilot commit, recorded in `tools/deckpilot.env`. A manifest that uses a field added to DeckPilot after that commit does not validate there until the pin moves.

## Review

Every submission is reviewed by the owner before it is merged. Nothing is merged automatically, and a green CI run is not an approval. The reviewer checks:

- **Publisher identity and listing.** The account exists, its numeric id matches, the namespace is not misleading, and the listing is accurate.
- **Source.** The repository and tag are yours, and the source diff since the previous version contains nothing unexplained: obfuscation, generated blobs, build scripts that fetch or execute anything, vendored binaries.
- **Permissions.** Each justification matches what the plugin does, network hosts belong to the service the plugin claims, credential slots match those hosts, and `openUrl` hosts are pages the plugin's actions open for the user.
- **Build.** An exact toolchain, a locked Cargo workspace and a plain `cargo build` of one package; the package was rebuilt by CI and matches the record byte for byte.
- **License.** The manifest declares an SPDX license and the package ships its `LICENSE`.

There are no guaranteed response times.

## Merging does not publish

A merged pull request publishes nothing. The owner publishes a version by creating its release tag, `<id>--v<version>`, on the merged commit. That starts the publishing workflow, which rebuilds the package, signs a new index and timestamp, creates the GitHub Release with the package, and deploys the metadata. The plugins listed on this site were published this way.

Contributors never get publish rights: not by having a plugin merged, not by being a verified publisher, and not by contributing regularly. Tags, releases, environments, keys and secrets belong to the owner. The verified publisher badge is a review decision of the owner, not a permission.

## Reporting problems

Bugs in the registry's tooling go into a *Bug report* issue, and a plugin that misbehaves or misleads into a *Plugin report* issue. Anything malicious, or a vulnerability, is reported privately; see [Security](../security/#reporting-a-vulnerability).

## Canonical reference

[CONTRIBUTING.md](https://github.com/knittingcode/deckpilot-plugins/blob/main/CONTRIBUTING.md), [docs/publishing.md](https://github.com/knittingcode/deckpilot-plugins/blob/main/docs/publishing.md) and [docs/review-policy.md](https://github.com/knittingcode/deckpilot-plugins/blob/main/docs/review-policy.md) in the registry repository.
