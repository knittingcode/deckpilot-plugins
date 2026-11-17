//! The plugin's storage: the chosen station, the last reading and the
//! provider's station list, and the rules for how long each is used.
//!
//! # Time and GIOŚ's two pulls an hour
//!
//! GIOŚ's terms ask API users to pull data no more often than twice an
//! hour. Every run of `current` reads the agent's clock (`time.now`, plugin
//! API 1.1; the plugin requires 1.2), so the rule holds for a key press as
//! much as for a scheduled run:
//!
//! * a reading pulled less than [`READING_TTL_SECONDS`] ago is served
//!   without a request;
//! * a request for a station's reading is never sent less than
//!   [`READING_TTL_SECONDS`] after the last one that may have reached
//!   GIOŚ, whatever it brought back (an answer, an error or a time-out);
//!   only a request that never left this computer (a DNS or connection
//!   failure, a denied permission) does not count;
//! * after an HTTP 429 nothing is asked before the time GIOŚ named.
//!
//! The clock is the system's and can be set back. A pull that appears to
//! be in the future has an unknown age and does not hold a request back,
//! so a clock set back allows one request early, once.
//!
//! `at`, the optional firing time a scheduled workflow passes, works as in
//! 0.2.1: two of them are subtracted (wrong by the shift when the UTC
//! offset changes in between; an apparent negative age is unknown), and a
//! reading younger than its lifetime by either the clock or `at` is served
//! from the cache. The `Date` header of an answer dates what GIOŚ sent
//! (`fetchedAt`).
//!
//! # Sizes
//!
//! A storage value may have 64 KiB. The station list (288 stations on
//! 2026-11-16) is kept as rows of id, name, city and voivodeship, without
//! coordinates and streets, in chunks of [`CHUNK`] stations: about 3 KiB a
//! chunk, and below 64 KiB even if every text had its full length.

use std::collections::BTreeMap;

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
    /// The agent's clock when it was fetched, Unix seconds (since 0.2.2;
    /// absent in a reading stored by an older version).
    #[serde(default)]
    pub pulled_at: Option<u64>,
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
    /// After an HTTP 429: no reading request before this time of the
    /// agent's clock (Unix seconds).
    #[serde(default)]
    pub wait_until: Option<u64>,
    /// Station number to the agent's clock (Unix seconds) of the last
    /// reading request that may have reached the provider. Only entries
    /// younger than [`READING_TTL_SECONDS`] are kept.
    #[serde(default)]
    pub pulls: BTreeMap<i64, u64>,
}

impl State {
    fn saw(&mut self, date: Option<u64>) {
        self.last_seen = self.last_seen.max(date);
    }

    /// Seconds since the last request for `station`'s reading, when the
    /// clock says so.
    fn since_pull(&self, station: i64, clock: u64) -> Option<u64> {
        let pulled = *self.pulls.get(&station)?;
        clock.checked_sub(pulled)
    }

    /// Records a reading request at `clock` and drops the records that no
    /// longer hold anything back.
    fn pulled(&mut self, station: i64, clock: u64) {
        self.pulls.retain(|_, at| {
            clock
                .checked_sub(*at)
                .is_some_and(|age| age < READING_TTL_SECONDS)
        });
        self.pulls.insert(station, clock);
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
    /// The present time by the agent's clock (Unix seconds). A
    /// measurement's own age is counted from it.
    pub now: u64,
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
/// be younger than [`READING_TTL_SECONDS`], otherwise a new one unless
/// GIOŚ's limit holds the request back, otherwise the stored one marked as
/// expired. `at` is the run's local time ([`crate::time::local_minutes`]),
/// `clock` the agent's clock (Unix seconds).
pub fn reading(
    ctx: &mut Context<'_>,
    provider: &dyn Provider,
    chosen: &Chosen,
    at: Option<i64>,
    clock: u64,
) -> Result<Served, PluginError> {
    let station = chosen.station.id;
    let entry = read::<ReadingEntry>(ctx, READING_KEY)?
        .filter(|e| e.v == FORMAT && e.provider == provider.id() && e.station == station);
    let entry_age = |e: &ReadingEntry| {
        let by_clock = age(e.pulled_at.or(e.fetched_at), None, Some(clock), None);
        let by_at = age(None, e.fetched_local, None, at);
        match (by_clock, by_at) {
            // Either time source saying "young" keeps the request back.
            (Some(c), Some(l)) => Some(c.min(l)),
            (c, l) => c.or(l),
        }
    };
    if let Some(e) = &entry
        && let Some(age) = entry_age(e)
        && age < READING_TTL_SECONDS
    {
        return Ok(Served {
            reading: e.reading.clone(),
            source: Source::Cache,
            age: Some(age),
            fetched_at: e.fetched_at,
            now: clock,
            expired: false,
            problem: None,
        });
    }

    let mut state = read::<State>(ctx, STATE_KEY)?.unwrap_or_default();
    let waiting = state.wait_until.is_some_and(|until| clock < until)
        || matches!((at, state.wait_until_local), (Some(now), Some(until)) if now < until);
    let recent = state
        .since_pull(station, clock)
        .is_some_and(|since| since < READING_TTL_SECONDS);
    let failure = if waiting {
        Failure::new(
            codes::THROTTLED,
            format!(
                "{} asked to wait after its request limit was reached; it is not asked again yet",
                provider.name()
            ),
        )
        .retryable()
    } else if recent {
        Failure::new(
            codes::THROTTLED,
            format!(
                "{} was asked less than 30 minutes ago, and its terms allow two pulls an hour; it is not asked again yet",
                provider.name()
            ),
        )
        .retryable()
    } else {
        match provider.reading(ctx, station, chosen.sensors) {
            Ok(fetched) => {
                state.saw(fetched.date);
                state.pulled(station, clock);
                state.wait_until_local = None;
                state.wait_until = None;
                save(
                    ctx,
                    READING_KEY,
                    &ReadingEntry {
                        v: FORMAT,
                        provider: provider.id().to_owned(),
                        station,
                        fetched_at: fetched.date,
                        fetched_local: at,
                        pulled_at: Some(clock),
                        reading: fetched.value.clone(),
                    },
                );
                save(ctx, STATE_KEY, &state);
                return Ok(Served {
                    reading: fetched.value,
                    source: Source::Network,
                    age: Some(0),
                    fetched_at: fetched.date,
                    now: clock,
                    expired: false,
                    problem: None,
                });
            }
            Err(failure) => {
                state.saw(failure.date);
                if failure.reached {
                    state.pulled(station, clock);
                }
                if let Some(wait) = failure.retry_after {
                    state.wait_until = Some(clock.saturating_add(wait));
                    if let Some(now) = at {
                        let minutes = i64::try_from(wait.div_ceil(60)).unwrap_or(i64::MAX);
                        state.wait_until_local = Some(now.saturating_add(minutes));
                    }
                }
                if failure.reached || failure.date.is_some() {
                    save(ctx, STATE_KEY, &state);
                }
                failure
            }
        }
    };
    match entry {
        Some(e) if failure.fallback => {
            let age = entry_age(&e);
            Ok(Served {
                reading: e.reading,
                source: Source::Cache,
                age,
                fetched_at: e.fetched_at,
                now: clock,
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

/// The stored reading of `chosen`, without asking the provider: what a key
/// shows (`crate::key`). Absent, of another station or another format:
/// `None`.
pub fn stored_reading(
    ctx: &mut Context<'_>,
    provider: &dyn Provider,
    chosen: &Chosen,
) -> Result<Option<ReadingEntry>, PluginError> {
    let station = chosen.station.id;
    Ok(read::<ReadingEntry>(ctx, READING_KEY)?
        .filter(|e| e.v == FORMAT && e.provider == provider.id() && e.station == station))
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
