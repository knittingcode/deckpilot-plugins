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
| `check` (`tools/check-official.py`) | a member not in `plugins.json`; a git dependency not on knittingcode/deckpilot at exactly `DECKPILOT_COMMIT` (no branch, no tag), a path dependency leaving `official/`, a missing or differently pinned `Cargo.lock`; a released id DeckPilot does not bundle; permission kinds other than the policy's; any plugin API 1.9 privileged usage while the gate is closed (an exact mirror of DeckPilot's `privileged.rs`: the four permission kinds, `openUrl.fromParams`, `node.changesContext`, `resource` parameters, variable host sources, host views, `statusItems`, `dashboardCards`; the package step's `package --first-party` runs the Rust validator itself); process execution (`tools`) other than git for `deckpilot.git` and docker for `deckpilot.docker`; `engines` the pinned DeckPilot (`DECKPILOT_VERSION`, `DECKPILOT_PLUGIN_API`) does not satisfy; a fixture that differs from DeckPilot's |
| `test` | `cargo test --locked`, `cargo fmt --check`, `cargo clippy -D warnings` |
| `package` | the manifest schema; the guest build; `deckpilot-registry package --first-party` (manifest rules, size limits, the module check) |
| `repro` | two clean builds in fresh target directories whose modules or packages differ (local use; CI's reproducibility check is the two independent jobs below) |
| `fresh-build` (jobs *Independent build A* and *B*, then *Two independent builds are byte-identical*) | two builds on separate fresh runners, with nothing of `official/` run before them (no tests, no other build scripts), whose module or package sha256 differ |
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
3. `checks` (no secret): checks, tests, the comparison with DeckPilot.
4. `build-a` and `build-b` (no secret): two independent builds of the tagged
   commit, each on its own fresh runner, with nothing run before them;
   module sha256, package sha256 and size.
5. `sign` (environment `official-release`): both builds must agree on
   module and package; build-a's package is re-checked against its sha256
   and size. One step, the only one with the key, runs `release sign --id
   --version` with the tag's id and version (the package must say the same;
   every check that parses the package runs before the key is loaded, and
   the key never touches the disk). The next step, without the key, runs
   `release verify` with the keys pinned in the DeckPilot build (never
   `--keys`), DeckPilot's rollback floor (`released-pins.json`) and
   revocations (`artifacts.json`), for each platform, with the audit log.
6. `release`: package and `.minisig` on the GitHub Release of the tag
   (draft, both assets, then published; never a tag).

Every job that builds or runs `deckpilot-registry` builds it from
`DECKPILOT_COMMIT` only after checking that the commit is on DeckPilot's
`main` (`require_on_main` in `tools/lib/common.sh`, run by
`tools/build-cli.sh` and `tools/check-schemas.sh`): GitHub serves a fork's
commits under DeckPilot's URL, and such a commit would otherwise build as
the verifier that runs next to the key.

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

Decisions only the owner can make (nothing here changes live settings):

- **Who may approve a signing run.** `check-environment.sh official-release`
  needs one required reviewer by default. The repository variables
  `OFFICIAL_RELEASE_REQUIRE_REVIEWERS` (for example `2`: at least two listed
  reviewers; GitHub then waits for one of them) and
  `OFFICIAL_RELEASE_REQUIRE_PREVENT_SELF_REVIEW` (`true`: the environment's
  "Prevent self-review" must be on, so whoever starts a run cannot approve
  it) make the gate stricter; together they mean someone other than the
  person who pushed the tag approves the signature. Set the environment
  first, then the variables.
- **Required checks.** The `main` ruleset requires six checks; *Official
  plugins* (the official/ pipeline and the two independent builds) is not
  among them. Adding *Build, test, check and package official plugins*,
  *Two independent builds are byte-identical* and *Build, test and package first-party plugins* to
  the required checks of the *Protect main* ruleset only adds conditions
  (nothing is weakened), and makes `check-required-checks.sh` hold every
  release to them. Changing a ruleset is the owner's.

Then the first plugin moves (Wave B): its source under `official/<name>`
at a new patch version, listed in `plugins.json` with its permissions; a
release tag; and in DeckPilot its `artifacts.json` entry.
