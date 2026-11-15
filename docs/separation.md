# Separation from DeckPilot application releases

DeckPilot ships through two channels that never share a release, a key, a
secret or an environment. This follows DeckPilot's distribution contract
(`docs/marketplace/distribution.md`, proposed in knittingcode/deckpilot#225).

| | Application | Plugins |
| --- | --- | --- |
| Repository | `knittingcode/deckpilot` | `knittingcode/deckpilot-plugins` (this one) |
| Releases | application versions, tags `v*` | plugin versions, tags `<id>--v<version>`; index-only `registry--v…` |
| Signed with | the application updater key and Apple Developer ID, environment `production-release` | registry roles: root (offline), index and timestamp keys, environments `registry-publish` and `registry-timestamp` |
| Served from | GitHub Releases of `knittingcode/deckpilot` | GitHub Pages of this repository (metadata) and its GitHub Releases (packages) |

## What this repository never does

- It never uses DeckPilot's application signing secrets (the updater key
  `TAURI_SIGNING_PRIVATE_KEY`, Apple signing and notarization secrets) or
  its `production-release` environment. Environment secrets are scoped to
  one repository, so none of them exists here.
- It never builds, signs or publishes a DeckPilot application release, an
  installer, an update manifest (`latest.json`, `prerelease.json`) or an
  updater signature.
- It never creates `v*` tags: `publish.yml` accepts only `*--v*` tags that
  name a plugin version or `registry`, and `tools/resolve-tag.sh` refuses
  anything else.
- It reads DeckPilot only as source, at the commit pinned in
  `tools/deckpilot.env`, to build the registry tool and to copy schemas.
  No workflow here has write access to DeckPilot.

## What DeckPilot never does

DeckPilot's release workflows never publish a plugin package or registry
metadata, never run a registry signing command and never use a registry
secret or environment. DeckPilot embeds only the **public** root keys
(`DECKPILOT_REGISTRY_ROOT_PUBKEYS`, from `keys/roots.json` here), so a
registry root rotation that clients must learn of is the one case that
needs an application release.

## Enforcement here

- `tools/resolve-tag.sh` (job `resolve` of *Publish*): only
  `<plugin-id>--v<version>` and `registry--v…` tags, on a commit of `main`.
- `tools/check-workflows.py` (job *actionlint and zizmor* of *Workflow
  lint*): workflows may reference only the secrets `REGISTRY_INDEX_KEY`,
  `REGISTRY_INDEX_KEY_PASSWORD`, `REGISTRY_TIMESTAMP_KEY` and
  `REGISTRY_TIMESTAMP_KEY_PASSWORD`, and only the environments
  `registry-publish`, `registry-timestamp` and `github-pages`;
  `production-release`, `TAURI_SIGNING*` and `APPLE_*` are refused, as are
  `pull_request_target`, `workflow_run`, caches, unpinned actions and
  checkouts that keep credentials.
- `tools/check-hygiene.py` refuses committed keys and generated output.
