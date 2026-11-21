# Official plugins

The sources of the plugins **DeckPilot bundles** (`deckpilot.docker`,
`.git`, `.github`, `.gitlab`, `.jira`, `.notifications`, `.projects`,
`.workflows`, `.workspace`), built, checked, packaged and released here as
signed `.dpplugin` artifacts that the application verifies and bundles.
This is the destination of moving them out of `knittingcode/deckpilot`
(`plugins/first-party/` there); their ids, action ids and settings stay
the same.

> **Status: Wave A, the foundation.** No plugin has moved yet. The
> workspace holds only a test fixture (`test-fixtures/example-default`, a
> byte-for-byte copy of DeckPilot's), which exercises the whole pipeline in
> CI. No publisher key exists, nothing is released, and the release
> workflow stops at its first step. Unlike `first-party/` these plugins are
> never registry entries: the agent never takes a bundled id from a
> registry. A verified release can still be turned into a checked
> *candidate* entry under `marketplace/`
> ([docs/official-marketplace.md](../docs/official-marketplace.md)).

## Layout

```text
official/
  Cargo.toml       one workspace; the SDK, the ABI crate and DeckPilot's
                   first-party support crate as git dependencies pinned to
                   DECKPILOT_COMMIT (tools/deckpilot.env); the guest profile
  Cargo.lock       committed; a build depends on this repository's revision alone
  plugins.json     which members are released (`plugins`) and which are
                   fixtures, and the permission kinds each may declare
  test-fixtures/   fixtures, never released
  marketplace/     candidate Marketplace entries, made only from verified releases
                   (tools/official-entry.py); never in the signed index
  <name>/          one plugin, id deckpilot.<name> (from Wave B on)
```

The toolchain is the repository's `rust-toolchain.toml` (DeckPilot's, checked
by `tools/check-schemas.sh`).

## Pipeline

`tools/official.sh` runs each step; `official.yml` runs them on every pull
request with no secret:

| Step | What fails it |
| --- | --- |
| `check` (`tools/check-official.py`) | a member not in `plugins.json`; a git dependency not on knittingcode/deckpilot at exactly `DECKPILOT_COMMIT` (no branch, no tag), a path dependency leaving `official/`, a missing or differently pinned `Cargo.lock`; a released id DeckPilot does not bundle; permission kinds other than the policy's; any plugin API 1.9 privileged kind while the gate is closed; process execution (`tools`) other than git for `deckpilot.git` and docker for `deckpilot.docker`; `engines` the pinned DeckPilot (`DECKPILOT_VERSION`, `DECKPILOT_PLUGIN_API`) does not satisfy; a fixture that differs from DeckPilot's |
| `test` | `cargo test --locked`, `cargo fmt --check`, `cargo clippy -D warnings` |
| `package` | the manifest schema; the guest build; `deckpilot-registry package --first-party` (manifest rules, size limits, the module check) |
| `repro` | two clean builds in fresh target directories whose modules or packages differ |
| `compare` | nothing: it reports whether each module equals DeckPilot's `plugins/first-party/build.sh` output at the pin (see below) |
| `sign-test` | `release sign` with a throwaway publisher key, `release verify` (claim, signature, compatibility, rollback floor from DeckPilot's `released-pins.json`, audit log), DeckPilot's released-pins guard on the verified files, and the keys pinned in DeckPilot not refusing for the expected reason (none provisioned) |

The build is the one of DeckPilot's `build.sh`: the `guest` profile, no
flags from the environment, and the paths remapped (`official/` to
`/deckpilot/plugins`, Cargo's home to `/cargo`, the pinned DeckPilot
checkout to the relative paths DeckPilot's own build records).

**Reproducibility, measured.** Two clean builds here are byte-identical
(modules and packages). Against DeckPilot's own build of the same source
they are generally not: Cargo hashes the SDK's git source (URL and `rev`)
into the crates' metadata, which changes the order of the module's code and
data (same size, same strings). The fixture happened to match at one pin and
not at the next. So each plugin's first release from here is a **new patch
version**, which DeckPilot's released-pins guard and the publisher
verifier's rollback floor enforce.

## Releases

`official-release.yml`, for a tag `deckpilot.<name>--v<version>` that the
owner creates (tags are admin-only), only when the owner has set it up:

1. `resolve`: `OFFICIAL_RELEASE_ENABLED` is `true`; the tag names a released
   plugin at its manifest version, at a commit on `main`.
2. `gate`: `tools/check-environment.sh official-release` (tags
   `deckpilot.*--v*` only, a required reviewer, no administrator bypass) and
   every required check of `main` passed for the commit.
3. `build` (no secret): checks, tests, two clean builds compared, the package.
4. `sign` (environment `official-release`): re-checks the package against
   the build job's sha256 and size, `release sign --key-env` (the key never
   touches the disk), then `release verify` with the keys pinned in the
   DeckPilot build, never `--keys`, for each platform, with the audit log.
5. `release`: package and `.minisig` on the GitHub Release of the tag
   (draft, both assets, then published; never a tag).

`publish.yml` (the registry) excludes these tags.

## What the owner must do before the first release

Not done by anyone yet; creating or rotating these secrets is the owner's:

1. Create the publisher key offline:
   `deckpilot-registry keys generate --role publisher --out publisher.key`
   (with a password); keep an encrypted offline backup.
2. Create the environment `official-release`: deployment tags
   `deckpilot.*--v*` only, a required reviewer, administrators cannot
   bypass. Add the secrets `DECKPILOT_PUBLISHER_KEY` (the secret key file's
   text) and `DECKPILOT_PUBLISHER_KEY_PASSWORD`.
3. Pin the public key (`publisher.key.pub`) in DeckPilot's
   `crates/deckpilot-plugin-registry/publisher/first-party.json` in a
   reviewed pull request, then move `tools/deckpilot.env` here to that
   commit.
4. Have the security review of DeckPilot's
   `docs/marketplace/publisher-review-request.md` done (it also decides
   whether the privileged-kinds gate may open for Wave C).
5. Set the repository variable `OFFICIAL_RELEASE_ENABLED` to `true`.

Then the first plugin moves (Wave B): its source under `official/<name>`
at a new patch version, listed in `plugins.json` with its permissions; a
release tag; and in DeckPilot its `artifacts.json` entry.
