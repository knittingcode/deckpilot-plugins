# Publishing process

How a plugin version goes from a pull request to a signed registry entry
and a release asset. The production setup exists (key set 1, the
environments and the variables of [Owner setup](#owner-setup)); DeckPilot's
first-party plugins are published this way.

## 1. Submission (contributor)

A pull request adds or changes, for a plugin `<id>` = `<namespace>.<name>`:

| File | First version | Later versions |
| --- | --- | --- |
| `registry/publishers/<login>.json` | yes, once per publisher | — |
| `registry/plugins/<id>/plugin.json` | yes | when the listing changes |
| `registry/plugins/<id>/versions/<version>.json` | yes | one new file per version |
| `plugins/<id>/build.json` | yes | when the build changes |
| `plugins/<id>/manifest.json` | yes | replaced by the new version's manifest |
| `registry/plugins/<id>/listing.json` (optional) | storefront listing: promo, screenshots, links, release notes | whenever it changes; no new version needed |
| `registry/plugins/<id>/media/*.png\|webp\|jpg` (optional) | the images `listing.json` names | with the listing |

The storefront listing and its media are described in
[storefront.md](storefront.md), with the asset rules. A listing never
places a plugin (no featured flag, order or collection): placements are
`editorial/`, which only the owner changes. Images go only in
`registry/plugins/<id>/media/`, as PNG, WebP or JPEG; SVG is refused.

The version record is the output of `deckpilot-registry add-version` for
the package built from the tagged source. Produce it with the pinned tools:

```sh
tools/build-cli.sh
tools/build-plugin.sh --recipe plugins/<id>/build.json \
  --repository <owner>/<repo> --commit <40-hex sha> --tag <tag> \
  [--path <plugin dir in the repo>] --out .work/<id>.dpplugin
.work/bin/deckpilot-registry add-version --dir . --package .work/<id>.dpplugin \
  [--plugin-detail registry/plugins/<id>/plugin.json] \
  --source-repository <owner>/<repo> --tag <tag> --commit <sha> [--source-path <dir>] \
  --builder 1.0.0
rm -rf releases   # add-version also stores the asset here; never commit it
```

The reference build is CI's (`ubuntu-24.04`, x86_64). Builds on another OS
or architecture can produce a different module, and so a different sha256
(observed: macOS arm64 and Linux x86_64 differ for the same source and
toolchain). If the record you commit does not match, the *Validate registry*
job fails and prints the record derived from its own build in the log and
the job summary. Commit that record; the source, tag and commit are yours,
and `published_at` is kept from your file.

### First-party plugins (`deckpilot.*`, owner only)

DeckPilot's own installable plugins live in [`first-party/`](../first-party/)
and are published by the owner under the publisher `deckpilot`
(`registry/publishers/knittingcode.json`, verified). A registry may serve
them only because its root-signed key set authorizes the namespace
([trust-model.md](trust-model.md#first-party-plugins-deckpilot)); the plugins
the application bundles (`deckpilot.docker`, `deckpilot.git`,
`deckpilot.github`, `deckpilot.gitlab`, `deckpilot.jira`,
`deckpilot.notifications`, `deckpilot.projects`, `deckpilot.workflows`,
`deckpilot.workspace`) are never listed.

`registry/plugins/deckpilot.<name>/plugin.json` and
`plugins/deckpilot.<name>/build.json` (`"workspace": "first-party"`) are
committed. A version record is made as above with two differences:

- `tools/build-plugin.sh` passes `--first-party` to `package` by itself
  (recipe id `deckpilot.*`); `add-version` needs `--first-party` too, with
  `--source-repository knittingcode/deckpilot-plugins --source-path
  first-party/<name>`.
- `add-version` checks a `deckpilot.*` package against the workspace's
  current key set, which must authorize `deckpilot`. The repository root
  has none under `site/v1/keys/`, so run it in a scratch workspace: copy
  `registry/`, `keys/roots.json` to `roots.json` and `keys/<n>.json(.minisig)`
  to `site/v1/keys/`, then copy the new record back. *Validate registry*
  re-derives it the same way under a throwaway authorizing key set.

The source tag must resolve to a commit of this repository that holds the
plugin's source, and it is not the release tag: `<id>--v<version>` goes on
the later `main` commit that holds the record. Use a name without `--v`
(e.g. `first-party/weather/v0.2.1`) so the push does not start *Publish*.

## 2. Validation (`validate.yml`, untrusted)

Runs on the pull request with no secrets and a read-only token:

- *Repository hygiene*: no private keys, key files, `site/`, `releases/`
  or packages; storefront images only in the `media/` directories, never
  SVG.
- *Submission rules*: the path guard, publisher identity (`github_id` and
  `login` equal to the pull request author's), no publisher id DeckPilot
  reserves for a contributor (`deckpilot` included), `deckpilot.*` records
  only from the owner (login and numeric id) with the owner's verified
  `deckpilot` record, never a recipe for a plugin the application bundles
  and in `registry/` only an official one without a privileged kind, as its
  committed candidate entry ([official-marketplace.md](official-marketplace.md)), append-only
  versions, and
  the permission diff against the previous version plus a source compare
  link in the job summary.
- *Schemas match DeckPilot*: `schemas/`, `rust-toolchain.toml`, the
  reserved publisher ids and the bundled plugin ids of `tools/` equal the
  pinned DeckPilot commit.
- *Validate registry*: schema and layout checks of all inputs; every
  recorded version rebuilt from its source (the tag must resolve to the
  commit in the named repository); each record re-derived with `add-version`
  and required to be byte-identical (an official plugin: its release asset
  downloaded and verified with the pinned publisher keys instead of a
  rebuild, the record its candidate's with the host floor); `plugins/<id>/manifest.json` equal to
  the newest package's manifest; the committed key set chain walked; the
  whole registry signed with throwaway keys made in the job and accepted by
  `verify-site --require-packages --min-remaining 6d`.
  Then the storefront inputs (`tools/validate.sh storefront`,
  [storefront.md](storefront.md#asset-rules)): `listing.json`,
  `editorial/` and every media file.
- *Build examples*: the same for `plugins/examples/*` (never published).
- *actionlint and zizmor* (workflow *Workflow lint*).

A pull request can change the workflow it runs, so these results are
advisory for changes to `.github/`, `tools/` and `schemas/`; code-owner
review is what protects them.

## 3. Review and merge (owner)

See [review-policy.md](review-policy.md). Merges are squash merges into
`main` after an approving code-owner review. Merging publishes nothing.

## 4. Publication (owner, `publish.yml`)

The owner creates the release tag on the merged commit of `main`. Tags are
restricted to the admin role by ruleset; `GITHUB_TOKEN` never creates one.

| Tag | Publishes |
| --- | --- |
| `<plugin-id>--v<version>`, e.g. `acme.hello-weather--v1.0.0` | that version: its package becomes the asset `<plugin-id>-<version>.dpplugin` of the release with that tag, and a new signed index lists it |
| `registry--v<anything>`, e.g. `registry--v2026.11.15.1` | a new signed index only: revocations, yanks, listing or publisher changes, a key set rotation |

The tag must equal `<id>--v<version>` because clients only download from
`https://github.com/knittingcode/deckpilot-plugins/releases/download/<id>--v<version>/<id>-<version>.dpplugin`
(the client's `DOWNLOAD_PREFIX` and `release_prefix`). `registry` is not a
valid plugin id (no `.`), so it can never collide. Application-style `v*`
tags are refused, and application releases never come from this repository
([separation.md](separation.md)).

```sh
git fetch origin && git tag acme.hello-weather--v1.0.0 origin/main && git push origin acme.hello-weather--v1.0.0
# retry or re-run on the same tag:
gh workflow run publish.yml --ref acme.hello-weather--v1.0.0
```

The push of the tag starts *Publish*:

| Job | Environment | Permissions | Secrets | Runs plugin code |
| --- | --- | --- | --- | --- |
| `resolve`: publishing enabled, run on a tag, tag commit on `main`, the version record exists and names this tag's asset URL, every other version already released | — | `contents: read` | — | no |
| `build`: rebuilds and checks every recorded package at the tagged commit (`tools/validate.sh registry`) | — | `contents: read` | — | **yes** |
| `sign`: builds only the pinned DeckPilot tool; takes the packages of `build` from this same run; re-checks every sha256 and size against the reviewed records; mirrors and verifies the live site (existing-client state); `build-index`, `sign-index`, with `REGISTRY_STOREFRONT_ENABLED=true` `storefront build` and `storefront sign` (index key; [storefront.md](storefront.md#workflow)), `timestamp --expires-in 7d`; `verify-site --state … --require-packages --min-remaining 6d` (and `--require-storefront`) | `registry-release` | `contents: read` | index and timestamp keys | no |
| `release`: re-checks the package against its pin; creates a draft release for the existing tag if there is none (`--verify-tag`), uploads the asset, publishes it (never latest; pre-release for a semver pre-release); downloads it anonymously and compares the sha256 | — | `contents: write` | — | no |
| `website`: builds the Marketplace website, its catalog generated from the registry `sign` verified (read-only) | — | `contents: read` | — | no |
| `deploy`: the signed `/v1/` byte for byte, with the website around it (`tools/compose-site.py`), to Pages | `github-pages` | `pages: write`, `id-token: write`, `contents: read` | — | no |
| `verify-published`: waits until Pages serves the new timestamp, mirrors it anonymously, downloads every package, `verify-site --require-packages --min-remaining 6d`; when a storefront was signed, the live `storefront/` and `media/` must be the signed bytes and `--require-storefront` | — | `contents: read` | — | no |

Artifacts from `validate.yml` or any other run are never used: `sign`
accepts only this run's `build` artifact and only bytes whose hashes equal
the records on the tagged commit. Plugin build scripts never run in a job
that has a secret.

If the owner creates the release themselves, they should leave it a
**draft** (or create only the tag): with immutable releases enabled, assets
cannot be added to a published release.

## 5. Keeping clients fresh (`refresh-timestamp.yml`)

Daily, the timestamp is re-signed with the timestamp key only (7 days of
validity, at most 14 accepted by clients), and the site redeployed. The
index is unchanged byte for byte. GitHub starts scheduled runs late (by
hours at busy times) and can drop them, so the workflow is scheduled four
times a day (04:23, 10:23, 16:23, 22:23 UTC); a scheduled run re-signs
only a live timestamp at least 20 hours old and otherwise deploys nothing.
A dispatch always re-signs. GitHub disables scheduled workflows in a
public repository after 60 days without activity; a dispatch of *Refresh
timestamp* restores freshness. If it stops, clients show the registry as
stale and keep running installed plugins.

Every run, scheduled or dispatched, ends with the **freshness monitor**
(`tools/check-freshness.sh`). It fails when the live timestamp has less
than 5 days left, or when a published `<id>--v<version>` release is not in
the live index. A failed scheduled run is reported by GitHub to the
workflow's owner (by e-mail, by default).

### Recovery

| Symptom | Cause | Action |
|---|---|---|
| Monitor: less than 5 days left | refreshes failing or not running | open the last *Refresh timestamp* runs; fix the cause; `gh workflow run refresh-timestamp.yml -R knittingcode/deckpilot-plugins`. If the workflow was disabled for inactivity, enable it (`gh workflow enable refresh-timestamp.yml`). |
| Monitor: the live timestamp has EXPIRED | as above, for 7 days | as above. Clients recover on their next refresh; installed plugins kept running. |
| Monitor: a release is not in the live index | its `publish.yml` run failed or was cancelled by a newer pending run of `registry-pages` | `gh workflow run publish.yml -R knittingcode/deckpilot-plugins --ref <id>--v<version>` |
| A run fails with "older registry than the last verified deployment" or "serves no registry" | the CDN still serves a cached copy (it is waited for 15 minutes), or the live site lost its registry | re-run the run after a few minutes. If it persists, compare `/v1/timestamp.json` with `state.json` on `registry-state`; a live site without the registry is restored by re-running the last successful *Publish* or *Refresh timestamp* run's workflow. Never delete or rewind `registry-state` to make a run pass: that would let a stale copy be redeployed. |
| The `sign` job refuses the mirror (`verify-site --state`) | the live registry is not one existing clients accept | stop publishing; see [trust-model.md](trust-model.md) |

## Revocation, yank, deprecation

All are reviewed pull requests by a maintainer, then a `registry--v…` tag:

```sh
.work/bin/deckpilot-registry revoke --dir . --id <id> --versions 1.2.0 --action disable \
  --reason "…" --advisory https://github.com/knittingcode/deckpilot-plugins/security/advisories/…
.work/bin/deckpilot-registry yank --dir . --id <id> --version 1.2.0 --reason "…"
```

Revocations are cumulative: the tool, the `sign` job and every client
refuse an index that drops or weakens one. Lifting one needs a root-signed
key set with `seq_floor_reset` ([trust-model.md](trust-model.md)).

## Owner setup

All of this exists since 2026-11-17 (key set 1). The exact, ordered steps with their commands, and the decisions still open,
are in [production-readiness.md](production-readiness.md).

1. **Key ceremony** (offline, PUBLISHING.md in DeckPilot, U-10): root keys,
   index key, timestamp key; `keyset init` and `keyset sign`; commit
   `keys/roots.json`, `keys/1.json` and `keys/1.json.minisig` by pull
   request. `roots.json` also becomes DeckPilot's
   `DECKPILOT_REGISTRY_ROOT_PUBKEYS`.
2. **Environment `registry-release`**: deployment rule "selected tags"
   `*--v*`, no reviewer (tag authority, see trust-model.md), administrators
   cannot bypass, and the secrets
   `REGISTRY_INDEX_KEY`, `REGISTRY_INDEX_KEY_PASSWORD`,
   `REGISTRY_TIMESTAMP_KEY`, `REGISTRY_TIMESTAMP_KEY_PASSWORD`
   (the minisign secret key file's text and its password).
3. **Environment `registry-timestamp`**: deployment branch `main` only, no
   reviewer (it runs daily), secrets `REGISTRY_TIMESTAMP_KEY` and
   `REGISTRY_TIMESTAMP_KEY_PASSWORD`. It holds the timestamp key only.
4. **Environment `github-pages`** (created by Pages): allow deployments
   from `main` and from tags `*--v*`, because `publish.yml` deploys from
   the tag.
5. **Repository variables** `REGISTRY_PUBLISH_ENABLED=true` and
   `REGISTRY_TIMESTAMP_ENABLED=true`, set last.

The root secret keys never go to GitHub.
