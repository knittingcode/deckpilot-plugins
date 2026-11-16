//! The plugin's storage: the chosen station, the last reading and the
//! provider's station list, and the rules for how long each is used.
//!
//! # Time without a clock
//!
//! Plugin API 1.0 gives a guest no clock, so "how old is this" has no
//! answer by itself. The plugin has two sources of the present time and
//! uses only these:
//!
//! * `at`, an optional parameter of `current` that a scheduled workflow
//!   fills with its trigger's firing time. With it the age of the cache is
//!   known **before** a request, and a reading younger than its lifetime is
//!   served without one. The value is the computer's local time to the
//!   minute; two of them are subtracted, which is wrong by the shift when
//!   the UTC offset changes in between (an apparent negative age is treated
//!   as unknown).
//! * the `Date` header of an answer from the provider. It dates what is
//!   stored, it is the "now" a measurement's own time is compared with, and
//!   when the provider answers with an error it tells how old the stored
//!   data is.
//!
//! A run without `at` (a key press) therefore cannot know the age before
//! it asks, and asks. When that request gets no answer at all (offline, a
//! time-out), the stored reading is served as stale, with the time it was
//! fetched and no age. `time.now` would remove all of this.
//!
//! # Sizes
//!
//! A storage value may have 64 KiB. The station list (288 stations on
//! 2026-11-16) is kept as rows of id, name, city and voivodeship, without
//! coordinates and streets, in chunks of [`CHUNK`] stations: about 3 KiB a
//! chunk, and below 64 KiB even if every text had its full length.

use deckpilot_plugin_sdk::prelude::*;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::provider::{Failure, Provider, Reading, Sensors, Station};

/// Storage keys.
pub const STATION_KEY: &str = "station";
pub const READING_KEY: &str = "reading";
pub const STATE_KEY: &str = "state";
pub const LIST_KEY: &str = "stations";
pub const LIST_PREFIX: &str = "stations:";

/// How long a reading is served without asking again, when the time is
/// known. The data is hourly, and GIOŚ's terms ask for no more than two
/// pulls an hour.
pub const READING_TTL_SECONDS: u64 = 30 * 60;
/// How long the station list is used. Stations change a few times a year
/// and the list service allows two requests a minute.
pub const LIST_TTL_SECONDS: u64 = 7 * 24 * 3_600;
/// Stations in one stored chunk of the list.
pub const CHUNK: usize = 50;

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

/// The station chosen with `select-station`, with its PM sensors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chosen {
    pub v: u32,
    pub provider: String,
    pub station: Station,
    pub sensors: Sensors,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadingEntry {
    pub v: u32,
    pub provider: String,
    pub station: i64,
    /// The answer's `Date`, Unix seconds.
    pub fetched_at: Option<u64>,
    /// The `at` of the run that fetched it ([`crate::time::local_minutes`]).
    pub fetched_local: Option<i64>,
    pub reading: Reading,
}

/// What is known about the stored station list; the stations themselves
/// are in `stations:0`, `stations:1`, ...
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListIndex {
    pub v: u32,
    pub provider: String,
    pub fetched_at: Option<u64>,
    pub chunks: usize,
    pub count: usize,
}

/// A station as it is stored in a chunk: id, name, city, voivodeship.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Row(i64, String, String, String);

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    /// The latest `Date` of any answer: "it is at least this late".
    #[serde(default)]
    pub last_seen: Option<u64>,
    /// After an HTTP 429: no reading request before this local minute.
    #[serde(default)]
    pub wait_until_local: Option<i64>,
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

/// A reading and what is known about when it was fetched.
#[derive(Debug, Clone)]
pub struct Served {
    pub reading: Reading,
    pub source: Source,
    /// Seconds since it was fetched, when a time source says so.
    pub age: Option<u64>,
    /// When it was fetched (the answer's `Date`).
    pub fetched_at: Option<u64>,
    /// The best knowledge of the present time (Unix seconds): the `Date`
    /// of this run's answer, or the fetch time plus the age. A
    /// measurement's own age is counted from it.
    pub now: Option<u64>,
    /// True unless the reading is known to be younger than its lifetime.
    pub expired: bool,
    /// The error code of the request that failed, when the stored reading
    /// is served instead of a new one.
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
fn save(ctx: &mut Context<'_>, key: &str, value: &impl Serialize) -> bool {
    let saved = ctx.storage_set(key, value).is_ok();
    if !saved {
        ctx.warn(
            "the plugin's storage refused a cache entry; the next run asks the provider again",
        );
    }
    saved
}

/// The reading of the chosen station: the stored one while it is known to
/// be younger than [`READING_TTL_SECONDS`], otherwise a new one, otherwise
/// the stored one marked as expired. `at` is the run's local time
/// ([`crate::time::local_minutes`]).
pub fn reading(
    ctx: &mut Context<'_>,
    provider: &dyn Provider,
    chosen: &Chosen,
    at: Option<i64>,
) -> Result<Served, PluginError> {
    let station = chosen.station.id;
    let entry = read::<ReadingEntry>(ctx, READING_KEY)?
        .filter(|e| e.v == FORMAT && e.provider == provider.id() && e.station == station);
    if let Some(e) = &entry
        && let Some(age) = age(None, e.fetched_local, None, at)
        && age < READING_TTL_SECONDS
    {
        return Ok(Served {
            reading: e.reading.clone(),
            source: Source::Cache,
            age: Some(age),
            fetched_at: e.fetched_at,
            now: e.fetched_at.map(|fetched| fetched.saturating_add(age)),
            expired: false,
            problem: None,
        });
    }

    let mut state = read::<State>(ctx, STATE_KEY)?.unwrap_or_default();
    let waiting = matches!((at, state.wait_until_local), (Some(now), Some(until)) if now < until);
    let failure = if waiting {
        Failure::new(
            codes::THROTTLED,
            format!(
                "{} asked to wait after its request limit was reached; it is not asked again yet",
                provider.name()
            ),
        )
        .retryable()
    } else {
        match provider.reading(ctx, station, chosen.sensors) {
            Ok(fetched) => {
                state.saw(fetched.date);
                state.wait_until_local = None;
                save(
                    ctx,
                    READING_KEY,
                    &ReadingEntry {
                        v: FORMAT,
                        provider: provider.id().to_owned(),
                        station,
                        fetched_at: fetched.date,
                        fetched_local: at,
                        reading: fetched.value.clone(),
                    },
                );
                save(ctx, STATE_KEY, &state);
                return Ok(Served {
                    reading: fetched.value,
                    source: Source::Network,
                    age: Some(0),
                    fetched_at: fetched.date,
                    now: fetched.date,
                    expired: false,
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
            let now = failure
                .date
                .or_else(|| Some(e.fetched_at?.saturating_add(age?)));
            Ok(Served {
                reading: e.reading,
                source: Source::Cache,
                age,
                fetched_at: e.fetched_at,
                now,
                expired: !age.is_some_and(|a| a < READING_TTL_SECONDS),
                problem: Some(failure.error.code().to_owned()),
            })
        }
        _ => Err(failure.error),
    }
}

/// The stored station list, when all of it is there.
fn stored_list(
    ctx: &mut Context<'_>,
    index: &ListIndex,
) -> Result<Option<Vec<Station>>, PluginError> {
    let mut stations = Vec::with_capacity(index.count.min(4_096));
    for chunk in 0..index.chunks {
        let Some(rows) = read::<Vec<Row>>(ctx, &format!("{LIST_PREFIX}{chunk}"))? else {
            return Ok(None);
        };
        stations.extend(rows.into_iter().map(|Row(id, name, city, region)| Station {
            id,
            name,
            city,
            region,
        }));
    }
    Ok((stations.len() == index.count && !stations.is_empty()).then_some(stations))
}

/// Stores the list in chunks, then its index. When the storage refuses a
/// chunk, nothing is kept: half a list must not answer a search.
fn store_list(
    ctx: &mut Context<'_>,
    provider: &dyn Provider,
    stations: &[Station],
    date: Option<u64>,
) {
    let previous = ctx.storage_keys(Some(LIST_PREFIX)).unwrap_or_default();
    // Without its index the old chunks are not a list any more.
    let _ = ctx.storage_delete(LIST_KEY);
    let chunks: Vec<&[Station]> = stations.chunks(CHUNK).collect();
    let mut complete = true;
    for (number, chunk) in chunks.iter().enumerate() {
        let rows: Vec<Row> = chunk
            .iter()
            .map(|s| Row(s.id, s.name.clone(), s.city.clone(), s.region.clone()))
            .collect();
        if !save(ctx, &format!("{LIST_PREFIX}{number}"), &rows) {
            complete = false;
            break;
        }
    }
    let kept = if complete { chunks.len() } else { 0 };
    for key in previous {
        let number = key
            .strip_prefix(LIST_PREFIX)
            .and_then(|n| n.parse::<usize>().ok());
        if !number.is_some_and(|n| n < kept) {
            let _ = ctx.storage_delete(key);
        }
    }
    if !complete {
        for number in 0..chunks.len() {
            let _ = ctx.storage_delete(format!("{LIST_PREFIX}{number}"));
        }
        return;
    }
    save(
        ctx,
        LIST_KEY,
        &ListIndex {
            v: FORMAT,
            provider: provider.id().to_owned(),
            fetched_at: date,
            chunks: chunks.len(),
            count: stations.len(),
        },
    );
}

/// Every station of the provider: the stored list unless it is known to be
/// older than [`LIST_TTL_SECONDS`], otherwise a new one, otherwise the
/// stored one all the same (a station list that is a week old is still the
/// list of stations).
pub fn stations(
    ctx: &mut Context<'_>,
    provider: &dyn Provider,
) -> Result<(Vec<Station>, Source), PluginError> {
    let mut state = read::<State>(ctx, STATE_KEY)?.unwrap_or_default();
    let index = read::<ListIndex>(ctx, LIST_KEY)?
        .filter(|index| index.v == FORMAT && index.provider == provider.id());
    let mut stored = None;
    if let Some(index) = &index {
        stored = stored_list(ctx, index)?;
        let expired = age(index.fetched_at, None, state.last_seen, None)
            .is_some_and(|age| age > LIST_TTL_SECONDS);
        if !expired && let Some(stations) = stored.take() {
            return Ok((stations, Source::Cache));
        }
    }
    match provider.stations(ctx) {
        Ok(fetched) => {
            state.saw(fetched.date);
            store_list(ctx, provider, &fetched.value, fetched.date);
            save(ctx, STATE_KEY, &state);
            Ok((fetched.value, Source::Network))
        }
        Err(failure) => match stored {
            Some(stations) if failure.fallback => {
                ctx.warn("the station list could not be renewed; the stored one is used");
                Ok((stations, Source::Cache))
            }
            _ => Err(failure.error),
        },
    }
}

/// The station chosen with `select-station`, when it is one of this
/// provider.
pub fn chosen(
    ctx: &mut Context<'_>,
    provider: &dyn Provider,
) -> Result<Option<Chosen>, PluginError> {
    Ok(read::<Chosen>(ctx, STATION_KEY)?
        .filter(|chosen| chosen.v == FORMAT && chosen.provider == provider.id()))
}

/// Stores the chosen station and drops the reading of the previous one.
pub fn choose(
    ctx: &mut Context<'_>,
    provider: &dyn Provider,
    station: &Station,
    sensors: Sensors,
) -> Result<(), PluginError> {
    ctx.storage_set(
        STATION_KEY,
        &Chosen {
            v: FORMAT,
            provider: provider.id().to_owned(),
            station: station.clone(),
            sensors,
        },
    )?;
    ctx.storage_delete(READING_KEY)?;
    Ok(())
}

/// Deletes what the plugin stored about the user's choice: the station,
/// its reading and the request state. The provider's public station list
/// stays, because asking for it again is what its request limit is for.
pub fn forget(ctx: &mut Context<'_>) -> Result<(), PluginError> {
    for key in [STATION_KEY, READING_KEY, STATE_KEY] {
        ctx.storage_delete(key)?;
    }
    Ok(())
}
