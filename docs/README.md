# Documentation

| Document | For |
| --- | --- |
| [architecture.md](architecture.md) | What lives where, what is built where, the workflows |
| [publishing.md](publishing.md) | From pull request to signed registry entry; release tags; owner setup |
| [production-readiness.md](production-readiness.md) | The owner's ordered checklist before the first production publication: decisions, key ceremony, environments, variables, DeckPilot's side, a test publication |
| [trust-model.md](trust-model.md) | Roles and keys, client checks, freshness, rollback and freeze protection, revocation, rotation |
| [review-policy.md](review-policy.md) | What a reviewer checks before approving |
| [storefront.md](storefront.md) | Marketplace listings (`listing.json`, media), the owner's editorial placements, the asset rules, the rollout stages and their sign-off |
| [third-party-readiness.md](third-party-readiness.md) | Opening the registry to other publishers: what exists, what is missing, the gate that keeps it closed |
| [plugin-factory.md](plugin-factory.md) | DeckPilot's own installable plugins: what plugin API 1.0 to 1.9 can do (capability matrix), each planned plugin, data providers, the capabilities proposed, and how `first-party/` is built |
| [release-governance.md](release-governance.md) | Who may authorize an official release: the team-managed default policy, the current single-maintainer exception, approval versus signing, the transition to team governance |
| [separation.md](separation.md) | Why and how this repository never touches DeckPilot application releases |
| [../official/README.md](../official/README.md) | The official plugins (the ones DeckPilot bundles): their pinned SDK, reproducible build, permission policy, publisher signature and release workflow |
| [official-marketplace.md](official-marketplace.md) | Official plugins as Marketplace entries: generated only from a verified release, the refusals, the procedure after a release, what promotion still needs, the tests |
| [official-compatibility-matrix.md](official-compatibility-matrix.md) | Generated: host, plugin API, SDK pin, and each official plugin's version, permissions, dependencies, signature and Marketplace status |

Contributor guide: [../CONTRIBUTING.md](../CONTRIBUTING.md). Reporting a
malicious plugin or a vulnerability: [../SECURITY.md](../SECURITY.md).
