# Break & Hydration (`deckpilot.break-reminder`)

Reminds you to take a break and to drink water, each at its own interval,
with snooze, skip, pause and quiet hours.

Polish: [README.pl.md](README.pl.md).

> **Read this first.** A DeckPilot plugin has no timer and no clock, and it
> runs only while one of its actions runs (plugin API 1.0). This plugin
> therefore does nothing by itself: **you add one workflow that runs its
> Tick action every minute** ([Setup](#setup)). And a key **cannot show the
> countdown** or whether the reminders are paused, because a plugin cannot
> give a key its state yet ([What it does not do](#what-it-does-not-do)).

## Setup

Once per profile in which you want reminders. Workflow triggers fire only
for the active profile, while DeckPilot's configuration is unlocked.

1. Install and enable the plugin; allow its one permission, notifications.
2. Create a workflow, for example "Break reminder tick".
3. In the workflow panel (nothing selected), add an **input**: key
   `firedAt`, type text, required.
4. Add a **trigger**: schedule, every **1** minute. For the input
   `firedAt` choose the event field **Date and time**.
5. Add one action node after the start node: **Break & Hydration › Tick**.
   Map its parameter **Date and time** to the start node's output
   `firedAt` (the workflow's input).
6. Save. The first tick starts both countdowns; nothing is shown before a
   countdown ends.

As stored in the profile, the parts that matter look like this (the action
node refers to an action of type `deckpilot.break-reminder.tick`):

```json
{
  "inputs": [{ "key": "firedAt", "label": "Date and time", "kind": "text", "required": true }],
  "triggers": [
    { "type": "schedule", "schedule": { "every": "interval", "minutes": 1 },
      "inputs": { "firedAt": { "type": "event", "field": "firedAt" } } }
  ],
  "graph": { "nodes": [
    { "id": "start", "type": "trigger" },
    { "id": "tick", "type": "action", "action": "<the Tick action>",
      "mappings": { "firedAt": { "type": "template", "template": "{{ steps.start.output.firedAt }}" } } }
  ] }
}
```

The trigger's date and time (`YYYY-MM-DD HH:MM`, your computer's local
time) is the plugin's only clock. A schedule of more than a minute works
too, with reminders up to that much late, as long as it is **shorter than
the setting "Away after"** (5 minutes by default): a longer pause between
two ticks is taken for a break and starts the countdowns again.

Then put the other actions on keys as you like: Snooze, Skip next, Pause
or resume, Restart countdown, Status.

## Actions

| Action | What |
| --- | --- |
| `tick` (Tick) | Takes the minutes since the last tick off both countdowns and shows the reminders that are due. Parameter `firedAt`: the trigger's date and time. For the workflow, not for a key |
| `snooze` (Snooze) | Shows a reminder again in `minutes` (1-120, default 10). `reminder`: `last` (the one shown most recently, the default), `break`, `hydration` or `both` |
| `skip` (Skip next) | Passes over the next reminder once: when it is due nothing is shown and its countdown starts again. `reminder`: `break` (default), `hydration`, `both` |
| `restart` (Restart countdown) | Starts a countdown again from its full interval, for a break you took by yourself. `reminder`: `both` (default), `break`, `hydration` |
| `pause` (Pause) | Stops the countdowns and the reminders until you resume, or for `minutes` (0-1440; 0, the default: until you resume) |
| `resume` (Resume) | Continues where the countdowns stopped |
| `toggle-pause` (Pause or resume) | One key for both |
| `status` (Status) | Reports the state as of the last tick; changes nothing |

Every action answers with a message such as `Break in 12 min, water in
22 min` and these outputs for later workflow nodes: `state` (`waiting`:
no tick yet, `running`, `paused`, `quiet`), `breakRemaining` and
`hydrationRemaining` (minutes; absent while that reminder is off).
`tick` adds `reminder` (`none`, `break`, `hydration`, `both`: what was due),
`notified` and, when a due reminder was not shown, `reason`. The pause
actions add `paused`. `status` adds `paused`, `quietHours`,
`pauseRemaining`, `breaksToday`, `hydrationToday` and `lastTick`.

The same three values are published as variables for workflows after every
action: `{{ plugins.deckpilot.breakReminder.state }}`,
`{{ plugins.deckpilot.breakReminder.breakRemaining }}` and
`{{ plugins.deckpilot.breakReminder.hydrationRemaining }}`. DeckPilot keeps
plugin variables in memory only; they are back after the first tick.

## Settings

| Setting | Default | |
| --- | --- | --- |
| Break reminders | on | |
| Minutes between breaks | 50 | 5-240 |
| Hydration reminders | on | |
| Minutes between water | 60 | 5-480 |
| Quiet hours from / until | empty | Times such as `22:00` and `07:00`; may cross midnight. In between nothing is shown and the countdowns wait |
| Away after (minutes) | 5 | 3-120. A pause this long between two ticks counts as a break |
| Language of reminders | `en` | `en` or `pl`: the plugin cannot learn the language of the app |
| Desktop notifications | on | DeckPilot shows reminders as notifications of the operating system |
| Status bar | on | DeckPilot also lists them in its status bar while it is open |

Changing an interval, or turning a reminder on, starts that countdown again.

## How it behaves

- **Precision is one tick.** A reminder comes with the first tick at or
  after its time. Snooze, skip, pause and restart work at once from a key,
  without knowing the time; their minutes are counted by the ticks that
  follow, so "10 minutes" is 9 to 11.
- **One notification per tick at most.** When both reminders are due in
  the same minute, one notification names both.
- **After sleep, no burst.** DeckPilot does not make up schedule firings
  that were missed while the computer slept, DeckPilot was locked or not
  running. The first tick after a pause of "Away after" minutes or more
  shows nothing and starts both countdowns again: the time away was the
  break. A shorter pause is counted, and a reminder that became due in it
  is shown once.
- **Quiet hours** hold the countdowns. A reminder that was due and not yet
  shown is shown at their end.
- **The clock going back** (set by hand, or the end of daylight saving
  time) takes nothing off the countdowns and shows nothing; they go on
  from the new time. Clocks going forward by an hour are handled like
  any pause of that length.
- **Pause** holds everything. A timed pause ends with the tick at which
  its minutes are over.
- **A notification DeckPilot refuses** (more than 5 a minute from this
  plugin, or a failure) stays due and is tried with the next tick; `tick`
  says so in `reason`. One that your notification settings show nowhere
  counts as shown.

## Permissions and data

- **Notifications**, the only permission: to show the reminders, where
  the two settings above say. No network access, no credentials.
- The plugin keeps one small value in its own storage on this computer:
  the two countdowns, the pause, the time of the last tick and counters of
  reminders shown, skipped and snoozed. Nothing leaves the computer, and
  DeckPilot deletes it when the plugin is uninstalled.
- The log gets a line when a notification was refused, when a long pause
  restarted the countdowns, when the clock went back and when stored state
  could not be used. No line contains a parameter or a setting.

## Errors

| Code | Meaning |
| --- | --- |
| `invalid-fired-at` | The time passed to Tick is not the trigger's date and time (`YYYY-MM-DD HH:MM`): check step 4 and 5 of the setup |
| `invalid-settings` | The quiet hours are not times such as `22:00`, or only one of the two is set |
| `storage-unavailable` | The reminder state could not be read or saved; nothing was shown, and the next tick tries again |
| `invalid-parameters`, `not-permitted`, `storage-quota` | DeckPilot's shared codes: a parameter is missing or out of range; the plugin is disabled or its notification permission was revoked; the plugin's storage is full |

## What it does not do

Not in this version, because plugin API 1.0 has no call for it:

- **No dynamic key state.** A key does not show the countdown, the next
  reminder or "paused"; use the Status action or the variables. DeckPilot
  is getting a key-state call for plugins; the plugin's stored state
  (minutes left per reminder, paused, quiet) is already what a key would
  show.
- **No timer of its own.** Without the workflow nothing happens, and the
  plugin cannot create that workflow for you.
- **No idle detection.** It does not know whether you are at the keyboard;
  only a pause between ticks counts as time away.
- **No sound and no action buttons** in the notification: snooze and skip
  are keys or workflow nodes.
- **Not installable from the Marketplace yet**: DeckPilot accepts the
  publisher `deckpilot` only in plugins bundled with the application
  ([docs/plugin-factory.md](../../docs/plugin-factory.md), B6).

## Building and testing

See [`first-party/README.md`](../README.md#commands). The scheduling logic
is a set of pure functions in `src/lib.rs` (`State::advance`), tested
minute by minute in `src/tests.rs`.
