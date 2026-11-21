---
title: "Storage"
description: "The persistent key-value store of a plugin: the Context storage API, quotas and limits, isolation, and what survives which event."
group: capabilities
order: 10
---

Every plugin has its own persistent key-value store. It is the place for anything that must outlive one invocation: the last result, a cursor, a small cache. Storage needs no permission, is isolated per plugin, and lives in the agent's encrypted plugin database.

## The API

All storage calls are methods of `Context`. Values are JSON.

| Method | Returns | Does |
| --- | --- | --- |
| `storage_get(key)` | `Option<serde_json::Value>` | Reads a value; `None` when the key is not set |
| `storage_get_as::<T>(key)` | `Option<T>` | Reads and deserializes into a `serde` type |
| `storage_set(key, value)` | `()` | Writes any value that serializes to JSON |
| `storage_delete(key)` | `()` | Removes a key |
| `storage_keys(prefix)` | `Vec<String>` | Lists the plugin's keys, sorted, optionally only those starting with a prefix |
| `storage_usage()` | `StorageUsage` | Reports `used_bytes`, `keys`, `quota_bytes` and `max_keys` |

Each returns a `Result` with a `CapabilityError`.

## Storing typed values

Define a `serde` type for what you keep:

```rust
/// What `check` keeps in storage.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LastCheck {
    pub service: String,
    pub status: String,
    pub checked_at: String,
}
```

Read the previous value, then write the new one:

```rust
    let previous: Option<LastCheck> = ctx.storage_get_as(LAST_CHECK)?;
    ctx.storage_set(LAST_CHECK, &last)?;
```

A later action can read it back without any network request:

```rust
    let Some(last) = ctx.storage_get_as::<LastCheck>(LAST_CHECK)? else {
        return Ok(Output::skipped("nothing checked yet"));
    };
```

If you change the shape of a stored type in a later version, values written by the old version are still there after the update. Make new fields optional or give them defaults, so that old values still deserialize.

## Quotas and limits

| Limit | Value |
| --- | --- |
| Total size | 256 KiB by default |
| Number of keys | 1,000 |
| Key length | 1 to 128 characters, no control characters |
| One value | At most 64 KiB of JSON |
| Call rate | 100 storage calls per second |

Bytes are counted as the key plus the JSON value, in UTF-8.

### Raising the quota

A `storage` permission raises the total size to its `quotaKb`, between 257 and 10,240 KiB (10 MiB):

```json
{
  "kind": "storage",
  "quotaKb": 1024,
  "justification": "Keeps a history of the last checks of each service you follow."
}
```

This is a permission like any other: the user sees it and must approve it. Ask for it only if 256 KiB is really not enough. The number of keys stays at 1,000.

### When the quota is reached

A write that would grow the store past the quota fails with the kind `Failed` and a detail that starts with `storage-quota`. A write that does not grow the store is still allowed above the quota, which matters after a `storage` permission was revoked: the plugin can still shrink or overwrite its data.

`storage_usage()` lets an action check beforehand or report how much it uses.

## Isolation

- The store belongs to one plugin on one computer. No plugin can reach another plugin's keys; the plugin id is never part of a request, because the agent knows which plugin's host is asking.
- It lives in the agent's encrypted plugin database.
- It is never exported with the DeckPilot configuration.
- It is not a place for secrets. A token belongs in a [credential slot](../credentials/), where plugin code cannot read it.

## What survives

| Event | Storage |
| --- | --- |
| The agent restarts | kept |
| The plugin is disabled and enabled again | kept |
| The plugin is updated or rolled back | kept |
| The plugin's host process crashes | kept |
| A permission is revoked | kept |
| The plugin is uninstalled | **deleted** |

Storage is always deleted on uninstall. It is the only durable state of a plugin that plugin code can read: values in statics of the module live only as long as the host process, and published [variables](../variables/#lifetime-of-published-values) are dropped when the agent restarts.

## Storage and the missing clock

A module has no clock of its own. From plugin API 1.1 it can ask the agent for the time with `Context::now`. The examples, written for 1.0, use the `Date` header of an HTTP response as the time of a check:

```rust
        // The guest has no clock: the time of the check is the answer's Date header.
        checked_at: response.header("date").unwrap_or_default().chars().take(64).collect(),
```

For the same reason a plugin cannot expire cached values by age unless it has such a reference time.

## Testing

`MockHost::allow_all()` answers every storage read with `null`, which reads as "not set". A scripted host can return stored values, and `host.requests()` shows what was written:

```rust
    assert!(host.requests().iter().any(|r| matches!(r,
        CapabilityRequest::StorageSet(s) if s.key == "last.check")));
```

The Developer Tools example goes further and keeps an in-memory map inside its mock host, so that a value written by one invocation is read by the next. See [Testing](../testing/).

## Canonical reference

The Storage section of [docs/marketplace/sdk/capabilities.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/capabilities.md).
