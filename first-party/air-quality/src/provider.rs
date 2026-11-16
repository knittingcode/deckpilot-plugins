//! What the plugin needs from an air quality provider, and the data it
//! keeps.
//!
//! Version 0.1.0 has one provider, [`crate::gios::Gios`]. The actions and
//! the cache know only this module, so another provider is one more
//! implementation of [`Provider`] plus its hosts (and, for a provider with
//! a key, its credential slot) in the manifest. Airly was read for this
//! version and left out: the README says why.
//!
//! Concentrations are in µg/m³, as the provider gives them. Times are the
//! provider's own texts; [`Provider::unix`] knows their zone.

use deckpilot_plugin_sdk::prelude::*;
use serde::{Deserialize, Serialize};

use crate::texts::Language;

/// A measuring station. The provider's list also has coordinates and a
/// street; the plugin drops both when it reads the list, so neither is ever
/// stored, logged or shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Station {
    pub id: i64,
    pub name: String,
    pub city: String,
    /// The voivodeship, in lower case.
    pub region: String,
}

impl Station {
    /// `Warszawa, Al. Niepodległości (mazowieckie)`.
    pub fn label(&self) -> String {
        let mut label = self.name.clone();
        if !self.city.is_empty() && !self.name.contains(&self.city) {
            label.push_str(", ");
            label.push_str(&self.city);
        }
        if !self.region.is_empty() {
            label.push_str(" (");
            label.push_str(&self.region);
            label.push(')');
        }
        label
    }
}

/// The provider's ids of a station's particulate matter sensors.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sensors {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pm25: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pm10: Option<i64>,
}

impl Sensors {
    pub fn is_empty(&self) -> bool {
        self.pm25.is_none() && self.pm10.is_none()
    }
}

/// One measured concentration with the time the provider gives for it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    /// µg/m³.
    pub value: f64,
    /// As the provider writes it (`2026-11-16 19:00:00`).
    pub time: String,
}

/// One answer of a provider about a station. Every part may be missing: a
/// station without a sensor, a sensor without a recent value, an index the
/// provider could not compute.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Reading {
    /// The overall air quality index on the provider's scale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<i64>,
    /// The time of the data the index was computed from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pm25: Option<Sample>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pm10: Option<Sample>,
    /// The index of single pollutants, when the index answer has them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no2: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub so2: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub o3: Option<i64>,
}

impl Reading {
    /// Nothing the plugin reports is there.
    pub fn is_empty(&self) -> bool {
        self.index.is_none() && self.pm25.is_none() && self.pm10.is_none()
    }

    /// The times of the values the plugin reports: of the PM values, and of
    /// the index when there is one.
    pub fn times(&self) -> Vec<&str> {
        let mut times: Vec<&str> = [&self.pm25, &self.pm10]
            .into_iter()
            .flatten()
            .map(|sample| sample.time.as_str())
            .collect();
        if self.index.is_some()
            && let Some(time) = &self.index_time
        {
            times.push(time);
        }
        times
    }
}

/// One category of a provider's air quality index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Level {
    /// A fixed name for workflows (`good`, `very-bad`).
    pub name: &'static str,
    pub en: &'static str,
    pub pl: &'static str,
}

impl Level {
    pub fn text(&self, language: Language) -> &'static str {
        language.pick(self.en, self.pl)
    }
}

/// The category reported when there is no index.
pub const NO_LEVEL: Level = Level {
    name: "none",
    en: "No index",
    pl: "Brak indeksu",
};

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
    /// the network.
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
    /// The provider's id in storage and in the `provider` output.
    fn id(&self) -> &'static str;
    /// The provider's name in messages.
    fn name(&self) -> &'static str;
    /// The source line its terms prescribe, for a text that has no room
    /// for more.
    fn source(&self) -> &'static str;
    /// The full attribution: the source line, the licence, and how the
    /// data was changed.
    fn attribution(&self, language: Language) -> &'static str;
    /// The name of its index scale, for a sentence.
    fn scale(&self, language: Language) -> &'static str;
    /// The category of an index value; `None` for a value off the scale.
    fn level(&self, index: i64) -> Option<Level>;
    /// A time of this provider's answers as Unix seconds.
    fn unix(&self, time: &str) -> Option<u64>;
    /// Every station, in the provider's order.
    fn stations(&self, ctx: &mut Context<'_>) -> Result<Fetched<Vec<Station>>, Failure>;
    /// The particulate matter sensors of a station.
    fn sensors(&self, ctx: &mut Context<'_>, station: i64) -> Result<Fetched<Sensors>, Failure>;
    /// The index and the newest PM values of a station.
    fn reading(
        &self,
        ctx: &mut Context<'_>,
        station: i64,
        sensors: Sensors,
    ) -> Result<Fetched<Reading>, Failure>;
}
