---
title: "Signing and trust"
description: "Plugin authors sign nothing. The registry is signed with minisign roles, and packages are pinned by sha256 and size in the signed index."
group: ship
order: 14
---

Plugin authors do not sign anything. There is no author key, no certificate to buy and no signature file in a package. What is signed is the registry: a small set of static documents that pin every published package by its sha256 and size. This page explains who signs what and what a DeckPilot client verifies before it installs a plugin.

> **Status:** The production registry at `https://knittingcode.github.io/deckpilot-plugins/v1/` is signed with key set 1: two offline root keys with a threshold of 1 of 2, and separate index and timestamp keys. The timestamp is re-signed daily. The root public keys are in [`keys/roots.json`](https://github.com/knittingcode/deckpilot-plugins/blob/main/keys/roots.json) of the registry repository, with the key ids `131875735F6B6F06` and `9D7D888C7D407086`. DeckPilot 0.7.0, the first release with plugin support, has them built in.

## What an author provides

An author provides source code at a tagged commit in a public GitHub repository, plus the submission files described in [Submission](../submission/). The registry builds the package from that source. The author's identity is the GitHub account that owns the publisher record, bound to its numeric GitHub user id.

A package itself carries no signature. Its authenticity comes entirely from the signed registry documents that name its hash.

## The signed documents

Registry metadata is served as static files. Every signature is a minisign (Ed25519) signature.

| Document | Signed by | Contains |
| --- | --- | --- |
| `keys/<n>.json` | root keys | The key set: the root, index and timestamp public keys |
| `index/<seq>.json` | index key | Every plugin, the publishers, the revocations, and the hash of each plugin's detail document |
| `plugins/<id>/<sha256>.json` | not signed itself; pinned by hash in the index | A plugin's detail: its versions, each with the package's sha256, size and permissions |
| `timestamp.json` | timestamp key | The sequence number, length and sha256 of the current index, with an expiry |

Each signature's trusted comment names its role, and for the index and timestamp the sequence number and key set. The comment must match exactly, so a signature for one role or sequence number never passes as another. A DeckPilot application updater signature never passes as a registry signature, or the reverse.

## The three roles

| Role | Signs | Where the private key is meant to live | What a compromise would mean |
| --- | --- | --- | --- |
| root | Key sets | Offline only, never on GitHub | Full registry trust, until a DeckPilot release ships new roots |
| index | The index | A secret of a protected environment that runs only for release tags, which only administrators can create | Malicious listings until the key is rotated |
| timestamp | The timestamp | A secret of the publishing and the daily refresh environments | Freezing or delaying only: it cannot point at an index the index key did not sign |

A key set in which two roles share a key is refused, so the unattended timestamp key can never act as the index key or as a root.

## What a client verifies

Before DeckPilot installs anything, its registry client checks, in order:

1. **Key sets.** Starting from the root public keys built into the application, each key set needs a threshold of signatures from the previous root role, and from the new one if the roots change.
2. **Timestamp.** Signed by the current timestamp key, naming the current key set, not expired, valid for at most 14 days, and with a sequence number that never goes down.
3. **Index.** Its length and sha256 must equal what the timestamp pins before it is even parsed; it must be signed by the index key and carry the same sequence number and key set.
4. **Detail.** Its length and sha256 must equal what the index pins; versions strictly increase; package URLs lie under the registry repository's release downloads.
5. **Package.** Its sha256 and size must equal the pins in the detail before it is unpacked. Then the package checks run: the manifest digest, the permissions byte for byte against what the consent dialog showed, and the static check of `plugin.wasm`.

Hosting and TLS are not part of this chain. The files could be served from anywhere; only the signatures and hashes count.

## Protections that follow

| Threat | Protection |
| --- | --- |
| A tampered package or metadata file | The hash or signature does not match; refused |
| Rollback to an older, validly signed registry state | The sequence number never goes down; a client keeps the highest it has seen |
| Two different indexes at the same sequence number | At an equal sequence number the timestamp must pin the same index |
| Freezing clients on an old state | Bounded by the timestamp's expiry. It is signed for 7 days and meant to be re-signed daily; clients refuse one valid for more than 14 days |
| Setting the computer's clock back | Clients keep a clock floor from the newest verified timestamp |

When the stored timestamp expires, a client shows the registry as stale, refuses installs and updates, and keeps running installed plugins with revocations enforced.

## Revocation

The owner can withdraw a published version in the signed index:

| Action | Installed copy | New installs and updates |
| --- | --- | --- |
| `yank` | Keeps running, with a notice | Not offered |
| `disable` | Stopped and disabled, with the reason | Refused |

Revocations are cumulative: a new index must keep every published revocation at least as strong, and clients refuse an index that drops one. They reach a client at its next successful refresh and are enforced offline from then on.

## After installation

The agent records the integrity of the installed files. Before every start of a plugin's host process it verifies the files against that record and validates the module again.

## Local keys for testing

`deckpilot-registry keys generate` creates key pairs for local use, so that you can build a private registry on your own machine and test the complete install flow; see [Testing](../testing/#against-a-real-agent). An agent trusts such a registry only when it is started with `--plugin-registry-roots` pointing at that registry's `roots.json`. These keys have no relation to the public registry.

## What remains before the registry opens to other publishers

The production keys exist, and the publishing workflows sign the live registry with them. DeckPilot uses it starting with DeckPilot 0.7.0, which has the root keys built in. On every refresh, DeckPilot checks the signed key set, the index, the timestamp and every package's size and SHA-256. What the registry repository lists before it opens to plugins from other publishers is in the owner's checklist, [docs/production-readiness.md](https://github.com/knittingcode/deckpilot-plugins/blob/main/docs/production-readiness.md), section "Before opening the registry to the public".

## Canonical reference

[docs/trust-model.md](https://github.com/knittingcode/deckpilot-plugins/blob/main/docs/trust-model.md) in the registry repository, and the client's [README](https://github.com/knittingcode/deckpilot/blob/main/crates/deckpilot-plugin-registry/README.md) and [PUBLISHING.md](https://github.com/knittingcode/deckpilot/blob/main/crates/deckpilot-plugin-registry/PUBLISHING.md) in DeckPilot.
