---
title: "Permissions"
description: "Every permission kind of the manifest, which ones do something in ABI 1 today, justification rules, user consent and revocation."
group: capabilities
order: 7
---

A plugin can do nothing outside its own module unless it holds a permission. The `permissions` array of `manifest.json` is the complete list of what a plugin may ever ask for, the user approves each entry, and the agent's permission broker checks every single capability call against the current grants. This page lists every kind and states exactly which ones have an effect today.

## Declaring permissions

```json
"permissions": [
  {
    "kind": "network",
    "hosts": ["status.example.com"],
    "methods": ["GET"],
    "justification": "Reads the status of the service you name from the Acme status API."
  },
  {
    "kind": "credentials",
    "refs": ["apiToken"],
    "justification": "Sends your Acme API token to status.example.com to authenticate the status request."
  },
  {
    "kind": "notifications",
    "justification": "Tells you when the status of a service changes, only if the action asks for it."
  },
  {
    "kind": "openUrl",
    "hosts": ["status.example.com"],
    "justification": "Opens the public status page of a service in your browser when you run Open status page."
  }
]
```

A manifest has at most 16 permissions and at most one of each kind. List several hosts in the one `network` permission. A plugin that needs nothing declares an empty array.

### Justifications

Every permission needs a `justification` of 10 to 200 characters. The consent dialog and the plugin's details show it next to the permission. Write one sentence for the user that says what the plugin does with the permission. Registry reviewers check that it matches what the plugin really does.

## Permission kinds

The schema of manifest version 1 defines nine kinds. Four have an effect in ABI 1, one raises a limit, and four validate but allow nothing yet.

| Kind | Fields | Status in ABI 1 |
| --- | --- | --- |
| `network` | `hosts`, `methods`, `userOrigins`, `privateNetwork` | **Usable.** Allows `Context::fetch` to `https://<host>` with a listed method |
| `credentials` | `refs` (1 to 8 slot keys) | **Usable.** Allows attaching a credential slot to a request and `Context::has_credential` |
| `notifications` | `routing` (optional) | **Usable.** Allows `Context::notify` |
| `openUrl` | `hosts` (0 to 16), `userOrigins` | **Usable.** Allows `Context::open_url` for `https://<host>` pages, and for the server the user entered and allowed |
| `storage` | `quotaKb` (257 to 10,240) | **Usable.** Raises the storage quota from 256 KiB to `quotaKb` KiB. Storage itself needs no permission |
| `profile` | `read` (`profile`, `project`) | **Schema only.** No ABI 1 capability reads the profile or the project |
| `variables` | `read` (variable ids of other owners) | **Schema only.** ABI 1 has no capability to read variables. A plugin's own variables need no permission |
| `background` | `minIntervalSeconds` (15 to 86,400) | **Schema only.** ABI 1 has no background work or timers |
| `device` | `state`, `icons` (booleans) | **Schema only.** ABI 1 has no device access |

Declaring a kind that does nothing yet is allowed, but it still costs the user a consent decision. Declare only what your code calls.

Some things are never grantable and have no kind at all: the file system, processes, the shell and the clipboard, among others. The manifest crate refuses such kinds with a clear message.

### Network

`hosts` holds 1 to 16 exact DNS names, each with an optional `:port`. `methods` holds at least one of `GET`, `HEAD`, `POST`, `PUT`, `PATCH`, `DELETE`.

- Requests are `https` only, to an exact granted origin, with a listed method.
- IP literals and `localhost` are not valid hosts, and every address a name resolves to must be public.
- The schema accepts wildcard hosts such as `*.example.com` and the flag `privateNetwork`, but **this version never grants either**: they are not passed to the broker, so a request that relies on them is denied.
- Every redirect is authorized again as a new request, at most 5 of them, never from `https` to `http`.
- Certain headers are refused, among them `Host`, `Authorization`, `Cookie`, `Content-Length`, `Origin` and anything starting with `proxy-` or `sec-`. Credentials go through slots only.

Limits per request: a URL of at most 8 KiB, at most 32 headers of at most 4 KiB, a text request body of at most 32 KiB, a response body of at most 512 KiB, and 15 seconds for all hops. Response headers are filtered to an allowlist. A status outside 200 to 299 is a normal `Response`, not an error.

### Credentials

`refs` lists keys of `contributes.credentials`. A slot's secret travels only to the slot's own hosts, each of which must also be a granted `network` host, and only in a declared scheme. See [Credentials](../credentials/).

### Notifications

`Context::notify(title, body)` shows a notification through the agent, titled with the plugin's name. The title is at most 64 characters and the body at most 300.

#### Routing

The permission may name the plugin's own settings that decide where its notifications appear. The agent reads them, as the user saved them, every time the plugin shows a notification; the plugin itself never decides where.

```json
{
  "kind": "notifications",
  "routing": { "desktop": "desktop", "statusBar": "statusBar", "minimumSeverity": "minimumSeverity" },
  "justification": "Shows the notifications your workflows ask for, where you choose in the settings."
}
```

| Member | The setting it names |
| --- | --- |
| `desktop` | A `boolean` setting: on, notifications are also shown by the operating system |
| `statusBar` | A `boolean` setting: on, they are listed in the status bar of an open DeckPilot window |
| `minimumSeverity` | A `choice` setting of `info`, `success`, `warning` and `error`: less severe notifications are not shown |

At least one member is required, and each must name a setting of `contributes.settings` of that kind. A member left out keeps the default of a plugin without routing: no operating-system notifications, the status bar, every severity. The grant, its low tier, the plugin-name prefix and the rate limit are the same with or without routing. An agent older than routing refuses the manifest.

### Open URL

`Context::open_url(url)` asks the agent to open a page in the user's browser.

- Only `https` URLs on an exactly granted host, without user information, of at most 2,048 bytes. Wildcard hosts validate but are never granted.
- Only in a run the user started: a key press, a gesture, the app, or a workflow the user started that way. In a run started by a schedule, a lifecycle event, the agent's start or a work session, the call is denied. `Context::run_context()` tells beforehand.
- `openUrl` gives no network access to the host, and `network` gives no right to open pages.
- For pages of a server the user enters, see [Pages of a user-entered server](#pages-of-a-user-entered-server).

## Always allowed

An enabled plugin may use these without any permission:

| Capability | Bound |
| --- | --- |
| Log | 50 lines per second |
| Own storage | 256 KiB and 1,000 keys by default; 100 calls per second |
| Own settings | |
| Run context | |
| Publishing and clearing own declared variables | 10 per second, 16 KiB per value |

## Rate limits

| Scope | Limit |
| --- | --- |
| All capability requests of a plugin | 200 per second |
| HTTP requests | 60 per minute |
| Notifications | 5 per minute |
| Opening URLs | 1 per 2 seconds, 20 per 10 minutes, 3 per invocation |
| Capability calls in one invocation | 256 |

A call over a limit fails with the kind `Throttled`, which `?` turns into the retryable code `throttled`.

## Consent

- **Installing grants nothing.** A plugin is installed disabled.
- **Enabling grants the permissions the user approves.** Every declared permission must be granted before a plugin can be enabled. A plugin without permissions is enabled with an empty consent.
- Each permission is granted under a key, for example `network:status.example.com` per host, `credentials:apiToken` per slot, `openUrl:status.example.com` per host, `network:user-origin:<setting>` and `openUrl:user-origin:<setting>` per origin setting, `notifications` and `storage`.
- **An update that adds or broadens a permission is an escalation.** The agent stages the update and asks for consent to the escalated keys. Declining keeps the old version. See [Compatibility](../compatibility/#updates-and-permission-escalation).

## Revocation

The user can revoke a granted permission at any time, and grant it again.

- The broker reads the grants on every call, so a revocation applies to the next capability call, even in the middle of a running action. A call that is already running is not interrupted.
- A denied call returns a `CapabilityError` of kind `NotPermitted`. In a handler, `?` turns it into the code `not-permitted` with a message that names the operation, the origin and the permission to declare.
- While a declared permission is not granted, the plugin's actions are listed as unavailable with the reason.
- A credential slot depends on its hosts' network grants. When the user revokes a host, the broker withholds that host from every slot that names it. A slot left without hosts is not granted at all.
- A disabled or uninstalled plugin has no grants: every call is denied, including the always-allowed ones.

Write handlers that expect a denial at any call and return its error.

## User-entered origins

Some services run on a host only the user knows: a self-managed Git server, for example. A `network` permission can name a text setting in which the user enters that server. This is implemented on `main`.

```json
{
  "kind": "network",
  "hosts": ["api.github.com"],
  "methods": ["GET", "POST"],
  "userOrigins": [{ "setting": "apiUrl", "pathPrefix": true, "privateNetwork": true }],
  "justification": "Reads pull requests from GitHub or your GitHub Enterprise server."
}
```

| Slot field | Rule |
| --- | --- |
| `setting` | The key of a one-line `text` setting in `contributes.settings` |
| `methods` | Optional subset of the permission's `methods` |
| `pathPrefix` | The user may enter a path; requests are then limited to that path and below |
| `privateNetwork` | The user may enter a server on a private network. Loopback and link-local addresses are never reachable |

A permission has at most 4 such slots, and `hosts` may be empty when `userOrigins` is present. A credential slot can be bound to one with `"userOrigin": "<setting>"`.

Consent works in steps:

1. Installing shows the slot as its own permission, `network:user-origin:<setting>`: the plugin may ask to reach a server the user enters.
2. Saving the setting validates and normalizes the value: `https` only, no user name or password, no query or fragment, no wildcard.
3. The user then allows exactly that server. A server on a private network is a separate, explicit choice in the same consent.
4. Changing the setting ends the consent at once, and so does revoking the slot permission or uninstalling.

The broker matches each request against the allowed server exactly, by scheme, host, port and path prefix, and checks every redirect the same way.

### Pages of a user-entered server

`openUrl.hosts` are fixed names, so they cannot cover a server only the user knows. The `openUrl` permission may name the origin settings whose server's pages the plugin opens:

```json
{
  "kind": "openUrl",
  "hosts": ["gitlab.com"],
  "userOrigins": ["apiUrl"],
  "justification": "Opens a project or a merge request on GitLab in your browser."
}
```

- Each entry must be the `setting` of a `network.userOrigins` slot; at most 4. `hosts` may then be empty.
- It is a permission of its own, `openUrl:user-origin:<setting>`, shown and granted like any other. An update that adds it is an escalation.
- By itself it opens nothing. Pages open only on the server the user entered in that setting **and allowed**, while this permission is granted: `https` pages of exactly that scheme, host and port. The consent to the server says so.
- It is by server, not by path prefix: a server's pages are usually not under its API's path. It allows no request; requests stay within the methods and path prefix the user allowed.
- A server on a private network opens only when it is exactly the one the user allowed on a private network.
- All other rules of [Open URL](#open-url) apply: only in a run the user started, and the same rate limits.
- Revoking this permission, revoking the consent to the server, changing the setting or uninstalling ends it on the next call. An agent older than this field refuses the manifest.

## Canonical reference

[docs/marketplace/sdk/permissions.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/permissions.md) has the exhaustive rules, including the full list of refused headers. The broker's source is [plugins/runtime/crates/broker](https://github.com/knittingcode/deckpilot/blob/main/plugins/runtime/crates/broker/src/lib.rs).
