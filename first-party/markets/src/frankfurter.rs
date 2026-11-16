//! Frankfurter (<https://frankfurter.dev>), without a key: the European
//! Central Bank's euro foreign exchange reference rates. Terms as read on
//! 2026-11-16 are in the README.
//!
//! One origin, `api.frankfurter.dev`, `GET` only, and one route: the ECB
//! provider's rates (`/v2/providers/ecb/rates`), so that nothing but ECB
//! data is ever served. Frankfurter's default `/v2/rates` blends several
//! providers, and its `/v1` API is deprecated.
//!
//! The ECB publishes one rate per currency against the euro, usually at
//! about 16:00 CET on every TARGET working day. Frankfurter computes a pair
//! without the euro (USD/PLN) from two of them.

use deckpilot_plugin_sdk::prelude::*;
use serde::Deserialize;
use serde_json::Value;

use crate::provider::{Failure, Fetched, Pair, Point, Provider};
use crate::time::{date_text, day_number, http_date};
use crate::{INVALID_RESPONSE, PROVIDER_ERROR, RESPONSE_TOO_LARGE, UNKNOWN_CURRENCY};

pub const HOST: &str = "api.frankfurter.dev";
pub const RATES_API: &str = "https://api.frankfurter.dev/v2/providers/ecb/rates";
/// The agent sends no User-Agent of its own for plugins.
pub const USER_AGENT: &str = concat!("DeckPilot-Markets/", env!("CARGO_PKG_VERSION"));
/// A series of a few weeks has a few kilobytes; anything much larger is not
/// the answer that was asked for and is not parsed. (The agent itself
/// refuses a body over 512 KiB.)
pub const MAX_BODY_BYTES: usize = 128 * 1024;
/// Rows read from one answer; older ones are dropped before parsing ends.
pub const MAX_ROWS: usize = 400;
/// Waiting time assumed for an HTTP 429 without a usable `Retry-After`,
/// and the longest one honoured.
pub const DEFAULT_RETRY_SECONDS: u64 = 15 * 60;
pub const MAX_RETRY_SECONDS: u64 = 24 * 3_600;

pub const ATTRIBUTION: &str = "Source: European Central Bank euro foreign exchange reference rates, via Frankfurter (frankfurter.dev). Published for information purposes only.";

#[derive(Debug, Clone, Copy, Default)]
pub struct Frankfurter;

impl Provider for Frankfurter {
    fn name(&self) -> &'static str {
        "Frankfurter"
    }

    fn source_id(&self) -> &'static str {
        "ecb-frankfurter"
    }

    fn attribution(&self) -> &'static str {
        ATTRIBUTION
    }

    fn delayed(&self) -> bool {
        true
    }

    fn series(
        &self,
        ctx: &mut Context<'_>,
        pair: &Pair,
        from: Option<i64>,
    ) -> Result<Fetched<Vec<Point>>, Failure> {
        let (body, date, max_age) = get(ctx, &rates_url(pair, from), pair)?;
        let points = parse_series(&body, pair).map_err(|what| malformed(what).at(date))?;
        if points.is_empty() {
            return Err(unknown(pair).at(date));
        }
        Ok(Fetched {
            value: points,
            date,
            max_age,
        })
    }
}

/// `…/v2/providers/ecb/rates?base=EUR&quotes=USD[&from=YYYY-MM-DD]`. The
/// codes are three ASCII letters ([`crate::provider::currency`]), so they
/// need no encoding.
pub fn rates_url(pair: &Pair, from: Option<i64>) -> String {
    let mut url = format!("{RATES_API}?base={}&quotes={}", pair.base, pair.quote);
    if let Some(from) = from {
        url.push_str("&from=");
        url.push_str(&date_text(from));
    }
    url
}

fn malformed(what: &str) -> Failure {
    Failure::new(
        INVALID_RESPONSE,
        format!("Frankfurter's answer could not be read: {what}"),
    )
}

fn unknown(pair: &Pair) -> Failure {
    Failure::new(
        UNKNOWN_CURRENCY,
        format!(
            "The ECB publishes no reference rate for {} or {}: use ISO 4217 codes the ECB lists, such as EUR, USD, GBP, PLN, CHF or JPY",
            pair.base, pair.quote
        ),
    )
    .final_answer()
}

fn too_large() -> Failure {
    Failure::new(
        RESPONSE_TOO_LARGE,
        format!(
            "Frankfurter's answer is larger than expected (over {} KiB) and was not read",
            MAX_BODY_BYTES / 1024
        ),
    )
}

/// `max-age` of a `Cache-Control` value.
pub fn max_age(header: Option<&str>) -> Option<u64> {
    header?.split(',').find_map(|part| {
        let (name, value) = part.trim().split_once('=')?;
        name.trim()
            .eq_ignore_ascii_case("max-age")
            .then(|| value.trim().trim_matches('"').parse().ok())
            .flatten()
    })
}

/// One `GET`: the parsed JSON body of a 2xx answer, its `Date` and its
/// `max-age`.
fn get(
    ctx: &mut Context<'_>,
    url: &str,
    pair: &Pair,
) -> Result<(Value, Option<u64>, Option<u64>), Failure> {
    let request = Request::get(url)
        .header("Accept", "application/json")
        .header("User-Agent", USER_AGENT);
    let response = ctx.fetch(request).map_err(|e| fetch_failure(&e))?;
    let date = response.header("date").and_then(http_date);
    match response.status {
        200..=299 => {}
        429 => {
            let mut failure = Failure::new(
                codes::THROTTLED,
                "Frankfurter's rate limit is reached (HTTP 429); try again later",
            )
            .retryable()
            .at(date);
            failure.retry_after = Some(retry_after(response.header("retry-after"), date));
            return Err(failure);
        }
        // `invalid currency: ABC` (422) or `not found` (404).
        404 | 422 => return Err(unknown(pair).at(date)),
        400 => {
            return Err(
                Failure::new(PROVIDER_ERROR, "Frankfurter refused the request (HTTP 400)")
                    .at(date)
                    .final_answer(),
            );
        }
        status => {
            let failure = Failure::new(
                PROVIDER_ERROR,
                format!("Frankfurter answered with an error (HTTP {status})"),
            )
            .at(date);
            return Err(if status >= 500 {
                failure.retryable()
            } else {
                failure
            });
        }
    }
    let body = response.text();
    if body.len() > MAX_BODY_BYTES {
        return Err(too_large().at(date));
    }
    let value: Value =
        serde_json::from_str(body).map_err(|_| malformed("it is not JSON").at(date))?;
    Ok((value, date, max_age(response.header("cache-control"))))
}

/// A request without an answer: denied, refused by the agent, or failed on
/// the network.
fn fetch_failure(e: &CapabilityError) -> Failure {
    if e.kind() == CapabilityErrorKind::Failed && e.detail().contains("larger than") {
        return too_large();
    }
    if e.is_denied() {
        return Failure::new(
            codes::NOT_PERMITTED,
            "DeckPilot did not let the plugin reach Frankfurter: allow its network permission (api.frankfurter.dev) on the plugin's page",
        )
        .final_answer();
    }
    Failure {
        error: PluginError::from(e.clone()),
        date: None,
        retry_after: None,
        fallback: true,
    }
}

/// Seconds to wait after an HTTP 429: `Retry-After` as seconds or as an
/// HTTP date counted from the answer's `Date`.
pub fn retry_after(header: Option<&str>, now: Option<u64>) -> u64 {
    let header = header.map(str::trim);
    let seconds = header
        .and_then(|h| h.parse::<u64>().ok())
        .or_else(|| Some(header.and_then(http_date)?.saturating_sub(now?)))
        .unwrap_or(DEFAULT_RETRY_SECONDS);
    seconds.clamp(60, MAX_RETRY_SECONDS)
}

#[derive(Deserialize)]
struct Row {
    date: String,
    base: String,
    quote: String,
    rate: f64,
}

/// The answer as points, oldest first, at most [`MAX_ROWS`] (the newest).
/// An empty array is an empty list: the ECB has no rate for the pair. The
/// error says what is wrong without quoting the answer.
pub fn parse_series(body: &Value, pair: &Pair) -> Result<Vec<Point>, &'static str> {
    let rows = body.as_array().ok_or("it is not a list of rates")?;
    let start = rows.len().saturating_sub(MAX_ROWS);
    let mut points: Vec<Point> = Vec::with_capacity(rows.len() - start);
    for row in &rows[start..] {
        let row = Row::deserialize(row).map_err(|_| "a rate is missing or has another type")?;
        if !row.base.eq_ignore_ascii_case(&pair.base)
            || !row.quote.eq_ignore_ascii_case(&pair.quote)
        {
            return Err("a rate is for another pair");
        }
        if day_number(&row.date).is_none() {
            return Err("a date is not YYYY-MM-DD");
        }
        if !(row.rate.is_finite() && row.rate > 0.0 && row.rate < 1e9) {
            return Err("a rate is not a positive number");
        }
        points.push(Point {
            d: row.date,
            r: row.rate,
        });
    }
    points.sort_by(|a, b| a.d.cmp(&b.d));
    points.dedup_by(|later, earlier| later.d == earlier.d);
    Ok(points)
}
