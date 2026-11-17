# Registry trust root (public material only)

This directory holds the registry's **public** trust material, committed
after the offline key ceremony (U-10 in the DeckPilot marketplace design):

| File | What |
| --- | --- |
| `roots.json` | The root role DeckPilot embeds (`DECKPILOT_REGISTRY_ROOT_PUBKEYS`) |
| `<n>.json` | Key set `n`: root, index and timestamp public keys, `seq_floor` |
| `<n>.json.minisig` | Root signatures of key set `n` (a JSON array) |

It holds key set 1 from the production key ceremony (2026-11-17): the roots
`131875735F6B6F06` and `9D7D888C7D407086` (threshold 1), the index key
`6A115779AF7CCBA2` and the timestamp key `7426063969C76BE9`, with
`first_party` authorizing the namespace `deckpilot`. Key sets are
immutable: a rotation adds `<n+1>.json`, signed by the current roots.

Never put a secret key here or anywhere in this repository. Root secret keys
stay offline; the index and timestamp keys exist only as environment secrets
(see [docs/trust-model.md](../docs/trust-model.md)). `tools/check-hygiene.py`
refuses key files and anything that looks like a private key, and only the
file names above are allowed in this directory.
