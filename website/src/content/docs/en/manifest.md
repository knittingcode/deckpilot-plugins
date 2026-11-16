---
title: "Manifest format"
description: "Every field of manifest.json, manifest version 1, with the rules the JSON Schema and the agent apply, and a complete valid example."
group: build
order: 4
---

`manifest.json` sits at the root of a plugin's source directory and of its package. It is the only thing DeckPilot reads to learn what a plugin is, what it asks for and what it offers. The agent never runs plugin code to discover metadata, so the manifest and the code must agree.

The rules below come from the JSON Schema (draft 2020-12) at [plugins/schemas/plugin-manifest.schema.json](https://github.com/knittingcode/deckpilot/blob/main/plugins/schemas/plugin-manifest.schema.json) and from the validation in the `deckpilot-plugin-manifest` crate, which adds cross-field checks. Unknown fields are refused everywhere, and a manifest is at most 256 KiB.

## Top-level fields

| Field | Required | Rule |
| --- | --- | --- |
| `$schema` | no | Any string of at most 256 characters. The examples use `https://knittingcode.github.io/deckpilot-plugins/v1/schemas/plugin-manifest.schema.json` |
| `manifestVersion` | yes | `1` |
| `id` | yes | `<publisher>.<name>`. Each part matches `[a-z][a-z0-9]*(-[a-z][a-z0-9]*)*`; the publisher is 2 to 24 characters, the name 2 to 32. It must start with `publisher.id` and a dot |
| `name` | yes | Display name, 1 to 48 characters |
| `version` | yes | `X.Y.Z` with an optional `-alpha.N`, `-beta.N` or `-rc.N` (N at least 1). No build metadata, no leading zeros. Must equal the version in `Plugin::new` |
| `publisher` | yes | `{ "id", "name", "url"? }`. The `id` must not be reserved; see [Submission](../submission/#namespaces). `name` is 1 to 64 characters, `url` an https URL |
| `description` | yes | 1 to 300 characters |
| `homepage` | no | An https URL of at most 512 characters |
| `repository` | no | `{ "type": "git", "url": "https://github.com/<owner>/<repo>", "directory"? }` |
| `license` | yes | An SPDX expression, 1 to 64 characters. The package must contain a `LICENSE` file |
| `keywords` | no | At most 10 unique keywords, each `[a-z0-9-]{2,24}` |
| `categories` | no | At most 3 unique values of `developer-tools`, `productivity`, `communication`, `monitoring`, `media`, `smart-home`, `other` |
| `engines` | yes | Version requirements; see [Compatibility](../compatibility/#engines) |
| `runtime` | in practice yes | `{ "kind": "wasm", "abi": 1 }`. Optional in the schema, but a package without it is refused |
| `entry` | yes | Informational relative path of the source entry, for example `src/lib.rs`. The agent ignores it |
| `platforms` | no | A non-empty unique list of `macos`, `windows`, `linux`. Omitted means all three |
| `icons` | yes | `{ "128": "assets/….png" }` is required; `32` and `256` are optional. The agent shows the 128 pixel icon only if it is a PNG of at most 256 KiB |
| `assets` | no | `keyIcons` (at most 64 PNGs by name), `screenshots` (at most 8 PNGs), `readme` (`"README.md"`), `changelog` (`"CHANGELOG.md"`). Everything named here is packaged |
| `permissions` | yes | An array of at most 16 entries; may be empty. See [Permissions](../permissions/) |
| `supports` | no | Device families the plugin requires or uses. Accepted by the schema, not used by the agent in this version |
| `contributes` | yes | `actions`, `variables`, `settings`, `credentials`, `errorCodes`; each is optional |

## Actions

`contributes.actions` holds at most 64 actions. The agent registers each as the action type `<plugin id>.<action id>`.

| Field | Required | Rule |
| --- | --- | --- |
| `id` | yes | `[a-z][a-z0-9]*(-[a-z0-9]+)*`, 1 to 48 characters, unique. Must equal the id in `Action::new` |
| `name` | yes | 1 to 48 characters |
| `description` | yes | 1 to 300 characters |
| `category` | yes | 1 to 32 characters: the group in the editors' pickers |
| `parameters` | yes | At most 32; may be empty. See [Parameters](../parameters/) |
| `node` | no | The workflow node contract: outputs, timeout, retry and effect. See [Actions](../actions/#node-metadata) |
| `requiresProject`, `deprecated`, `replacedBy` | no | `replacedBy` names another action of this plugin and needs `deprecated: true` |
| `state` | no | Validated, but ABI 1 has no state call, so no button state is ever pulled from a plugin |
| `device` | no | Accepted, not used |

## Variables

`contributes.variables` declares at most 64 variables the plugin can publish. See [Variables](../variables/).

| Field | Rule |
| --- | --- |
| `id` | A relative path: `[a-z][A-Za-z0-9_]{0,31}` with up to two more segments separated by dots, at most 100 characters, unique |
| `name`, `description` | 1 to 48 characters; at most 300 |
| `valueType` | `{ "type": "string" }`, `integer`, `boolean` or `json` |
| `scope` | `machine`, `profile` or `project`. Only `machine` can be published in this version |
| `requires`, `sensitive`, `deprecated` | Optional |

## Settings

`contributes.settings` is `{ "portabilityVersion"?, "fields": [...] }` with at most 32 fields. A field has the same `key`, `label`, `description`, `kind`, `required` and `default` as an action parameter, plus `portability`: `portable`, `machineSpecific` (the default) or `sensitive`. The key `privateHosts` is reserved. There are no secret settings; secrets are credential slots. See [Parameters](../parameters/#plugin-settings).

## Credentials

`contributes.credentials` declares at most 8 credential slots. See [Credentials](../credentials/).

| Field | Rule |
| --- | --- |
| `key` | camelCase, unique. Must be listed in the `refs` of a `credentials` permission, and every ref must be a slot |
| `label`, `description` | 1 to 48 characters; at most 300 |
| `hosts` | 1 to 8 exact hosts, each covered by the `network` permission's `hosts`. May be empty only with `userOrigin` |
| `userOrigin` | The setting of a `network.userOrigins` slot; see [Permissions](../permissions/#user-entered-origins) |
| `schemes` | A non-empty subset of `bearer`, `basic`, `header`, `query` |
| `headerName` | Required with `header`: `[A-Za-z0-9-]{1,64}` |
| `queryName` | Required with `query`: `[A-Za-z0-9_.-]{1,64}` |
| `setupUrl` | An https URL where the user creates the secret |

## Error codes

`contributes.errorCodes` maps at most 64 codes to a meaning of 1 to 200 characters. A code matches `[a-z][a-z0-9]*(-[a-z0-9]+)*`, is at most 64 characters, and must not redeclare a shared code. See [Actions](../actions/#error-codes).

## The module must agree

Three checks keep the manifest and the code together:

- **Action ids.** When the host loads the module, the supervisor compares the action ids the module reports with the manifest's and refuses a mismatch.
- **Parameters.** The agent validates every invocation against the manifest before the host runs, and the SDK validates again against the `Param` declarations.
- **A unit test.** `testing::manifest_mismatches` lists every difference in id, version, `engines.pluginApi`, `engines.sdk`, action ids and order, and each parameter's `required`, kind, bounds, options and default. See [Testing](../testing/#code-and-manifest-agree).

## A complete example

This manifest belongs to the worked example used throughout these pages, a plugin that checks a service on a status API. It uses four permission kinds, four actions with outputs, two variables, one setting, one credential slot and five error codes. It validates against the schema and was accepted by `deckpilot-registry package` together with its module.

```json
{
  "$schema": "https://knittingcode.github.io/deckpilot-plugins/v1/schemas/plugin-manifest.schema.json",
  "manifestVersion": 1,
  "id": "acme.status-check",
  "name": "Status Check",
  "version": "0.1.0",
  "publisher": { "id": "acme", "name": "Acme Tools", "url": "https://acme.example" },
  "description": "Checks whether a service is up on the Acme status API and remembers the last result.",
  "repository": { "type": "git", "url": "https://github.com/acme/status-check" },
  "license": "Apache-2.0",
  "keywords": ["status", "monitoring"],
  "categories": ["monitoring"],
  "engines": { "deckpilot": ">=0.5.0 <1.0.0", "pluginApi": "^1.0", "sdk": "0.1.0" },
  "runtime": { "kind": "wasm", "abi": 1 },
  "entry": "src/lib.rs",
  "icons": { "128": "assets/icon-128.png" },
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
  ],
  "contributes": {
    "actions": [
      {
        "id": "check",
        "name": "Check service",
        "description": "Reads the current status of a service and publishes it as a variable.",
        "category": "Status Check",
        "parameters": [
          {
            "key": "service",
            "label": "Service",
            "description": "The service to check. Empty uses the default service from the settings.",
            "required": false,
            "kind": { "type": "text", "maxLength": 64 }
          },
          {
            "key": "minUptime",
            "label": "Minimum uptime (%)",
            "required": false,
            "kind": { "type": "integer", "min": 0, "max": 100 },
            "default": { "type": "integer", "value": 99 }
          },
          {
            "key": "notify",
            "label": "Notify on change",
            "required": false,
            "kind": { "type": "boolean" },
            "default": { "type": "boolean", "value": false }
          }
        ],
        "node": {
          "version": 1,
          "icon": "activity",
          "outputs": [
            { "key": "status", "label": "Status", "kind": "text", "always": true },
            { "key": "uptime", "label": "Uptime (%)", "kind": "integer", "always": true },
            { "key": "healthy", "label": "Healthy", "kind": "boolean", "always": true },
            { "key": "checkedAt", "label": "Checked at", "kind": "text" }
          ],
          "timeoutMs": 20000,
          "retrySafe": true,
          "idempotent": true,
          "effect": "readOnly"
        }
      },
      {
        "id": "last-check",
        "name": "Last check",
        "description": "Shows the result of the last check from the plugin's storage, without a request.",
        "category": "Status Check",
        "parameters": [],
        "node": {
          "version": 1,
          "outputs": [
            { "key": "service", "label": "Service", "kind": "text" },
            { "key": "status", "label": "Status", "kind": "text" },
            { "key": "checkedAt", "label": "Checked at", "kind": "text" }
          ],
          "timeoutMs": 5000,
          "retrySafe": true,
          "idempotent": true,
          "effect": "readOnly"
        }
      },
      {
        "id": "open-page",
        "name": "Open status page",
        "description": "Opens the status page of a service in the browser.",
        "category": "Status Check",
        "parameters": [
          {
            "key": "service",
            "label": "Service",
            "required": true,
            "kind": { "type": "text", "maxLength": 64 }
          }
        ],
        "node": {
          "version": 1,
          "outputs": [{ "key": "url", "label": "Page", "kind": "url", "always": true }],
          "timeoutMs": 5000,
          "effect": "opens"
        }
      },
      {
        "id": "reset",
        "name": "Forget last check",
        "description": "Deletes the stored result and clears the published variables.",
        "category": "Status Check",
        "parameters": [],
        "node": { "version": 1, "timeoutMs": 5000, "idempotent": true, "effect": "modifies" }
      }
    ],
    "variables": [
      {
        "id": "status",
        "name": "Service status",
        "description": "The status the last check found, for example up or degraded.",
        "valueType": { "type": "string" },
        "scope": "machine"
      },
      {
        "id": "healthy",
        "name": "Service healthy",
        "valueType": { "type": "boolean" },
        "scope": "machine"
      }
    ],
    "settings": {
      "fields": [
        {
          "key": "defaultService",
          "label": "Default service",
          "description": "Checked when an action names no service.",
          "required": false,
          "kind": { "type": "text", "maxLength": 64 },
          "portability": "portable"
        }
      ]
    },
    "credentials": [
      {
        "key": "apiToken",
        "label": "Acme API token",
        "description": "A read-only token of the Acme status API.",
        "hosts": ["status.example.com"],
        "schemes": ["bearer"],
        "setupUrl": "https://status.example.com/account/tokens"
      }
    ],
    "errorCodes": {
      "service-required": "No service was named and the settings have no default service.",
      "invalid-service": "The service name contains characters the status API does not use.",
      "service-not-found": "The status API does not know this service.",
      "status-api-error": "The status API answered with an error status.",
      "invalid-response": "The status API answered with something that is not a status."
    }
  }
}
```

The host `status.example.com` is a placeholder. Real manifests of the three example plugins are in the DeckPilot repository; [Developer Tools](https://github.com/knittingcode/deckpilot/blob/main/plugins/examples/developer-tools/manifest.json) uses every section.

## Canonical reference

[docs/marketplace/sdk/manifest.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/manifest.md) is the exhaustive field reference.
