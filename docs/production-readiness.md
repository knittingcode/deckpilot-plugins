# Production readiness: owner checklist

This is what the owner has to do, in order, before the first production
publication of the registry. Only the owner (the `knittingcode` account) can
do any of it. Nothing on this list has been done yet. Today the repository
has no keys, no secrets, no `registry-*` environments, no repository
variables, no tags and no releases. Pages serves only the status page
(`/v1/README.txt`).

The list was checked against this repository's workflows (`publish.yml`,
`_site.yml`, `pages.yml`, `refresh-timestamp.yml`) and `tools/`. It was
also checked against these DeckPilot documents at the pinned commit
(`tools/deckpilot.env`):

- [`crates/deckpilot-plugin-registry/PUBLISHING.md`](https://github.com/knittingcode/deckpilot/blob/main/crates/deckpilot-plugin-registry/PUBLISHING.md)
  (commands, key ceremony, environments);
- [`docs/marketplace/distribution.md`](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/distribution.md)
  (the separation from application releases);
- the marketplace design `docs/marketplace/architecture.md` (DeckPilot pull
  request #191; open decisions in its section 20);
- `docs/marketplace/module-migration.md` (B6, B7).

Throughout:

- `R=knittingcode/deckpilot-plugins`;
- `CLI` is `deckpilot-registry` built by `tools/build-cli.sh` from the
  DeckPilot commit pinned in `tools/deckpilot.env`. Use the same pin for
  the ceremony as the one on `main` at the time.

Each step says where it happens:

- **[offline]**: an air-gapped machine;
- **[local]**: the owner's normal machine;
- **[GitHub]**: repository settings or `gh`;
- **[DeckPilot]**: the application repository.

## 0. Decisions to take first

These open decisions change what the ceremony and the first publication
look like. The recommendations come from the design. "Blocks" means the
step below cannot be done as written until the decision is taken.

| Id | Decision | Recommendation | Blocks |
| --- | --- | --- | --- |
| U-10 | Root threshold: 1-of-2 or 2-of-2 | 1-of-2 in v1 | §2 (how many root keys, `--root-threshold`) |
| U-12 | Timestamp role, or an index signed monthly for 30 days | Timestamp role | §2 to §5. The workflows implement only the timestamp role (`refresh-timestamp.yml`, environment `registry-timestamp`). Choosing the 30-day index means changing the workflows first. |
| U-11 | If reproducibility (spike S6) fails: accept "the registry's own trusted job builds it, without three-way hash agreement" | Don't launch without reproducibility | §6, the first publication. Today CI's rebuild must match the committed record byte for byte, but a contributor may copy that record from the CI summary. That is two-way agreement, not three-way. S6 is still open in DeckPilot (`docs/marketplace/spike-findings.md`). |
| U-8 | Single maintainer: PR-only admin bypass, or a second maintainer | A second maintainer; bypass only for revocations | §3 (who reviews `registry-publish`). The ruleset uses the PR-only admin bypass today. Until U-8 is decided, keep "Prevent self-review" off on `registry-publish`, or the only reviewer cannot approve. |
| B6 | Reserve the `deckpilot` publisher in the registry | (open) | Registry publication of first-party plugins. Not needed for the first test publication. Today, *Submission rules* refuses `deckpilot` (and every other id DeckPilot reserves) in community pull requests only, and DeckPilot itself refuses `deckpilot.*` in registry packages: `verify_package` runs `Manifest::validate`, which treats `deckpilot` as reserved. Only bundled plugins go through `validate_first_party`. Publishing a `deckpilot.*` plugin here therefore needs this decision **and** a DeckPilot change. |
| B7 | When a legacy built-in implementation may be removed (module-migration §5.7) | (open) | Not the first publication. It decides when a registry-delivered first-party plugin can replace a built-in module (migration step P12). |
| U-1 | Engine build: Javy plugin API or an own rquickjs build | Decide by size, start-up and maintenance cost | Not the first publication. The registry accepts only ABI 1 WebAssembly modules built from Rust (`schemas/inputs/build-recipe.schema.json`). U-1 decides whether and how JavaScript plugins come later, and with them a new recipe kind. |

## 1. Repository settings without secrets [GitHub]

1. Enable **immutable releases** (Settings › General › Releases). It is
   off today. `tools/upload-release.sh` creates a draft, uploads the asset
   and only then publishes, so it works with immutable releases on.
2. Leave as they are (checked 2026-11-16):
   - fork pull request approval for all external contributors;
   - `sha_pinning_required`;
   - default `GITHUB_TOKEN` permissions read-only, and it cannot approve
     pull requests;
   - squash merges only;
   - the rulesets "Protect main" (the six required checks, code-owner
     review, PR-only admin bypass) and "Protect tags" (only the admin
     creates, moves or deletes tags).

## 2. Key ceremony

### 2.1 Rehearsal with throwaway keys [local]

Run steps 2.2 to 2.4 once in a scratch directory, with passwords you throw
away. Then run `tools/validate.sh registry` on a scratch branch with the
resulting `keys/` files copied in. Its step 4 walks the key set chain with
DeckPilot's client rules. This proves that the minisign version and the
public key encoding work before real keys exist. Delete the scratch
directory afterwards; never commit or push it.

### 2.2 Prepare [local, then offline]

On the local machine:

```sh
git clone https://github.com/knittingcode/deckpilot-plugins && cd deckpilot-plugins
tools/build-cli.sh                      # .work/bin/deckpilot-registry at the pinned commit
shasum -a 256 .work/bin/deckpilot-registry
```

1. Copy `.work/bin/deckpilot-registry` to the offline machine (same OS and
   architecture), together with a verified
   [minisign](https://github.com/jedisct1/minisign) binary.
2. On the offline machine, check the CLI's sha256 against the one above.

Production keys are made with **minisign**, not with `CLI keys generate`.
DeckPilot's `publish/keys.rs` says `keys generate` is for local and test
registries only.

### 2.3 Keys [offline]

Each key gets its own strong password (minisign prompts for it; it is
scrypt-encrypted). This makes two roots for U-10's 1-of-2; for 2-of-2,
use the same two keys with `--root-threshold 2`.

```sh
umask 077
mkdir -p ceremony/secret ceremony/public ceremony/ws && cd ceremony
minisign -G -p public/root-a.pub      -s secret/root-a.key
minisign -G -p public/root-b.pub      -s secret/root-b.key
minisign -G -p public/index-1.pub     -s secret/index-1.key
minisign -G -p public/timestamp-1.pub -s secret/timestamp-1.key
# The registry documents take the base64-wrapped minisign public key
# (the DECKPILOT_UPDATER_PUBKEY format):
for k in root-a root-b index-1 timestamp-1; do
  base64 < "public/$k.pub" | tr -d '\n' > "public/$k.b64"
done
```

### 2.4 First key set [offline]

```sh
CLI=/path/to/deckpilot-registry
"$CLI" keyset init --dir ws \
  --root public/root-a.b64 --root public/root-b.b64 --root-threshold 1 \
  --index public/index-1.b64 --timestamp public/timestamp-1.b64
# One signature per root; for 1-of-2 one is enough, both are better.
read -rs ROOT_PW && export ROOT_PW
"$CLI" keyset sign --dir ws --key secret/root-a.key --password-env ROOT_PW
read -rs ROOT_PW && export ROOT_PW
"$CLI" keyset sign --dir ws --key secret/root-b.key --password-env ROOT_PW
unset ROOT_PW
```

`keyset sign` prints how many of the root thresholds are met; it must say
complete. The result:

- `ws/roots.json`: the root role;
- `ws/site/v1/keys/1.json`: key set 1 (root, index and timestamp keys;
  `seq_floor`);
- `ws/site/v1/keys/1.json.minisig`: its root signatures, as a JSON array.

### 2.5 Where every file goes

| File | Goes to | Never |
| --- | --- | --- |
| `ws/roots.json` | `keys/roots.json` in this repository (by pull request, §2.6). Also DeckPilot's trusted roots, §5. | — |
| `ws/site/v1/keys/1.json` | `keys/1.json` (by pull request). `tools/publish-site.sh` deploys it to `/v1/keys/1.json`. | changed after it is published (key sets are immutable) |
| `ws/site/v1/keys/1.json.minisig` | `keys/1.json.minisig` (by pull request) | — |
| `secret/root-a.key`, `secret/root-b.key` and their passwords | offline only: the ceremony machine and one offline backup each, ideally in two places | GitHub, any online machine |
| `secret/index-1.key` and its password | `REGISTRY_INDEX_KEY` and `REGISTRY_INDEX_KEY_PASSWORD` of `registry-publish` (§3.1); an encrypted offline backup | this repository; `knittingcode/deckpilot` |
| `secret/timestamp-1.key` and its password | `REGISTRY_TIMESTAMP_KEY` and `REGISTRY_TIMESTAMP_KEY_PASSWORD` of **both** `registry-publish` and `registry-timestamp` (§3.1, §3.2); an encrypted offline backup | this repository; `knittingcode/deckpilot` |
| `public/*.pub`, `public/*.b64` | not needed in the repository: `roots.json` and `1.json` hold them | — |

Move only the two **encrypted** online key files (`index-1.key`,
`timestamp-1.key`) to the machine that sets the secrets, and delete them
there once §3 is done.

### 2.6 Commit the public trust material [local, GitHub]

```sh
git switch -c chore/key-set-1 origin/main
cp ceremony/ws/roots.json keys/roots.json
cp ceremony/ws/site/v1/keys/1.json keys/1.json
cp ceremony/ws/site/v1/keys/1.json.minisig keys/1.json.minisig
python3 -I tools/check-hygiene.py    # only public material in keys/
tools/build-cli.sh && tools/validate.sh registry   # step 4 walks the committed chain
```

Open the pull request and wait for all six required checks. *Validate
registry* runs "the committed key set chain verifies". Then merge.

After the merge, `pages.yml` still deploys only the status page: no live
registry exists yet.

## 3. Environments and secrets [GitHub]

The owner's numeric id is `109468044` (`gh api users/knittingcode -q .id`).
Set every secret through `gh`'s prompt or from a file; never on a command
line, because it ends up in shell history.

### 3.1 `registry-publish` (publication, release tags only)

1. Create the environment:
   - deployment branches and tags: **selected**, with exactly one tag rule
     `*--v*` and no branches;
   - required reviewer: `knittingcode`;
   - "Prevent self-review" off while U-8 is open;
   - no wait timer, and administrators cannot bypass.
2. Set the secrets: `REGISTRY_INDEX_KEY`, `REGISTRY_INDEX_KEY_PASSWORD`,
   `REGISTRY_TIMESTAMP_KEY`, `REGISTRY_TIMESTAMP_KEY_PASSWORD`. Each key
   is the full text of the minisign secret key file.

```sh
gh api -X PUT repos/$R/environments/registry-publish --input - <<'JSON'
{ "reviewers": [{ "type": "User", "id": 109468044 }], "prevent_self_review": false,
  "can_admins_bypass": false,
  "deployment_branch_policy": { "protected_branches": false, "custom_branch_policies": true } }
JSON
gh api -X POST repos/$R/environments/registry-publish/deployment-branch-policies -f name='*--v*' -f type=tag
gh secret set REGISTRY_INDEX_KEY     --env registry-publish -R $R < index-1.key
gh secret set REGISTRY_INDEX_KEY_PASSWORD     --env registry-publish -R $R   # prompts
gh secret set REGISTRY_TIMESTAMP_KEY --env registry-publish -R $R < timestamp-1.key
gh secret set REGISTRY_TIMESTAMP_KEY_PASSWORD --env registry-publish -R $R   # prompts
```

Used by `publish.yml`, job `sign` (`tools/publish-site.sh index`). That job
builds nothing and takes only this run's packages, after re-checking their
sha256 and size.

### 3.2 `registry-timestamp` (daily refresh, `main` only)

1. Create the environment:
   - deployment branches: **selected**, with exactly the branch rule
     `main`;
   - no reviewer, because it runs daily.
2. Set the secrets `REGISTRY_TIMESTAMP_KEY` and
   `REGISTRY_TIMESTAMP_KEY_PASSWORD` only. It never holds the index key.

```sh
gh api -X PUT repos/$R/environments/registry-timestamp --input - <<'JSON'
{ "deployment_branch_policy": { "protected_branches": false, "custom_branch_policies": true } }
JSON
gh api -X POST repos/$R/environments/registry-timestamp/deployment-branch-policies -f name=main -f type=branch
gh secret set REGISTRY_TIMESTAMP_KEY --env registry-timestamp -R $R < timestamp-1.key
gh secret set REGISTRY_TIMESTAMP_KEY_PASSWORD --env registry-timestamp -R $R   # prompts
```

Used by `_site.yml`, job `timestamp` (`tools/publish-site.sh timestamp`).
`refresh-timestamp.yml` calls it daily, and so does `pages.yml` when it is
dispatched with `refresh-timestamp`.

### 3.3 `github-pages` (exists; add the tag rule)

Pages created this environment with the single branch rule `main`.
`publish.yml` deploys from the release tag, so add the tag rule `*--v*` and
keep `main`. Without it, the `deploy` job of the first publication is
refused after the release asset has been uploaded.

```sh
gh api -X POST repos/$R/environments/github-pages/deployment-branch-policies -f name='*--v*' -f type=tag
gh api repos/$R/environments/github-pages/deployment-branch-policies -q '.branch_policies[] | .name + " " + .type'
# main branch
# *--v* tag
```

### 3.4 Check

```sh
gh api repos/$R/environments -q '.environments[].name'   # github-pages, registry-publish, registry-timestamp
gh secret list -R $R                                     # empty: no repository-level secrets
gh secret list --env registry-publish -R $R              # the four REGISTRY_* names
gh secret list --env registry-timestamp -R $R            # the two REGISTRY_TIMESTAMP_* names
```

## 4. Repository variables [GitHub]

Set these last, after §2 and §3:

```sh
gh variable set REGISTRY_PUBLISH_ENABLED   --body true -R $R
gh variable set REGISTRY_TIMESTAMP_ENABLED --body true -R $R
```

- `REGISTRY_PUBLISH_ENABLED`: the `resolve` job of `publish.yml` refuses
  to run without it.
- `REGISTRY_TIMESTAMP_ENABLED`: the `timestamp` job of `_site.yml` is
  skipped without it, and never touches `registry-timestamp`.

Turning a variable off again is the quickest way to stop publication or
refreshing. `tools/publish-site.sh` also refuses to sign:

- without the secrets;
- without `keys/roots.json` and `keys/1.json.minisig`.

## 5. DeckPilot's side [DeckPilot]

These are changes to the application repository. They need the owner's
authorization there and are not made from here.

1. **Trusted roots.**
   - At the pinned commit, the agent embeds **no** root keys. Its only
     source of roots is `--plugin-registry-roots <roots.json>`, or
     `DECKPILOT_PLUGIN_REGISTRY_ROOTS`.
   - `DECKPILOT_REGISTRY_ROOT_PUBKEYS` is named in `PUBLISHING.md`,
     `distribution.md` and the client's docs, but no build step reads it
     yet.
   - Before end users can use the registry, two things are needed:
     - a DeckPilot change that embeds the roots at build time from
       `DECKPILOT_REGISTRY_ROOT_PUBKEYS`;
     - that repository variable in `knittingcode/deckpilot`, set to the
       content of `keys/roots.json`. It is public; it is not a secret.
   - The first test publication (§6) does not wait for this: it passes
     `keys/roots.json` with `--plugin-registry-roots`.
2. **Default URL.** With roots given and no other registry named, the
   agent uses `https://knittingcode.github.io/deckpilot-plugins/`
   (`DEFAULT_REGISTRY_URL`, DeckPilot #229). Nothing needs configuring.
3. **Plugins in release builds.** The plugin backend is the cargo feature
   `plugins`, off in every release build, and even then it needs
   `--enable-plugins`. Shipping it to users is a separate DeckPilot
   decision (the design's release criteria R-1 to R-9 in section 19).
4. **Root rotation** is the one registry event that needs an application
   release: clients must learn the new roots. Plan the embedding so that
   a rotation is possible.
5. No registry key, secret or environment ever goes into
   `knittingcode/deckpilot` (`distribution.md`).

## 6. First test publication

The first publication permanently records a version: versions are
append-only, and a published key set is immutable. Use a sample plugin
that may stay listed, or yank it afterwards (step 7).

1. **Choose the sample and its publisher** [local].
   - The design's P12 acceptance asks for "a test plugin from a test
     publisher". So submit from a separate, non-maintainer GitHub account.
     That way *Submission rules* is exercised for a contributor too.
   - Its namespace must not be reserved. DeckPilot refuses `deckpilot`,
     `knittingcode` and the other reserved ids in every package, so even
     the owner cannot use them here.
   - The source is a public repository with a tag, e.g. DeckPilot's
     `hello-weather` sample, with its manifest's `id` and `publisher.id`
     changed to the test namespace.
2. **Submit** it as `CONTRIBUTING.md` describes:
   - the publisher record;
   - `plugin.json`, `build.json` and `manifest.json`;
   - the version record from the pinned tools.

   Review it with [review-policy.md](review-policy.md) and merge it once
   the six checks are green.
3. **Tag and publish** [local]:

   ```sh
   git fetch origin
   git tag <ns>.<name>--v<version> origin/main
   git push origin <ns>.<name>--v<version>
   gh run watch -R $R "$(gh run list -R $R -w publish.yml -L 1 --json databaseId -q '.[0].databaseId')"
   ```

   Approve the `registry-publish` deployment when *Publish* asks for it.
   Every job must succeed: `resolve`, `build`, `sign`, `release`,
   `deploy` and `verify-published`. To retry on the same tag:

   ```sh
   gh workflow run publish.yml -R $R --ref <tag>
   ```
4. **Verify the live site independently** [local], as a fresh client
   with every package downloaded and checked against its pins:

   ```sh
   tools/build-cli.sh
   curl -fsS -o .work/live-timestamp.json https://knittingcode.github.io/deckpilot-plugins/v1/timestamp.json
   tools/verify-published.sh https://knittingcode.github.io/deckpilot-plugins .work/live-timestamp.json
   # runs: deckpilot-registry verify-site --roots keys/roots.json --require-packages --min-remaining 6d
   ```
5. **Install it with a real DeckPilot agent** [local, DeckPilot checkout].
   Use a fresh data directory, which is a first-use client. The steps
   follow DeckPilot's `docs/marketplace/local-testing.md`, sections 1 and
   3, with the official URL instead of a local registry:

   ```sh
   cargo build -p deckpilot-agent --features plugins
   (cd plugins/runtime && cargo build --locked -p deckpilot-plugin-host --bin deckpilot-plugin-host)
   export DECKPILOT_DATA_DIR=$(mktemp -d)
   export DECKPILOT_ENABLE_PLUGINS=1
   export DECKPILOT_PLUGIN_HOST=$PWD/plugins/runtime/target/debug/deckpilot-plugin-host
   export DECKPILOT_PLUGIN_REGISTRY_ROOTS=/path/to/deckpilot-plugins/keys/roots.json
   (cd apps/desktop && VITE_DECKPILOT_PLUGINS=1 pnpm tauri dev)
   ```

   In Settings › Extensions › Plugins, check:

   - the registry is fresh;
   - Discover lists the sample from the test publisher;
   - install, consent and enable all work;
   - its action runs on a device or a mock device.

   The agent log shows the download from
   `github.com/knittingcode/deckpilot-plugins/releases/download/<tag>/…`
   and no verification error.
6. **Check the refresh path.**
   - Dispatch the refresh once:

     ```sh
     gh workflow run pages.yml -R $R -f refresh-timestamp=true
     ```

     `Re-sign timestamp` must run, and the new `timestamp.json` must
     expire about 7 days later. Then repeat step 4.
   - Confirm that the scheduled `refresh-timestamp.yml` succeeds the next
     day.
   - Confirm that a later push to `main` (`pages.yml`) mirrors, verifies
     and keeps the live registry.
7. **Optional: yank the test version.** By pull request:

   ```sh
   .work/bin/deckpilot-registry yank --dir . --id <id> --version <v> --reason "…"
   ```

   After the merge, push a `registry--v<date>` tag.

## 7. Before opening the registry to the public

These come from the design, sections 19.2 and R-6. They are not needed for
the test publication:

- U-8 resolved: a second maintainer, or the documented single-maintainer
  bypass;
- the review checklist and the takedown procedure published;
- three first-party or invited plugins published through the normal path;
- two consecutive real key rotations (index and timestamp) performed and
  verified by clients, with the commands in `PUBLISHING.md`, "Production
  keys";
- DeckPilot's roots embedded (§5.1), and plugins enabled in release builds
  by DeckPilot's own criteria.
