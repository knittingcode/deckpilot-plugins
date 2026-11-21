---
title: "Security"
description: "Security guidelines for plugin authors, derived from the sandbox model and the registry's review policy, and how to report a vulnerability."
group: reference
order: 17
---

DeckPilot treats every plugin as untrusted code: it runs in a sandbox, holds only the permissions a user granted, and never sees a secret. That protects users from a plugin, but a well-behaved plugin still has work to do. This page collects guidelines for authors, derived from the sandbox model and from what registry reviewers check, and explains where to report a vulnerability.

> **Status:** DeckPilot 0.7.0 is the first release with plugin support, and 0.10.0 is the current release. Neither DeckPilot nor the plugin registry has had an independent security review, and both repositories say so in their security policies.

## What the platform enforces

The module has no file system, network, clock or process of its own. Every outside effect is a capability request checked against the user's current grants, requests go only to exact `https` origins, secrets are injected by the agent and redacted in responses, and each invocation runs under limits on time, memory and calls. [Architecture](../architecture/) describes the model. What follows is the part that is left to you.

## Least privilege

- **Declare only what your code calls.** Every permission costs the user a consent decision, including kinds that have no effect yet.
- **Add permissions late.** Adding or broadening a permission in an update is an escalation: users are asked again, and reviewers treat it as carefully as a new plugin.
- **Prefer no permission.** Storage, settings, the plugin's own variables and logging need none. Many useful actions are pure computation.
- **Do not ask for more storage** than the default 256 KiB unless you need it.
- **Write honest justifications.** The justification is shown in the consent dialog and compared by reviewers with what the plugin does. Say concretely what data goes where and why.

## Narrow hosts and methods

- List exact hosts that belong to the service your plugin claims to use. Reviewers check this.
- List only the HTTP methods you use. A plugin that only reads declares `GET`.
- Do not declare wildcard hosts. They validate against the schema but are never granted, so they only make the plugin harder to review.
- Bind each credential slot to the smallest set of hosts. The secret can travel to every host of its slot.
- `openUrl` hosts should be the pages your actions open for the user, such as your service or its documentation.

## Keep secrets out of everything

- Use credential slots. Never ask a user to paste a token into a parameter or a setting: those are ordinary values that the plugin reads, and they get none of the handling a slot gets (injection by the agent, redaction in responses).
- Never write a secret, a token or personal data to the log, an output, a message, a variable or storage.
- Do not put full URLs with query strings into messages. Queries often carry tokens.
- Do not copy response bodies wholesale into outputs or error messages. Extract the fields you need.
- Outputs can be marked `sensitive` in the manifest, and so can variables. Use the flag for values a user would not want displayed.

## Treat all input as hostile

Parameters arrive validated against their kinds, but the kinds do not know your domain, and remote responses are not validated at all.

- Validate values before building a URL from them. The example in these pages accepts a service name only if it consists of lowercase letters, digits and hyphens.
- Check that a parsed response has the shape and ranges you expect before using or storing it.
- Bound what you keep: cut strings from remote sources to a sensible length before putting them into outputs, variables or storage.
- Never panic on input. A panic ends the host process, and five crashes in ten minutes disable the plugin. Return a `PluginError`.

## Fail safely

- Handle a denial at every capability call. A user can revoke a permission between two calls of the same action.
- Treat an optional capability as optional. A notification that cannot be shown should not fail a check that otherwise succeeded.
- Mark an error as retryable only when a retry is harmless, and set `retrySafe` on an action only when running it twice cannot cause damage.
- Declare the `effect` of each action truthfully: `readOnly`, `opens` or `modifies`.

## Hermetic, reviewable builds

The registry rebuilds your plugin from source, and your build scripts and procedural macros run in its CI. Reviewers look for anything unexplained.

- Pin an exact toolchain and commit `Cargo.lock`.
- No build script that downloads or executes anything, and no network access beyond Cargo's locked dependencies.
- No obfuscated code, generated blobs or vendored binaries.
- Keep dependencies few. Each must build for `wasm32-unknown-unknown` without WASI.
- Never move or delete a tag that a registry record names.

## Protect your publisher account

Your namespace is bound to your GitHub account, and the registry's security policy lists a compromised publisher account or source repository among the things to report. Secure the account and the repository your records point at.

## Reporting a vulnerability

Report privately, through GitHub's private vulnerability reporting. Which repository depends on what is affected.

| What | Where |
| --- | --- |
| A malicious or dangerous plugin listed in, or submitted to, the registry | The registry repository |
| A registry vulnerability: getting a document or package accepted by clients without the registry keys; rollback, freeze or equivocation beyond the documented bounds; bypassing revocation | The registry repository |
| A vulnerability in the registry repository's workflows or tools | The registry repository |
| Exposure of a private key, password or secret of the registry repository | The registry repository |
| A vulnerability in the DeckPilot app or agent, the plugin sandbox and host, the plugin SDK, or the client-side registry verification code | The DeckPilot repository |

- Registry repository: open its Security tab and choose "Report a vulnerability", or go to <https://github.com/knittingcode/deckpilot-plugins/security/advisories/new>. Its policy states that there is no separate email address or other channel.
- DeckPilot repository: <https://github.com/knittingcode/deckpilot/security/advisories/new>.

Both policies ask the same of reporters: do not open a public issue or pull request for a suspected vulnerability or malicious plugin, and do not post exploit details publicly. The registry's policy adds: do not test against other users' installations.

A useful report names what is affected (the plugin id and version, or the affected DeckPilot version or commit and the operating system), what you observed, how to reproduce it, and what an attacker gains. Each policy lists what it asks for.

For a problem with a plugin that is not a security issue, such as a crash or a misleading description, use the registry's *Plugin report* issue form.

### What the policies say

The registry's policy gives no guaranteed response times and says reports are handled before any other work. For a confirmed malicious or dangerous plugin it describes revoking the plugin in the signed index and reviewing the publisher's other plugins. DeckPilot's policy names an acknowledgement within 7 days as a target, not a guarantee, and asks reporters to give a reasonable time to fix an issue before disclosing it. Both credit reporters in the advisory unless they prefer otherwise.

The policies themselves are authoritative: [SECURITY.md of the registry](https://github.com/knittingcode/deckpilot-plugins/blob/main/SECURITY.md) and [SECURITY.md of DeckPilot](https://github.com/knittingcode/deckpilot/blob/main/SECURITY.md).

> **Note:** At the time of writing, the scope section of DeckPilot's policy still describes plugins as a design proposal. The registry's policy directs reports about the sandbox, the host, the SDK and client-side verification to the DeckPilot repository.

## Canonical reference

The registry's [review policy](https://github.com/knittingcode/deckpilot-plugins/blob/main/docs/review-policy.md) and [trust model](https://github.com/knittingcode/deckpilot-plugins/blob/main/docs/trust-model.md), and DeckPilot's [permission rules](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/permissions.md).
