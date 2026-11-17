//! The plugin's storage: the chosen place, the last weather answer and the
//! last place searches, and the rules for how long each is used.
//!
//! # Time without a clock
//!
//! Plugin API 1.0 gives a guest no clock, so "how old is the cache" has no
//! answer by itself. The plugin has two real sources of time and uses only
//! these:
//!
//! * `at`, an optional parameter of `current` and `forecast` that a
//!   scheduled workflow fills with its trigger's firing time. With it the
//!   age of the cache is known **before** a request, and an answer younger
//!   than its lifetime is served without one. The value is the computer's
//!   local time to the minute; two of them are subtracted, which is wrong
//!   by the shift when the UTC offset changes in between (an apparent
//!   negative age is treated as unknown).
//! * the `Date` header of an answer from the provider. It dates what is
//!   stored, and when the provider answers with an error it tells how old
//!   the stored data is.
//!
//! A run without `at` (a key press) therefore cannot know the age before
//! it asks, and asks: one request per run. When that request gets no
//! answer at all (offline, a time-out), the stored data is served as stale
//! with the time it was fetched and no age. `time.now` would remove all of
//! this: every run would know the age first.
//!
//! Stored values are a few kilobytes at most, far below the 64 KiB a
//! storage value may have; a unit test checks the largest ones.

use deckpilot_plugin_sdk::prelude::*;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::conditions::Language;
use crate::provider::{Failure, Place, Provider, Report};

/// Storage keys.
pub const WEATHER_KEY: &str = "weather";
pub const LOCATION_KEY: &str = "location";
pub const STATE_KEY: &str = "state";
pub const GEO_PREFIX: &str = "geo:";

/// How long an answer is served without asking again, when the time is
/// known: current conditions (Open-Meteo updates them every 15 minutes).
pub const CURRENT_TTL_SECONDS: u64 = 15 * 60;
/// The daily forecast.
pub const FORECAST_TTL_SECONDS: u64 = 60 * 60;
/// A place search. Places do not move; this only bounds how long a
/// correction of the provider's data stays unseen.
pub const GEO_TTL_SECONDS: u64 = 30 * 24 * 3_600;
/// Place searches kept; the oldest is dropped.
pub const MAX_GEO_ENTRIES: usize = 20;

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
pub struct WeatherEntry {
    pub v: u32,
    /// [`Place::grid`] of the place the answer is for.
    pub grid: String,
    /// The answer's `Date`, Unix seconds.
    pub fetched_at: Option<u64>,
    /// The `at` of the run that fetched it ([`crate::time::local_minutes`]).
    pub fetched_local: Option<i64>,
    pub report: Report,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoEntry {
    pub v: u32,
    pub fetched_at: Option<u64>,
    pub places: Vec<Place>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    /// The latest `Date` of any answer: "it is at least this late".
    #[serde(default)]
    pub last_seen: Option<u64>,
    /// After an HTTP 429: no weather request before this local minute.
    #[serde(default)]
    pub wait_until_local: Option<i64>,
    /// Keys of the stored place searches, oldest first.
    #[serde(default)]
    pub geo_keys: Vec<String>,
}

impl State {
    fn saw(&mut self, date: Option<u64>) {
        self.last_seen = self.last_seen.max(date);
    }
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

/// A weather report and how far it can be trusted.
#[derive(Debug, Clone)]
pub struct Served {
    pub report: Report,
    pub source: Source,
    /// Seconds since it was fetched, when a time source says so.
    pub age: Option<u64>,
    /// When it was fetched (the answer's `Date`).
    pub fetched_at: Option<u64>,
    /// True unless the report is known to be younger than its lifetime.
    pub stale: bool,
    /// The error code of the request that failed, when stored data is
    /// served instead of a new answer.
    pub problem: Option<String>,
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
        ctx.warn(
            "the plugin's storage refused a cache entry; the next run asks the provider again",
        );
    }
}

/// The weather for a place: the stored answer while it is known to be
/// younger than `ttl`, otherwise a new one, otherwise the stored one marked
/// as stale. `at` is the run's local time ([`crate::time::local_minutes`]).
pub fn weather(
    ctx: &mut Context<'_>,
    provider: &dyn Provider,
    place: &Place,
    at: Option<i64>,
    ttl: u64,
) -> Result<Served, PluginError> {
    let grid = place.grid();
    let entry = read::<WeatherEntry>(ctx, WEATHER_KEY)?.filter(|e| e.v == FORMAT && e.grid == grid);
    if let Some(e) = &entry
        && let Some(age) = age(None, e.fetched_local, None, at)
        && age < ttl
    {
        return Ok(Served {
            report: e.report.clone(),
            source: Source::Cache,
            age: Some(age),
            fetched_at: e.fetched_at,
            stale: false,
            problem: None,
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
        match provider.weather(ctx, place) {
            Ok(fetched) => {
                state.saw(fetched.date);
                state.wait_until_local = None;
                save(
                    ctx,
                    WEATHER_KEY,
                    &WeatherEntry {
                        v: FORMAT,
                        grid,
                        fetched_at: fetched.date,
                        fetched_local: at,
                        report: fetched.value.clone(),
                    },
                );
                save(ctx, STATE_KEY, &state);
                return Ok(Served {
                    report: fetched.value,
                    source: Source::Network,
                    age: Some(0),
                    fetched_at: fetched.date,
                    stale: false,
                    problem: None,
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
            Ok(Served {
                report: e.report,
                source: Source::Cache,
                age,
                fetched_at: e.fetched_at,
                stale: !age.is_some_and(|a| a < ttl),
                problem: Some(failure.error.code().to_owned()),
            })
        }
        _ => Err(failure.error),
    }
}

/// The stored report for `place`, without asking the provider: what a key
/// shows (`crate::key`). Absent, of another place or another format:
/// `None`.
pub fn stored_weather(
    ctx: &mut Context<'_>,
    place: &Place,
) -> Result<Option<WeatherEntry>, PluginError> {
    let grid = place.grid();
    Ok(read::<WeatherEntry>(ctx, WEATHER_KEY)?.filter(|e| e.v == FORMAT && e.grid == grid))
}

/// A name as one line with single spaces, at most 80 characters.
pub fn normalize_name(name: &str) -> String {
    let line: String = name
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
    line.chars().take(80).collect()
}

/// A place name as it is searched and keyed: [`normalize_name`] in lower
/// case, at most 100 characters.
pub fn normalize_query(query: &str) -> String {
    normalize_name(query)
        .to_lowercase()
        .chars()
        .take(100)
        .collect()
}

fn geo_key(query: &str, language: Language) -> String {
    format!("{GEO_PREFIX}{}:{query}", language.code())
}

/// Places for a normalized query: the stored answer unless it is known to
/// be older than [`GEO_TTL_SECONDS`], otherwise a new one. A search without
/// a match is not stored.
pub fn places(
    ctx: &mut Context<'_>,
    provider: &dyn Provider,
    query: &str,
    language: Language,
) -> Result<(Vec<Place>, Source), PluginError> {
    let key = geo_key(query, language);
    let mut state = read::<State>(ctx, STATE_KEY)?.unwrap_or_default();
    if let Some(entry) = read::<GeoEntry>(ctx, &key)?.filter(|e| e.v == FORMAT) {
        let expired = age(entry.fetched_at, None, state.last_seen, None)
            .is_some_and(|age| age > GEO_TTL_SECONDS);
        if !expired {
            return Ok((entry.places, Source::Cache));
        }
    }
    let fetched = provider.search(ctx, query, language).map_err(|f| f.error)?;
    state.saw(fetched.date);
    if !fetched.value.is_empty() {
        save(
            ctx,
            &key,
            &GeoEntry {
                v: FORMAT,
                fetched_at: fetched.date,
                places: fetched.value.clone(),
            },
        );
        state.geo_keys.retain(|k| *k != key);
        state.geo_keys.push(key);
        while state.geo_keys.len() > MAX_GEO_ENTRIES {
            let oldest = state.geo_keys.remove(0);
            // Best effort: a key left behind is found by `forget`.
            let _ = ctx.storage_delete(oldest);
        }
    }
    save(ctx, STATE_KEY, &state);
    Ok((fetched.value, Source::Network))
}

/// The place chosen with `find-location`.
pub fn location(ctx: &mut Context<'_>) -> Result<Option<Place>, PluginError> {
    read(ctx, LOCATION_KEY)
}

/// Stores the chosen place and drops the weather of the previous one.
pub fn set_location(ctx: &mut Context<'_>, place: &Place) -> Result<(), PluginError> {
    ctx.storage_set(LOCATION_KEY, place)?;
    ctx.storage_delete(WEATHER_KEY)?;
    Ok(())
}

/// Deletes everything the plugin stored.
pub fn forget(ctx: &mut Context<'_>) -> Result<(), PluginError> {
    for key in [LOCATION_KEY, WEATHER_KEY, STATE_KEY] {
        ctx.storage_delete(key)?;
    }
    for key in ctx.storage_keys(Some(GEO_PREFIX))? {
        ctx.storage_delete(key)?;
    }
    Ok(())
}
