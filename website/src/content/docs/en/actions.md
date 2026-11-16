---
title: "Actions"
description: "Declare plugin actions in Rust and in manifest.json, return outputs and typed errors, and set node metadata such as timeout and effect."
group: build
order: 5
---

An action is the unit of work a plugin offers: something a key, a workflow node or the app's Run button can invoke. Every action is declared twice, once in `manifest.json` for the agent, the editors and the user, and once in code for what actually runs. The two declarations must agree.

## Declaring actions in code

The plugin definition lists the actions with their parameters. This is the worked example of these pages, a plugin that checks a service on a status API:

```rust
pub fn plugin() -> Plugin {
    Plugin::new("acme.status-check", "0.1.0")
        .on_init(init)
        .on_shutdown(shutdown)
        .action(
            Action::new("check", check)
                .param(Param::text("service").max_length(64))
                .param(Param::integer("minUptime").min(0.0).max(100.0).default(99))
                .param(Param::boolean("notify").default(false)),
        )
        .action(Action::new("last-check", last_check))
        .action(
            Action::new("open-page", open_page)
                .param(Param::text("service").required().max_length(64)),
        )
        .action(Action::new("reset", reset))
}

deckpilot_plugin_sdk::plugin!(plugin);
```

- The arguments of `Plugin::new` equal the manifest's `id` and `version`.
- The first argument of `Action::new` equals `contributes.actions[].id`.
- A handler has the type `fn(&mut Context<'_>, &Args<'_>) -> ActionResult`.
- A plugin has at most 64 actions, and an action at most 32 parameters.

## Declaring actions in the manifest

The matching manifest entry adds what users and editors see: a name, a description, a category, labels for the parameters and the node metadata.

```json
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
}
```

The agent registers the action as the type `<plugin id>.<action id>`, here `acme.status-check.last-check`. Field rules are on the [Manifest format](../manifest/#actions) page.

## Handlers

A handler reads its parameters from `Args`, reaches the outside world through `Context`, and returns a result. This one reads the plugin's storage and needs no permission:

```rust
fn last_check(ctx: &mut Context<'_>, _args: &Args<'_>) -> ActionResult {
    let Some(last) = ctx.storage_get_as::<LastCheck>(LAST_CHECK)? else {
        return Ok(Output::skipped("nothing checked yet"));
    };
    Ok(Output::message(format!("{}: {}", last.service, last.status))
        .with("service", last.service)
        .with("status", last.status)
        .with("checkedAt", last.checked_at))
}
```

Before a handler runs, the parameters have been validated twice: by the agent against the manifest, and by the SDK against the `Param` declarations. See [Parameters](../parameters/#validation).

## Results

A handler returns `ActionResult`, which is `Result<Output, PluginError>`.

| Constructor | Outcome | Meaning |
| --- | --- | --- |
| `Output::ok()` | `ok` | Success, no message |
| `Output::message(text)` | `ok` | Success with a message for the user, at most 1,024 characters |
| `Output::skipped(text)` | `skipped` | Nothing to do; a workflow step counts as skipped |
| `.with(key, value)` | | Adds one named output |
| `Err(PluginError::new(code, message))` | `failed` | A typed failure; `.retryable()` marks it transient |

`.with` accepts strings, booleans, integers (`i8` to `i64`, `u8` to `u32`), `f32` and `f64`, and a `Vec` of those.

Before a result leaves the module, the SDK checks it against the bounds of ABI 1: at most 64 outputs, names of at most 64 characters, text values of at most 16 KiB, lists of at most 256 items, nesting of at most 4 levels and finite numbers. The whole output is at most 192 KiB of JSON. A result outside those bounds becomes a failure with the code `invalid-result`.

Never put a secret into a message or an output.

## Outputs

An output is a named value a workflow can use after the action ran. Declare each one in `node.outputs`:

| Field | Rule |
| --- | --- |
| `key` | camelCase, `[a-z][A-Za-z0-9]{0,31}`; the key passed to `.with` |
| `label` | 1 to 48 characters |
| `kind` | `text` (the default), `integer`, `boolean`, `path`, `url` or `projectKey` |
| `always` | Whether every successful run sets it |
| `sensitive` | Marks the value as sensitive |

The agent keeps only declared outputs. Anything else an action returns with `.with` is dropped. An action has at most 32 outputs. In workflows every output value becomes text; the app's Run button receives them as JSON.

## Node metadata

The optional `node` object tells the workflow engine how the action behaves.

| Field | Values | Meaning |
| --- | --- | --- |
| `version` | integer, at least 1 | Version of the node contract |
| `icon` | `[a-z0-9-]{1,64}` | Icon name in the editors |
| `outputs` | at most 32 | See above |
| `timeoutMs` | 1,000 to 600,000; default 30,000 | The wall-clock limit of one invocation. When it passes, the host is stopped and the action fails |
| `retrySafe` | boolean, default `false` | A retry cannot cause harm. A retryable error is honoured only for actions that set it |
| `idempotent` | boolean, default `false` | Running twice has the same effect as running once |
| `effect` | `readOnly` (default), `opens`, `modifies` | What the action does to the outside world. Validation warns when a `modifies` action is `retrySafe` |
| `cancellable` | boolean | Accepted; the agent treats every plugin action as cancellable |

Choose the timeout honestly. An action that makes one HTTP request needs little more than the 15 seconds the agent allows for a request.

## Error codes

A failed action carries a code, a message of at most 1,024 characters and whether a retry may help. A code matches `[a-z][a-z0-9]*(-[a-z0-9]+)*` and is at most 64 characters.

### Shared codes

Shared codes may be returned without being declared and must not be redeclared. The SDK exposes the common ones as constants in `codes`:

| Code | Constant | Typical source |
| --- | --- | --- |
| `not-permitted` | `NOT_PERMITTED` | A capability the user did not grant, through `?` |
| `invalid-parameters` | `INVALID_PARAMETERS` | The agent's or the SDK's parameter check; input the kinds cannot express |
| `credential-missing` | `CREDENTIAL_MISSING` | A credential slot without a stored secret |
| `request-failed` | `REQUEST_FAILED` | A capability call that failed, through `?` |
| `throttled` | `THROTTLED` | A rate limit, local or remote |
| `storage-quota` | `STORAGE_QUOTA` | The storage quota |
| `timeout` | `TIMEOUT` | A remote timeout |
| `unsupported` | `UNSUPPORTED` | An unknown action id or operation |
| `plugin-error` | `PLUGIN_ERROR` | A bug: an invalid definition, a failed init |
| `invalid-result` | `INVALID_RESULT` | A result outside the bounds of ABI 1 |

A failed HTTP request can also surface one of the network codes `blocked-address`, `dns-failed`, `connection-failed`, `tls-failed`, `too-many-redirects` or `insecure-redirect`. The full list is `codes::SHARED`.

The agent adds codes of its own for keys and workflow nodes, among them `cancelled`, `plugin-host-failed`, `plugin-busy`, `plugin-revoked` and `plugin-unavailable`.

### Your own codes

Map remote failures to codes the user can act on, and declare each in `contributes.errorCodes`:

```rust
    match response.status {
        200..=299 => Ok(response),
        404 => Err(PluginError::new(
            "service-not-found",
            "the status API does not know this service",
        )),
        429 => Err(PluginError::new(codes::THROTTLED, "the status API is rate limiting").retryable()),
        s @ 500..=599 => Err(PluginError::new(
            "status-api-error",
            format!("the status API failed (HTTP {s})"),
        )
        .retryable()),
        s => Err(PluginError::new(
            "status-api-error",
            format!("the status API answered HTTP {s}"),
        )),
    }
```

```json
"errorCodes": {
  "service-required": "No service was named and the settings have no default service.",
  "invalid-service": "The service name contains characters the status API does not use.",
  "service-not-found": "The status API does not know this service.",
  "status-api-error": "The status API answered with an error status.",
  "invalid-response": "The status API answered with something that is not a status."
}
```

The declaration documents the code for users and reviewers. This version of the agent does not refuse an undeclared code at run time, only a malformed one. A unit test that compares the codes the plugin returns with the manifest keeps the two in step; see [Testing](../testing/).

Do not panic. A panic ends the host process, the action fails with `plugin-host-failed`, and repeated crashes disable the plugin; see [Lifecycle](../lifecycle/#crashes-and-restarts).

## Canonical reference

[docs/marketplace/sdk/actions.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/actions.md) and [docs/marketplace/sdk/errors-and-versions.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/errors-and-versions.md).
