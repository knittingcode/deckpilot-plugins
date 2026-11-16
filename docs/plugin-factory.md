# Plugin factory: what a first-party plugin can do today

Status: **audit and plan**, taken on 2026-11-16 from the code of
`knittingcode/deckpilot` at `16e685cb` (`main`; the SDK pinned here is
`dc1a8a2e`, `tools/deckpilot.env`) and of this repository at `5d98318`.
It records what plugin API 1.0 (guest ABI 1, SDK 0.1.0) offers, what each
plugin of the planned catalog needs, and where the sources of DeckPilot's
own independently installable plugins live. It changes nothing in
DeckPilot. Paths without a repository are DeckPilot's.

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
| Dynamic key images | **Missing** | `permissions[kind=device]` `icons` and `assets.keyIcons` validate; no operation uses them (`sdk/permissions.md`: "Nothing yet"). The agent's renderer draws a typed `KeyFace` only (`crates/deckpilot-core/src/face.rs`) | — | memory, minesweeper, snake, system-monitor (charts), weather (condition icon), markets (sparkline) |
| Dynamic key titles | **Missing** for plugins | The host has it for built-in modules: `ButtonState { label, detail, indicator, progress, toggle }` (`crates/deckpilot-core/src/button_state.rs`), pulled by `apps/agent/src/service/states.rs`. A plugin action never provides one: `contributes.actions[].state` validates, "ABI 1 has no state call" (`sdk/manifest.md`); the plugin handler in `apps/agent/src/plugins/engine.rs` has no `state_policy`. Key labels expand only `{profile}`, `{project.key}`, `{project.name}` (`face.rs` `render_label`), never a plugin variable | Built-ins: label and detail 32 characters, progress 0-100, refresh 1 s to 24 h, 4 providers at a time, 20 s per call | break-reminder (countdown, state), pomodoro, system-monitor, weather, air-quality, markets, all games |
| Key press and release events | **Partly**: a press runs an action | A key bound to `<plugin id>.<action>` invokes it on click, double click or long press (`crates/deckpilot-core/src/gesture/`); the plugin learns the trigger (`run.context`: `key`, `gesture`). No release event, no key position, no device id reach the plugin; a key is told apart only by its action parameters | Lane of 8 queued invocations per plugin, then `busy`; default 30 s per invocation | snake (held keys, timing). Enough for memory and minesweeper (one parameter per cell) once keys can show state |
| Timers | **Missing** | No operation; no clock (`SystemTime::now()` fails in the guest), no sleep (`module-migration.md` G10 `time.sleep`: sketched, not implemented). A scheduled workflow can pass its firing time to an action parameter: `EventField::FiredAt`, local `YYYY-MM-DD HH:MM` (`trigger.rs`) | — | break-reminder, pomodoro, snake; cache ages in weather, air-quality, markets (today: the response's `date` header) |
| Notifications | **Supported** | `notify` (`Context::notify`, `notify_with`, `notify_delivered`); `permissions[kind=notifications]` with optional `routing` to the plugin's own settings | Title 64, body 300 characters; 5 per minute; desktop or status bar is the agent's and the user's decision | none |
| Device capabilities | **Missing** | `supports.device` and `actions[].device.requires` (`keys`, `keyDisplays`, `dials`, `touchStrip`, `brightness`) validate; "Accepted, not used" (`sdk/manifest.md`). No operation reports a device, its grid or its image size | — | games (grid size), anything that draws |
| System metrics | **Missing** | No operation. The built-in `system.process-status` reads `sysinfo` inside the agent; G11 (`process.query`) is sketched as "possible, low value" | — | system-monitor |
| Platform-specific host services | **Missing**, several refused by design | `FORBIDDEN_PERMISSION_KINDS` (`crates/deckpilot-plugin-manifest/src/lib.rs`): `filesystem`, `process`, `exec`, `shell`, `clipboard`, `hid`, `usb`, `brightness`, `machineResources`, `projectContext`, `workflows`, `ui` are refused in a manifest; the broker never grants them (`plugins/runtime/crates/broker/src/permissions.rs`). `platforms` only restricts where a plugin installs. On Windows plugins install but do not run | — | clipboard, power, screen-capture, system-monitor; Audio Mixer, Meeting Controls, SSH |
| Cancellation and cleanup | **Supported** by the host; the guest is not told | A cancelled or late run interrupts the capability call in progress (`unavailable`), refuses later ones, discards the result; the host is killed after 2 s (`sdk/lifecycle.md`). `dp_shutdown` (`Plugin::on_shutdown`): best effort, no capabilities, 1 s. Storage and credentials are deleted on uninstall, variables dropped on disable | Wall clock per action `node.timeoutMs` (1 s to 10 min, default 30 s); 250 ms CPU between host calls; 5,000,000,000 fuel; 64 MiB memory; 256 capability calls; 5 crashes in 10 minutes disable the plugin | none. A plugin cannot clean up with a capability call at shutdown; keep state consistent after every action |

Also missing and needed by the catalog: **randomness** (no operation;
`getrandom` cannot be linked, the package check refuses every other
import) blocks memory and minesweeper, which must shuffle; and **localized
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
| `deckpilot.memory` | **Blocked on key state and randomness** | The game logic, the board in storage and one action per cell (`flip` with a `cell` parameter) work today. Showing a card on a key (6.1) and shuffling (6.4) do not exist. Without them the game is not playable on the deck |
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
| Independently installable (this catalog) | **this repository, `first-party/<name>/`** | this repository's CI; a published version by `tools/build-plugin.sh` from a version record whose `source` names this repository | the registry's signatures (blocked: B1, B6) |

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
   (`guest` profile), `deckpilot-registry package`, and all packages in a
   registry signed with **throwaway keys** and accepted by
   `verify-site --require-packages`.

### Can a `deckpilot.*` plugin be installed from a registry today? No.

Checked by running the tool, not only by reading:

- `deckpilot-registry package` refuses the manifest:
  ``publisher.id: `deckpilot` is reserved for DeckPilot``. `verify_package`
  (`crates/deckpilot-plugin-package/src/contents.rs`, `parse_manifest`)
  calls `Manifest::validate`, which refuses every `RESERVED_PUBLISHERS`
  id. The same function verifies a package the agent downloads from a
  registry, so a test registry cannot deliver one either.
- Only `verify_bundled_files` uses `Manifest::validate_first_party`, for
  files matched against the pins compiled into the agent.
- The store itself would allow it (`InstallSource::Registry` allows a
  first-party id, `crates/deckpilot-plugin-store/src/store.rs`); a file
  (sideload) or a dev install of a `deckpilot.*` id is refused, so that
  nothing but the bundle or a registry can take the built-in modules'
  action aliases.
- Here, *Submission rules* refuses reserved publishers for contributors
  only; a maintainer's pull request passes it and then fails *Validate
  registry* at `package`.

So today a catalog plugin can be **built, tested and packaged under a test
publisher** (step 3 above copies the manifest with `id` and `publisher.id`
changed to `deckpilot-test`; the module is the one built from the committed
source), and **run in a real agent only that way**, from a local test
registry (`docs/marketplace/local-testing.md`). It cannot reach a user as
`deckpilot.<name>`. `tools/first-party.sh` fails as soon as the pinned tool
accepts the real manifest, so the workaround cannot outlive the blocker.

**B6, the smallest reviewed change** (DeckPilot; owner decision, not made
here): let the package verifier accept the publisher `deckpilot` when, and
only when, the caller says the package comes from a registry whose signed
index lists `deckpilot` as a publisher bound to the owner's GitHub id and
marked verified, under the roots compiled into the application. In code:
an `ExpectedPackage` flag set by the registry client from the verified
index (never by `File` or `Dev` installs), `validate_first_party` instead
of `validate` under that flag, the same flag for `deckpilot-registry
package` and `add-version` behind an explicit option, and
`registry/publishers/knittingcode.json` here with the namespace
`deckpilot`. It is useless without **B1** (no production keys exist; U-10,
U-12 in [production-readiness.md](production-readiness.md)): with test
roots it works only in an agent started with those roots.

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

None is implemented. Each is one reviewable DeckPilot change with its own
permission, in the order the catalog needs them. 6.1 answers the request
for a reusable mechanism for dynamic key content: **none exists for
plugins**, while the host already has one for built-in modules.

### 6.1 Key state for plugin actions (dynamic key content)

Principle: **the plugin describes, the host draws.** A plugin never sends
pixels and never learns the device. It returns a small typed description;
the agent's renderer, which already knows every device's resolution and
image format, draws it or drops it. That gives device independence and a
fallback for free, and keeps image decoding of untrusted bytes out of the
agent.

ABI shape (compatible within ABI 1, like `dp_init`):

- A new optional export `dp_state(ptr, len) -> packed`, with the SDK's
  `Action::state(fn(&mut Context, &Args) -> Result<KeyState, PluginError>)`.
  The agent calls it only for an action whose manifest entry has `state`
  (already in the schema: `refresh: "onEvents" | "every"`,
  `intervalSeconds`), for a key that is **shown on a device now**, with the
  key's normalized parameters. This is the existing pull model of
  `apps/agent/src/service/states.rs`, with the plugin as one more provider.
- `KeyState` is `ButtonState` extended by two members, all optional:

  | Member | Bound | Drawn as |
  | --- | --- | --- |
  | `label`, `detail` | 32 characters each, no control characters | title and second line |
  | `indicator` | `ok`, `warning`, `error`, `running`, `unknown` | corner mark |
  | `progress` | 0-100 | bar along the bottom |
  | `toggle` | boolean | mark along the top |
  | `icon` (new) | a name of the package's `assets.keyIcons` (at most 64 PNGs, verified at install) | the key's icon; unknown name: ignored |
  | `series` (new) | at most 32 integers 0-100 | a sparkline behind the title |

  No colours beyond what the user's key appearance sets, no free drawing.
- One new operation, `state.invalidate {}`: "my keys may have changed".
  The agent then pulls the shown keys of that plugin again, coalesced. An
  action that flips a card calls it; nothing is pushed.
- Inside `dp_state` only `storage.get`, `storage.keys`, `settings.get` and
  `log` are allowed (the broker refuses the rest): a state call cannot
  reach the network, notify, or write. Data that needs the network is
  fetched by an action or a timer (6.2) and read from storage here.
- Permission: the existing `device` kind, `state: true` (low tier) for
  texts, marks and bars, `icons: true` for `icon` and `series`.

Limits and budgets (per plugin unless stated):

| What | Bound | When exceeded |
| --- | --- | --- |
| Refresh interval (`every`) | at least 5 s for a plugin (built-ins keep 1 s), at most 24 h | clamped |
| Invalidations | 4 per second, burst 8 | coalesced into one pull; `throttled` |
| State calls running | 1 per plugin, on a lane of their own below the action lane, so a key press is never queued behind a redraw; they also count in the agent's global limit of 4 providers | the pull waits; a newer one replaces a queued one for the same key |
| One state call | 2 s wall clock, 50 ms CPU between host calls, 1/20 of an action's fuel, 8 capability calls, 2 KiB of output | the call is dropped; the key keeps its last state with an error mark |
| Keys pulled per plugin | 32 shown keys | the rest show the static face |
| Repeated failures | 3 in a row for a key: back off 5 s, 10 s … 5 min | error mark; after 20 failures in 10 minutes the plugin's state calls stop until it is re-enabled (the crash-loop breaker already covers crashes) |
| Redraws | the agent redraws only when the typed state changed (states are comparable today) | nothing is sent to the device |

Fallback: a device or control without a display gets no state call. An
agent without the feature ignores `state` as today; a module without
`dp_state` is never asked. A denied or revoked `device` permission leaves
the key with its static face.

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

`time.now {}` → `{ unixMs, utcOffsetMinutes }`, always allowed. No risk
beyond what every response's `date` header already tells a plugin. With it:
snooze "for 10 minutes" at a key press, quiet hours, cache ages.

### 6.4 Randomness

`random.bytes { n ≤ 64 }` from the agent's CSPRNG, always allowed, counted
in the 200/s overall limit. Games shuffle with it; nothing else needs it.

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
| Memory | `first-party/memory/` | the game core as a library with exhaustive tests, the actions (`new-game`, `flip`), the board in storage. **Not shippable** as a game until 6.1 and 6.4 exist: until then a new game takes a seed from the user, and the board is only visible in the action's message | 6.1, 6.4 |

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
