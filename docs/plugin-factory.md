# Plugin factory: what a first-party plugin can do today

Status: **audit and plan**, taken on 2026-11-16 from the code of
`knittingcode/deckpilot` at `16e685cb` (`main`; the SDK pinned here is
`dc1a8a2e`, `tools/deckpilot.env`) and of this repository at `5d98318`.
It records what plugin API 1.0 (guest ABI 1, SDK 0.1.0) offers, what each
plugin of the planned catalog needs, and where the sources of DeckPilot's
own independently installable plugins live. It changes nothing in
DeckPilot. Paths without a repository are DeckPilot's.

**Update, 2026-11-17.** Three of the proposals of section 6 are now in
DeckPilot (design: `docs/marketplace/plugin-key-state.md`): the time (6.3)
and randomness (6.4) as plugin API 1.1 (#291), and key state (6.1) as
plugin API 1.2: the renderer's marks (#292), the runtime and SDK (#293),
and the agent asking for it (#294). All four are on DeckPilot's `main`,
and the commit pinned here (`tools/deckpilot.env`) includes them. Sections 1 to 5 are the audit as
taken; where a row of the matrix says **Missing** for one of these, read
its section in 6.

The rule for everything below: a plugin uses only capabilities that exist.
Where one is missing, the plugin waits for a narrow, permission-controlled
capability that is reviewed on its own; nothing here asks for unrestricted
file system, process, screen or input access.

## 1. The plugin model in one paragraph

A plugin is one WebAssembly module (`wasm32-unknown-unknown`, no WASI) run
by a sandboxed host process. Its only import is `deckpilot_v1.host_call`;
every call is one of 14 operations (`CapabilityRequest`,
`plugins/runtime/crates/abi/src/capability.rs`) that the permission broker
checks against the user's grants. **A plugin runs only while one of its
actions is invoked** (a key, a gesture, the app, a workflow node), one
invocation at a time. It has no clock, no random source, no timer, no
knowledge of keys or devices, and it cannot draw
(`docs/marketplace/sdk/lifecycle.md`, "Not in ABI 1").

The 14 operations: `http.fetch`, `url.open`, `notify`, `log`,
`storage.get|set|delete|keys|usage`, `settings.get`, `credentials.has`,
`variables.publish|clear`, `run.context`.

## 2. Capability matrix

"Proof" is the ABI operation or manifest field and the file that
implements or refuses it. "Blocks" lists catalog plugins that cannot be
built as described without it.

| Capability | State | Proof | Limits | Blocks |
| --- | --- | --- | --- | --- |
| Periodic background tasks | **Missing** in the plugin; **partly** through the user's own workflow | The `background` permission (`minIntervalSeconds` 15-86,400) validates and grants nothing (`sdk/permissions.md`; `crates/deckpilot-plugin-manifest/src/model.rs` `Permission::Background`). A workflow with a schedule trigger can run a plugin action: `crates/deckpilot-core/src/trigger.rs` (`Schedule::Interval`, `Daily`, `Weekly`), reported to the plugin as `RunCause::Schedule` | Workflow schedules: at least 1 minute, up to 2 minutes late, no catch-up after sleep, only for the active profile and while unlocked. A plugin cannot create a workflow | break-reminder, pomodoro, system-monitor, weather and air-quality and markets (refresh without a key press), snake |
| Persistent plugin state | **Supported** | `storage.get/set/delete/keys/usage` (`Context::storage_*`); `apps/agent/src/plugins/data.rs`, `crates/deckpilot-plugin-store/src/data.rs` | 256 KiB and 1,000 keys (to 10 MiB with the `storage` permission); key 128 characters, value 64 KiB of JSON; 100 calls/s; per computer, deleted on uninstall, never exported | none |
| Network requests | **Supported** (request and response; no streaming, no WebSocket) | `http.fetch` (`Context::fetch`); manifest `permissions[kind=network]` `hosts`, `methods`, `userOrigins`; `plugins/runtime/crates/broker/src/lib.rs` `Session::authorize`, `apps/agent/src/plugins/executor.rs` | `https` only, exact hosts (no wildcard), public addresses; 60 requests/min; 15 s for all hops; 5 redirects; URL 8 KiB; request body 32 KiB of text; response 512 KiB of text; 32 response headers from an allowlist | none for weather, air-quality, markets. Binary bodies and streams are not possible |
| Secure credentials | **Supported** (handles only) | `contributes.credentials[]`, `permissions[kind=credentials]`; `http.fetch` `auth`, `credentials.has` (`SecretHandle`, `Request::bearer`, `Request::credential`) | 8 slots; schemes bearer, basic, header, query; secret 1 byte to 16 KiB, never readable by the plugin, redacted in responses; sent only to the slot's granted hosts | none (Airly's user key fits a `header` slot) |
| Dynamic key images | **Missing**; key state (6.1, API 1.2) draws 16 built-in glyphs, a colour from a fixed palette and a sparkline, never a plugin's image | `permissions[kind=device]` `icons` and `assets.keyIcons` validate; no operation uses them (`sdk/permissions.md`: "Nothing yet"). The agent's renderer draws a typed `KeyFace` only (`crates/deckpilot-core/src/face.rs`) | — | memory, minesweeper, snake, system-monitor (charts), weather (condition icon), markets (sparkline) |
| Dynamic key titles | **Missing** for plugins; **implemented** as key state (6.1, API 1.2) | The host has it for built-in modules: `ButtonState { label, detail, indicator, progress, toggle }` (`crates/deckpilot-core/src/button_state.rs`), pulled by `apps/agent/src/service/states.rs`. A plugin action never provides one: `contributes.actions[].state` validates, "ABI 1 has no state call" (`sdk/manifest.md`); the plugin handler in `apps/agent/src/plugins/engine.rs` has no `state_policy`. Key labels expand only `{profile}`, `{project.key}`, `{project.name}` (`face.rs` `render_label`), never a plugin variable | Built-ins: label and detail 32 characters, progress 0-100, refresh 1 s to 24 h, 4 providers at a time, 20 s per call | break-reminder (countdown, state), pomodoro, system-monitor, weather, air-quality, markets, all games |
| Key press and release events | **Partly**: a press runs an action | A key bound to `<plugin id>.<action>` invokes it on click, double click or long press (`crates/deckpilot-core/src/gesture/`); the plugin learns the trigger (`run.context`: `key`, `gesture`). No release event, no key position, no device id reach the plugin; a key is told apart only by its action parameters | Lane of 8 queued invocations per plugin, then `busy`; default 30 s per invocation | snake (held keys, timing). Enough for memory and minesweeper (one parameter per cell) once keys can show state |
| Timers | **Missing** (the time itself: 6.3, API 1.1) | No operation; no clock (`SystemTime::now()` fails in the guest), no sleep (`module-migration.md` G10 `time.sleep`: sketched, not implemented). A scheduled workflow can pass its firing time to an action parameter: `EventField::FiredAt`, local `YYYY-MM-DD HH:MM` (`trigger.rs`) | — | break-reminder, pomodoro, snake; cache ages in weather, air-quality, markets (today: the response's `date` header) |
| Notifications | **Supported** | `notify` (`Context::notify`, `notify_with`, `notify_delivered`); `permissions[kind=notifications]` with optional `routing` to the plugin's own settings | Title 64, body 300 characters; 5 per minute; desktop or status bar is the agent's and the user's decision | none |
| Device capabilities | **Missing** | `supports.device` and `actions[].device.requires` (`keys`, `keyDisplays`, `dials`, `touchStrip`, `brightness`) validate; "Accepted, not used" (`sdk/manifest.md`). No operation reports a device, its grid or its image size | — | games (grid size), anything that draws |
| System metrics | **Missing** | No operation. The built-in `system.process-status` reads `sysinfo` inside the agent; G11 (`process.query`) is sketched as "possible, low value" | — | system-monitor |
| Platform-specific host services | **Missing**, several refused by design | `FORBIDDEN_PERMISSION_KINDS` (`crates/deckpilot-plugin-manifest/src/lib.rs`): `filesystem`, `process`, `exec`, `shell`, `clipboard`, `hid`, `usb`, `brightness`, `machineResources`, `projectContext`, `workflows`, `ui` are refused in a manifest; the broker never grants them (`plugins/runtime/crates/broker/src/permissions.rs`). `platforms` only restricts where a plugin installs. On Windows plugins install but do not run | — | clipboard, power, screen-capture, system-monitor; Audio Mixer, Meeting Controls, SSH |
| Cancellation and cleanup | **Supported** by the host; the guest is not told | A cancelled or late run interrupts the capability call in progress (`unavailable`), refuses later ones, discards the result; the host is killed after 2 s (`sdk/lifecycle.md`). `dp_shutdown` (`Plugin::on_shutdown`): best effort, no capabilities, 1 s. Storage and credentials are deleted on uninstall, variables dropped on disable | Wall clock per action `node.timeoutMs` (1 s to 10 min, default 30 s); 250 ms CPU between host calls; 5,000,000,000 fuel; 64 MiB memory; 256 capability calls; 5 crashes in 10 minutes disable the plugin | none. A plugin cannot clean up with a capability call at shutdown; keep state consistent after every action |

Also missing and needed by the catalog: **randomness** (no operation;
`getrandom` cannot be linked, the package check refuses every other
import) blocks memory and minesweeper, which must shuffle (now
`random.bytes`, 6.4, API 1.1); and **localized
metadata** (the manifest, the registry's `plugin.json` and the app have one
language) blocks nothing but is why Polish texts are kept beside the source
(`first-party/README.md`).

## 3. The catalog, plugin by plugin

**Today** means: with plugin API 1.0 as it is, honestly described to the
user. **Narrow capability** names the smallest addition that would make
the full plugin possible (section 6). **Privileged** needs the owner's
decision on a host service that reads or controls the computer; none is
designed here.

### Productivity

| Plugin | Verdict | What works today | What is missing |
| --- | --- | --- | --- |
| `deckpilot.break-reminder` | **Reduced today; full with timers and key state** | Actions `tick`, `snooze`, `skip`, `pause`, `status`; intervals, quiet hours, snooze and counters as settings and storage; `notify` with routing; variables for workflows. The user drives it with one workflow on a schedule ("every minute") that passes `firedAt` to `tick`, which is the plugin's only clock | It cannot start its own timer, a snooze pressed on a key does not know the time until the next tick, and a key cannot show the countdown or state. Timers (6.2), time (6.3), key state (6.1) |
| `deckpilot.pomodoro` | **Reduced today; full with timers and key state** | The same pattern: a per-minute workflow tick, phases in storage, notifications at phase ends | A countdown on the key is the point of the plugin: key state (6.1) and timers (6.2) |
| `deckpilot.clipboard` | **Privileged** | Nothing | `clipboard` is a forbidden permission kind. Reading the clipboard exposes passwords; even write-only is a new host service. Owner decision first |

### System

| Plugin | Verdict | What is missing |
| --- | --- | --- |
| `deckpilot.system-monitor` | **Privileged** (medium) | A read-only `system.metrics` host service (CPU, memory, disk, network totals; no process list, no command lines), plus timers and key state to show it. Which numbers leave the agent is the decision |
| `deckpilot.power` | **Privileged** (high) | Sleep, lock, restart, shut down are control of the computer. A fixed list of host-side actions with a confirmation is imaginable; `process.exec` (G13) stays refused. DeckPilot's built-in `system` module is where this lives today |
| `deckpilot.screen-capture` | **Privileged** (high) | Screen content is the most sensitive input there is, needs an OS permission on macOS, and the result is a file or the clipboard, both forbidden kinds. Not a plugin until the owner decides otherwise |

### Information

| Plugin | Verdict | What works today | What is missing |
| --- | --- | --- | --- |
| `deckpilot.weather` | **Today** as actions and variables; display needs key state | `current` and `forecast` actions for a place from settings (Open-Meteo, section 5); outputs and variables for workflows; a notification on demand; a cache in storage with the response's `date` header as its time | The key cannot show the temperature or an icon (6.1); refresh without a press needs the user's scheduled workflow or timers (6.2) |
| `deckpilot.air-quality` | **Today**, the same shape | GIOŚ without a key; Airly with the user's key in a credential slot (terms: section 5) | As weather |
| `deckpilot.markets` | **Today**, the same shape | Frankfurter (ECB reference rates) without a key; CoinGecko with the user's demo key | As weather; a sparkline needs key content (6.1). Stock quotes: no provider whose terms allow this use was found |

### Games

| Plugin | Verdict | What is missing |
| --- | --- | --- |
| `deckpilot.memory` | **Buildable** with plugin API 1.2 (was: blocked on key state and randomness) | The game logic, the board in storage and one action per cell (`flip` with a `cell` parameter) work today. Showing a card on a key (6.1) and shuffling (6.4) are implemented; the manifest declares `"pluginApi": "^1.2"` |
| `deckpilot.minesweeper` | **Blocked**, the same two | As memory; long press as "flag" works |
| `deckpilot.snake` | **Blocked; not with the capabilities proposed here** | It needs several frames per second and key timing. The proposed timers (at least 15 s) and pulled key state are deliberately too slow. A turn-based variant (one step per press) is possible with 6.1 and 6.4; a real-time one needs its own decision about an interactive mode and its budget |

### Future integrations (design only)

| Candidate | Assessment |
| --- | --- |
| OBS Studio | obs-websocket is a WebSocket on `localhost`: no WebSocket, and `localhost` is never reachable. Needs a local-service capability with its own consent. Not today |
| Home Assistant | Its REST API on a server the user enters fits `network.userOrigins` with `privateNetwork` and a bearer credential slot: **actions are feasible today** (toggle, scene, read a state). Live state on keys needs 6.1 and 6.2; `http://` servers are not reachable (https only) |
| Spotify | The Web API host is fixed and https, but it needs OAuth with PKCE and token refresh: no redirect receiver, and a credential slot holds a static secret. Needs an OAuth capability in the agent. Not today |
| Audio Mixer | Per-application volume is an OS service on three platforms: privileged |
| Calendar | ICS over https from a server the user enters is feasible as an action ("next event") today, within 512 KiB; Google and Microsoft need OAuth, as Spotify. Reminders need timers |
| Meeting Controls | Mute and camera need input injection or each vendor's local API: privileged or a local-service capability. Not today |
| SSH | A raw TCP client with the user's keys: process or socket access, refused (G13). Stays out |

## 4. Where the sources live and how they are built

The owner's decision is "sources in both repositories". Applied:

| Plugins | Source | Built by | Trusted through |
| --- | --- | --- | --- |
| Bundled with the application (`default` and `migration` classes: `deckpilot.github`, `deckpilot.notifications`, …) | `knittingcode/deckpilot`, `plugins/first-party/` | the application's release build (`plugins/first-party/build.sh`) | sha256 pins compiled into the agent |
| Independently installable (this catalog) | **this repository, `first-party/<name>/`** | this repository's CI; a published version by `tools/build-plugin.sh` from a version record whose `source` names this repository | the registry's signatures, with the root-signed first-party authorization (B6, done; blocked: B1) |

`first-party/` is one Cargo workspace with one `Cargo.lock`. The SDK is
not on crates.io; the workspace takes it as a git dependency at the commit
of `tools/deckpilot.env`, the same commit the registry tool and `schemas/`
come from (`tools/first-party.sh check` refuses any other `rev`). It is
deliberately **not** under `plugins/`: every directory there is a registry
submission and must have a `registry/plugins/<id>/` (`tools/check-inputs.py`).
Layout, commands and how to add a plugin: [`first-party/README.md`](../first-party/README.md).

CI (`.github/workflows/first-party.yml`, job *Build, test and package
first-party plugins*; untrusted, no secrets) runs `tools/first-party.sh`:

1. `check`: layout, ids, the SDK pin, Polish texts against the manifest.
2. `test`: `cargo test --locked`, `cargo fmt --check`, `cargo clippy -D warnings`.
3. `package`: each manifest against the pinned schema, the wasm build
   (`guest` profile), `deckpilot-registry package --first-party`, and all
   packages (`add-version --first-party`) in a registry signed with
   **throwaway keys** whose key set authorizes `deckpilot`, with the
   committed verified `deckpilot` publisher, accepted by
   `verify-site --require-packages`.

### Can a `deckpilot.*` plugin be installed from a registry?

Yes, since DeckPilot B6 (PR #311, `70490125`, the pinned commit), under
conditions the client checks before any download
(`RegistryClient::resolve_install`; DeckPilot
`crates/deckpilot-plugin-registry/PUBLISHING.md`, "First-party plugins"):

- the current key set, signed by the **offline root role**, carries
  `"first_party": {"namespaces": ["deckpilot"]}` (`keyset init
  --first-party`). The online index key alone cannot open the namespace;
  an earlier sketch here relied on the index's publisher record only;
- the verified index lists the publisher `deckpilot` with
  `verified: true` (`registry/publishers/knittingcode.json`);
- the package passes `Manifest::validate_first_party`
  (`ExpectedPackage::first_party`, set only by the registry client for
  such a plugin). Every other reserved publisher stays refused.

`deckpilot-registry package` and `add-version` need `--first-party` for a
`deckpilot.*` plugin, and `add-version` also checks the authorizing key
set and the verified publisher. `tools/first-party.sh` and
`tools/build-plugin.sh` pass it; the earlier `deckpilot-test` workaround is
gone. The agent never takes the plugins it bundles (`deckpilot.github`,
`deckpilot.gitlab`, `deckpilot.jira`, `deckpilot.notifications`) from any
registry; *Submission rules* and `tools/first-party.sh check` refuse them
here.

What still blocks a user install is **B1**: no production keys exist
(U-10, U-12 in [production-readiness.md](production-readiness.md)), and
no version is recorded. With test roots it works only in an agent started
with those roots (`docs/marketplace/local-testing.md`).

## 5. External data providers

Checked on 2026-11-16 against the providers' own pages. Every plugin calls
the provider from the user's computer, without a key of ours: this
repository is public, so a shared key is impossible, and a key is always
the user's own in a credential slot. Read the terms again before a plugin
is published; quotations are the providers' words at that date.

| Provider | Hosts | Key | Limits | Attribution and rules | Verdict |
| --- | --- | --- | --- | --- | --- |
| Open-Meteo (weather, geocoding, air quality) | `api.open-meteo.com`, `geocoding-api.open-meteo.com`, `air-quality-api.open-meteo.com` | none for non-commercial use | 600/min, 5,000/h, 10,000/day, 300,000/month, "applied by IP addresses and application" | CC BY 4.0: a link "Weather data by Open-Meteo.com" where the data is shown; geocoding: GeoNames; air quality: CAMS. Free use is non-commercial only | **Use** for weather. The terms do not address an application distributed to many users whose calls may count together: ask Open-Meteo before publication, cache at least 15 minutes |
| GIOŚ (Polish air quality) | `api.gios.gov.pl` (`/pjp-api/v1/rest/…`; older versions were withdrawn on 2025-06-30) | none | 1,500/min for current measurements and the index; 2/min for stations, archives and statistics; the terms advise at most two pulls an hour | CC BY 4.0: "Źródło danych: GIOŚ - EKOINFONET", the time of the data, and a note when data is processed | **Use**. Data is hourly: cache 30-60 minutes; cache the station list for days (2/min) |
| Airly | `airapi.airly.eu` | the user's own (`apikey` header) | free plan: 100/day per the support page (the developer documentation could not be loaded; other numbers circulate) | Name Airly as the source, the measurement time, a link and the unmodified logo next to the data. The free plan forbids sharing the key, building a database and commercial use | **Optional source, user's key**. One clause about use "on any websites, platforms or mobile applications" could not be read in full: read the terms by hand before publication |
| MET Norway | `api.met.no` | none; an identifying `User-Agent` is mandatory | 20/s for the whole application; honour `Expires` and `If-Modified-Since` | NLOD 2.0 / CC BY 4.0. "Browsers and mobile apps should not contact the API directly" | **Not chosen**: direct calls from users' computers are discouraged and the limit is shared |
| Frankfurter (ECB reference rates) | `api.frankfurter.dev` | none | "no monthly or daily caps", abuse limits unnamed | Daily reference rates, not live prices; local caching encouraged | **Use** for currencies; cache for hours |
| CoinGecko | `api.coingecko.com` | the user's demo key (`x-cg-demo-api-key`) | about 30-100/min and 10,000/month per key (two official pages differ) | "Powered by CoinGecko" shown prominently; no redistribution of API access | **Optional, user's key** |
| Alpha Vantage, Finnhub | | the user's key | 25/day; 60/min (unverified) | personal, non-commercial use only | Possible with the user's key, of little use at 25/day; not planned |
| Twelve Data | | the user's key | 800/day | the free plan is "internal non-display usage" | **No**: a key that shows a price is display |
| Stooq | | | | no API and no terms; `robots.txt` disallows everything | **No**: it would be scraping |

Rules for every network plugin: exact hosts and `GET` only in the
manifest; a `User-Agent` naming the plugin and its version; a cache in
storage with a minimum age that a key press cannot bypass; the attribution
in the README and in the action's message or output where the data is
shown; the provider's HTTP 429 and `retry-after` honoured; no personal
data in a request beyond the coordinates or station the user chose.

## 6. Proposals: the narrow capabilities the catalog waits for

6.1, 6.3 and 6.4 are **implemented**; 6.2, 6.5 and 6.6 are not. Each is
one reviewable DeckPilot change with its own permission, in the order the
catalog needs them. 6.1 answers the request for a reusable mechanism for
dynamic key content. Where the implementation differs from the proposal,
its section says so.

### 6.1 Key state for plugin actions (dynamic key content)

**Implemented** as plugin API 1.2, all of it on DeckPilot's `main`: #292
(the renderer's marks), #293 (runtime and SDK) and #294 (the agent asks
for key state and draws it, for keys shown on a device). Reference:
`docs/marketplace/plugin-key-state.md` section 4 and
`docs/marketplace/sdk/actions.md`, "Key state".

Principle: **the plugin describes, the host draws.** A plugin never sends
pixels and never learns the device. It returns a small typed `KeyState`;
the agent's renderer, which knows every device's resolution and image
format, draws it or drops it. No image, font or markup of a plugin is
decoded.

- **ABI**: the optional export `dp_state(ptr, len) -> packed`; SDK
  `Action::state(fn(&mut Context, &Args) -> Result<KeyState, PluginError>)`
  and `MockHost::key_state` for tests. The agent asks only for an action
  whose manifest entry has `state` (`refresh: "onEvents" | "every"`,
  `intervalSeconds`), for a key **shown on a device now**, with the key's
  normalized parameters: the pull model of built-in modules
  (`apps/agent/src/service/states.rs`), with the plugin as one more
  provider.
- **`KeyState`**, every member optional:

  | Member | Bound | Drawn as |
  | --- | --- | --- |
  | `label`, `detail` | 32 characters each, no control characters | title and second line |
  | `indicator` | `ok`, `warning`, `error`, `running`, `unknown` | corner mark |
  | `progress` | 0-100 | bar along the bottom |
  | `toggle` | boolean | mark along the top |
  | `accent` | `blue`, `green`, `amber`, `red`, `purple`, `teal`, `grey` | the key's background; the user's own colours win |
  | `glyph` | one of 16 built-in shapes (`check`, `cross`, `play`, `pause`, `stop`, `timer`, `bell`, `up`, `down`, `dot`, `circle`, `square`, `diamond`, `triangle`, `star`, `heart`) | above the title; the user's own icon wins |
  | `badge` | 4 characters | top left corner |
  | `series` | at most 32 integers 0-100 | a sparkline behind the texts |

- **Read only**: inside `dp_state` only `storage.get`, `storage.keys`,
  `settings.get`, `time.now` and `log` answer, whatever else is granted.
  Data that needs the network is fetched by an action and read from
  storage here.
- **Permission**: the existing `device` kind with `state: true` (low
  tier). The manifest rule that `state` needs a `readOnly` action stays.

| What | Bound | When exceeded |
| --- | --- | --- |
| Refresh | at most every 5 s per key (built-ins keep 1 s), at most 24 h | clamped |
| State calls running | 1 per plugin; its keys take turns. Never queued behind an action, so a key press finds at most one state call ahead of it | the key keeps what it showed |
| One state call | 2 s wall clock, 250 ms CPU between host calls, 1/10 of an action's fuel, 8 capability calls, 4 KiB of output | invalid or late: error mark; a crash counts for the breaker |
| Repeated failures | 3 in a row for a key: asked again after 5 s, 10 s … 5 min | the static face with the error mark |
| Redraws | only when the typed state changed | nothing is sent to the device |

Fallback: a control without a display gets no state call; a plugin that
is not granted `device:state`, disabled, uninstalled or stopped by the
crash-loop breaker leaves its keys as the user designed them; an agent
below API 1.2 never asks. A plugin whose keys only look better with state
keeps `"pluginApi": "^1.0"`.

**Differences from the proposal**:
- No `state.invalidate`. The agent pulls again after anything happens in
  DeckPilot, a key press included, which covers an action that flips a
  card. Kept as a follow-up.
- Packaged key icons (`icon`, `assets.keyIcons`, `device:icons`) are not
  implemented; the 16 glyphs and the accent stand in for them.
- `accent` and `badge` are new.
- The output cap is 4 KiB, not 2 KiB.
- There is no separate lane; state calls share the plugin's one lane and
  never queue behind an action.
- No 32-key cap per plugin; the 5 s floor and one call at a time bound
  the work.

### 6.2 Timers (the `background` permission, made effective)

`contributes.background: { "action": "<id>", "intervalSeconds": n }` with
`n` at least the permission's `minIntervalSeconds` (15 s to 24 h, already
in the manifest). The agent invokes that action like a scheduled workflow
does (`run.context`: `workflow`/`schedule`, so `url.open` stays refused),
only while the plugin is enabled and the configuration unlocked, with no
catch-up after sleep. One invocation at a time; a tick that finds the lane
busy is skipped; at most 10 s wall clock per tick; three failures in a row
double the interval up to an hour. The consent text says how often. It is
not a real-time loop and must not become one.

### 6.3 Time

**Implemented** as plugin API 1.1 (DeckPilot #291), as proposed:
`time.now {}` → `{ unixMs, utcOffsetMinutes }`, always allowed, 20 calls per
second. SDK: `Context::now()` → `Now` (`unix_seconds`, `local_minutes_of_day`,
`local_weekday`, `millis_since`). No risk beyond what every response's
`date` header already tells a plugin. With it: snooze "for 10 minutes" at a
key press, quiet hours, cache ages. A plugin that cannot work without it
declares `"pluginApi": "^1.1"` and `Plugin::requires_api`.

### 6.4 Randomness

**Implemented** as plugin API 1.1 (DeckPilot #291): `random.bytes { length }`,
1 to 64 bytes from the operating system's CSPRNG, always allowed. SDK:
`Context::random_bytes`, `random_u64`, `random_below` (unbiased). Games
shuffle with it; nothing else needs it.

**Difference from the proposal**: besides the 200/s overall limit, a
budget of 4,096 bytes per minute per plugin; seed a generator in the
plugin rather than asking per card.

### 6.5 Localized metadata

Optional `localizations: { "<bcp47>": "locales/<tag>.json" }` in the
manifest, packaged and verified like every named file, in the shape
`first-party/README.md` describes; the same for the registry's
`plugin.json`. Until then Polish texts live beside the source.

### 6.6 Privileged services (not proposed here)

`system.metrics`, power actions, clipboard and screen capture each need
the owner's decision on what leaves the agent, on which platforms, and
under which consent tier. They are listed in section 3 and stop there.

## 7. The first wave

| Plugin | Directory | First version | Depends on |
| --- | --- | --- | --- |
| Break & Hydration | `first-party/break-reminder/` | 0.1.0: tick-driven reminders, snooze, skip, pause, quiet hours, counters, notifications, variables; a README that shows the one workflow the user adds | nothing new. 6.1-6.3 later turn the workflow into the plugin's own timer and put the countdown on a key |
| Weather | `first-party/weather/` | 0.1.0: current conditions and a short forecast from Open-Meteo for a place from settings, a cache, outputs and variables, attribution | nothing new; 6.1 for the key |
| Memory | `first-party/memory/` | the game core as a library with exhaustive tests, the actions (`new-game`, `flip`), the board in storage, the cards on their keys (6.1) and a shuffle from `random.bytes` (6.4) | 6.1, 6.4 (implemented; `"pluginApi": "^1.2"`) |

Each plugin is one pull request that adds its directory, one line in
`first-party/Cargo.toml` (`members`) and its `Cargo.lock` entries; they
conflict only in those two files.

## 8. What was run and what was only read

Run: the registry tool built from DeckPilot `16e685cb` refusing a
`deckpilot.*` manifest and packaging the same plugin under a test
publisher; the template's unit tests, wasm build, package, throwaway-key
registry and `verify-site` through `tools/first-party.sh`; the git
dependency on the SDK at the pin. Read, not run: everything else in the
matrix (the ABI, the broker, the agent's scheduler and state code, the
SDK documentation), DeckPilot pull request #289 (it adds no capability),
and the providers' terms.
