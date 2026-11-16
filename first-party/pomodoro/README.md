# Pomodoro (`deckpilot.pomodoro`)

A focus timer with short and long breaks: a notification when a phase
ends, optional automatic start of the next phase, and statistics of the
focus sessions you completed.

Polish: [README.pl.md](README.pl.md).

> **Read this first.** A DeckPilot plugin has no timer and no clock, and it
> runs only while one of its actions runs (plugin API 1.0). This plugin
> therefore counts nothing by itself: **you add one workflow that runs its
> Tick action every minute** ([Setup](#setup)).
>
> **The countdown on a key is not delivered yet.** A plugin cannot give a
> key its state in this API, so a key shows neither the minutes left nor
> the phase; you learn them from the notifications, the Status action and
> the variables. And the timer counts **whole minutes, not seconds**: the
> only clock a plugin gets is the schedule trigger's date and time, once a
> minute ([What it does not do](#what-it-does-not-do)).

## Setup

Once per profile in which you want the timer. Workflow triggers fire only
for the active profile, while DeckPilot's configuration is unlocked.

1. Install and enable the plugin; allow its one permission, notifications.
2. Create a workflow, for example "Pomodoro tick".
3. In the workflow panel (nothing selected), add an **input**: key
   `firedAt`, type text, required.
4. Add a **trigger**: schedule, every **1** minute. For the input
   `firedAt` choose the event field **Date and time**.
5. Add one action node after the start node: **Pomodoro › Tick**. Map its
   parameter **Date and time** to the start node's output `firedAt` (the
   workflow's input).
6. Save. The ticks alone start nothing: the timer waits for Start.
7. Put **Start, pause or resume** (`toggle`) on a key. Add **Skip phase**,
   **Reset** and **Status** on other keys if you want them.

As stored in the profile, the parts that matter look like this (the action
node refers to an action of type `deckpilot.pomodoro.tick`):

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
too, with phases ending up to that much late, as long as it is **shorter
than the setting "Away after"** (5 minutes by default): a longer pause
between two ticks pauses the timer.

## The cycle

Focus session 1, short break, focus session 2, short break, … and after
the last focus session of a cycle (the fourth by default) a long break;
then it starts again with session 1. With the default settings a break
starts by itself when a focus session ends, and the next focus session
waits for Start.

## Actions

| Action | What |
| --- | --- |
| `tick` (Tick) | Takes the minutes since the last tick off a running countdown; when the phase ends, shows a notification and goes on to the next phase. Parameter `firedAt`: the trigger's date and time. For the workflow, not for a key |
| `start` (Start) | Starts the countdown of the current phase, or continues a paused one |
| `pause` (Pause) | Stops the countdown where it is |
| `resume` (Resume) | Continues a paused countdown; does nothing when the timer is not paused |
| `toggle` (Start, pause or resume) | One key for the three above |
| `skip` (Skip phase) | Ends the current phase now and goes on to the next one, which runs if the timer was running. A skipped focus session is not counted in the statistics; the cycle goes on as if it had ended |
| `reset` (Reset) | `scope`: `phase` (the default) stops the timer and puts the current phase back to its full length; `cycle` goes back to focus session 1; `statistics` clears the counters and leaves the timer alone; `all` is `cycle` and `statistics` |
| `status` (Status) | Reports the state as of the last tick; changes nothing |

Every action answers with a message such as `Focus 2 of 4: 12 min left` and
these outputs for later workflow nodes:

| Output | |
| --- | --- |
| `state` | `idle` (the phase has not started), `running`, `paused` |
| `phase` | `focus`, `shortBreak`, `longBreak` |
| `remaining` | Minutes left of the phase |
| `session` | The focus session in the cycle, from 1 |
| `completedToday`, `focusMinutesToday` | Focus sessions completed on the date of the last tick, and their minutes |
| `completedTotal`, `focusMinutesTotal` | The same since the plugin was installed or the statistics were reset |

`tick` adds `ended` (`none`, `focus`, `shortBreak`, `longBreak`: the phase
that ended with this tick), `notified`, `away` (the timer was paused by a
long pause between ticks) and, when a notification was not shown, `reason`.
`start`, `pause`, `resume` and `toggle` add `changed`. `skip` adds
`skipped`. `status` adds `away`, `sessionsPerCycle`, `phaseMinutes`,
`progress` (0-100), `statisticsDate`, `lastTick`, and `keyLabel` and
`keyDetail`: the two lines a key would show (see below).

The same eight values are published as variables for workflows:
`{{ plugins.deckpilot.pomodoro.state }}`, `.phase`, `.remaining`,
`.session` after every action, and `.completedToday`, `.focusMinutesToday`,
`.completedTotal`, `.focusMinutesTotal` after Tick, Status and Reset.
DeckPilot keeps plugin variables in memory only; they are back after the
first tick.

## Settings

| Setting | Default | |
| --- | --- | --- |
| Focus session (minutes) | 25 | 1-180 |
| Short break (minutes) | 5 | 1-60 |
| Long break (minutes) | 15 | 1-120 |
| Focus sessions before a long break | 4 | 1-12. With 1 every break is the long one |
| Start breaks automatically | on | Off: the break waits for Start |
| Start focus sessions automatically | off | On: the next session starts when the break ends |
| Away after (minutes) | 5 | 3-120. A pause this long between two ticks pauses a running timer |
| Language of notifications | `en` | `en` or `pl`: the plugin cannot learn the language of the app |
| Desktop notifications | on | DeckPilot shows the notifications as notifications of the operating system |
| Status bar | on | DeckPilot also lists them in its status bar while it is open |

A changed length applies at once to a phase that has not started, and from
the next phase to one that runs or is paused.

## How it behaves

- **Precision is one tick, a minute.** The state stores the minutes left,
  not a deadline. Start, pause, resume, skip and reset work at once from a
  key, without knowing the time; the minutes are counted by the ticks that
  follow. A phase started between two ticks is therefore up to a minute
  shorter than its setting, and DeckPilot may fire a schedule up to two
  minutes late. Seconds are not possible: the trigger's time has none.
- **One phase ends per tick at most.** When more time passed than the phase
  had left, the phase ends once and the next one begins with its full
  length; the surplus is not carried over.
- **After sleep the timer is paused, not finished.** DeckPilot does not
  make up schedule firings that were missed while the computer slept,
  DeckPilot was locked or not running. When a tick comes "Away after"
  minutes or more after the previous one and the timer was running, none
  of that time is counted: the timer is paused where it was, one
  notification says so, and Resume (or Start, or the toggle key) continues
  it. Nothing changes for a timer that was paused or not started.
- **The clock going back** (set by hand, or the end of daylight saving
  time) takes nothing off the countdown; it goes on from the new time.
  Clocks going forward by an hour are a long pause, as above.
- **Statistics** count a focus session when its countdown runs out, with
  the minutes the session was started with. Skipped and reset sessions
  are not counted. "Today" is the date of the last tick: the first tick of
  another date starts the day's counters again, and a session that began
  before midnight counts on the day it ended. A key cannot learn the
  date, so after a night without ticks Status shows yesterday's counters
  until the first tick; `statisticsDate` says which date they are of.
- **A notification DeckPilot refuses** (more than 5 a minute from this
  plugin, or a failure) is tried again with the next ticks for 5 minutes,
  then dropped; `tick` says so in `reason`. The timer and the statistics
  never wait for it. One that your notification settings show nowhere
  counts as shown.
- **If the plugin's storage fails**, the action fails with
  `storage-unavailable`, nothing is shown and nothing is lost: the next
  tick that can be saved counts the minutes since the last saved one.

## What a key would show

DeckPilot is getting a key-state call for plugins. This version already
computes what it will return, in one place, from the stored state: the
title `12 min`, the second line `Focus 2/4` (`Short break`, `Long break`,
with `· paused` or `· ready`; in Polish when the language setting says
so), a progress bar, and whether the countdown runs. Today that is only
visible as the Status outputs `keyLabel`, `keyDetail` and `progress`.

## Permissions and data

- **Notifications**, the only permission: to show the end of a phase,
  where the two settings above say. No network access, no credentials.
- The plugin keeps one small value in its own storage on this computer:
  the phase, the minutes left, the session number, the time of the last
  tick, a notification that waits to be shown, and the counters. Nothing
  leaves the computer, and DeckPilot deletes it when the plugin is
  uninstalled.
- The log gets a line when a notification was refused or dropped, when a
  long pause paused the timer, when the clock went back and when stored
  state could not be used. No line contains a parameter or a setting.

## Errors

| Code | Meaning |
| --- | --- |
| `invalid-fired-at` | The time passed to Tick is not the trigger's date and time (`YYYY-MM-DD HH:MM`): check step 4 and 5 of the setup |
| `storage-unavailable` | The timer state could not be read or saved; the action can be run again |
| `invalid-parameters`, `not-permitted`, `storage-quota` | DeckPilot's shared codes: a parameter is missing or out of range; the plugin is disabled or its notification permission was revoked (the timer still counts; only the notification fails); the plugin's storage is full |

## What it does not do

Not in this version, because plugin API 1.0 has no call for it:

- **No countdown on the key, and no dynamic key state at all.** A key does
  not show the minutes left, the phase, or whether the timer runs. This is
  the main thing a Pomodoro key is for, and it is not delivered; it needs
  DeckPilot's key-state call for plugins.
- **No seconds.** The countdown has whole minutes, and its precision is one
  tick.
- **No timer of its own.** Without the workflow nothing counts down, and
  the plugin cannot create that workflow for you. It also cannot tell
  that the ticks stopped: a key pressed while no tick arrives still
  answers, with the state of the last tick.
- **No idle detection.** It does not know whether you are at the keyboard;
  only a pause between ticks counts as time away.
- **No sound and no action buttons** in the notification: start and skip
  are keys or workflow nodes.
- **No history per day**: today's counters and the totals, nothing else.
- **Not installable from the Marketplace yet**: DeckPilot accepts the
  publisher `deckpilot` only in plugins bundled with the application
  ([docs/plugin-factory.md](../../docs/plugin-factory.md), B6).

## Building and testing

See [`first-party/README.md`](../README.md#commands). The timer is a set of
pure functions in `src/lib.rs` (`State::advance` and the key actions),
tested minute by minute in `src/tests.rs`.
