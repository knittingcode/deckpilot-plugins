# Official plugins in the Marketplace

How a signed release of an official plugin (one DeckPilot bundles,
[`official/`](../official/README.md)) becomes a Marketplace entry, what has
to be true first, and how it is tested. Current state of every plugin:
[official-compatibility-matrix.md](official-compatibility-matrix.md).

> **Status: prepared, not live.** No official release exists (no publisher
> key is pinned in DeckPilot), and DeckPilot's agent refuses every bundled id
> from any registry (`is_bundled_id`, X12 in DeckPilot). Until DeckPilot
> changes that in a reviewed release, an entry is only a **candidate** under
> `official/marketplace/`: committed and checked, never copied into
> `registry/` and never in the signed index.

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
3. **Assets**: exactly `<id>-<v>.dpplugin` and its `.minisig`
   signature(s); each downloaded file's size and sha256 equal to what
   GitHub reports for the asset.
4. **Signature and compatibility**: `deckpilot-registry release verify`
   built from the pinned DeckPilot commit, with the publisher keys
   compiled into it (never `--keys`), DeckPilot's `released-pins.json` as
   rollback floor and `artifacts.json` revocations, for macOS, Windows and
   Linux. The checkout must be exactly `DECKPILOT_COMMIT`.
5. **Manifest**: the claimed id and version; `engines` satisfied by the
   pinned DeckPilot (`DECKPILOT_VERSION`, `DECKPILOT_PLUGIN_API`); the
   permission kinds of `plugins.json`; no plugin API 1.9 privileged kind
   (`tools`, `resources`, `projectContext`, `hostViews`,
   `openUrl.fromParams`) while the gate is closed; process execution only
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
record against its release (asset URL, sha256, size, tag, commit, path),
the gate, dependencies, and that the matrix is current.

## Procedure after an official release

Once `official-release.yml` has published `deckpilot.<name>--v<version>`
(package and `.minisig` on its GitHub Release):

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

**Promotion into the signed index is not possible yet** and needs, in this
order: (1) a publisher key pinned in a DeckPilot release: the agent
(knittingcode/deckpilot#378) takes a registry copy of an official plugin,
a bundled one included, only as a release signed by the publisher keys
compiled into it and verified before install, and refuses it before any
download while no key is pinned; (2) `MARKETPLACE_SERVES_BUNDLED` and a promotion step
here, plus `check-submission.py` and `check-first-party.py` allowing those
ids and `publish.yml` publishing an index for an official tag without
rebuilding or re-uploading the asset (its `build` job rebuilds from a
`plugins/<id>/build.json` recipe, which official plugins do not have); (3)
the production registry key set authorizing `deckpilot`. None of these is
done here.

## Tests

| What | Where | Runs |
| --- | --- | --- |
| Rules without artifacts: semver and ranges, ids, release state, manifest compatibility, permission policy and the privileged gate (each kind), dependency resolution (candidate, bundle, unmet, optional, community, built-in module, highest fitting), order (downgrade, pre-release), committed-candidate checks | `tools/tests/test_official_entry.py` | `official.yml`, no build |
| End to end with fixture packages (`deckpilot.example-default` 0.0.9/0.1.0/0.1.1, a dependent fixture) signed with a **throwaway** publisher key made in the test: refusals (draft, unpublished, off main, other tag, pre-release flag, missing signature or package, extra asset, size and sha256 mismatch, tampered bytes, foreign key, incompatible host, unmet dependency, downgrade, rewrite, ids and flags outside test mode, writing into `official/marketplace/`); generation (record = release asset, sha256, size, source; provenance; idempotence); then the candidates in a registry signed with throwaway keys: `verify-site --require-packages` accepts it (**discovery**: listed with name and category; **update offer**: `latest` 0.1.1 over 0.1.0; **dependencies** in the signed detail) | `tools/tests/test-official-marketplace.sh` | `official.yml`, after `package` |

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
