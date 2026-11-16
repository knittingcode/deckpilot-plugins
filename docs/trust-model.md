# Trust model

DeckPilot trusts this registry **only through signatures it verifies
itself**, against root public keys built into the application. Where the
files are hosted (GitHub Pages, GitHub Releases) and TLS are not part of
the trust: a compromised or impersonated host can withhold updates, which
clients detect as staleness, but cannot make clients accept anything. The
rules below are implemented by `RegistryClient` in DeckPilot
`crates/deckpilot-plugin-registry` and are exactly what `verify-site` runs
before every deploy.

## Roles and keys

All signatures are minisign (Ed25519). Every trusted comment starts with
`deckpilot-registry v1 role:` and must match exactly, so a signature of one
role, seq or key set never passes as another, and a DeckPilot application
updater signature never passes as a registry signature (or the reverse).

| Role | Signs | Trusted comment | Private key lives | Compromise means |
| --- | --- | --- | --- | --- |
| root (v1 plan: 2 keys, threshold 1, U-10) | `keys/<n>.json` | `… role:keys version:<n>` | **offline only**: the owner's machine and an offline backup; never in GitHub | full registry trust until a DeckPilot release ships new roots |
| index | `index/<seq>.json` | `… role:index seq:<seq> keys:<n>` | secret `REGISTRY_INDEX_KEY` of environment `registry-publish` (tag runs, required reviewer) | malicious listings until rotation; every use is reviewer-gated |
| timestamp | `timestamp.json` | `… role:timestamp seq:<seq> keys:<n>` | secret `REGISTRY_TIMESTAMP_KEY` of `registry-publish` and `registry-timestamp` | freezing or delaying only: it cannot point at an index the index key did not sign |

A key set whose roles share a key is refused, so the unattended timestamp
key can never act as the index key or a root. The index and timestamp
thresholds are 1.

## What a client checks

1. **Key sets.** From the embedded roots, `keys/1.json`, `keys/2.json`…
   while they exist. Each needs a threshold of signatures from the previous
   root role, and from the new one if the roots change.
2. **Timestamp** (`timestamp.json`, at most 4 KiB). Signed by the current
   timestamp key; names the current key set; `expires - signed_at` at most
   **14 days**; unexpired (`now < expires + 5 min`); `seq` at least the
   stored seq and, at an equal seq, the same index hash.
3. **Index** (`index/<seq>.json`, at most 4 MiB). Only when the timestamp's
   reference changes; length and sha256 must equal the timestamp's before
   parsing; signed by the index key; same seq and key set; fixed download
   prefix; content-addressed details; revocations cumulative.
4. **Detail** (`plugins/<id>/<sha256>.json`, at most 256 KiB). Length and
   sha256 against the index; versions strictly increasing; package URLs
   under `…/releases/download/<id>--v<version>/`.
5. **Package.** sha256 and size must equal the pins in the detail before
   it is unpacked; then the package checks of `deckpilot-plugin-package`
   (manifest digest, permissions byte-for-byte, `plugin.wasm` ABI 1).

## Freshness and expiry

The **timestamp governs freshness**. It is signed for 7 days and re-signed
daily (`refresh-timestamp.yml`); clients refuse one valid for more than 14
days, so a stolen timestamp key cannot stretch a freeze. `index.expires` is
only checked to be after `generated_at` (the DeckPilot crate's decision 1):
the index is pinned by hash from the timestamp, and seq is monotonic. When
the stored timestamp expires, clients show the registry as stale, refuse
installs and updates, and keep running installed plugins with revocations
enforced.

## Rollback and freeze protection

- **Rollback**: `seq` never goes down. A client keeps the highest seq and
  the current key set; an older timestamp or index is refused even if
  validly signed. `verify-site --state` in `publish.yml` proves before every
  deploy that an existing client accepts the new site.
- **Equivocation**: at the same seq, a timestamp must pin the same index.
- **Freeze**: bounded by the timestamp's expiry plus 5 minutes of skew
  (7 days with the daily schedule, 14 at most).
- **Clock tricks**: clients keep a clock floor from the newest verified
  `signed_at`; setting the clock back does not revive an expired timestamp.

## Revocation

`registry/revocations.json` lists `{id, versions | "*", action, reason,
advisory?, at}`:

| Action | Installed copy | New installs and updates |
| --- | --- | --- |
| `yank` (also `yank` on a version) | keeps running, with a notice | not offered |
| `disable` | stopped, disabled with the reason and advisory | refused |

Revocations are **cumulative**: a new index must keep every published
revocation at least as strong (`disable` covers `yank`; `"*"` is only
covered by `"*"`). The tool, the `sign` job and clients all refuse to drop
one. The only way to lift a revocation is a root-signed key set with
`seq_floor_reset: true`, after which the next index may drop revocations
once. That is the index-key compromise recovery, done in an offline root
ceremony. Revocations reach clients at their next successful refresh;
they are enforced offline from then on.

## Rotation and recovery

| Event | Procedure |
| --- | --- |
| Rotate the index or timestamp key | offline: new key; `keyset rotate --index NEW.pub` (or `--timestamp`); `keyset sign` with a root; PR adding `keys/<n+1>.json(.minisig)`; replace the environment secret; publish with a `registry--v…` tag (the key set, a new index and timestamp deploy together) |
| Index-key compromise | as above with `--seq-floor-reset` (`--seq-floor` = last legitimate seq); `lift-revocation` for revocations the attacker added; republish |
| Timestamp-key compromise | as above for `--timestamp`; the impact was limited to freezing |
| Root rotation | `keyset rotate --root …`, signed by the current and the new roots |
| Root loss or compromise | a DeckPilot application release with new embedded roots (outside this repository) |

## Repository protections this relies on

- Ruleset on `main`: pull request required, squash only, linear history,
  approving code-owner review of the latest push, no force push or deletion.
  The admin may bypass only through a pull request.
- Ruleset on tags: creating, updating and deleting any tag is restricted to
  the admin role, so only the owner can start `publish.yml`.
- `registry-publish` deploys only for tags `*--v*` and requires a reviewer.
- CODEOWNERS covers workflows, `tools/`, `schemas/`, `keys/`, `registry/`
  and the policy documents.
- No secret is reachable from a pull request: `validate.yml` uses no
  environment, and forks get no secrets.

## Current status

No production keys exist. `keys/` is empty, `registry-publish` and
`registry-timestamp` do not exist, and the public site serves only the
Marketplace website and `/v1/README.txt`. Registries signed with throwaway keys
exist only inside CI jobs and are discarded.
