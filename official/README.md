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

## Moved plugins and the privileged-kinds gate

A moved plugin is DeckPilot's `plugins/first-party/<name>` at the pinned
commit with its version (a new patch: the bytes differ) and `repository`
changed. Its `src/contract.rs` compares it with the pinned DeckPilot
checkout while DeckPilot still has the source: the manifest (ids, action
ids, parameter schemas, settings, permissions, dependencies) but those two
fields, a higher version, and for Projects and Workspace also the files a
bundle takes besides the module (`LICENSE`, `assets/`, `locales/`) byte for
byte.

| Plugin | Version (DeckPilot's) | Permission kinds | Gate status |
| --- | --- | --- | --- |
| `deckpilot.projects` | 0.1.1 (0.1.0) | `hostViews`, `profile`, `projectContext`, `resources` | held |
| `deckpilot.workspace` | 0.1.1 (0.1.0) | `openUrl` (`fromParams`), `profile`, `resources` | held |

Workspace's optional dependency on `deckpilot.projects` (`>=0.1.0 <1.0.0`)
is DeckPilot's, unchanged, and 0.1.1 satisfies it.

Both are `held` in `plugins.json` (see "Held plugins" below): their
source is on main and goes through `check`, `test` and `compare`, while
`package --first-party` must refuse them and they are never signed,
released or given a Marketplace entry until the security review of
DeckPilot's `docs/marketplace/publisher-review-request.md` opens the gate,
in a reviewed change here and in DeckPilot.

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
| `check` (`tools/check-official.py`) | a member not in `plugins.json`; a git dependency not on knittingcode/deckpilot at exactly `DECKPILOT_COMMIT` (no branch, no tag), a path dependency leaving `official/`, a missing or differently pinned `Cargo.lock`; a released id DeckPilot does not bundle; permission kinds other than the policy's; any plugin API 1.9 privileged usage while the gate is closed, except in the source of a `held` plugin (below) (an exact mirror of DeckPilot's `privileged.rs`: the four permission kinds, `openUrl.fromParams`, `node.changesContext`, `resource` parameters, variable host sources, host views, `statusItems`, `dashboardCards`; the package step's `package --first-party` runs the Rust validator itself); process execution (`tools`) other than git for `deckpilot.git` and docker for `deckpilot.docker`; `engines` the pinned DeckPilot (`DECKPILOT_VERSION`, `DECKPILOT_PLUGIN_API`) does not satisfy; a fixture that differs from DeckPilot's |
| `test` | `cargo test --locked`, `cargo fmt --check`, `cargo clippy -D warnings` |
| `package` | the manifest schema; the guest build; `deckpilot-registry package --first-party` (manifest rules, size limits, the module check) |
| `repro` | two clean builds in fresh target directories whose modules or packages differ (local use; CI's reproducibility check is the two independent jobs below) |
| `fresh-build` (jobs *Independent build A* and *B*, then *Two independent builds are byte-identical*) | two builds on separate fresh runners, with nothing of `official/` run before them (no tests, no other build scripts), whose module or package sha256 differ |
| `compare` | nothing: it reports whether each module equals DeckPilot's `plugins/first-party/build.sh` output at the pin (see below) |
| `sign-test` | two throwaway publisher keys made in the test, threshold 2: `release sign` with each, `release verify` with both (claim, signatures, compatibility, rollback floor from DeckPilot's `released-pins.json`, audit log naming two signers); refusing one signature, the same signature twice, a key that is not in the set, a revoked key (also next to both keys), modified bytes, and a signature set other than two `<package>.<KEYID>.minisig` files by distinct keys; DeckPilot's released-pins guard on the verified files; and the keys pinned in DeckPilot not refusing for the expected reason (none provisioned) |

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

## Held plugins: source on main, no release

A plugin whose manifest uses a plugin API 1.9 privileged kind (the
`tools`, `resources`, `projectContext` or `hostViews` permission,
`openUrl.fromParams`, `node.changesContext`, a `resource` parameter, a
variable host source, a host view, `statusItems`, `dashboardCards`:
`privileged_usages` in `tools/check-official.py`, the same usages
DeckPilot's `privileged.rs` reports) cannot have a release artifact while
the gate of DeckPilot's publisher signature is closed. The owner approved
keeping such a plugin's source on main as **held**: its entry in
`plugins.json` carries `held`, the reason as text.

| Step | A held plugin |
| --- | --- |
| `check` | allowed in source with its privileged usages; its permission policy, process allow-list and contract against DeckPilot's `plugins/first-party/<name>` apply as to any plugin; `held` is refused on a fixture, without a reason, or with no privileged usage (it stays valid with the gate open); prints each held plugin with its reason |
| `test`, `compare` | as any plugin |
| `package`, `fresh-build`, `repro` | the module is built (and reproducible); while the gate is closed `deckpilot-registry package --first-party` must **refuse** it for its privileged usages, and the step fails if it is accepted or refused for another reason; once the gate is open in the pinned DeckPilot and here, an accepted package is discarded |
| `sign-test` | skipped: there is no package |
| `resolve`, `release-build` (official-release.yml) | refused: not a released plugin |
| `official-entry.py` | refused: no Marketplace entry; the entry rules refuse privileged usages in any case, whatever the gate (privileged plugins reach users only bundled in DeckPilot) |

`tools/tests/test_check_official.py` holds the privileged-usage count of
each held manifest to the pinned Rust validator's. Removing `held` is a
reviewed change after the security review of DeckPilot's
`docs/marketplace/publisher-review-request.md` opens the gate here and in
DeckPilot. Until a held plugin is released, DeckPilot keeps bundling its
own build of the version it shipped.

## The reviewed privileged policy

`official/privileged-policy.json` is a byte copy of DeckPilot's
`crates/deckpilot-plugin-registry/publisher/privileged-policy.json`
(`check --deckpilot` compares them at the pinned commit): for each of the
five privileged plugins, the **one** reviewed version, its exact
`permissions` and every privileged usage with its JSON value, and the
security review (`review`, `null` until it is recorded). `check` refuses
an invalid policy (version 1, `minThreshold` at least 2, only those five,
each once, process execution only git for `deckpilot.git` and docker for
`deckpilot.docker`). With the gate open (`PRIVILEGED_GATE_OPEN = True`
here, `PRIVILEGED_GATE_OPEN = true` in DeckPilot, both reviewed changes),
a plugin that is no longer held must be **exactly** its entry, with an
approved review: a new construct, a broader permission, an extra
executable, another plugin or another version fails `check`, and the
pinned DeckPilot's `package --first-party`, `release sign` and `release
verify` refuse the same (DeckPilot `docs/marketplace/publisher-signature.md`,
"Privileged plugins"). A privileged plugin is never given a Marketplace
entry: it reaches users only bundled in DeckPilot.

| Plugin | Held for |
| --- | --- |
| `deckpilot.docker` 0.1.1 | the `tools` permission (docker only), the `resources` permission, `resource` action parameters |
| `deckpilot.git` 0.1.1 | the `tools` permission (git only), the `resources` permission, `resource` action parameters |
| `deckpilot.projects` 0.1.1 | the `projectContext`, `resources` and `hostViews` permissions, `node.changesContext`, `host:project.*` variable sources, the `projects` view, `statusItems`, `dashboardCards` |
| `deckpilot.workflows` 0.1.1 | the `hostViews` permission and the `workflows` host view |
| `deckpilot.workspace` 0.1.1 | the `resources` permission, `resource` action parameters, `openUrl.fromParams` |

## Releases

Who may authorize a release, and what the two keys do and do not prove:
[docs/release-governance.md](../docs/release-governance.md). DeckPilot has
one maintainer, so releases run under a **documented single-maintainer
exception** (2026-11-21): the owner is the sole release approver and the
release tag is the authorization (tag authority); both keys stay
mandatory (threshold 2, two signing jobs) and are both controlled by the
owner, which is **not** independent custody.

`official-release.yml`, for a tag `deckpilot.<name>--v<version>` that the
owner creates (tags are admin-only), only when the owner has set it up:

1. `resolve`: `OFFICIAL_RELEASE_ENABLED` is `true`; the tag names a released
   plugin at its manifest version, at a commit on `main`.
2. `gate`: `tools/check-environment.sh official-release` (tags
   `deckpilot.*--v*` only, no administrator bypass, and the run authorized
   by the admin-only tag ruleset *Protect tags* or by a required reviewer;
   the optional reviewer variables, if set, met) and every required check
   of `main` passed for the commit.
3. `checks` (no secret): checks, tests, the comparison with DeckPilot.
4. `build-a` and `build-b` (no secret): two independent builds of the tagged
   commit, each on its own fresh runner, with nothing run before them;
   module sha256, package sha256 and size.
5. `compare` (no secret): both builds agree on module and package; it
   hands on build-a's package sha256 and size.
6. `cli` (no secret) builds the pinned `deckpilot-registry` (DeckPilot code
   only) and hands it on as an artifact with its sha256.
7. `sign-a` and `sign-b` (environment `official-release`; deployments
   only from the tag, approved by its required reviewers if it lists any):
   **two publisher keys, one per job**. `sign-a` holds only `DECKPILOT_PUBLISHER_KEY_A` and its password, `sign-b` only
   `DECKPILOT_PUBLISHER_KEY_B` and its password; no runner ever has both.
   Each compiles nothing: it installs the `cli` job's binaries only if
   their sha256 match (`tools/cli-artifact.sh`) and re-checks build-a's
   package against `compare`'s sha256 and size. One step, the only one with
   the key, runs `release sign --id --version` with the tag's id and
   version (the package must say the same; every check that parses the
   package runs before the key is loaded, and the key never touches the
   disk); it refuses without its key and password, and with any other
   publisher secret in its environment. Each job uploads exactly one file,
   `<id>-<version>.dpplugin.<KEYID>.minisig` (KEYID: the minisign key id,
   16 uppercase hex, as minisign prints it), and reports its key id.
8. `verify` (no environment, no secret) gathers both files and requires
   exactly those two (the key ids `sign-a` and `sign-b` reported, distinct,
   each file named by the key id it carries), DeckPilot pinning a threshold
   of at least two distinct keys, and `release verify` with both
   signatures and the keys pinned in the DeckPilot build (never `--keys`),
   DeckPilot's rollback floor (`released-pins.json`) and revocations
   (`artifacts.json`), for each platform, with the audit log (two
   signers). Each signature alone must be refused.
9. `release`: the package and both signature files on the GitHub Release
   of the tag (a draft, all three assets, exactly those, then published;
   never a tag).

One key missing means no release: a signing job that fails or is skipped
stops `verify`, `verify` refuses one signature, and `release` needs
`verify`.

Every job that builds or runs `deckpilot-registry` builds it from
`DECKPILOT_COMMIT` only after checking that the commit is on DeckPilot's
`main` (`require_on_main` in `tools/lib/common.sh`, run by
`tools/build-cli.sh` and `tools/check-schemas.sh`): GitHub serves a fork's
commits under DeckPilot's URL, and such a commit would otherwise build as
the verifier that runs next to the key.

No job that holds a signing key compiles anything: `publish.yml`'s and
`_site.yml`'s registry signing jobs install the tools from a `cli` job the
same way, and `tools/check-workflows.py` refuses `build-cli.sh`, `cargo`,
`rustup`, `rustc`, `build-plugin.sh` and any `official.sh` step but the
workflow's one signing command (`release-sign` in `official-release.yml`,
`test-sign` in `official-test-sign.yml`, none elsewhere) in a job of
`registry-release`, `registry-timestamp` or `official-release`, so no build script of a locked dependency can run in,
or linger into, a step that gets a key.

`tools/check-workflows.py` also holds `official-release.yml` to the
two-key layout: each publisher secret only in its own signing job (names
compared case-insensitively), no job with both keys, no other
`DECKPILOT_PUBLISHER_KEY*` secret anywhere; a job holding a key names
`official-release`, needs `gate`, has the exact condition
`github.repository == 'knittingcode/deckpilot-plugins' && github.ref_type == 'tag' && startsWith(github.ref, 'refs/tags/deckpilot.') && contains(github.ref, '--v')`,
gives the key to one `run` step's env only (as the variable of the same
name), and compiles nothing; the workflow is triggered only by a push of
`deckpilot.<name>--v*` tags and an input-less `workflow_dispatch` (no
`pull_request`, `pull_request_target`, `workflow_run`, `workflow_call`,
schedule, inputs or event payloads); `verify` has no secret and needs both
signing jobs, and `release` needs `verify`.

The key test `official-test-sign.yml` is held to the same rules (its key
jobs' condition names its own tags,
`github.repository == 'knittingcode/deckpilot-plugins' && github.event_name == 'workflow_dispatch' && github.ref_type == 'tag' && startsWith(github.ref, 'refs/tags/deckpilot.publisher-test--v')`),
plus: `workflow_dispatch` only, no `release` job, no write permission, no
release command (`release-sign`, `release sign`, `release-upload`,
`gh release`), every artifact kept one day. Its `gate` and every key job
need what `official-release.yml`'s do; see
[Key test](#key-test-of-the-publisher-keys).

`tools/check-publisher-pin.py` (Repository hygiene, a required check)
fetches DeckPilot at `tools/deckpilot.env`'s commit (git only) and refuses
a `first-party.json` there with fewer than two distinct keys or a
threshold below 2, and any change that would make either `verify` job
compare against another file: both fetch the pinned DeckPilot and run
`release-verify` or `test-verify`, which read exactly that file and
require every signer to be one of its keys, and nothing passes `--keys`.

`publish.yml` (the registry) excludes these tags, and the key test's
(`deckpilot.publisher-test--v*`; `tools/resolve-tag.sh` refuses them too).

## What the owner must do before the first release

Not done by anyone yet; creating or rotating these secrets is the owner's:

1. Create the **two** publisher keys offline, each with its own strong
   password (under the default policy each by its own holder, on separate
   machines; under the single-maintainer exception both by the owner):
   `deckpilot-registry keys generate --role publisher --out publisher-a.key`
   (and `publisher-b.key`), or `minisign -G`; keep encrypted offline
   backups. Note each key id, 16 uppercase hex:
   `python3 -I tools/lib/minisign_keyid.py publisher-a.key.pub` (the same
   id the `.pub` file's comment `minisign public key: <KEYID>` names).
2. In the environment `official-release` (deployment tags
   `deckpilot.*--v*` only, administrators cannot bypass; under the
   single-maintainer exception no required reviewer, see
   [the settings](../docs/release-governance.md#settings-of-the-exception))
   add the secrets
   `DECKPILOT_PUBLISHER_KEY_A` (key A's secret key file text) and
   `DECKPILOT_PUBLISHER_KEY_A_PASSWORD`, `DECKPILOT_PUBLISHER_KEY_B` and
   `DECKPILOT_PUBLISHER_KEY_B_PASSWORD`. Nothing else holds them.
3. Leave `OFFICIAL_RELEASE_REQUIRE_REVIEWERS` and
   `OFFICIAL_RELEASE_REQUIRE_PREVENT_SELF_REVIEW` unset while there is one
   maintainer (set, they fail closed until a second maintainer is listed;
   [transition](../docs/release-governance.md#transition-to-team-governance)).
4. Pin both public keys with `threshold: 2` in DeckPilot's
   `crates/deckpilot-plugin-registry/publisher/first-party.json` in a
   reviewed pull request, then move `tools/deckpilot.env` here to that
   commit (`verify` refuses a DeckPilot pinning fewer than two keys or a
   lower threshold). **Done 2026-11-21**: key A `BDCDC80985EB60B5`, key B
   `8C3C4E42700FA702`, threshold 2 (DeckPilot
   `docs/marketplace/publisher-signature.md`, "Provisioning"). Both keys
   are held in one 1Password vault of one account, the owner's: no
   independent custody, an accepted risk of the single-maintainer
   exception; independent custody needs a second maintainer holding key B.
   Then run the [key test](#key-test-of-the-publisher-keys) once.
5. Have the security review of DeckPilot's
   `docs/marketplace/publisher-review-request.md` done (it also decides
   whether the privileged-kinds gate may open for Wave C).
6. Set the repository variable `OFFICIAL_RELEASE_ENABLED` to `true`.

Decisions only the owner can make (nothing here changes live settings):

- **Who may approve a signing run.** Decided 2026-11-21: the owner alone,
  by creating the tag (tag authority,
  [docs/release-governance.md](../docs/release-governance.md)).
  `check-environment.sh official-release` accepts the active, admin-only
  tag ruleset *Protect tags* as the authorization, or a required reviewer
  if one is listed. The repository variables
  `OFFICIAL_RELEASE_REQUIRE_REVIEWERS` (for example `2`: at least two listed
  reviewers) and `OFFICIAL_RELEASE_REQUIRE_PREVENT_SELF_REVIEW` (`true`:
  the environment's "Prevent self-review" must be on) are unset; they are
  for a second maintainer. GitHub then still waits for **one** approval,
  from a listed reviewer other than the person who started the run: one
  initiator and one approver, not two approvals. Set the environment
  first, then the variables.
- **Required checks.** The `main` ruleset requires six checks; *Official
  plugins* (the official/ pipeline and the two independent builds) is not
  among them. Adding *Build, test, check and package official plugins*,
  *Two independent builds are byte-identical* and *Build, test and package first-party plugins* to
  the required checks of the *Protect main* ruleset only adds conditions
  (nothing is weakened), and makes `check-required-checks.sh` hold every
  release to them. Changing a ruleset is the owner's.

## Key test of the publisher keys

`official-test-sign.yml` proves that the secrets stored in
`official-release` are the two keys DeckPilot pins and that their
passwords unlock them, without signing anything that could ever be a
release. It signs DeckPilot's fixed key test message of run `<n>`
(`deckpilot-registry release test-sign --run <n>`: a text that is not a
package, trusted comment `deckpilot-publisher-test v1 role:test run:<n>
sha256:... size:...`, another domain than a release's
`deckpilot-publisher v1 role:release ...`; the command takes no file to
sign), with key A in `sign-a` and key B in `sign-b`, each exactly as
`official-release.yml` signs (environment `official-release`, one key per
job, the prebuilt CLI installed after its sha256 check, nothing compiled).
`verify`, without any secret, checks that both key ids are in DeckPilot's
`first-party.json` at the pin, that `release test-verify` with the keys
compiled into the pinned build (threshold 2, never `--keys`) accepts both
and refuses each alone, and that `release verify` **refuses** the two
signatures as a release of every official plugin id and version
(DeckPilot's `a_key_test_signature_is_never_a_release_signature` tests the
same in the verifier). Only the two signatures (and the cli job's
binaries) are uploaded, as artifacts kept one day; nothing is released,
tagged or published, and no job has a write permission.

The environment's deployment policy is unchanged: `official-release`
deploys `deckpilot.*--v*` tags, which the key test's dedicated tag
`deckpilot.publisher-test--v<n>` matches. `publish.yml`, `resolve-tag.sh`
and `official-release.yml` never act on it.

**Owner steps (each run):**

1. Prerequisites, as for a release: the four secrets in `official-release`
   (present), and the gate's settings met: under the single-maintainer
   exception no required reviewer and the reviewer variables unset, the
   tag being authorized by *Protect tags* (which covers
   `deckpilot.publisher-test--v<n>`).
2. Pick the next unused run number `<n>` (1 to 9999; `git ls-remote --tags
   origin 'deckpilot.publisher-test--v*'`) and create the tag on the
   current `main` (tags are admin-only):
   ```sh
   git fetch origin main
   git tag deckpilot.publisher-test--v<n> origin/main
   git push origin deckpilot.publisher-test--v<n>
   ```
   Pushing it starts nothing.
3. Start the key test on that tag:
   `gh workflow run official-test-sign.yml --ref deckpilot.publisher-test--v<n>`
4. When `gate` passes, `sign-a` and `sign-b` start (with a required
   reviewer listed, they first wait for its approval).
5. `verify` passes and its summary names both key ids, DeckPilot's
   commit and the sha256 of `first-party.json`. A failure in `sign-a` or
   `sign-b` means that secret or its password is wrong; a failure in
   `verify` that a stored key is not a pinned key.
6. Optional: delete the tag afterwards
   (`git push origin :refs/tags/deckpilot.publisher-test--v<n>`); use a
   new `<n>` for the next run.

**What a pass proves:** each stored secret key decrypts with its stored
password and is one of the two keys pinned in DeckPilot at
`tools/deckpilot.env`'s commit, and the two are distinct (threshold 2).
It proves nothing about custody (both keys are in one vault of one
account, the owner's: not independent custody) and releases nothing.

Then the first plugin moves (Wave B): its source under `official/<name>`
at a new patch version, listed in `plugins.json` with its permissions; a
release tag; and in DeckPilot its `artifacts.json` entry.
