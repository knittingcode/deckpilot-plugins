# Weather (`deckpilot.weather`)

The current weather and a forecast of up to seven days for one place, as a
message, as outputs for workflows and as variables. The data comes from
[Open-Meteo](https://open-meteo.com/), without an account or a key.

[Weather data by Open-Meteo.com](https://open-meteo.com/) (CC BY 4.0).
Location data based on [GeoNames](https://www.geonames.org/).

Polish: [README.pl.md](README.pl.md).

> **Status.** Version 0.1.0 is built, tested and packaged by CI and is
> **not published**. It cannot be installed from a registry yet
> ([first-party/README.md](../README.md)). Before it is published, the
> owner has to confirm with Open-Meteo that the free API may be used by an
> application distributed to many users ([below](#open-meteos-terms)).

## What it does not do

Plugin API 1.0 has no way for a plugin to draw on a key, to run by itself,
or to learn the time. So this version does **not**:

- **show the weather on a key.** No temperature, no dynamic weather icon.
  A key bound to an action runs it; the result is the action's message.
  The outputs `conditionName` and `isDay` are what an icon would be chosen
  by once plugins can set key content.
- **refresh by itself.** It runs when a key is pressed or a workflow runs
  it. A workflow with a schedule is the way to refresh the variables
  ([below](#refreshing-on-a-schedule)).
- **know how old its cache is on a key press** ([below](#the-cache-and-time)).
- **write its settings.** A place found by name is kept in the plugin's
  storage instead.
- send notifications, offer hourly values or weather alerts, or use any
  provider other than Open-Meteo.

## Actions

| Action | What |
| --- | --- |
| `current` (Current weather) | Condition, temperature, feels-like temperature, wind and humidity now |
| `forecast` (Forecast) | The next `days` days (1-7, default 3): lowest and highest temperature, condition, precipitation chance |
| `find-location` (Find location) | Searches places by `query`. `select` 0 lists up to five matches; `select` 1-5 stores that match as the place |
| `forget-location` (Forget location) | Deletes the stored place, the cached weather and the cached searches, and clears the variables |

`current` and `forecast` take an optional `at`, the run's time
([below](#refreshing-on-a-schedule)).

### Outputs of `current`

| Output | Kind | Example |
| --- | --- | --- |
| `summary` | text | `Warsaw: Light drizzle, 7 °C (feels like 3 °C). Wind 15 km/h S, humidity 92%.` |
| `condition` | text | `Light drizzle` (in the language of the settings) |
| `conditionName` | text | `drizzle` (a fixed name, [below](#conditions)) |
| `weatherCode` | integer | `51` (WMO code) |
| `isDay` | boolean | `false` |
| `temperature`, `feelsLike` | integer | `7`, `3` |
| `temperatureUnit` | text | `°C` or `°F` |
| `windSpeed`, `windUnit` | integer, text | `15`, `km/h` or `mph` |
| `windDirection`, `windCompass` | integer, text | `191`, `S` (where the wind blows from) |
| `humidity` | integer | `92` (percent) |
| `units` | text | `metric` or `imperial` |
| `location` | text | `Warsaw` |
| `observedAt` | text | `2026-11-16T19:15`, local to the place |
| `stale` | boolean | `true` unless the data is known to be younger than 15 minutes |
| `source` | text | `network` or `cache` |
| `fetchedAt` | text | `2026-11-16T18:16:34Z`; absent when unknown |
| `ageMinutes` | integer | absent when the plugin cannot know the age |
| `problem` | text | the error code of a failed request when saved data is shown instead |
| `attribution` | text | `Weather data by Open-Meteo.com (https://open-meteo.com/), CC BY 4.0` |

### Outputs of `forecast`

`summary` (one line per day), `json` (the days as a JSON array: `date`,
`min`, `max`, `weatherCode`, `condition`, `conditionName`,
`precipitationProbability`), `days` (how many), `temperatureUnit`, `units`,
`location`, and for the first three days `day1Date`, `day1Min`, `day1Max`,
`day1Condition`, `day1ConditionName`, `day1Precipitation` (absent when the
model gives no probability), `day2…`, `day3…`. Then `stale` (unless known to
be younger than an hour), `source`, `fetchedAt`, `ageMinutes`, `problem` and
`attribution` as above.

Day 1 is the day the data was fetched, at the place. The plugin has no
clock, so it does not call a day "today": read `day1Date`.

### Outputs of `find-location`

`count`, `matches` (one numbered line per place: name, region, country,
time zone), `source`, `attribution`, and after a choice `selected` and
`timezone`. Coordinates are never an output.

## Variables

Published by `current`: `temperature`, `feelsLike` (integers), `condition`,
`conditionName`, `summary` (texts), `stale` (boolean). Published by
`forecast`: `forecast` (JSON, the same array as the output `json`),
`forecastSummary` (text), `forecastStale` (boolean).

Read them in workflows as `{{ plugins.deckpilot.weather.temperature }}`.
When an action fails, its `stale` variable is set to true, and `summary`
and `forecastSummary` start with `[stale]` when the data is not known to be
current. Variables are gone after DeckPilot restarts, until the action runs
again.

## Setting the place

Either of:

1. **By name.** Run *Find location* with a place name (the plugin's page in
   DeckPilot has a Run button). It lists the matches. Run it again with
   *Select* set to the number of the right one. The place is stored on this
   computer.
2. **By coordinates.** Enter *Latitude* and *Longitude* in the plugin's
   settings (decimal degrees; a decimal comma works), and optionally a
   *Location name* and a *Time zone*. When both are set, they take
   precedence over a stored place.

Other settings: *Units* (`metric`: °C and km/h; `imperial`: °F and mph) and
*Language of the texts* (`en` or `pl`) for conditions and messages. Error
messages are in English. Names and descriptions in the app are English,
because a plugin's manifest has no translations yet; the Polish ones are in
[locales/pl.json](locales/pl.json).

## Refreshing on a schedule

To keep the variables current, add a workflow with a **schedule trigger**
(every 15 minutes is as often as it is useful) that runs *Current weather*
or *Forecast*. Pass the trigger's firing time (the event field `firedAt`,
`YYYY-MM-DD HH:MM`) to the action's *Run time* (`at`) parameter through a
workflow input. That time is the only clock the plugin can get before it
asks the network, and with it the cache works as intended:

- an answer younger than 15 minutes (`current`) or one hour (`forecast`) is
  used again without a request;
- after Open-Meteo answers "too many requests" (HTTP 429), no request is
  sent until the time it asked to wait (15 minutes if it named none) is
  over.

## The cache and time

One request brings the current conditions and the seven days together; it
is stored (a few kilobytes, in metric units) in the plugin's storage, with
the time from the answer's `Date` header.

A plugin has **no clock** in plugin API 1.0. The plugin therefore knows the
age of its cache only

- before a request, when the run has an `at` (above), by comparing it with
  the `at` of the run that fetched the data. These are local times to the
  minute: across a change of the clock (daylight saving time) the age is
  wrong by the shift once, and an age that would be negative counts as
  unknown;
- after a request that Open-Meteo answered with an error, from that
  answer's `Date`.

Consequences, said plainly:

- **A key press without `at` always sends one request**, however fresh the
  cache is. The 15 minutes cannot be enforced for it. DeckPilot limits a
  plugin to 60 requests a minute.
- **Offline** (no connection, a time-out, an unreadable or oversized
  answer, an error status): the saved data is shown with `stale: true`,
  the time it was fetched, and its age when one of the two sources above
  gives it. Without saved data for the place the action fails with the
  error's code. Saved data is never shown for another place, and not when
  Open-Meteo refuses the request itself (HTTP 400) or when the network
  permission is denied.
- Data is called fresh (`stale: false`) only right after a successful
  request or when its age is known to be within its lifetime.
- Place searches are stored (the last 20) and used again without a request.
  They are asked again only once an answer dated more than 30 days later
  has been seen.

`time.now`, a capability proposed for DeckPilot
([docs/plugin-factory.md](../../docs/plugin-factory.md) §6.3), would remove
these limits: every run would know the age first, the minimum age would
hold for key presses, the wait after HTTP 429 would hold for every run, the
age of offline data would always be shown, and `at` would not be needed.

## Conditions

Open-Meteo reports a WMO weather code. The plugin turns it into a text
(English or Polish) and one of these names: `clear`, `mostly-clear`,
`partly-cloudy`, `overcast`, `fog`, `drizzle`, `freezing-drizzle`, `rain`,
`freezing-rain`, `snow`, `rain-showers`, `snow-showers`, `thunderstorm`,
`thunderstorm-hail`, `unknown`.

| Codes | Name | Texts |
| --- | --- | --- |
| 0 | `clear` | Clear sky |
| 1 | `mostly-clear` | Mainly clear |
| 2 | `partly-cloudy` | Partly cloudy |
| 3 | `overcast` | Overcast |
| 45, 48 | `fog` | Fog; Depositing rime fog |
| 51, 53, 55 | `drizzle` | Light, Moderate, Dense drizzle |
| 56, 57 | `freezing-drizzle` | Light, Dense freezing drizzle |
| 61, 63, 65 | `rain` | Slight, Moderate, Heavy rain |
| 66, 67 | `freezing-rain` | Light, Heavy freezing rain |
| 71, 73, 75, 77 | `snow` | Slight, Moderate, Heavy snowfall; Snow grains |
| 80, 81, 82 | `rain-showers` | Slight, Moderate, Violent rain showers |
| 85, 86 | `snow-showers` | Slight, Heavy snow showers |
| 95, 97 | `thunderstorm` | Thunderstorm; Heavy thunderstorm |
| 96, 99 | `thunderstorm-hail` | Thunderstorm with slight, heavy hail |
| any other | `unknown` | Unknown conditions |

The plugin changes Open-Meteo's values in these ways only: temperatures and
wind speed are rounded to whole numbers, converted to °F and mph when the
units are imperial, and the weather code and wind direction are also given
as words.

## Permissions

| Permission | Why |
| --- | --- |
| `network`: `api.open-meteo.com`, `GET` | The weather for your place |
| `network`: `geocoding-api.open-meteo.com`, `GET` | Searching a place by name |

Nothing else. Storage, settings, the plugin's own variables and the log
need no permission; the cache is far below the default storage quota, so
no larger quota is asked for. There is no credential slot: Open-Meteo's
free API has no key, and the plugin contains none.

## Privacy

- **Open-Meteo receives** the latitude and longitude of your place, rounded
  to two decimals (about a kilometre), the time zone name, and the place
  names you search, together with your IP address as with any request. See
  [Open-Meteo's terms and privacy policy](https://open-meteo.com/en/terms).
  The requests carry `User-Agent: DeckPilot-Weather/0.1.0` and nothing that
  identifies you or your DeckPilot.
- **On this computer** the plugin stores the chosen place, the last weather
  answer and the last 20 place searches in its storage (DeckPilot's
  encrypted plugin database). *Forget location* deletes all of it;
  uninstalling the plugin does too.
- Coordinates are personal data. The plugin never writes them to the log,
  to a message, to an output or to a variable. The place's **name** is in
  messages and in the `location` output.

## Open-Meteo's terms

Read on 2026-11-16 at <https://open-meteo.com/en/terms> and
<https://open-meteo.com/en/licence>; quotations are Open-Meteo's words on
that day. Read them again before relying on this.

- **Free API, non-commercial use only.** "You may only use the free API
  services for non-commercial purposes." Open-Meteo's examples of
  non-commercial use: "Private or non-profit websites or apps that do not
  have subscriptions or advertising", "Personal home automation purposes".
  Commercial use needs a paid plan with an API key; this plugin has no
  place for one. **If you use DeckPilot commercially, check whether your
  use is covered, or do not use this plugin.**
- **Rate limits.** "Less than 10'000 API calls per day, 5'000 per hour and
  600 per minute"; the plans table adds 300,000 calls a month. "Rate limits
  are applied by IP addresses and application." A request for many values
  can count as more than one call (the plugin's forecast request was
  answered with `X-RateLimit-Cost: 1.1`).
- **Attribution.** The data is under CC BY 4.0: "You must include a link
  next to any location Open-Meteo data are displayed", for example
  "Weather data by Open-Meteo.com" linking to <https://open-meteo.com/>.
  The plugin puts that line with the address at the end of every weather
  message and in the `attribution` output. **If you show the plugin's
  outputs or variables somewhere else, show the attribution with them.**
  The place search is "Location data based on GeoNames".
- **No warranty.** "The accuracy and completeness of the data products and
  their uninterrupted provision are not guaranteed by Open-Meteo."
  Open-Meteo may "block applications and IP addresses that misuse our
  service without prior notice".

**Open question for the owner.** The terms speak of a user of the API and
apply limits "by IP addresses and application". They do not say whether an
application that many people install, each calling from their own computer,
counts as one application for the limits, or whether distributing such an
application is itself within "non-commercial use" when some of its users
are companies. The requests name the application
(`DeckPilot-Weather/<version>`), so Open-Meteo can count them together.
**Ask Open-Meteo before this plugin is published in a production
registry.** Until then it is a development build.

## Errors

| Code | Meaning |
| --- | --- |
| `no-location` | No place is set: run *Find location*, or enter a latitude and longitude in the settings |
| `invalid-location` | The latitude, longitude or time zone in the settings cannot be used |
| `location-not-found` | The search found no place with that name |
| `invalid-selection` | *Select* is larger than the number of places the search found |
| `provider-error` | Open-Meteo answered with an error, and no saved data could be shown instead |
| `invalid-response` | Open-Meteo's answer could not be read, and no saved data could be shown instead |
| `response-too-large` | Open-Meteo's answer was larger than the plugin accepts (64 KiB) |
| `timed-out` | Open-Meteo did not answer in time, and no saved data could be shown instead |

Shared codes it also reports: `throttled` (Open-Meteo's or DeckPilot's rate
limit), `not-permitted` (the network permission is not granted),
`invalid-parameters`, `dns-failed`, `connection-failed`, `tls-failed`,
`request-failed`.

## Development

```sh
cd first-party
cargo test --locked -p deckpilot-weather
cargo build --locked -p deckpilot-weather --target wasm32-unknown-unknown --profile guest
```

The unit tests use canned Open-Meteo answers and never reach the network.
The provider is behind the trait `provider::Provider`
(`src/provider.rs`); `src/open_meteo.rs` is its only implementation.
Another provider needs an implementation, its hosts in the manifest and,
if it has a key, a credential slot.

The icon (`assets/icon-128.png`) is original artwork made for this plugin:
a sun behind a cloud, drawn from circles and rectangles.
