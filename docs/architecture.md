# Architecture

This repository is the DeckPilot plugin registry: the reviewed source data,
the tooling that checks it, and the workflows that turn it into a signed
static site and plugin release assets. The format, the client verification
rules and the publishing tool all come from DeckPilot; this repository only
uses them, pinned to one DeckPilot commit.

| Source of truth | Where (DeckPilot, `knittingcode/deckpilot`) |
| --- | --- |
| Registry format, client verification, `deckpilot-registry` tool | `crates/deckpilot-plugin-registry/` (README.md, PUBLISHING.md) |
| Package format and checks | `crates/deckpilot-plugin-package/` |
| Manifest format | `crates/deckpilot-plugin-manifest/`, `plugins/schemas/` |
| Executable artifact (`plugin.wasm`, ABI 1) | `docs/marketplace/artifact-contract.md` |
| How the agent installs from the registry | `docs/marketplace/agent-integration.md` |
| Application and plugin distribution split | `docs/marketplace/distribution.md` |
| Design (threat model, registry, trust) | `docs/marketplace/architecture.md`, pull request #191 |

The pin is `tools/deckpilot.env` (`DECKPILOT_COMMIT`). `schemas/` and
`rust-toolchain.toml` must match that commit (`tools/check-schemas.sh`, job
*Schemas match DeckPilot*).

## Layout

```text
registry/                  reviewed source data, the tool's --dir inputs
  publishers/<login>.json  namespace, numeric GitHub id, login, verified
  plugins/<id>/plugin.json listing (--plugin-detail)
  plugins/<id>/versions/<version>.json   one record per version, append-only
  revocations.json         cumulative revocations (absent: none)
plugins/<id>/              per-plugin submission
  build.json               build recipe (toolchain, workspace, package, profile, module)
  manifest.json            copy of the newest version's manifest (reviewed in the diff)
plugins/examples/          CI fixtures, never published
first-party/               sources of DeckPilot's own installable plugins (one Cargo
                           workspace); built, tested and packaged by CI, not a submission
keys/                      public trust material after the key ceremony (empty now)
schemas/                   DeckPilot's schemas, byte-identical at the pin
schemas/inputs/            this repository's input schemas (build recipe, plugin.json)
pages/                     static registry files: /v1/README.txt
website/                   the Marketplace website (Astro), a read-only consumer
tools/                     scripts used by the workflows (and locally)
.github/workflows/         validate, first-party, workflow lint, website, pages, publish, refresh-timestamp
```

Never committed (`tools/check-hygiene.py` refuses them): `site/` (the
generated Pages tree), `releases/` and `*.dpplugin` (release assets), any
key file or private key.

## What is built where

```text
plugin author's repository (tag -> commit)
        |  tools/build-plugin.sh: cargo build --locked (pinned toolchain,
        |  wasm32-unknown-unknown) + deckpilot-registry package
        v
<id>-<version>.dpplugin  --sha256/size must equal-->  registry/plugins/<id>/versions/<v>.json
        |                                                     |
        | publish.yml `release`                               | publish.yml `sign`
        v                                                     v
GitHub Release <id>--v<version>          site/v1: keys/, index/<seq>.json, plugins/<id>/<sha>.json,
(this repository)                        timestamp.json (+ .minisig)  -> GitHub Pages
```

DeckPilot clients fetch
`https://knittingcode.github.io/deckpilot-plugins/v1/timestamp.json`, then
the index it pins, then details on demand, and download packages from
`https://github.com/knittingcode/deckpilot-plugins/releases/download/<id>--v<version>/<id>-<version>.dpplugin`.
That prefix is a constant of the client (`DOWNLOAD_PREFIX`), so the release
tag of a plugin version is always exactly `<id>--v<version>`.

## Workflows

| Workflow | Trigger | Secrets | Does |
| --- | --- | --- | --- |
| `validate.yml` (*Validate*) | every pull request, push to `main` | none | hygiene, submission rules, schema pin, rebuild and verify every version, throwaway-key signed registry with `verify-site --require-packages`, examples |
| `first-party.yml` (*First-party plugins*) | every pull request, push to `main` | none | `first-party/`: layout and SDK pin, unit tests, wasm build, packages under a test publisher in a throwaway-key signed registry with `verify-site --require-packages`; publishes nothing ([plugin-factory.md](plugin-factory.md)) |
| `workflow-lint.yml` (*Workflow lint*) | every pull request, push to `main` | none | actionlint and zizmor |
| `website.yml` (*Website*) | every pull request | none | builds and tests the website; composes it with the registry tree and checks that no registry path changes; never deploys |
| `pages.yml` (*Pages*) | push to `main`, dispatch | none (timestamp only if enabled) | `/v1/` README, or the verified live registry; the website around it |
| `publish.yml` (*Publish*) | owner-created tag `*--v*`, dispatch on a tag | `registry-publish`: index and timestamp keys | build, sign, release, website, deploy, verify published |
| `refresh-timestamp.yml` (*Refresh timestamp*) | daily, dispatch | `registry-timestamp`: timestamp key | re-sign `timestamp.json`, redeploy |

`_site.yml` is the reusable build and deploy of the Pages site used by
*Pages* and *Refresh timestamp*. `_website.yml` is the reusable build and
test of the website used by *Website*, `_site.yml` and *Publish*. *Pages*,
*Publish* and *Refresh timestamp* share the concurrency group
`registry-pages` and never cancel each other.

## The website and the registry on one Pages site

GitHub Pages serves one tree per repository, so the Marketplace website
(`website/`) and the registry (`/v1/`) are deployed together, and kept
apart by construction:

```text
build registry tree            build website (_website.yml)
/v1/** and /icon.svg:          npm ci, build, tests; reads the registry
mirrored live registry,        tree only to generate the catalog
verify-site with the           no environment, no secret, contents: read
committed roots (or README)             |
        |                               |
        +-------------+-----------------+
                      v
        compose (tools/compose-site.py, runs no npm code)
        registry tree copied byte for byte; fails if the website has a
        file under v1/ or at any registry path, or is a fixture build;
        sha256 of every registry path read back from the artifact
                      |
                      v
        deploy (github-pages) -> verify (tools/verify-live-registry.sh:
        the live registry paths serve exactly those bytes)
```

- The website is a read-only consumer. Its job has no environment and no
  secret, and it cannot reach `registry-publish` or `registry-timestamp`.
  It never signs, re-signs or rewrites a registry document.
- The catalog is generated from the registry tree that `verify-site`
  accepted in the same run, never from `registry/` source data, examples
  or fixtures. While no registry is published, the catalog is empty.
- The gates of registry publication are unchanged: a signed registry still
  reaches Pages only through `publish.yml` (owner's tag, `registry-publish`)
  or as the mirrored, verified copy of what is already live. A deploy from
  `main` republishes the registry that is already published, byte for byte,
  or `/v1/README.txt` while there is none.
- The site root (`/`, `/index.html`) forwards people to `/en/` or `/pl/`.
  DeckPilot clients read only `<site>/v1/...`, so nothing at the root is a
  registry path. Missing paths answer 404 as before (now with the website's
  404 page as the body).
- The daily timestamp refresh rebuilds the website from `main` too (without
  the browser tests). If the website cannot be built, that run fails and
  the live site, including its registry, stays as it was; the timestamp is
  valid for 7 days and the refresh runs daily, so one failed run does not
  expire it.

Rules every workflow follows: top-level `permissions: {}` and only what
each job needs; third-party actions pinned to a full commit SHA with the
version in a comment; `persist-credentials: false`; no `pull_request_target`
or `workflow_run`; no caches; timeouts on every job; untrusted values reach
`run:` only through `env:`.

## Open decisions from the design (DeckPilot #191, section 20)

| Id | Decision | Here |
| --- | --- | --- |
| U-1 | JavaScript engine build | Not needed for v1: the artifact is an ABI 1 `plugin.wasm` built from Rust (DeckPilot's artifact contract); a JavaScript SDK would produce the same artifact |
| U-8 | Single maintainer | One maintainer; the owner merges through the ruleset's pull-request-only admin bypass. A second maintainer is recommended before the registry opens |
| U-10 | Root threshold | Open; the tooling supports any threshold (`keyset init --root-threshold`) |
| U-11 | Reproducible builds | The registry rebuilds every package itself, and the record must equal CI's build byte for byte. Builds are reproducible on the same platform but **not across platforms** (macOS arm64 and Linux x86_64 give different hashes for the sample), so CI's `ubuntu-24.04` x86_64 build is the reference, and authors take the record CI derives. There is no independent three-way hash agreement yet |
| U-12 | Timestamp role or a monthly index | Timestamp role: 7 days of validity, re-signed daily, at most 14 accepted |

## Status

Foundation only. There are no production keys (`keys/` is empty), no
published plugins and no signed registry at the public URL. See
[trust-model.md](trust-model.md) for what the owner has to set up first.
