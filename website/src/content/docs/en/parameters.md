---
title: "Parameters and settings"
description: "Parameter kinds, required values and defaults, the two validation passes, reading values with Args, and per-plugin settings."
group: build
order: 6
---

Parameters are the typed inputs of an action. The user fills them in when configuring a key or a workflow node, the agent validates them, and the handler reads them from `Args`. Settings use the same kinds but belong to the plugin as a whole.

## Declaring a parameter

In the manifest, a parameter is an object in `contributes.actions[].parameters`:

```json
{
  "key": "minUptime",
  "label": "Minimum uptime (%)",
  "required": false,
  "kind": { "type": "integer", "min": 0, "max": 100 },
  "default": { "type": "integer", "value": 99 }
}
```

| Field | Rule |
| --- | --- |
| `key` | camelCase, `[a-z][A-Za-z0-9]{0,31}` |
| `label` | 1 to 48 characters |
| `description` | Optional, at most 300 characters |
| `kind` | An object with a `type` and the fields of that kind |
| `required` | A boolean, always present |
| `default` | Only on optional parameters: `{ "type": "text" \| "integer" \| "boolean" \| "choice", "value": … }` |

In code, the same parameter is a `Param` on the action:

```rust
            Action::new("check", check)
                .param(Param::text("service").max_length(64))
                .param(Param::integer("minUptime").min(0.0).max(100.0).default(99))
                .param(Param::boolean("notify").default(false)),
```

A parameter is optional unless `.required()` is called. A default is allowed on optional parameters only, and `Plugin::validate()` checks it against the kind.

## Kinds

| `kind.type` | Kind fields | Arrives as | SDK declaration | Read as |
| --- | --- | --- | --- | --- |
| `text` | `maxLength` (1 to 100,000), `multiline` | string | `Param::text(key)`, `.max_length(n)` | `String` |
| `integer` | `min`, `max` | integer | `Param::integer(key)`, `.min(a)`, `.max(b)` | `i64` |
| `boolean` | | boolean | `Param::boolean(key)` | `bool` |
| `choice` | `options` (1 to 64 unique options of 1 to 64 characters) | string | `Param::choice(key, &[..])` | `String` |
| `project` | | the project's id, as a string | `Param::text(key)` | `String` |
| `lines` | `itemLabel`, `maxItems`, `maxLength` | the text | `Param::text(key)` | `String` |
| `pairs` | `separator` (`=` or `:`), `nameLabel`, `valueLabel`, `maxItems`, `caseInsensitive`, `noSecrets` | the text | `Param::text(key)` | `String` |
| `code` | `language` (`json`, `text`, `yaml`, `markdown`), `maxLength` | the text | `Param::text(key)` | `String` |

The last four kinds give the user a richer editor but reach the plugin as text, so they are declared as `Param::text` in code.

One text value is at most 16 KiB of UTF-8 in ABI 1. A `maxLength` above 4,096 characters can therefore be refused by the host for non-ASCII text, rather than by the agent's friendlier check.

### Kinds the manifest does not have

The SDK also offers `Param::number` (a finite `f64`) and `Param::secret` (a credential slot key). Manifest version 1 has no kind for either, so the agent refuses any value for such a parameter as undeclared. They work under the mock host in unit tests but cannot be reached through the agent. Use `integer` or `text`, and name credential slots in code; see [Credentials](../credentials/).

## Validation

Parameters are validated twice before a handler runs.

1. **The agent** normalizes the parameters against the manifest, whatever started the action. Unknown parameters, wrong types, values out of range, options not offered, texts that are too long and missing required parameters fail with `invalid-parameters` and a message listing every problem. A blank required text counts as missing. Defaults are filled in. References and templates are resolved by the agent, so the plugin only ever sees literal values.
2. **The SDK** checks the same against the `Param` declarations: unknown keys, kinds, bounds and options. It fills defaults and refuses with `invalid-parameters`, naming the parameter but never quoting its value.

A parameter the manifest declares but the code does not is refused by the SDK as unknown, which is one reason to keep the two in step with a test.

Rules the kinds cannot express are yours to check. Return `invalid-parameters` or a code of your own:

```rust
fn is_service(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}
```

## Reading values with `Args`

`Args::get::<T>(key)` returns the checked value. `T` is `String`, `i64`, `f64`, `bool`, `SecretHandle`, or `Option<T>` of those.

```rust
    let settings = ctx.settings()?;
    let service = service_of(args, &settings)?;
    let min_uptime: i64 = args.get("minUptime")?; // default filled in: 99
    let notify: bool = args.get("notify")?;
```

Read an optional parameter without a default as `Option<T>`:

```rust
/// The service from the parameter, else the `defaultService` setting.
fn service_of(args: &Args<'_>, settings: &Settings) -> Result<String, PluginError> {
    let service = match args.get::<Option<String>>("service")? {
        Some(s) if !s.trim().is_empty() => s.trim().to_owned(),
        _ => match settings.text("defaultService") {
            Some(s) if !s.trim().is_empty() => s.trim().to_owned(),
            _ => {
                return Err(PluginError::new(
                    "service-required",
                    "name a service or set a default service in the plugin's settings",
                ));
            }
        },
    };
    if !is_service(&service) {
        return Err(PluginError::new(
            "invalid-service",
            "a service name has lowercase letters, digits and hyphens only",
        ));
    }
    Ok(service)
}
```

The errors of `get` are typed:

| Situation | Code |
| --- | --- |
| An optional parameter without a value, read as `T` instead of `Option<T>` | `invalid-parameters` |
| A key the action does not declare | `plugin-error` |
| A `T` that does not fit the parameter's kind | `plugin-error` |

`Args::has(key)` tells whether a value is present, and `Args::values()` gives the raw checked values.

## Plugin settings

Settings are values the user sets once for the whole plugin, such as a default repository. They are implemented in ABI 1.

Declare them in `contributes.settings.fields`. A field is a parameter with one more property, `portability`:

```json
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
}
```

`portability` is `portable`, `machineSpecific` (the default) or `sensitive`. A plugin has at most 32 settings fields.

Read them with `Context::settings()`, which needs no permission. The returned `Settings` has `text(key)`, `integer(key)`, `boolean(key)`, `get::<T>(key)` and `contains(key)`.

What to know about settings:

- The agent validates saved values like action parameters and refuses a value that does not fit its field.
- A saved value that no longer fits after an update falls back to the default. A field with neither a value nor a default is absent.
- A missing required setting is reported to the app, but the action still runs. Handle the absent case in code, as `service_of` does.
- There is no change notification in ABI 1. Read the settings in each invocation.
- Settings are stored in the agent's plugin database and are not exported with the configuration.
- There are no secret settings. Never ask a user to put a secret into a setting or a parameter; use a credential slot.

A text setting can also hold a server address the user enters, when a `network` permission names it in `userOrigins`; see [Permissions](../permissions/#user-entered-origins).

## Canonical reference

[docs/marketplace/sdk/actions.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/actions.md) for parameters and [docs/marketplace/sdk/capabilities.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/capabilities.md) for settings.
