# deckpilot.calls

> **Held ahead of the pin.** This plugin needs plugin API 1.16 (`calls`,
> `contributes.callOverlay`, `contributes.layoutTemplates`), which no
> DeckPilot provides yet. `plugins.json` marks it `held` with
> `hostApi: "1.16"`, so it is built and tested here but never packaged,
> signed, released or given a Marketplace entry. Before that, DeckPilot
> needs the agent's plugin API at 1.16, the reviewed privileged policy
> entry of `deckpilot.calls` 0.1.0, and bundling (design-calls.md §12, C9).
> Paths below that are not relative links are in
> [knittingcode/deckpilot](https://github.com/knittingcode/deckpilot).

Calls ("Rozmowy") shows your calls and meetings on your Stream Deck:

- the **Calls pages**, which DeckPilot shows over your keys during a call
  (incoming, outgoing, in a call);
- **call keys** you put on your own pages, which show the state of the
  call;
- **Back to work**, which shows your own keys again and brings the call
  back after a time, or once what you started next is done.

It controls a call only where the call app allows it. **Microsoft Teams
can only be detected.**

## Microsoft Teams: detection only

Microsoft retired the Teams local API for devices on 2026-06-30
(Microsoft 365 Message Center MC1266901). Nothing supported controls the
Teams desktop client since then. DeckPilot never uses undocumented Teams
APIs, never injects into a process, never drives Teams through
accessibility, and never records sound or picture.

What DeckPilot can tell is that Teams holds the microphone: "a call or
meeting is probably active". That comes from the operating system's
audio sessions, on macOS and Windows. The Teams keys stay on the page,
disabled, with the reason.

## What works with which call app

| Call app | Detect a call | Answer | Reject | Leave | Mute | Camera | Microphone state | Camera state |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Microsoft Teams | inferred on macOS and Windows (Teams holds the microphone); Linux: not attributed (Teams is a browser tab there) | unsupported: Microsoft retired the device API (2026-06-30) | unsupported: Microsoft retired the device API | unsupported: Microsoft retired the device API | unsupported: Microsoft retired the device API | unsupported: Microsoft retired the device API | unknown: Teams may hold the microphone while muted | macOS: a camera is in use (not attributed to an app); Windows: unsupported |
| A provider with an API (later: Zoom, Meet) | as the app reports it | where the app allows it | where the app allows it | where the app allows it | where the app allows it | where the app allows it | as the app reports it | as the app reports it |

An incoming call is never shown for Teams, because no supported signal
rings. A provider that can control a call plugs into the same contract
later, and its keys then work without any change here.

## Consent

- `calls` (shown as `calls:observe`, medium): whether you are in a call,
  and its state.
- `calls:control` (high): asking the call app to answer, reject, leave,
  mute or switch the camera. You consent to it separately; without it
  every control key is disabled, and the plugin still shows calls.
- `device` (key state): the call keys show the call's state.

The plugin has no way out of DeckPilot: no network, credentials, opening
pages, tools or resources. DeckPilot's manifest rules and its broker
refuse them together with `calls`. Only the bundled `deckpilot.calls`
may use the Calls operations at all.

## Actions

| Action | What it asks for | Key |
| --- | --- | --- |
| `answer`, `reject`, `cancel-outgoing`, `end` | the same action on the focused call | green or red while it applies; a cross when the app does not allow it |
| `mic-toggle`, `mic-mute`, `mic-unmute` | mute, unmute (a set the app can only toggle reads the confirmed state first) | on or off only as the app reported it |
| `camera-toggle`, `camera-on`, `camera-off` | the camera on or off | on or off only as the app reported it |
| `return-to-work` | on the Calls pages, Back to work (DeckPilot runs it); on another key, it says it works on the Calls pages (`calls-pages-only`) | — |

A control answers only once the call app **confirmed** it. Otherwise it
fails: `calls-unconfirmed` when there is no confirmation in time, and
`calls-unsupported` with the reason when the app does not allow it.
Nothing shows as done before it is done. The keys show states only (a
toggle, a mark, a colour), never the meeting title, a name or a number.

## The Calls pages (`layoutTemplates`)

One template per device class, the same as DeckPilot's built-in ones
(`crates/deckpilot-calls/src/layouts.rs`): `grid-small` (Mini),
`grid` (MK.2, Neo), `grid-large` (XL), `keys-dials` (+) and `no-display`
(Pedal). DeckPilot generates a managed layout per device model from
them. It never changes your profiles, and an edited layout is never
rewritten. The last key of every page is always the protected dismiss.

## Tests

`cargo test -p deckpilot-calls` (native):

- `src/contract.rs`: the manifest and the module agree (actions,
  parameters, error codes); both locales have every text; the templates
  are DeckPilot's; the permissions are `calls` and `device` only; no text
  claims a Teams control.
- `src/tests.rs`: against a scripted host, every control asks for exactly
  its action and reports the confirmed outcome. Every refusal keeps its
  code and reason (Teams: `calls-unsupported: provider-has-no-api`). Back
  to work asks the host nothing. Faces show states only.

Nothing here was checked against a live Teams meeting or a physical
Stream Deck.
