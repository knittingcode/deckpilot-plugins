# Markets (`deckpilot.markets`)

Exchange rates for DeckPilot: the European Central Bank's **euro foreign
exchange reference rates**, read through [Frankfurter](https://frankfurter.dev/)
without an account or a key. A conversion, the change since the previous
working day or over up to 20 working days, the range of recent rates, and an
alert that notifies you once when a rate crosses your threshold, as a
message, as outputs for workflows and as variables.

Source: European Central Bank euro foreign exchange reference rates, via
Frankfurter. The ECB publishes them "for information purposes only".

**These are daily reference rates, not live prices.** The ECB publishes one
rate per currency once per working day, at about 16:00 CET. Every output
says which day a rate is for (`rateDate`), where it is from (`source`,
`attribution`), and carries `delayed` (always `true`) and `stale`.

**Information, not financial advice.** The plugin reports published
reference rates. It recommends nothing, and its rates are not the rates at
which you can buy or sell; the ECB "strongly discourages" using them for
transactions.

Polish: [README.pl.md](README.pl.md).

> **Status.** Version 0.1.0 is built, tested and packaged by CI and is
> **not published**. It cannot be installed from a registry yet
> ([first-party/README.md](../README.md)).

## What it does not do

- **No live prices, no intraday values.** The ECB's reference rates are
  the only data. Before about 16:00 CET, on weekends and on TARGET holidays
  the newest rate is the previous working day's; the message says so.
- **No cryptocurrency prices** in this version
  ([below](#cryptocurrencies-deferred)).
- **No stock quotes** ([below](#stock-quotes-not-provided)).
- **No refresh by itself** (no timers) and **no clock**
  ([below](#the-cache-and-time)). A scheduled workflow refreshes it.

## Actions

| Action | What |
| --- | --- |
| `rate` (Exchange rate) | The newest reference rate of `base`/`quote`, an optional conversion of `amount`, the change since the previous rate, the lowest and highest saved rate and the trend |
| `change` (Rate change) | The change over the last `days` ECB working days (1-20, default 1) |
| `check-alert` (Check alert) | Compares the rate with `threshold` (`condition` `above` or `below`) and notifies once per crossing |
| `forget` (Forget saved data) | Deletes the saved rates and the state of every alert, and clears the variables |

`base` is an ISO 4217 code (default `EUR`), `quote` another one (`USD`,
`PLN`, `GBP`, `CHF`, `JPY` …). A pair without the euro (`USD`/`PLN`) is a
**cross rate** that Frankfurter computes from the ECB's two euro rates;
`cross` is then `true` and the message says so. Amounts and thresholds are
typed as text, with a decimal point or comma (`99.50`, `4,35`), because a
plugin parameter has no decimal kind. Every rate action takes an optional
`at`, the run's time ([below](#refreshing-on-a-schedule)).

### Outputs every rate action has

| Output | Kind | Example |
| --- | --- | --- |
| `pair` | text | `EUR/USD` |
| `rate` | text | `1.1014` (a decimal text: outputs have no decimal kind) |
| `rateDate` | text | `2026-11-16`, the day the reference rate is for |
| `cross` | boolean | `false` |
| `delayed` | boolean | always `true`: a daily reference rate, never live |
| `stale` | boolean | `true` when the rate is not known to be current |
| `source` | text | `ecb-frankfurter` |
| `attribution` | text | `Source: European Central Bank euro foreign exchange reference rates, via Frankfurter (frankfurter.dev). Published for information purposes only.` |
| `fetchedAt` | text | `2026-11-16T19:03:29Z` (the answer's `Date`), when known |
| `ageMinutes` | integer | when known |
| `problem` | text | why the rate is stale or not new: an error code, or `old-rate` |

### Outputs of `rate`

`summary` (one sentence, starting with `[stale]` when stale), `base`,
`quote`, `asOf` (`ECB reference rate of 2026-11-16, about 16:00 CET`),
`amount` and `converted` (with an `amount`), `previousRate`,
`previousDate`, `change` (`-0.0038`), `changePercent` (`-0.34`) and
`direction` (`up`, `down` or `flat` within ±0.1%) when a previous rate is
saved, `min`, `max`, `trend` (from the oldest saved rate to the newest),
`historyCount`, `historyFrom`.

### Outputs of `change`

`summary`, `fromRate`, `fromDate`, `days`, `change`, `changePercent`,
`direction`. With fewer saved rates than `days` + 1 it fails with
`not-enough-history`.

### Outputs of `check-alert`

`summary`, `condition`, `threshold`, `triggered` (the condition holds for
the rate shown), `crossed` (it holds now and did not at the previous
check), `notified`, `evaluated` (false for a stale rate), and
`notification` when a notification was refused or the user's settings
showed it nowhere.

## Variables

Published by `rate`: `pair`, `rate`, `rateDate`, `changePercent`,
`summary` (texts), `stale` (boolean), `history` (JSON: up to 32
`{"date", "rate"}`, oldest first). Published by `check-alert` when it
evaluated: `alertTriggered` (boolean). Read them as
`{{ plugins.deckpilot.markets.rate }}`. They hold the last run, whatever
pair it was for. When a rate action fails, `stale` is set to true.

## Alerts

*Check alert* compares the newest reference rate with your threshold
(`above`: at or above it; `below`: at or below it) and keeps, per pair,
condition and threshold, whether the condition held at the last check. It
notifies **once when the condition starts to hold**, not at every check;
when the rate goes back, the alert is re-armed and the next crossing
notifies again. The first check of a new alert whose condition already
holds counts as a crossing. A stale rate is not compared and changes
nothing. A notification DeckPilot refuses (its limit is 5 a minute) is
tried again at the next check. Where a notification appears is set in the
plugin's settings (*Desktop notifications*, *Status bar*) and DeckPilot's.

Run it from a scheduled workflow ([below](#refreshing-on-a-schedule)). At
most 64 alerts are kept, the least recently checked ones are dropped.

## Refreshing on a schedule

To keep the variables current or to check alerts, add a workflow with a
**schedule trigger** that runs *Exchange rate* or *Check alert*. The rates
change once a working day, so every hour or two is plenty. Pass the
trigger's firing time (the event field `firedAt`, `YYYY-MM-DD HH:MM`) to
the action's *Run time* (`at`) parameter through a workflow input. With it:

- saved rates younger than their lifetime are used again without a
  request. The lifetime is the answer's own `Cache-Control: max-age`
  (Frankfurter sets it to the time until it expects new rates, about five
  hours on the evening of a working day), kept between 30 minutes and 12
  hours, 4 hours without one;
- after Frankfurter answers "too many requests" (HTTP 429), nothing is sent
  until the `Retry-After` it named (15 minutes if none) is over.

## The cache and time

Each pair is one request to
`https://api.frankfurter.dev/v2/providers/ecb/rates?base=EUR&quotes=USD&from=<31 days before the newest known day>`,
which brings up to about 22 working days of rates; they are merged into the
saved history of the pair (at most 32 rates, none older than 92 days before
the newest). The first request for a pair, when no date is known yet, asks
for the newest rate alone first. At most 32 pairs are kept.

A plugin has **no clock** in plugin API 1.0. Consequences, said plainly:

- **A key press without `at` always sends one request**, however fresh the
  saved rates are, and cannot honour a `Retry-After` (it cannot tell when
  that is over). DeckPilot limits a plugin to 60 requests a minute;
  Frankfurter names no quota.
- **Offline** or after an error, the saved rates are shown with
  `stale: true` and their fetch time, unless their age is known (from the
  error answer's `Date` or `at`) to be within the lifetime. Without saved
  rates the action fails with the error's code. Saved rates are never
  shown for an unknown currency or when the network permission is denied.
- A newest rate more than 5 days older than the day of the answer is
  `stale` with `problem: old-rate` even when the request succeeded: the ECB
  stopped publishing that currency (it did for the Russian rouble in 2022)
  or the source is behind. Weekends and holidays (Easter: Thursday to
  Tuesday) are within those 5 days.

`time.now` and timers (docs/plugin-factory.md §6.2, §6.3) would remove
these limits.

## On a key

*Since 0.2.0; needs DeckPilot with plugin API 1.2.* A key bound to
**Exchange rate** shows the pair's newest reference rate with its change
since the previous working day (`4.2731 +0.12%`), the pair and the rate's
date below it (`EUR/PLN 11-16`), and a sparkline of the stored rates (up to
32 working days) behind them. Its colour is green when the rate went up,
red when it went down, none when it is flat.

**Never shown as live.** These are daily reference rates, and the key
always names the day the rate is for. When the rates are current it adds
no "ok" mark. When they are not known to be current, the rates stay but
the key adds a warning and a badge:

- fetched longer ago than the answer's lifetime: the age of the fetch, such
  as `5h`;
- a newest rate more than 5 days old: its lag in days, such as `9d`;
- an unknown age: `?`.

The key shows the **rates the last run of the action fetched** (a press of
the key, or a scheduled workflow); showing them never asks Frankfurter.
Each key shows its own pair. Before the first run for a pair, the key shows
the title you gave it. A device without displays shows nothing.

## Permissions

| Permission | Why |
| --- | --- |
| `network`: `api.frankfurter.dev`, `GET` | The ECB rates of the pairs you ask for |
| `notifications` (routed by the settings *Desktop notifications* and *Status bar*) | *Check alert* |
| `device`: `state` | The rate, its date and a sparkline on the keys bound to *Exchange rate* ([On a key](#on-a-key)) |

Nothing else. No credential: Frankfurter has no key, and the plugin
contains none.

## Privacy

- **Frankfurter receives** the two currency codes and a start date, with
  your IP address as with any request, and `User-Agent:
  DeckPilot-Markets/0.1.0`. Never an amount or a threshold.
- **On this computer** the plugin stores the recent rates of each pair and,
  per alert, its pair, condition, threshold and last state (DeckPilot's
  plugin storage). *Forget saved data* deletes all of it; uninstalling the
  plugin does too.
- Amounts and thresholds are yours: they appear in the action's message,
  outputs and the alert's notification, and **never in the log**.

## Frankfurter's and the ECB's terms

Read on 2026-11-16; read them again before relying on this.

- **Frankfurter** (<https://frankfurter.dev/>, <https://frankfurter.dev/license/>):
  open source (MIT); "There are no quotas", requests are rate-limited
  against abuse with "no monthly or daily caps"; "Storing daily
  observations locally for caching or historical analytics is encouraged
  to keep API traffic light"; "Rates and the API come as is, with no
  warranty", they "can be late, missing, or revised". Frankfurter "doesn't
  own the rates": each provider's terms apply. Its default `/v2/rates`
  blends several providers; this plugin uses only the ECB provider's route
  (`/v2/providers/ecb/rates`), which the documentation recommends for ECB
  data. The older `/v1` API is deprecated.
- **ECB** (<https://www.ecb.europa.eu/services/using-our-site/disclaimer/html/index.en.html>,
  <https://www.ecb.europa.eu/stats/policy_and_exchange_rates/euro_reference_exchange_rates/html/index.en.html>):
  information may be used freely when "it must appear accurately and the
  ECB must be cited as the source"; modified information must be stated as
  such. "The reference rates are usually updated at around 16:00 CET every
  working day, except on TARGET closing days", "are published for
  information purposes only", and "using the rates for transaction
  purposes is strongly discouraged". The plugin cites the ECB in every
  message and in `attribution`, and says when a value is computed: a cross
  rate (by Frankfurter), a conversion, a change (by the plugin). **If you
  show the outputs elsewhere, keep the source with them.**

## Cryptocurrencies (deferred)

Cryptocurrency prices from CoinGecko, with the user's own free *Demo* API
key in a credential slot, were planned and are **not in this version**.
CoinGecko's API Terms (latest version 5 September 2025), as read on
2026-11-16:

- §7.1: if a product using the API is "offered for use to others outside
  your entity", it needs a binding user agreement and privacy policy that
  credit CoinGecko, disclaim its liability and make the product's maker
  "solely responsible for all disclosures, disclaimers, and warnings",
  including the "inherent volatility and risks associated with
  cryptocurrency markets". A plugin distributed by DeckPilot is such a
  product, and DeckPilot has no such agreement. That is the owner's
  decision, not a plugin's.
- §6: caching is "not encouraged", stored data "should" be strongly
  encrypted and refreshed at least every 24 hours, and §6.2 forbids to
  "store, derive from" data except as permitted, which conflicts with the
  bounded history and the min/max/trend this plugin keeps.
- The terms say nothing about a user's own key in someone else's
  application; the pricing page calls Demo "a free plan to try out the
  CoinGecko API" (100 calls a minute, 10,000 a month) with "Attribution
  required" ("Powered by CoinGecko").

What adding it would take, once the owner has decided: a second
`Provider` (`src/provider.rs`) for `api.coingecko.com` (`/api/v3/simple/price`
with `include_24hr_change` and `include_last_updated_at`, which gives the
as-of time), a credential slot with the `header` scheme and `headerName`
`x-cg-demo-api-key`, `credentials.has` before each request with a
`credential-missing` error that points to the plugin's settings, no stored
history for its data unless the terms allow it, and the attribution
"Powered by CoinGecko" in every message.

## Stock quotes (not provided)

The provider audit ([docs/plugin-factory.md](../../docs/plugin-factory.md)
§5) found no provider whose terms allow showing quotes in a distributed
desktop application: Twelve Data's free plan is for "internal non-display
usage", and a key that shows a price is display; Stooq has no API and no
terms, and its `robots.txt` disallows everything, so using it would be
scraping; Alpha Vantage and Finnhub need the user's key and are for
personal, non-commercial use (Alpha Vantage: 25 requests a day). A provider for stocks would need: a key
of the user's own in a credential slot (the plugin can never ship a key),
terms that allow display in an application the user runs for themselves,
quotes with their exchange time and delay stated by the provider, limits a
cache can respect, and exact https hosts.

## Errors

| Code | Meaning |
| --- | --- |
| `unknown-currency` | The ECB publishes no reference rate for that currency, or Frankfurter does not know it |
| `invalid-amount` | The amount is not a positive number |
| `invalid-threshold` | The threshold is not a positive number |
| `not-enough-history` | Fewer saved rates than the working days asked for |
| `provider-error` | Frankfurter answered with an error, and no saved rates could be shown instead |
| `invalid-response` | Frankfurter's answer could not be read, and no saved rates could be shown instead |
| `response-too-large` | Frankfurter's answer was larger than the plugin accepts (128 KiB) |
| `timed-out` | Frankfurter did not answer in time, and no saved rates could be shown instead |

Shared codes it also reports: `throttled` (Frankfurter's or DeckPilot's
rate limit), `not-permitted` (the network permission is not granted),
`invalid-parameters` (a currency code that is not three letters, the same
currency twice, a malformed `at`), `dns-failed`, `connection-failed`,
`tls-failed`, `request-failed`.

## Settings

*Language of the texts* (`en` or `pl`) for messages and notifications;
error messages are in English. *Desktop notifications* and *Status bar*
route alerts. Names and descriptions in the app are English, because a
plugin's manifest has no translations yet; the Polish ones are in
[locales/pl.json](locales/pl.json).

## Development

```sh
cd first-party
cargo test --locked -p deckpilot-markets
cargo build --locked -p deckpilot-markets --target wasm32-unknown-unknown --profile guest
```

The unit tests use canned Frankfurter answers and never reach the network.
The source is behind the trait `provider::Provider` (`src/provider.rs`);
`src/frankfurter.rs` is its only implementation.

The icon (`assets/icon-128.png`) is original artwork made for this plugin:
two opposite arrows and a line of values, drawn from lines and triangles.
