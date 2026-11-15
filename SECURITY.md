# Security policy

This repository is the DeckPilot plugin registry. It is a foundation under
construction: no production keys exist and no plugin is published yet. It
has not had an independent security review.

## What to report here

| In scope | Examples |
| --- | --- |
| **A malicious or dangerous plugin** listed in, or submitted to, this registry | data exfiltration, permissions used for something other than their justification, hosts unrelated to the plugin's purpose, obfuscated code, a compromised publisher account or source repository |
| **Registry vulnerabilities** | a way to get a document or package accepted by DeckPilot clients without the registry keys; rollback, freeze or equivocation beyond the documented bounds; bypassing revocation |
| **Pipeline vulnerabilities** in this repository's workflows and `tools/` | a pull request that can reach a secret, a signing job that runs untrusted code, artifact poisoning, a path-guard or validation bypass that would publish unreviewed content |
| **Key exposure** | any private key, password or secret of this repository seen anywhere |

Out of scope here, report to [DeckPilot](https://github.com/knittingcode/deckpilot/security/advisories/new)
instead: vulnerabilities in the DeckPilot app or agent, the plugin sandbox
and host, the plugin SDK, or the client-side registry verification code.
These live in the DeckPilot repository. For a non-security problem with a
plugin (a crash, a misleading description), use the *Plugin report* issue
form.

## How to report

Report **privately** through GitHub's private vulnerability reporting:
open this repository's **Security** tab and choose **Report a
vulnerability**
(<https://github.com/knittingcode/deckpilot-plugins/security/advisories/new>).
There is no separate email address or other channel.

Do not open a public issue or pull request for a suspected malicious plugin
or vulnerability, and do not post exploit details publicly. Do not test
against other users' installations.

A useful report contains the plugin id and version (or the registry file,
workflow or script), what you observed, how to reproduce it, and what an
attacker gains.

## What happens next

There are no guaranteed response times; reports are handled before any
other work. For a malicious or dangerous plugin the owner, as soon as the
report is confirmed:

1. **Revokes it** in a reviewed pull request to `registry/revocations.json`
   (`deckpilot-registry revoke --action disable`, or `yank` for a
   non-malicious problem), with the reason and, where useful, a link to a
   published advisory.
2. **Publishes a new signed index** with a `registry--v…` tag
   ([docs/publishing.md](docs/publishing.md)). Clients apply the revocation
   at their next successful refresh: `disable` stops the installed plugin
   and refuses to load it again, `yank` stops new installs and updates.
   Revocations are cumulative and enforced offline once received.
3. May delete the plugin's GitHub Release. A deleted release's tag name can
   never be reused, so that version can never be republished.
4. Reviews the publisher's other plugins and may revoke the namespace
   (`--versions '*'`).

A registry key compromise is handled by key rotation (an offline root
ceremony; [docs/trust-model.md](docs/trust-model.md#rotation-and-recovery)).
A root key compromise needs a DeckPilot application release with new
embedded roots.

Reporters are credited in the advisory unless they prefer otherwise.
