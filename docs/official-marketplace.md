# Official plugins in the Marketplace

How a signed release of an official plugin (one DeckPilot bundles,
[`official/`](../official/README.md)) becomes a Marketplace entry, what has
to be true first, and how it is tested. Current state of every plugin:
[official-compatibility-matrix.md](official-compatibility-matrix.md).

> **Status: live for DeckPilot 0.11.0 and later.** DeckPilot 0.11.0 pins
> the two production publisher keys (threshold 2) and installs an official
> plugin, a bundled one included, from a registry only as a release they
> signed, verified before install (knittingcode/deckpilot#378, the narrowed
> X12 rule). The official plugins without a privileged kind
> (`deckpilot.github`, `deckpilot.gitlab`, `deckpilot.jira`,
> `deckpilot.notifications`) are promoted from their candidate entries under
> `official/marketplace/` into `registry/` and the signed index. The five
> privileged ones (`deckpilot.workflows`, `deckpilot.git`,
> `deckpilot.docker`, `deckpilot.projects`, `deckpilot.workspace`) are
> bundled in DeckPilot only: `generate` and `promote` refuse them, and so do
> the submission rules and the signing gate.

## What an official entry is

The same documents as any `deckpilot.*` registry entry
([publishing.md](publishing.md#first-party-plugins-deckpilot-owner-only)),
derived from the released artifact and nothing else:

| Part | Source |
| --- | --- |
| `official/marketplace/<id>/plugin.json` (listing) | the verified package: English `name` and `description` (resolved through `locales/en.json` for `t:` keys), `categories`, `license`; `homepage` `…/tree/main/official/<name>`; `source_repository` this repository |
| `versions/<v>.json` (version record) | `deckpilot-registry add-version --first-party` on the verified package, in a scratch workspace under a throwaway key set authorizing `deckpilot` (as *Validate registry* re-derives records): engines (host range, plugin API, SDK), platforms, permissions and dependencies byte-for-byte from the manifest, `manifest_sha256`, `package` = the release asset URL `…/releases/download/<id>--v<v>/<id>-<v>.dpplugin` with its sha256 and size, `source` = the release tag, its commit and `official/<name>`, `published_at` = the release's |
| `versions/<v>.release.json` (provenance) | the release URL and commit, package and signature digests, the verification (`keys: pinned`, signer key ids, host version, plugin API and DeckPilot commit it was checked for, platforms), the localized names and descriptions (`en`, and `pl` when the package has it), how each dependency resolved |

Publisher: `deckpilot` (`registry/publishers/knittingcode.json`, verified).
Update metadata is the version list itself: the client offers the highest
compatible, non-yanked version above the installed one.

## Refusals (`tools/official-entry.py generate`)

Before anything is written:

1. **Id**: one of DeckPilot's `BUNDLED_PLUGIN_IDS` *and* released from
   `official/` (`official/plugins.json` `plugins`). Community and
   installable first-party plugins (`first-party/`, Pomodoro, Memory,
   Markets, Break Reminder, Air Quality, Weather) are untouched by this
   tool.
2. **Release**: tag `deckpilot.<name>--v<version>`, published (not a
   draft), of this repository, at a commit on `main`, pre-release exactly
   when the version is one.
3. **Assets**: exactly `<id>-<v>.dpplugin` and its two signatures
   `<id>-<v>.dpplugin.<KEYID>.minisig` (KEYID: the minisign key id, 16
   uppercase hex), each file carrying the key id it is named by, from two
   distinct keys; one signature, a third, a duplicate or the old single
   `<id>-<v>.dpplugin.minisig` is refused; each downloaded file's size and
   sha256 equal to what GitHub reports for the asset.
4. **Signatures and compatibility**: `deckpilot-registry release verify`
   built from the pinned DeckPilot commit, with both signatures and the
   publisher keys compiled into it (never `--keys`), which must pin a
   threshold of at least two; DeckPilot's `released-pins.json` as rollback
   floor and `artifacts.json` revocations, for macOS, Windows and Linux;
   the audit must name two distinct signers. The checkout must be exactly
   `DECKPILOT_COMMIT`. The release record keeps both signature files and
   both signer key ids (`verification.signerKeyIds`).
5. **Manifest**: the claimed id and version; `engines` satisfied by the
   pinned DeckPilot (`DECKPILOT_VERSION`, `DECKPILOT_PLUGIN_API`); the
   permission kinds of `plugins.json`; no plugin API 1.9 privileged kind
   (`tools`, `resources`, `projectContext`, `hostViews`,
   `openUrl.fromParams`), whatever the gate (privileged plugins are
   bundled in DeckPilot only, never a Marketplace entry); process execution only
   git for `deckpilot.git`, docker for `deckpilot.docker`.
6. **Dependencies**: a built-in module (matched against DeckPilot's
   version) or an official plugin, nothing else. A required one must be
   satisfied by a candidate entry or by the version the pinned DeckPilot
   bundles; an optional one is recorded either way.
7. **Order**: strictly above every candidate version of the plugin; the
   same version again must give the identical entry (`unchanged`),
   anything else is refused (append-only).

`tools/official-entry.py check` (in `official.yml`) re-checks every
committed candidate: id, `keys: pinned` (a test-key record is refused),
two signatures by distinct key ids and the same `signerKeyIds`, two
signers, record against its release (asset URL, sha256, size, tag, commit, path),
the gate, dependencies, and that the matrix is current.

## Procedure after an official release

Once `official-release.yml` has published `deckpilot.<name>--v<version>`
(the package and its two `.<KEYID>.minisig` signatures on its GitHub
Release):

```sh
git fetch origin && git switch -c feat/marketplace-<name>-<version> origin/main
tools/build-cli.sh                       # the pinned verifier and DeckPilot checkout
d=$(mktemp -d)/bundle                    # a new, empty directory
tools/official-entry.py fetch --tag deckpilot.<name>--v<version> --out "$d"
tools/official-entry.py generate --bundle "$d"
tools/official-entry.py matrix
tools/official-entry.py check
git add official/marketplace docs/official-compatibility-matrix.md
git commit -m "feat(marketplace): official entry for deckpilot.<name> <version>"
```

`generate --dry-run` prints the record without writing. A refusal names the
reason; there is no override. After a change of `tools/deckpilot.env` or
when a plugin moves to `official/`, run `matrix` again (the check fails
otherwise).

### Promotion into the signed index

What it rests on, all in place since DeckPilot 0.11.0: (1) the publisher
keys pinned in a DeckPilot release, whose agent takes an official plugin
only as a release signed by them; (2) the production registry key set
(`keys/1.json`, root-signed in the 2026-11-17 ceremony with
`--first-party`) authorizing `deckpilot` in `first_party.namespaces`, the
verified `deckpilot` publisher record and the registry roots embedded in
DeckPilot (`keys/roots.json` = DeckPilot's
`vars.DECKPILOT_REGISTRY_ROOT_PUBKEYS`); (3) the steps here:

```sh
tools/official-entry.py promote --id deckpilot.<name>   # into registry/plugins/<id>/
tools/official-entry.py matrix && tools/official-entry.py check
git add registry/plugins/deckpilot.<name> official/marketplace docs/official-compatibility-matrix.md
```

then, after the pull request is merged, a `registry--v<date>.<n>` tag on
the merged commit (publish.yml signs and deploys a new index; the release
asset already exists, so nothing is uploaded).

* **The registry record** is the candidate's record byte for byte, except
  `engines.deckpilot`: its lower bound becomes `OFFICIAL_REGISTRY_MIN_HOST`
  (0.11.0), e.g. `>=0.6.0 <1.0.0` becomes `>=0.11.0 <1.0.0`. An older
  DeckPilot would refuse the install after offering it (0.10.0 refuses every
  bundled id from a registry; before that, no publisher signature is
  checked), so it must see the version as incompatible and offer nothing:
  Marketplace offers and update candidates take only versions whose
  `engines` the host satisfies. The package and its signed manifest are
  unchanged; the installer compares the record's permissions, dependencies
  and digests with the package, not its engines, and still checks the
  manifest's own engines.
* **Refusals of `promote`**: a plugin in `BUNDLED_ONLY_IDS` or whose policy
  allows a privileged kind, a held or non-official id, no candidate, a
  registry version that differs from what it would write (recorded
  versions are never overwritten), a version not above every recorded one.
* **`gate`** (in `check`, `validate.sh` and `publish-site.sh` before any
  key is written): every `registry/plugins/<bundled id>` is an eligible
  plugin whose listing is the candidate's and each version exactly its
  candidate's registry record.
* **No rebuild**: an official plugin has no `plugins/<id>/build.json`
  recipe (the submission rules refuse one). `validate.sh` (and so the
  unprivileged `build` job of `publish.yml`) instead downloads the release
  asset and both signatures (`official-entry.py package`), checks them
  against the committed release record, runs `release verify` with the
  pinned keys for every platform again, re-derives the record with
  `add-version` (it must equal the candidate byte for byte) and hands the
  verified package to the signing job, which checks sha256 and size against
  the record as for every package.

## Tests

| What | Where | Runs |
| --- | --- | --- |
| Rules without artifacts: semver and ranges, ids, release state, release assets (two signatures by distinct key ids; one, three, a duplicate, the single-key name, a lower-case key id, a non-minisign file refused), manifest compatibility, permission policy and the privileged gate (each kind), dependency resolution (candidate, bundle, unmet, optional, community, built-in module, highest fitting), order (downgrade, pre-release), committed-candidate checks | `tools/tests/test_official_entry.py` | `official.yml`, no build |
| End to end with fixture packages (`deckpilot.example-default` 0.0.9/0.1.0/0.1.1, a dependent fixture) signed with **two throwaway** publisher keys made in the test (threshold 2): refusals (draft, unpublished, off main, other tag, pre-release flag, missing signature or package, one signature, a duplicated signature, the single-key name, test keys with threshold 1, extra asset, size and sha256 mismatch, tampered bytes, foreign key, incompatible host, unmet dependency, downgrade, rewrite, ids and flags outside test mode, writing into `official/marketplace/`); generation (record = release asset, sha256, size, source; provenance with both signer key ids; idempotence); then the candidates in a registry signed with throwaway keys: `verify-site --require-packages` accepts it (**discovery**: listed with name and category; **update offer**: `latest` 0.1.1 over 0.1.0; **dependencies** in the signed detail) | `tools/tests/test-official-marketplace.sh` | `official.yml`, after `package` |

Fixture ids are used because a client may take them from a registry; the
bundled ids it refuses (X12). Throwaway keys never leave the scratch
directory and nothing is uploaded.

### Integration plan once DeckPilot serves official entries

Not automatable until step (1) above exists; to run against a DeckPilot
build with that change and a test registry (DeckPilot's
`apps/agent/tests/plugins_e2e.rs` harness, throwaway roots):

1. **Discovery**: an official entry appears in Marketplace search and its
   category, with the localized name (`en`, `pl`) from the package.
2. **Update offer**: bundled `deckpilot.github` 0.2.0 seeded; the registry
   offers 0.2.1; Marketplace shows Update; installing keeps settings and
   credentials; the bundled seeding never moves it back.
3. **Downgrade refusal**: a registry offering only a version at or below
   the bundled or installed one offers no update; `released-pins.json`
   floor refuses it in `release verify`.
4. **Dependency resolution**: `deckpilot.workspace` (optional
   `deckpilot.projects`) plans without Projects, and with it when selected;
   a required dependency above the bundled version is planned from the
   registry first.
5. **Permissions**: the consent dialog shows exactly the entry's
   permissions; medium tier and above wait for consent, as for a bundled
   copy.
6. **Gate**: no entry for a plugin with a privileged kind exists, and the
   client refuses one served anyway (first-party rules).
7. **Withdrawal**: yank and `revoke --action disable` of an official
   version, and `keyset rotate --no-first-party`, behave as for
   `first-party/` plugins.
