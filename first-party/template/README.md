# Template (`deckpilot.template`)

The starting point for a DeckPilot first-party plugin. It is built and
tested on every pull request and is **never published**.

Polish: [README.pl.md](README.pl.md).

## What it does

| Action | What |
| --- | --- |
| `count` | Adds `step` (1-100, default 1) to a counter and reports the new value |
| `reset` | Sets the counter back to zero |

The counter is kept in the plugin's storage, so it survives a restart of
DeckPilot, and is published as the variable
`{{ plugins.deckpilot.template.count }}` for workflows. The setting
**Label** is the word in front of the count in the action's message.

## Permissions

None. Storage, settings, the plugin's own variables and the log need no
permission.

## Errors

| Code | Meaning |
| --- | --- |
| `counter-full` | The counter reached its largest value; reset it |

## Starting a plugin from it

See [`first-party/README.md`](../README.md#adding-a-plugin).
