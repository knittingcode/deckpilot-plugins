# Opening the registry to other publishers

The production registry signs DeckPilot's own plugins only. This page is
the gate for opening it: what a third-party developer already has, what the
trust and authorization model still lacks, and the control that keeps the
registry closed until then. It extends
[production-readiness.md](production-readiness.md) §7.

## The control that keeps it closed

`tools/check-first-party-only.sh` runs in `tools/publish-site.sh index`
before any key is written. It refuses to sign while any publisher record or
plugin is outside `first_party.namespaces` of the newest root-signed key set
(`keys/<n>.json`, today `["deckpilot"]`). So a merged third-party submission
plus its release tag still publishes nothing. Opening the registry is a
reviewed change of that gate, made only when every item marked **missing**
below is done. The key set's `first_party` list is root-signed and is not
the way to open the registry: it also tells DeckPilot which namespaces are
first-party.

The `deckpilot` namespace stays protected independently of this gate:
*Submission rules* refuses reserved publisher ids for anyone but the
maintainer, and DeckPilot installs a `deckpilot.*` plugin only when the
root-signed key set names `deckpilot` first-party and the index lists the
publisher as verified ([trust-model.md](trust-model.md)).

## What exists, and what is missing

| Area | State | Where |
|---|---|---|
| Plugin SDK documentation | exists: manifest, actions, capabilities, permissions, lifecycle, errors and versions, building | DeckPilot `docs/marketplace/sdk/` |
| Development template | partial: the `hello-weather` sample and `plugins/examples/`; **missing** a minimal starter (one action, one permission) that CI builds and that `cargo generate` or a copy turns into a new plugin | DeckPilot `plugins/examples/`, here `plugins/examples/` |
| Local testing | exists: run a plugin in DeckPilot from a local directory | DeckPilot `docs/marketplace/local-testing.md` |
| Manifest validation | exists: the schema pinned from DeckPilot (*Schemas match DeckPilot*), the manifest validator, and DeckPilot refusing invalid manifests at install | `schemas/`, `tools/check-schemas.sh`, `tools/validate.sh` |
| Permission declaration and review | exists: declared permissions with justifications, the permission diff of every update in *Submission rules*, consent again in DeckPilot for any new permission | `CONTRIBUTING.md`, [review-policy.md](review-policy.md) §3 |
| Publisher identity | partial: a namespace is bound to a numeric GitHub user id and taken first come; the owner alone sets `verified`. **Missing**: a documented verification procedure for organizations (domain or organization ownership) and for transfers of a namespace | `tools/check-submission.py`, [review-policy.md](review-policy.md) §1 |
| Package signing | partial: every package is pinned by sha256 and size in the reviewed record and covered by the index signature. **Missing** a decision on publisher signatures: today the index key alone can list any package for any publisher, so a compromise of the index key or of GitHub Actions is a supply-chain compromise of every publisher (limited by consent and the sandbox). Either accept and document that, or add per-publisher keys pinned in publisher records and checked by DeckPilot | [trust-model.md](trust-model.md) |
| Reproducible builds | exists: CI rebuilds every version from the recorded tag and commit with an exact toolchain and a locked workspace, and the package must match byte for byte | `tools/validate.sh`, [review-policy.md](review-policy.md) §4 |
| Submission and review workflow | exists for one maintainer: pull request, *Validate registry*, *Submission rules*, CODEOWNERS review, owner-created tag. **Missing**: U-8 (a second maintainer or the documented single-maintainer bypass) and the published review checklist with response times for third parties | `CONTRIBUTING.md`, [review-policy.md](review-policy.md) |
| Version compatibility | exists: `engines.deckpilot` ranges and `pluginApi` levels checked by the registry tooling and by DeckPilot before install | DeckPilot `docs/marketplace/sdk/errors-and-versions.md` |
| Security scanning | partial: the sandbox refuses every import but `deckpilot_v1.host_call`, the broker checks every operation, and the reviewer reads the source diff. **Missing**: an automated scan in *Validate registry* that lists the module's imports and exports, its size and memory limits, and flags build scripts and dependencies that fetch or run code, so the review does not depend on reading alone | DeckPilot `plugins/runtime/crates/host` |
| Revocation | exists: `revoke` and `yank`, cumulative revocations that clients enforce on installed plugins, `registry--v…` tags | [publishing.md](publishing.md) |
| Abuse handling | partial: reporting a malicious plugin is in `SECURITY.md`, and revocation is immediate once signed. **Missing**: the published takedown procedure (who decides, the time to revoke, how the publisher is told and can appeal) | [../SECURITY.md](../SECURITY.md), [production-readiness.md](production-readiness.md) §7 |
| Key rotation | **missing**: two consecutive real rotations of the index and timestamp keys verified by clients, required before opening | [production-readiness.md](production-readiness.md) §7 |

## Order of work

1. Decide publisher signatures (the table's *Package signing* row); it
   changes DeckPilot's client and the publisher record schema, so it comes
   first.
2. Takedown procedure and review checklist, published in this repository.
3. The automated plugin scan in *Validate registry*.
4. The minimal starter template, built by CI.
5. Two key rotations, then U-8.
6. A reviewed change of `tools/check-first-party-only.sh` that admits
   verified publishers, with its tests.
