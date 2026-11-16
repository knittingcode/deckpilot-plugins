//! What the plugin needs from a weather provider, and the data it keeps.
//!
//! Version 0.1.0 has one provider, [`crate::open_meteo::OpenMeteo`]. The
//! actions and the cache know only this module, so another provider is one
//! more implementation of [`Provider`] plus its hosts in the manifest.
//!
//! All values are metric (°C, km/h): [`crate::units`] converts on the way
//! out, so what is cached does not depend on a setting.

use deckpilot_plugin_sdk::prelude::*;
use serde::{Deserialize, Serialize};

use crate::conditions::Language;

/// A place on the map. Coordinates are personal data: they are kept in the
/// plugin's storage and sent to the provider, and never written to the log,
/// to an output or into a message.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    /// A tz database name; without one the provider picks it from the
    /// coordinates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    /// The first-level administrative area, to tell places of one name apart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
}

/// Without the coordinates, so that a `{:?}` in a log line or a test
/// failure cannot leak them.
impl std::fmt::Debug for Place {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Place")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl Place {
    /// `Springfield, Missouri, United States`.
    pub fn label(&self) -> String {
        let mut label = self.name.clone();
        for part in [&self.region, &self.country].into_iter().flatten() {
            if !part.is_empty() && *part != self.name {
                label.push_str(", ");
                label.push_str(part);
            }
        }
        label
    }

    /// The coordinates as the provider is asked for them and as the cache
    /// is keyed: two decimals, about a kilometre. A weather model's grid is
    /// coarser, and the provider learns no more than it needs.
    pub fn grid(&self) -> String {
        format!("{:.2},{:.2}", self.latitude, self.longitude)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Current {
    /// The time the values are for, local to the place, as the provider
    /// writes it (`2026-11-16T19:15`).
    pub time: String,
    pub temperature: f64,
    pub feels_like: f64,
    /// Relative humidity in percent.
    pub humidity: i64,
    pub wind_speed: f64,
    /// Degrees the wind blows from, 0-359.
    pub wind_direction: i64,
    /// WMO weather code ([`crate::conditions`]).
    pub code: i64,
    pub is_day: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Day {
    /// `YYYY-MM-DD`, local to the place.
    pub date: String,
    pub min: f64,
    pub max: f64,
    pub code: i64,
    /// Largest precipitation probability of the day in percent, when the
    /// provider's model has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precipitation_probability: Option<i64>,
}

/// One answer of a provider: the conditions now and the days from today on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub current: Current,
    /// At least one and at most [`MAX_DAYS`] days.
    pub days: Vec<Day>,
}

/// Days asked for and kept.
pub const MAX_DAYS: usize = 7;
/// Places asked for in one search.
pub const MAX_MATCHES: usize = 5;

/// An answer with the time the provider's server gave it (its `Date`
/// header, Unix seconds), which is the plugin's only clock for that moment.
#[derive(Debug, Clone, PartialEq)]
pub struct Fetched<T> {
    pub value: T,
    pub date: Option<u64>,
}

/// A request that brought no usable answer.
#[derive(Debug, Clone)]
pub struct Failure {
    /// What the action reports when nothing can be served instead.
    pub error: PluginError,
    /// The `Date` of the answer, when there was one.
    pub date: Option<u64>,
    /// Seconds the provider asked to wait (HTTP 429).
    pub retry_after: Option<u64>,
    /// Whether saved data may be served instead. Not when the user denied
    /// the network, and not when the provider refused the request itself.
    pub fallback: bool,
}

impl Failure {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            error: PluginError::new(code, message),
            date: None,
            retry_after: None,
            fallback: true,
        }
    }

    pub fn retryable(mut self) -> Self {
        self.error = self.error.retryable();
        self
    }

    pub fn at(mut self, date: Option<u64>) -> Self {
        self.date = date;
        self
    }

    pub fn final_answer(mut self) -> Self {
        self.fallback = false;
        self
    }
}

pub trait Provider {
    /// The provider's name in messages.
    fn name(&self) -> &'static str;
    /// The attribution its licence asks for next to weather data.
    fn attribution(&self) -> &'static str;
    /// The attribution for the results of a place search.
    fn places_attribution(&self) -> &'static str;
    /// Current conditions and the daily forecast for a place: one request.
    fn weather(&self, ctx: &mut Context<'_>, place: &Place) -> Result<Fetched<Report>, Failure>;
    /// Places with a name, best match first, at most [`MAX_MATCHES`].
    fn search(
        &self,
        ctx: &mut Context<'_>,
        query: &str,
        language: Language,
    ) -> Result<Fetched<Vec<Place>>, Failure>;
}
