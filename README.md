<div align="center">

<p>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset=".github/assets/wordmark-dark.svg" />
    <img src=".github/assets/wordmark.svg" alt="DeckPilot" height="72" />
  </picture>
</p>

**DeckPilot plugins** is the community plugin registry of
[DeckPilot](https://github.com/knittingcode/deckpilot), the open-source,
local-first Stream Deck controller and automation app.

[Marketplace website](https://knittingcode.github.io/deckpilot-plugins/) &nbsp;&bull;&nbsp; [Contributing a plugin](CONTRIBUTING.md) &nbsp;&bull;&nbsp; [Security](SECURITY.md) &nbsp;&bull;&nbsp; [Documentation](docs/README.md)

[![Validate](https://github.com/knittingcode/deckpilot-plugins/actions/workflows/validate.yml/badge.svg)](https://github.com/knittingcode/deckpilot-plugins/actions/workflows/validate.yml)

</div>

> **Status:** the signed production registry is live at
> <https://knittingcode.github.io/deckpilot-plugins/v1/> (key set 1: two
> offline roots, 1-of-2; index and timestamp keys in protected
> environments; the timestamp is re-signed daily). It serves DeckPilot's own
> first-party plugins, published through `publish.yml`. The registry is
> not open to other publishers yet. DeckPilot
> [0.7.0](https://github.com/knittingcode/deckpilot/releases/tag/v0.7.0), the first
> release with plugin support, installs them from its Marketplace page
> (Settings › Extensions › Plugins). What remains before the registry opens
> to the public:
> [docs/production-readiness.md](docs/production-readiness.md) ("Before
> opening the registry to the public").

## What this is

A signed, static plugin registry. Plugin authors submit their plugins here
by pull request; after review, the owner publishes them, and DeckPilot can
then browse, install and update them from inside the app.

- **Registry metadata** is served by GitHub Pages under
  <https://knittingcode.github.io/deckpilot-plugins/v1/>: root-signed key
  sets, a signed index of every plugin, content-addressed plugin details,
  and a timestamp re-signed daily.
- **Plugin packages** (`<id>-<version>.dpplugin`) are assets of this
  repository's GitHub Releases, one release per plugin version, tagged
  `<id>--v<version>`.
- **The Marketplace website** at
  <https://knittingcode.github.io/deckpilot-plugins/> (English and Polish)
  is the human-facing side: the plugin directory, plugin pages and the
  developer documentation. It is generated from the published registry and
  only reads it; see [`website/`](website/).

## How DeckPilot uses it

The DeckPilot agent fetches `v1/timestamp.json`, the index it pins and the
details it needs, and verifies every signature against root public keys
compiled into the app. It downloads a package only from this repository's
releases and installs it only if its sha256 and size equal what the signed
index pins. It runs a plugin as a WebAssembly module (`plugin.wasm`, plugin
ABI 1) in a sandboxed host, with only the permissions the user granted at
install. Hosting and TLS are not trusted; signatures are. See
[docs/trust-model.md](docs/trust-model.md).

The registry format, the client checks and the publishing tool
(`deckpilot-registry`) all live in DeckPilot; this repository builds them
from one pinned DeckPilot commit (`tools/deckpilot.env`).

## Repository layout

| Path | What |
| --- | --- |
| [`registry/`](registry/) | Reviewed source data: publishers, plugin listings, version records, revocations |
| [`plugins/`](plugins/) | One directory per plugin: build recipe and manifest. [`plugins/examples/`](plugins/examples/) holds CI fixtures that are never published |
| [`keys/`](keys/) | Public trust material: the roots and the root-signed key sets |
| [`schemas/`](schemas/) | DeckPilot's JSON schemas, pinned to the DeckPilot commit |
| [`tools/`](tools/) | Build, validation and publishing scripts used by CI |
| [`website/`](website/) | The Marketplace website (Astro): [README](website/README.md), [design system](website/DESIGN.md) |
| [`docs/`](docs/) | [Architecture](docs/architecture.md), [publishing](docs/publishing.md), [production readiness](docs/production-readiness.md), [trust model](docs/trust-model.md), [review policy](docs/review-policy.md), [separation from app releases](docs/separation.md) |

The generated site (`site/v1/`) is never committed; CI builds it.

## Run the checks locally

Needs `rustup`, `git`, `jq`, `python3` and `curl`.

```sh
tools/build-cli.sh      # pinned deckpilot-registry + schema-check into .work/bin
tools/validate.sh       # what the Validate workflow runs
tools/lint-workflows.sh # actionlint + zizmor
```

The website has its own checks (needs Node.js): `cd website && npm ci &&
npm run verify`, see [website/README.md](website/README.md).

`tools/validate.sh` builds plugin code from its source: run it only on
plugins you are willing to build on your machine.

## License

This repository's own content (tooling, workflows, documentation, schemas
copied from DeckPilot) is licensed under the [Apache License 2.0](LICENSE).

**Each plugin carries its own license**, declared in its `manifest.json`
(`license`, an SPDX expression) and shipped as `LICENSE` in its package.
Listing a plugin here does not relicense it, and the Apache-2.0 license of
this repository does not apply to plugin source or packages.

DeckPilot and the DeckPilot logo are part of the DeckPilot project; the
logo files in `.github/assets/` are copied unchanged from it.
