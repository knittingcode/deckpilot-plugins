---
title: "Variables"
description: "Declare variables in the manifest, publish and clear them from an action, and how workflows and keys read them."
group: capabilities
order: 8
---

A plugin can publish named values, such as a build status or a counter, that workflows and keys read with a template. The plugin declares its variables in the manifest and sets them from an action. Publishing needs no permission, because a plugin can only publish variables it declared itself.

## Declaring variables

Each variable is an entry in `contributes.variables`:

```json
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
]
```

| Field | Rule |
| --- | --- |
| `id` | A relative path: `[a-z][A-Za-z0-9_]{0,31}`, with up to two more segments separated by dots, at most 100 characters, unique in the plugin |
| `name` | 1 to 48 characters |
| `description` | Optional, at most 300 characters |
| `valueType` | `{ "type": "string" }`, `"integer"`, `"boolean"` or `"json"` |
| `scope` | `machine`, `profile` or `project` |
| `requires`, `sensitive`, `deprecated` | Optional |

A plugin declares at most 64 variables.

> **Note:** The schema accepts three scopes, but only `machine` can be published in this version. Publishing a variable declared with `profile` or `project` scope fails with the kind `Unavailable`.

## Publishing

Inside a handler, `Context::publish_variable(id, value)` sets a variable. The value is anything that serializes to the declared type:

```rust
    ctx.publish_variable("status", &last.status)?;
    ctx.publish_variable("healthy", healthy)?;
```

The agent checks each call:

| Situation | Error kind |
| --- | --- |
| The id is not declared in the manifest | `NotPermitted` |
| The value does not have the declared `valueType` | `BadRequest` |
| The variable's scope is not `machine` | `Unavailable` |

Limits: 10 publish or clear calls per second, a value of at most 16 KiB, and an id of at most 100 characters.

## Clearing

A cleared variable has no value. `clear_variable(id)` clears one, `clear_variables()` clears every variable the plugin published, and publishing `null` clears as well.

```rust
fn reset(ctx: &mut Context<'_>, _args: &Args<'_>) -> ActionResult {
    ctx.storage_delete(LAST_CHECK)?;
    ctx.clear_variables()?;
    Ok(Output::message("forgot the last check"))
}
```

## How variables are read

Workflows and keys read a plugin variable with a template of the form:

```text
{{ plugins.<publisherCamel>.<nameCamel>.<id> }}
```

The publisher and the plugin name are written in lower camel case. For the plugin `acme.status-check` and the variable `status`, that is:

```text
{{ plugins.acme.statusCheck.status }}
```

The plugin id rules make this mapping unique: in an id, a hyphen is always followed by a letter.

The agent resolves the template from the last published value and announces each change to the app. No plugin code runs when a variable is read.

## Reading rules for plugins

A plugin cannot read variables in ABI 1, neither its own nor those of other plugins or of DeckPilot. There is no capability for it. The manifest schema already defines a `variables` permission with a `read` list for other owners' variables, but it allows nothing yet; see [Permissions](../permissions/#permission-kinds).

If an action needs a value it published earlier, keep a copy in [storage](../storage/).

## Lifetime of published values

Published values live in the agent's memory only.

| Event | Published values |
| --- | --- |
| The agent restarts, or the configuration is locked and unlocked | dropped |
| The plugin is disabled, updated, rolled back, uninstalled or quarantined | dropped |
| A permission is revoked or granted | kept |
| The plugin's host process crashes | kept |

Because a plugin runs only while one of its actions runs, it cannot republish on its own after a restart. Keep what must survive in storage and publish it again the next time an action runs. The example does exactly that: `check` stores the result and publishes it, and `last-check` can read the stored copy without a request.

## Testing

Under the mock host, a publish call is recorded as a `CapabilityRequest::VariablesPublish` with its `id` and `value`, so a test can assert what was published:

```rust
    assert!(host.requests().iter().any(|r| matches!(r,
        CapabilityRequest::VariablesPublish(v) if v.id == "status" && v.value == "up")));
```

The mock host does not check ids against the manifest. That check happens in the agent. See [Testing](../testing/).

## Canonical reference

The Variables section of [docs/marketplace/sdk/capabilities.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/capabilities.md) and the variables table in [docs/marketplace/sdk/manifest.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/manifest.md).
