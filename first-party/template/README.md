# Template (`deckpilot.template`)

The starting point for a DeckPilot first-party plugin. It is built and
tested on every pull request and is **never published**.

Polish: [README.pl.md](README.pl.md).

## What it does

| Action | Controls | What |
| --- | --- | --- |
| `count` | key: press; dial: clockwise turn | Adds `step` (1-100, default 1) to a counter and reports the new value |
| `reset` | keys only (no `controls`) | Sets the counter back to zero |

The counter is kept in the plugin's storage, so it survives a restart of
DeckPilot, and is published as the variable
`{{ plugins.deckpilot.template.count }}` for workflows. The setting
**Label** is the word in front of the count in the action's message.

## Plugin API 1.3

The manifest declares `"pluginApi": "^1.3"` (DeckPilot 0.8.0 or later)
because it uses what 1.3 added:

- `controls` on `count`: it can be bound to a key (press) and to a dial
  (each clockwise detent adds `step`). There is no counter-clockwise turn,
  because nothing counts down. The code declares the same with
  `Action::controls`, and a unit test compares the two.
- No `controls` on `reset`: an action without them works on keys only
  (press, release, click, double click, long press), never on a dial or a
  touch zone. This is right for most actions.
- `contributes.navigation` and `contributes.views`: a toolbar entry
  **Counter** with the icon `assets/nav-counter.svg` opens a view of the
  plugin's variables, drawn by DeckPilot itself; the plugin ships no UI
  code. Navigation icons are PNG, WebP or SVG files under `assets/`; an SVG
  with a script, an event handler, an external link or the like is refused
  when the package is built.

A plugin that needs none of this keeps `"pluginApi"` at the lowest level it
uses; `controls`, `navigation` and `views` below `^1.3` are refused.

## Permissions

None. Storage, settings, the plugin's own variables and the log need no
permission.

## Errors

| Code | Meaning |
| --- | --- |
| `counter-full` | The counter reached its largest value; reset it |

## Starting a plugin from it

See [`first-party/README.md`](../README.md#adding-a-plugin).
