# Air Quality (`deckpilot.air-quality`)

PM2.5, PM10 and the Polish air quality index of one measuring station, with
the time they were measured, as a message, as outputs for workflows and as
variables. The data comes from [GIOŚ](https://powietrze.gios.gov.pl/pjp/home),
Poland's Chief Inspectorate of Environmental Protection, without an account
or a key.

Źródło danych: GIOŚ - EKOINFONET
([CC BY 4.0](https://creativecommons.org/licenses/by/4.0/deed.pl)). The
plugin shows an excerpt of that data (the newest one-hour values and the
index of one station), rounds concentrations to whole µg/m³ and, in English,
translates the names of the index categories.

Polish: [README.pl.md](README.pl.md).

> **Status.** Version 0.1.0 is built, tested and packaged by CI and is
> **not published**. It cannot be installed from a registry yet
> ([first-party/README.md](../README.md)). Read the provider's terms again
> before it is published ([below](#the-providers-terms)).

## What it does not do

Plugin API 1.0 has no way for a plugin to draw on a key, to run by itself,
or to learn the time. So this version does **not**:

- **show the air quality on a key.** No value, no colour, no icon that
  changes. A key bound to an action runs it; the result is the action's
  message. The outputs `categoryName` and `index` are what a key's colour
  would be chosen by once plugins can set key content.
- **refresh by itself.** It runs when a key is pressed or a workflow runs
  it. A workflow with a schedule is the way to keep the variables current
  ([below](#refreshing-on-a-schedule)).
- **know the time on a key press**, and therefore how old its cache is
  before it asks ([below](#freshness)).
- **use Airly**, or any provider other than GIOŚ
  ([below](#airly-not-in-this-version)). There is no key to enter and no
  credential slot.
- find the station nearest to you (it never asks for a location), cover
  places outside Poland, show a forecast, a history, a map or alerts, or
  send notifications.

## Actions

| Action | What |
| --- | --- |
| `current` (Current air quality) | The index, PM2.5 and PM10 of the chosen station, the index of NO2, O3 and SO2 where the station has one, and the time of the values |
| `find-station` (Find station) | Searches the stations by `query` (a city, a station name, a voivodeship) and lists up to eight with their numbers |
| `select-station` (Select station) | Stores the station with the number `station` as the one `current` reports |
| `forget-station` (Forget station) | Deletes the chosen station and its saved values, and clears the variables |

`current` takes an optional `at`, the run's time
([below](#refreshing-on-a-schedule)).

### Outputs of `current`

| Output | Kind | Example |
| --- | --- | --- |
| `summary` | text | `Warszawa, Al. Niepodległości (mazowieckie): air quality Good (index 1; Polish air quality index, 0 best to 5 worst). PM2.5 15 µg/m³, PM10 23 µg/m³. Index by pollutant: NO2: Good. Measured 2026-11-16 19:00 (Polish time). Źródło danych: GIOŚ - EKOINFONET.` It starts with `[stale]` when the data is not known to be current |
| `index` | integer | `1`; absent when the station has no index now |
| `category` | text | `Good` (in the language of the settings); `No index` |
| `categoryName` | text | `good` (a fixed name, [below](#the-index)); `none` |
| `pm25`, `pm10` | integer | `15`, `23` (µg/m³, rounded); absent when the station has no such value |
| `unit` | text | `µg/m³` |
| `pm25Time`, `pm10Time`, `indexTime` | text | `2026-11-16 19:00`, Polish time; each absent with its value |
| `no2Category`, `o3Category`, `so2Category` | text | `good`; absent when the station has no index for the pollutant |
| `station` | text | `Warszawa, Al. Niepodległości (mazowieckie)` |
| `provider` | text | `gios` |
| `measuredAt` | text | `2026-11-16 19:00`: the **oldest** of the times above, Polish time |
| `measurementAgeMinutes` | integer | `42`: from `measuredAt` to the present; absent when the plugin does not know the present time |
| `stale` | boolean | `true` unless the plugin can show that the values are recent ([below](#freshness)) |
| `source` | text | `network` or `cache` |
| `fetchedAt` | text | `2026-11-16T18:42:09Z`; absent when unknown |
| `ageMinutes` | integer | minutes since the values were fetched; absent when unknown |
| `problem` | text | the error code of a failed request when saved data is shown instead |
| `attribution` | text | `Źródło danych: GIOŚ - EKOINFONET (data source: Poland's Chief Inspectorate of Environmental Protection, CC BY 4.0; an excerpt, values rounded, category names translated).` |

Every message ends with the attribution, and `summary` with its first
sentence. If you show a value of this plugin somewhere else (a
notification, a file, another service), show the source and the time of the
measurement with it: `attribution` and `measuredAt` are there for that.

### Outputs of `find-station` and `select-station`

`find-station`: `count` (all matches), `shown` (how many are listed, at most
eight), `matches` (one line per station: `530: Warszawa, Al. Niepodległości
(mazowieckie)`), `firstId` (the number of the first match), `source`
(`network` or `cache`: the station list) and `attribution`.

`select-station`: `selected` (the station), `stationId`, `hasPm25`,
`hasPm10` (whether the station measures them) and `attribution`.

Coordinates and street addresses are never an output: the plugin does not
read them.

## Variables

`current` publishes `pm25`, `pm10`, `index` (integers), `category`,
`categoryName`, `summary`, `measuredAt`, `station` (texts) and `stale`
(boolean). A value the station does not have now is cleared, not left from
the run before.

Read them in workflows as `{{ plugins.deckpilot.airQuality.pm25 }}`. When
the action fails, `stale` is set to true and the others keep their last
values. Variables are gone after DeckPilot restarts, until the action runs
again. `select-station` and `forget-station` clear them all.

## Choosing a station

1. Run *Find station* with a city (the plugin's page in DeckPilot has a Run
   button): `Kraków`, or `krakow`; Polish letters may be typed without
   their marks. More words narrow the search (`warszawa wokalna`); a
   voivodeship works too. It lists up to eight stations, each with a
   number in front.
2. Run *Select station* with that number. The plugin asks GIOŚ which of
   PM2.5 and PM10 the station measures and stores the choice on this
   computer.

GIOŚ has about 290 stations in over 200 towns. Many measure only some
pollutants: a station without PM2.5 and PM10 can be chosen, and *Current
air quality* then reports its index only. If a station's sensors change,
select it again.

Settings: *Language of the texts* (`en` or `pl`) for categories and
messages, and *Data provider*, which has one value, `gios`. Error messages
are in English. Names and descriptions in the app are English, because a
plugin's manifest has no translations yet; the Polish ones are in
[locales/pl.json](locales/pl.json).

## Refreshing on a schedule

To keep the variables current, add a workflow with a **schedule trigger**
that runs *Current air quality* every 30 or 60 minutes: the data is hourly,
and GIOŚ asks for no more than two pulls an hour. Pass the trigger's firing
time (the event field `firedAt`, `YYYY-MM-DD HH:MM`) to the action's *Run
time* (`at`) parameter through a workflow input. That time is the only
clock the plugin can get before it asks the network, and with it the cache
works as intended:

- a reading fetched less than 30 minutes ago is used again without a
  request;
- after GIOŚ answers "too many requests" (HTTP 429), no request is sent
  until the time it asked to wait (15 minutes if it named none) is over.

A run without `at`, such as a key press, cannot know how old the saved
reading is and asks GIOŚ every time (three small requests). **The plugin
cannot enforce the two pulls an hour for key presses**; that is a limit of
plugin API 1.0 (no clock), and yours to keep until it has one. The simplest
way: let the scheduled workflow do the asking, and read the variables.

## Freshness

Air quality that is hours old is not air quality now, so the plugin never
implies that a value is current. It reports the time GIOŚ gives for every
value, and `stale` is **true unless both** of these hold:

- the reading was fetched in this run, or less than 30 minutes ago by a
  time the plugin knows;
- the **oldest** value in it was measured at most 3 hours before the
  present time, by a present time the plugin knows.

GIOŚ publishes the value of an hour during the hour after it (on
2026-11-16 the values of 19:00 were there at 19:42), so a working station
stays below two hours; three allow one missed publication. A
station whose analyser is down keeps reporting its last values: those come
with `stale` true and a message that starts with "Not current".

The plugin has no clock. Its "present time" is the `Date` header of GIOŚ's
answer in this run, or the time a saved reading was fetched plus the
minutes between two `at` values. When neither is known (GIOŚ could not be
reached and there is no `at`), saved values are shown with `stale` true,
the time they were fetched and no age; the message says that the plugin
cannot tell how old they are.

Times in outputs and messages are Polish time, as GIOŚ gives them
(`fetchedAt` is UTC). The plugin converts them with the European summer
time rule to compare them with the present.

When a request fails and nothing is saved, the action fails with the error
of the request. A denied network permission is always an error: saved data
is not shown in its place.

## The index

The Polish air quality index (Polski indeks jakości powietrza) of GIOŚ has
six categories. `index` is GIOŚ's number, `categoryName` a fixed name, and
`category` the text in the language of the settings.

| `index` | `categoryName` | English | Polish (GIOŚ's name) |
| --- | --- | --- | --- |
| 0 | `very-good` | Very good | Bardzo dobry |
| 1 | `good` | Good | Dobry |
| 2 | `moderate` | Moderate | Umiarkowany |
| 3 | `sufficient` | Sufficient | Dostateczny |
| 4 | `bad` | Bad | Zły |
| 5 | `very-bad` | Very bad | Bardzo zły |
| absent | `none` | No index | Brak indeksu |

The English names are this plugin's translations, not GIOŚ's. The index is
computed by GIOŚ from the pollutants a station measures; the plugin does
not compute one from PM values.

## Permissions

| Permission | Why |
| --- | --- |
| `network`: `api.gios.gov.pl`, `GET` only | The list of stations, the sensors of the station you select, and that station's index and PM values |

Nothing else: no credentials, no notifications, no opening of pages. The
plugin's storage (at most a few dozen kilobytes) needs no permission.

## Privacy

- The station you choose says roughly where you are. Its number goes to
  GIOŚ in the requests for its values (as it would from GIOŚ's own page),
  and the station is kept in the plugin's storage on this computer, in the
  action's message and outputs and in the variable `station`. It is never
  written to the log.
- What you type into *Find station* is not sent anywhere: the plugin
  downloads the whole public list once and searches it on your computer.
- The plugin reads no location and no coordinates. The coordinates and
  street addresses in GIOŚ's station list are dropped when the list is
  read and are never stored.
- Requests carry no key, no cookie and no identifier; the `User-Agent` is
  `DeckPilot-AirQuality/0.1.0`. GIOŚ sees your IP address, as any site
  does.
- *Forget station* deletes the choice and the saved values; uninstalling
  the plugin deletes everything it stored.

## The providers' terms

Read on 2026-11-16; quotations are translated from the providers' words at
that date. Read them again before the plugin is published.

### GIOŚ

Sources: the [API page](https://powietrze.gios.gov.pl/pjp/content/api), the
portal's [terms of use](https://powietrze.gios.gov.pl/pjp/content/terms_of_service)
(read in full) and the OpenAPI description at
`https://api.gios.gov.pl/pjp-api/v3/api-docs`.

| What the terms say | What the plugin does |
| --- | --- |
| The data may be reused, in whole or in part, free of charge, under CC BY 4.0 | Uses it without a key; names the licence in `attribution` |
| The source must be shown "legibly and clearly", in the form "Źródło danych: GIOŚ - EKOINFONET" | Those words end every message and `summary`, start `attribution`, and are in the manifest's description |
| The time the information was produced must be given | Every message and `summary` has the measurement time; outputs `measuredAt`, `pm25Time`, `pm10Time`, `indexTime` |
| Say whether the information is used in whole or in part, and which part | `attribution` says it is an excerpt; this README says which |
| A change of the information must be noted | `attribution` notes rounding and, in English, translated category names |
| "Data is not verified on an ongoing basis and may change later"; values are in µg/m³; gaps happen | Values are reported as given; a missing value is said to be missing |
| Users of the API "undertake to pull data no more often than twice an hour" | A 30-minute cache that scheduled runs cannot bypass; **key presses can** ([above](#refreshing-on-a-schedule)) |
| Limits: 1,500 requests a minute for current measurements and the index; "2 and 1,500" for the list of stations and sensors; 2 a minute for metadata, archives and statistics | The station list is one request (two if GIOŚ ever has more than 500 stations), kept for a week; a station's sensors are asked once, when it is selected; a reading is three requests |
| Versions of the services before `v1` were withdrawn on 2025-06-30 | Uses `/pjp-api/v1/rest/` only |

No logo is required and none is shipped. The icon is this plugin's own
drawing.

Measured on 2026-11-16: the station list has 288 stations; the default
page has 20, and `size=500` returns all of them in one answer of 87 KiB,
below the 512 KiB DeckPilot lets a plugin receive. Stored without
coordinates and streets it has about 18 KiB, in chunks of 50 stations,
each far below the 64 KiB of one storage value.

### Airly: not in this version

Airly was to be a second provider, with the user's own API key in a
credential slot. Its [API terms of service](https://airly.org/en/legal/terms-of-service-airly-api/)
(in force since 2021-01-01, read in full) do not clearly allow that, so the
plugin has no Airly code, host or slot:

- § 7(5)(d): a client on the free plan is forbidden to use "the API, data
  or Database obtained within the API on any websites, platforms or mobile
  applications (commercial or non-commercial)". Whether a desktop
  application showing the key owner their own data is a "platform" is
  Airly's to say, not ours.
- § 9(1)(e): the client must "place a link to the Platform and the Airly
  logo near the data used". A plugin's output is text; plugin API 1.0
  cannot show an image next to it. § 10 licenses the logo to the client
  only, unmodified, so the plugin could not ship it either.
- § 9(1) also asks for Airly as the source, the exact time of the
  measurement, a note on any modification and the scale used; those the
  plugin could do.
- § 6: the key must not be shared (a credential slot fits that), one
  account and key per client, and the plan's daily number of requests must
  not be exceeded (the headers `x-ratelimit-remaining-day` and
  `-minute` tell). Airly's support page gives 100 requests a day without
  an own sensor. The developer documentation
  (`developer.airly.org`) could not be loaded by a program, and the
  `termsOfService` link in the API's own description
  (`https://airly.eu/API-TOS.pdf`) is dead.

Adding Airly needs Airly's written answer on the first two points, and a
way to show its logo. The code is ready for a second provider
(`src/provider.rs`).

## Errors

| Code | When |
| --- | --- |
| `no-station` | No station is chosen |
| `station-not-found` | The search matches no station, or no station has the given number |
| `no-data` | The station reports no PM2.5 value, no PM10 value and no index now |
| `provider-error` | GIOŚ answered with an error |
| `invalid-response` | GIOŚ's answer could not be read |
| `response-too-large` | GIOŚ's answer was larger than the plugin accepts |
| `timed-out` | GIOŚ did not answer in time |
| `throttled` (shared) | GIOŚ answered HTTP 429, or a scheduled run came before the time it asked to wait was over |
| `not-permitted` (shared) | The network permission is not granted |
| `invalid-parameters` (shared) | A query shorter than two letters, a station number out of range, an `at` that is not `YYYY-MM-DD HH:MM` |
| `connection-failed`, `dns-failed`, `tls-failed`, … (shared) | The request did not reach GIOŚ |

For `current`, the provider's errors are reported only when there is no
saved reading of the station to show instead (marked `stale` unless it is
still within its 30 minutes).

## Development

```sh
cd first-party
cargo test --locked -p deckpilot-air-quality
cargo build --locked -p deckpilot-air-quality --target wasm32-unknown-unknown --profile guest
```

The unit tests replace the agent with the SDK's `MockHost` and answer with
canned GIOŚ responses; they never reach the network. `src/provider.rs` is
the trait a second provider implements; `src/gios.rs` is GIOŚ;
`src/cache.rs` is the storage and its rules; `src/time.rs` the calendar
arithmetic.
