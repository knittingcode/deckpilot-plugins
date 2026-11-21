# Candidate Marketplace entries of the official plugins

One directory per official plugin with a verified release, written only by
`tools/official-entry.py generate` from that release
([docs/official-marketplace.md](../../docs/official-marketplace.md)):

```text
<id>/plugin.json                 the listing
<id>/versions/<v>.json           the version record (deckpilot-registry add-version)
<id>/versions/<v>.release.json   the release and how it was verified
```

Never edit these by hand, never copy them into `registry/`: DeckPilot does
not take a bundled plugin from a registry, so they are not in the signed
index. `tools/official-entry.py check` (official.yml) checks them.
