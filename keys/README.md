# Registry trust root (public material only)

This directory holds the registry's **public** trust material, committed
after the offline key ceremony (U-10 in the DeckPilot marketplace design):

| File | What |
| --- | --- |
| `roots.json` | The root role DeckPilot embeds (`DECKPILOT_REGISTRY_ROOT_PUBKEYS`) |
| `<n>.json` | Key set `n`: root, index and timestamp public keys, `seq_floor` |
| `<n>.json.minisig` | Root signatures of key set `n` (a JSON array) |

It is **empty until the owner runs the key ceremony**; until then nothing
can be signed for production, and the publishing workflows refuse to run.

Never put a secret key here or anywhere in this repository. Root secret keys
stay offline; the index and timestamp keys exist only as environment secrets
(see [docs/trust-model.md](../docs/trust-model.md)). `tools/check-hygiene.py`
refuses key files and anything that looks like a private key, and only the
file names above are allowed in this directory.
