//! What the plugin needs from a source of rates, and the data it keeps.
//!
//! Version 0.1.0 has one provider, [`crate::frankfurter::Frankfurter`]
//! (the ECB's euro reference rates). The actions and the cache know only
//! this module, so another source is one more implementation of
//! [`Provider`], its hosts in the manifest and, for one that needs a key, a
//! credential slot: the plugin names the slot and the agent injects the
//! secret, which the plugin never sees (README, "Cryptocurrencies").

use deckpilot_plugin_sdk::prelude::*;
use serde::{Deserialize, Serialize};

/// A currency pair, both ISO 4217 codes in upper case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pair {
    pub base: String,
    pub quote: String,
}

impl Pair {
    /// `EUR-USD`: the storage key part.
    pub fn id(&self) -> String {
        format!("{}-{}", self.base, self.quote)
    }

    /// `EUR/USD`: as shown.
    pub fn label(&self) -> String {
        format!("{}/{}", self.base, self.quote)
    }

    /// Whether the provider computes it from two euro rates (neither side
    /// is the euro).
    pub fn is_cross(&self) -> bool {
        self.base != "EUR" && self.quote != "EUR"
    }
}

/// A currency code as typed: three ASCII letters, any case, spaces around.
pub fn currency(text: &str) -> Option<String> {
    let code = text.trim();
    (code.len() == 3 && code.bytes().all(|b| b.is_ascii_alphabetic()))
        .then(|| code.to_ascii_uppercase())
}

/// One reference rate: the day it is for and how many units of the quote
/// currency one unit of the base currency was worth.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Point {
    /// `YYYY-MM-DD`.
    pub d: String,
    pub r: f64,
}

/// An answer with what the provider's server said about time: its `Date`
/// header (Unix seconds) and its `Cache-Control: max-age` (seconds).
#[derive(Debug, Clone, PartialEq)]
pub struct Fetched<T> {
    pub value: T,
    pub date: Option<u64>,
    pub max_age: Option<u64>,
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
    /// the network, and not when the provider says the pair does not exist.
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
    /// `source` output: a fixed id of the data's origin.
    fn source_id(&self) -> &'static str;
    /// The attribution next to the data.
    fn attribution(&self) -> &'static str;
    /// Whether every value is a delayed one (a daily reference rate, not a
    /// price of now). True for the ECB's reference rates.
    fn delayed(&self) -> bool;
    /// The rates of a pair from the day `from` (days since 1970-01-01) to
    /// the newest one, oldest first, or the newest one alone without
    /// `from`. Never empty.
    fn series(
        &self,
        ctx: &mut Context<'_>,
        pair: &Pair,
        from: Option<i64>,
    ) -> Result<Fetched<Vec<Point>>, Failure>;
}
