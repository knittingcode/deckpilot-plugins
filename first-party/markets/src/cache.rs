//! The plugin's storage: the recent rates of each pair, the state of each
//! alert, and the rules for how long a stored rate is used.
//!
//! # Time without a clock
//!
//! Plugin API 1.0 gives a guest no clock. The plugin has two real sources
//! of time and uses only these (as `deckpilot.weather` does):
//!
//! * `at`, an optional parameter of every rate action that a scheduled
//!   workflow fills with its trigger's firing time. With it the age of the
//!   stored rates is known **before** a request, and rates younger than
//!   their lifetime are served without one. The lifetime is the answer's
//!   own `Cache-Control: max-age` (Frankfurter sets it to the time until it
//!   expects new rates), kept between [`MIN_TTL_SECONDS`] and
//!   [`MAX_TTL_SECONDS`].
//! * the `Date` header of an answer. It dates what is stored and, when the
//!   provider answers with an error, tells how old the stored data is.
//!
//! A run without `at` (a key press) cannot know the age before it asks, and
//! asks: one request per run. When that request gets no answer at all, the
//! stored rates are served as stale. After an HTTP 429 a run with `at`
//! sends nothing until `Retry-After` has passed; a run without `at` cannot
//! tell when that is and asks again only when the user presses the key.
//!
//! # Bounds
//!
//! One pair is one value of at most [`MAX_POINTS`] rates (about 1.5 KB);
//! at most [`MAX_PAIRS`] pairs and [`MAX_ALERTS`] alerts are kept, the
//! least recently used ones are dropped. All of it stays far below the
//! 64 KiB a storage value and the 256 KiB a plugin may have.

use deckpilot_plugin_sdk::prelude::*;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::provider::{Failure, Pair, Point, Provider};
use crate::time::{day_number, local_day, utc_day};

/// Storage keys.
pub const STATE_KEY: &str = "state";
pub const PAIR_PREFIX: &str = "pair:";
pub const ALERT_PREFIX: &str = "alert:";

/// Rates kept per pair: the newest ones.
pub const MAX_POINTS: usize = 32;
/// Days before the newest known date that a request asks for: enough for
/// 20 ECB working days and a change over them.
pub const WINDOW_DAYS: i64 = 31;
/// Rates older than this many days before the newest one are dropped, so
/// that the history stays recent after a long pause.
pub const HISTORY_DAYS: i64 = 92;
/// Pairs and alerts kept.
pub const MAX_PAIRS: usize = 32;
pub const MAX_ALERTS: usize = 64;
/// Lifetime of stored rates without a `max-age`, and its bounds.
pub const DEFAULT_TTL_SECONDS: u64 = 4 * 3_600;
pub const MIN_TTL_SECONDS: u64 = 30 * 60;
pub const MAX_TTL_SECONDS: u64 = 12 * 3_600;
/// A newest rate older than this many days on the day of the answer is
/// stale even when the answer is new: more than a long weekend with a
/// holiday on either side (Easter: Thursday to Tuesday is 5), so the ECB
/// stopped publishing it or the provider is behind.
pub const MAX_RATE_LAG_DAYS: i64 = 5;

/// `problem` of rates that are new but old.
pub const OLD_RATE: &str = "old-rate";

/// Format of the stored values; another number is read as "nothing stored".
const FORMAT: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Network,
    Cache,
}

impl Source {
    pub fn id(self) -> &'static str {
        match self {
            Self::Network => "network",
            Self::Cache => "cache",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairEntry {
    pub v: u32,
    /// [`Pair::id`].
    pub pair: String,
    /// The `Date` of the answer, Unix seconds.
    pub fetched_at: Option<u64>,
    /// The `at` of the run that fetched it ([`crate::time::local_minutes`]).
    pub fetched_local: Option<i64>,
    /// Seconds the answer may be served without asking again.
    pub ttl: u64,
    /// Oldest first, unique dates, at most [`MAX_POINTS`].
    pub points: Vec<Point>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    /// The latest `Date` of any answer: "it is at least this late".
    #[serde(default)]
    pub last_seen: Option<u64>,
    /// After an HTTP 429: no request before this local minute.
    #[serde(default)]
    pub wait_until_local: Option<i64>,
    /// Pair ids stored, least recently used first.
    #[serde(default)]
    pub pairs: Vec<String>,
    /// Alert keys stored, least recently used first.
    #[serde(default)]
    pub alerts: Vec<String>,
}

impl State {
    fn saw(&mut self, date: Option<u64>) {
        self.last_seen = self.last_seen.max(date);
    }
}

/// Moves `key` to the end of `list`, and returns the keys beyond `limit`
/// (from the front) that are no longer listed.
fn touch(list: &mut Vec<String>, key: &str, limit: usize) -> Vec<String> {
    list.retain(|k| k != key);
    list.push(key.to_owned());
    let excess = list.len().saturating_sub(limit);
    list.drain(..excess).collect()
}

/// The age in seconds of something fetched at `fetched_at` (UTC) by a run
/// at `fetched_local`, seen at `now_utc` or by a run at `now_local`. `None`
/// when no pair of times of the same kind is known, or when the pair says
/// the data is from the future.
pub fn age(
    fetched_at: Option<u64>,
    fetched_local: Option<i64>,
    now_utc: Option<u64>,
    now_local: Option<i64>,
) -> Option<u64> {
    if let (Some(now), Some(then)) = (now_utc, fetched_at)
        && now >= then
    {
        return Some(now - then);
    }
    if let (Some(now), Some(then)) = (now_local, fetched_local)
        && now >= then
    {
        return u64::try_from((now - then).saturating_mul(60)).ok();
    }
    None
}

/// Rates of a pair and how far they can be trusted.
#[derive(Debug, Clone)]
pub struct Served {
    /// Oldest first, never empty.
    pub points: Vec<Point>,
    pub source: Source,
    /// Seconds since they were fetched, when a time source says so.
    pub age: Option<u64>,
    /// When they were fetched (the answer's `Date`).
    pub fetched_at: Option<u64>,
    /// Days between the newest rate and the day of the answer, when known.
    pub lag_days: Option<i64>,
    /// True unless they are known to be within their lifetime and the
    /// newest rate is a recent one.
    pub stale: bool,
    /// Why: the error code of a request that failed, or [`OLD_RATE`].
    pub problem: Option<String>,
}

impl Served {
    pub fn latest(&self) -> &Point {
        self.points.last().expect("served rates are never empty")
    }
}

/// A stored value; one that does not have the expected shape (an older
/// format, a damaged entry) counts as absent.
fn read<T: DeserializeOwned>(ctx: &mut Context<'_>, key: &str) -> Result<Option<T>, PluginError> {
    Ok(ctx
        .storage_get(key)?
        .and_then(|value| serde_json::from_value(value).ok()))
}

/// Saving is best effort: a full or failing storage must not hide an
/// answer the plugin already has.
fn save(ctx: &mut Context<'_>, key: &str, value: &impl Serialize) {
    if ctx.storage_set(key, value).is_err() {
        ctx.warn("the plugin's storage refused a value; the next run asks the provider again");
    }
}

pub fn pair_key(pair: &Pair) -> String {
    format!("{PAIR_PREFIX}{}", pair.id())
}

/// `points` merged into `old`: a date of `points` replaces the same date of
/// `old`, rates older than [`HISTORY_DAYS`] before the newest are dropped,
/// at most [`MAX_POINTS`] stay.
pub fn merge(old: &[Point], points: &[Point]) -> Vec<Point> {
    let mut all: Vec<Point> = old
        .iter()
        .filter(|p| !points.iter().any(|n| n.d == p.d))
        .chain(points.iter())
        .filter(|p| day_number(&p.d).is_some())
        .cloned()
        .collect();
    all.sort_by(|a, b| a.d.cmp(&b.d));
    if let Some(newest) = all.last().and_then(|p| day_number(&p.d)) {
        all.retain(|p| day_number(&p.d).is_some_and(|d| newest - d <= HISTORY_DAYS));
    }
    let excess = all.len().saturating_sub(MAX_POINTS);
    all.drain(..excess);
    all
}

fn lag_days(points: &[Point], date: Option<u64>) -> Option<i64> {
    let newest = day_number(&points.last()?.d)?;
    Some(utc_day(date?) - newest)
}

fn is_old(lag: Option<i64>) -> bool {
    lag.is_some_and(|days| days > MAX_RATE_LAG_DAYS)
}

/// The rates of a pair: the stored ones while they are known to be within
/// their lifetime, otherwise new ones, otherwise the stored ones marked as
/// stale. `at` is the run's local time ([`crate::time::local_minutes`]).
pub fn rates(
    ctx: &mut Context<'_>,
    provider: &dyn Provider,
    pair: &Pair,
    at: Option<i64>,
) -> Result<Served, PluginError> {
    let key = pair_key(pair);
    let entry = read::<PairEntry>(ctx, &key)?
        .filter(|e| e.v == FORMAT && e.pair == pair.id() && !e.points.is_empty());
    if let Some(e) = &entry
        && let Some(age) = age(None, e.fetched_local, None, at)
        && age < e.ttl
    {
        let lag = lag_days(&e.points, e.fetched_at);
        return Ok(Served {
            points: e.points.clone(),
            source: Source::Cache,
            age: Some(age),
            fetched_at: e.fetched_at,
            lag_days: lag,
            stale: is_old(lag),
            problem: is_old(lag).then(|| OLD_RATE.to_owned()),
        });
    }

    let mut state = read::<State>(ctx, STATE_KEY)?.unwrap_or_default();
    let waiting = matches!((at, state.wait_until_local), (Some(now), Some(until)) if now < until);
    let failure = if waiting {
        Failure::new(
            codes::THROTTLED,
            format!(
                "{} asked to wait after its rate limit was reached; it is not asked again yet",
                provider.name()
            ),
        )
        .retryable()
    } else {
        // The day the window ends: the newest one the plugin knows of.
        let known = [
            entry
                .as_ref()
                .and_then(|e| e.points.last())
                .and_then(|p| day_number(&p.d)),
            state.last_seen.map(utc_day),
            at.map(local_day),
        ]
        .into_iter()
        .flatten()
        .max();
        match fetch(ctx, provider, pair, known) {
            Ok(fetched) => {
                state.saw(fetched.date);
                state.wait_until_local = None;
                let old = entry.as_ref().map(|e| e.points.as_slice()).unwrap_or(&[]);
                let points = merge(old, &fetched.value);
                let ttl = fetched
                    .max_age
                    .unwrap_or(DEFAULT_TTL_SECONDS)
                    .clamp(MIN_TTL_SECONDS, MAX_TTL_SECONDS);
                save(
                    ctx,
                    &key,
                    &PairEntry {
                        v: FORMAT,
                        pair: pair.id(),
                        fetched_at: fetched.date,
                        fetched_local: at,
                        ttl,
                        points: points.clone(),
                    },
                );
                for dropped in touch(&mut state.pairs, &pair.id(), MAX_PAIRS) {
                    // Best effort: a key left behind is found by `forget`.
                    let _ = ctx.storage_delete(format!("{PAIR_PREFIX}{dropped}"));
                }
                save(ctx, STATE_KEY, &state);
                let lag = lag_days(&points, fetched.date);
                return Ok(Served {
                    points,
                    source: Source::Network,
                    age: Some(0),
                    fetched_at: fetched.date,
                    lag_days: lag,
                    stale: is_old(lag),
                    problem: is_old(lag).then(|| OLD_RATE.to_owned()),
                });
            }
            Err(failure) => {
                state.saw(failure.date);
                if let (Some(wait), Some(now)) = (failure.retry_after, at) {
                    let minutes = i64::try_from(wait.div_ceil(60)).unwrap_or(i64::MAX);
                    state.wait_until_local = Some(now.saturating_add(minutes));
                }
                if failure.date.is_some() || failure.retry_after.is_some() {
                    save(ctx, STATE_KEY, &state);
                }
                failure
            }
        }
    };
    match entry {
        Some(e) if failure.fallback => {
            let age = age(e.fetched_at, e.fetched_local, failure.date, at);
            let lag = lag_days(&e.points, failure.date.or(e.fetched_at));
            Ok(Served {
                points: e.points,
                source: Source::Cache,
                age,
                fetched_at: e.fetched_at,
                lag_days: lag,
                // Within its lifetime the ECB has published nothing newer;
                // the failed request is reported all the same.
                stale: !age.is_some_and(|a| a < e.ttl) || is_old(lag),
                problem: Some(failure.error.code().to_owned()),
            })
        }
        _ => Err(failure.error),
    }
}

/// The window up to the newest known day; without one, the newest rate
/// alone and then the window up to its day. A failure of that second
/// request leaves the one rate.
fn fetch(
    ctx: &mut Context<'_>,
    provider: &dyn Provider,
    pair: &Pair,
    known: Option<i64>,
) -> Result<crate::provider::Fetched<Vec<Point>>, Failure> {
    if let Some(day) = known {
        return provider.series(ctx, pair, Some(day - WINDOW_DAYS));
    }
    let first = provider.series(ctx, pair, None)?;
    let Some(newest) = first.value.last().and_then(|p| day_number(&p.d)) else {
        return Ok(first);
    };
    match provider.series(ctx, pair, Some(newest - WINDOW_DAYS)) {
        Ok(window) => Ok(window),
        Err(_) => Ok(first),
    }
}

/// The state of one alert: whether its condition held at the last check
/// that could be evaluated, and for which rate date.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertEntry {
    pub v: u32,
    pub triggered: bool,
    pub rate_date: String,
}

/// `alert:EUR-USD:above:1.1`; the threshold in [`crate::numbers::canonical`]
/// form, so that `1,10` and `1.1` are one alert.
pub fn alert_key(pair: &Pair, condition: &str, threshold: &str) -> String {
    let key = format!("{ALERT_PREFIX}{}:{condition}:{threshold}", pair.id());
    key.chars().take(128).collect()
}

pub fn alert(ctx: &mut Context<'_>, key: &str) -> Result<Option<AlertEntry>, PluginError> {
    Ok(read::<AlertEntry>(ctx, key)?.filter(|e| e.v == FORMAT))
}

pub fn set_alert(ctx: &mut Context<'_>, key: &str, triggered: bool, rate_date: &str) {
    save(
        ctx,
        key,
        &AlertEntry {
            v: FORMAT,
            triggered,
            rate_date: rate_date.to_owned(),
        },
    );
    let Ok(state) = read::<State>(ctx, STATE_KEY) else {
        return;
    };
    let mut state = state.unwrap_or_default();
    for dropped in touch(&mut state.alerts, key, MAX_ALERTS) {
        let _ = ctx.storage_delete(dropped);
    }
    save(ctx, STATE_KEY, &state);
}

/// Deletes everything the plugin stored.
pub fn forget(ctx: &mut Context<'_>) -> Result<usize, PluginError> {
    let mut deleted = 0;
    for prefix in [PAIR_PREFIX, ALERT_PREFIX] {
        for key in ctx.storage_keys(Some(prefix))? {
            ctx.storage_delete(key)?;
            deleted += 1;
        }
    }
    ctx.storage_delete(STATE_KEY)?;
    Ok(deleted)
}
