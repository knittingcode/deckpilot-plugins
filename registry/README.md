# Registry source data

The reviewed source of truth for the registry, in exactly the layout the
pinned `deckpilot-registry` tool reads (its `--dir` working directory,
DeckPilot `crates/deckpilot-plugin-registry/PUBLISHING.md`):

| Path | What | Schema |
| --- | --- | --- |
| `publishers/<login>.json` | A publisher: namespace, numeric GitHub id, login, `verified` | `registry-index.schema.json#/properties/publishers/items` |
| `plugins/<id>/plugin.json` | The listing (`--plugin-detail`) | `schemas/inputs/plugin-info.schema.json` |
| `plugins/<id>/versions/<version>.json` | One published version, append-only | `registry-plugin.schema.json#/$defs/version` |
| `revocations.json` | Cumulative revocations (absent: none) | `registry-index.schema.json#/properties/revocations` |

How a version record is produced and checked: [docs/publishing.md](../docs/publishing.md).
The generated site (`site/v1/`) and release assets (`releases/`) are never
committed; CI builds them.
