//! DeckPilot's markets plugin: the European Central Bank's euro foreign
//! exchange reference rates, via Frankfurter, without a key.
//!
//! Four actions. `rate` reports a pair's newest reference rate, an optional
//! conversion, the change since the previous working day and the lowest,
//! highest and trend of the stored rates. `change` reports the change over
//! up to 20 working days. `check-alert` compares the rate with the user's
//! threshold and notifies once each time it crosses it. `forget` deletes
//! what the plugin stored.
//!
//! **Daily reference rates, never live.** The ECB publishes one rate per
//! currency once per TARGET working day (about 16:00 CET), "for
//! information purposes only". Every output says when the rate is for
//! (`rateDate`), where it is from (`source`, `attribution`), and carries
//! `delayed` (always true here) and `stale` (not known to be current).
//!
//! A key bound to `rate` shows the pair's newest rate, its change, its date
//! and a sparkline of the stored rates (plugin API 1.2, [`key`]), with a
//! warning and a badge whenever they are not known to be current; never
//! as live. What the plugin still does not do: it cannot refresh by itself
//! (no timers; a key shows what the last run fetched), and its actions
//! read the time only from `at` and the answers' `Date` ([`cache`]).
//!
//! Not in this version: cryptocurrency prices (CoinGecko's API terms ask
//! for more than a plugin can promise; README) and stock quotes (no
//! provider whose terms allow this use).
//!
//! Privacy: amounts and thresholds are the user's. They go into the
//! action's message and outputs and, for an alert, into the plugin's
//! storage and the notification, and never into the log or a request.
#![deny(unsafe_code)]

use deckpilot_plugin_sdk::ApiLevel;
use deckpilot_plugin_sdk::prelude::*;
use serde::Serialize;

pub mod cache;
pub mod frankfurter;
pub mod history;
pub mod key;
pub mod numbers;
pub mod provider;
pub mod time;

use cache::{Served, Source};
use frankfurter::Frankfurter;
use history::{Change, Snapshot};
use provider::{Pair, Provider, currency};

/// Must equal `manifest.json` (`id`, `version`).
pub const ID: &str = "deckpilot.markets";
pub const VERSION: &str = "0.2.0";

/// Error codes of this plugin, each declared in `contributes.errorCodes`.
pub const UNKNOWN_CURRENCY: &str = "unknown-currency";
pub const INVALID_AMOUNT: &str = "invalid-amount";
pub const INVALID_THRESHOLD: &str = "invalid-threshold";
pub const NOT_ENOUGH_HISTORY: &str = "not-enough-history";
pub const PROVIDER_ERROR: &str = "provider-error";
pub const INVALID_RESPONSE: &str = "invalid-response";
pub const RESPONSE_TOO_LARGE: &str = "response-too-large";
/// Reported by the SDK for a request that timed out; not a shared code.
pub const TIMED_OUT: &str = "timed-out";
pub const PLUGIN_CODES: &[&str] = &[
    UNKNOWN_CURRENCY,
    INVALID_AMOUNT,
    INVALID_THRESHOLD,
    NOT_ENOUGH_HISTORY,
    PROVIDER_ERROR,
    INVALID_RESPONSE,
    RESPONSE_TOO_LARGE,
    TIMED_OUT,
];

/// The plugin's own variables (`contributes.variables`), read as
/// `{{ plugins.deckpilot.markets.<id> }}`.
pub const RATE_VARIABLES: &[&str] = &[
    "pair",
    "rate",
    "rateDate",
    "changePercent",
    "summary",
    "stale",
    "history",
];
pub const ALERT_VARIABLES: &[&str] = &["alertTriggered"];

/// The largest number of working days `change` looks back.
pub const MAX_CHANGE_DAYS: i64 = 20;

/// The definition. The action ids, their order and their parameters must
/// equal `manifest.json`; a unit test compares them.
pub fn plugin() -> Plugin {
    let base = || Param::text("base").max_length(3).default("EUR");
    let quote = || Param::text("quote").required().max_length(3);
    let at = || Param::text("at").max_length(16);
    Plugin::new(ID, VERSION)
        .requires_api(ApiLevel { major: 1, minor: 2 })
        .action(
            Action::new("rate", rate)
                .param(base())
                .param(quote())
                .param(Param::text("amount").max_length(32))
                .param(at())
                .state(key::rate_state),
        )
        .action(
            Action::new("change", change)
                .param(base())
                .param(quote())
                .param(
                    Param::integer("days")
                        .min(1.0)
                        .max(MAX_CHANGE_DAYS as f64)
                        .default(1),
                )
                .param(at()),
        )
        .action(
            Action::new("check-alert", check_alert)
                .param(base())
                .param(quote())
                .param(Param::choice("condition", &["above", "below"]).default("above"))
                .param(Param::text("threshold").required().max_length(32))
                .param(at()),
        )
        .action(Action::new("forget", forget))
}

deckpilot_plugin_sdk::plugin!(plugin);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    En,
    Pl,
}

impl Language {
    fn from_setting(value: Option<&str>) -> Self {
        match value {
            Some("pl") => Self::Pl,
            _ => Self::En,
        }
    }
}

fn language(ctx: &mut Context<'_>) -> Result<Language, PluginError> {
    Ok(Language::from_setting(ctx.settings()?.text("language")))
}

fn pair(args: &Args<'_>) -> Result<Pair, PluginError> {
    let base: String = args.get("base")?;
    let quote: String = args.get("quote")?;
    let invalid = |what: &str| {
        PluginError::new(
            codes::INVALID_PARAMETERS,
            format!(
                "{what}: expected a three-letter ISO 4217 currency code such as EUR, USD or PLN"
            ),
        )
    };
    let base = currency(&base).ok_or_else(|| invalid("base"))?;
    let quote = currency(&quote).ok_or_else(|| invalid("quote"))?;
    if base == quote {
        return Err(PluginError::new(
            codes::INVALID_PARAMETERS,
            "base and quote are the same currency",
        ));
    }
    Ok(Pair { base, quote })
}

/// The run's local time from the optional `at` parameter.
fn run_time(args: &Args<'_>) -> Result<Option<i64>, PluginError> {
    let at: Option<String> = args.get("at")?;
    match at.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        None => Ok(None),
        Some(text) => time::local_minutes(text).map(Some).ok_or_else(|| {
            PluginError::new(
                codes::INVALID_PARAMETERS,
                "at: expected a schedule trigger's firing time, YYYY-MM-DD HH:MM",
            )
        }),
    }
}

/// Publishing a variable is best effort: the action's result does not
/// depend on it.
fn publish(ctx: &mut Context<'_>, id: &str, value: impl Serialize) {
    if ctx.publish_variable(id, value).is_err() {
        ctx.warn(format!("the variable {id} could not be published"));
    }
}

/// The rates of a pair, or the error; `stale` is published on an error so
/// that old variables are not taken for current ones.
fn served(
    ctx: &mut Context<'_>,
    provider: &dyn Provider,
    pair: &Pair,
    at: Option<i64>,
) -> Result<Served, PluginError> {
    cache::rates(ctx, provider, pair, at).inspect_err(|_| publish(ctx, "stale", true))
}

/// `Mon 2026-11-16`.
fn dated(date: &str, language: Language) -> String {
    let Some(day) = time::day_number(date) else {
        return date.to_owned();
    };
    let names = match language {
        Language::En => ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"],
        Language::Pl => ["pon.", "wt.", "śr.", "czw.", "pt.", "sob.", "niedz."],
    };
    let index = usize::try_from(time::weekday(day)).unwrap_or(0);
    format!("{} {date}", names[index])
}

/// `[stale] ` in front of a text that is not known to be current.
fn stale_mark(stale: bool, language: Language) -> &'static str {
    match (stale, language) {
        (false, _) => "",
        (true, Language::En) => "[stale] ",
        (true, Language::Pl) => "[nieaktualne] ",
    }
}

/// `asOf`: what the time of the value is.
fn as_of(date: &str, language: Language) -> String {
    match language {
        Language::En => format!("ECB reference rate of {date}, about 16:00 CET"),
        Language::Pl => format!("kurs referencyjny EBC z {date}, ok. 16:00 CET"),
    }
}

/// What the user must know about the freshness and the origin of a rate,
/// around the action's own sentence.
fn message(
    sentence: &str,
    served: &Served,
    pair: &Pair,
    provider: &dyn Provider,
    language: Language,
) -> String {
    let mut text = String::new();
    let minutes = served.age.map(|seconds| seconds / 60);
    if served.stale {
        let problem = served.problem.as_deref().unwrap_or("unknown");
        text.push_str(&match (problem == cache::OLD_RATE, language) {
            (true, Language::En) => format!(
                "Not current: the newest ECB rate for {} is {} days old. ",
                pair.label(),
                served.lag_days.unwrap_or_default()
            ),
            (true, Language::Pl) => format!(
                "Nieaktualne: najnowszy kurs EBC dla {} ma {} dni. ",
                pair.label(),
                served.lag_days.unwrap_or_default()
            ),
            (false, Language::En) => {
                let when = match (served.fetched_at.map(time::utc_text), minutes) {
                    (Some(at), Some(m)) => format!("{at}, about {m} min ago"),
                    (Some(at), None) => {
                        format!("{at}; the plugin has no clock and cannot tell how old that is")
                    }
                    (None, Some(m)) => format!("about {m} min ago"),
                    (None, None) => "an unknown time".into(),
                };
                format!(
                    "Not current: {} gave no new data ({problem}). Saved rates fetched {when}. ",
                    provider.name()
                )
            }
            (false, Language::Pl) => {
                let when = match (served.fetched_at.map(time::utc_text), minutes) {
                    (Some(at), Some(m)) => format!("{at}, około {m} min temu"),
                    (Some(at), None) => {
                        format!("{at}; wtyczka nie ma zegara i nie wie, jak dawno to było")
                    }
                    (None, Some(m)) => format!("około {m} min temu"),
                    (None, None) => "w nieznanym czasie".into(),
                };
                format!(
                    "Nieaktualne: {} nie podał nowych danych ({problem}). Zapisane kursy pobrano {when}. ",
                    provider.name()
                )
            }
        });
    }
    text.push_str(sentence);
    if !served.stale && served.source == Source::Cache {
        let problem = served.problem.as_deref();
        text.push_str(&match (minutes, problem, language) {
            (Some(m), None, Language::En) => format!(" (saved {m} min ago)"),
            (Some(m), None, Language::Pl) => format!(" (zapisane {m} min temu)"),
            (_, Some(p), Language::En) => {
                format!(" (saved; {} gave no new data: {p})", provider.name())
            }
            (_, Some(p), Language::Pl) => {
                format!(
                    " (zapisane; {} nie podał nowych danych: {p})",
                    provider.name()
                )
            }
            (None, None, _) => String::new(),
        });
    }
    // A rate for a day before the answer's: a weekend, a holiday, or
    // before today's publication.
    if served.lag_days.is_some_and(|d| d >= 1) && !served.stale {
        text.push_str(match language {
            Language::En => {
                " The ECB publishes once per TARGET working day at about 16:00 CET, none on weekends and TARGET holidays."
            }
            Language::Pl => {
                " EBC publikuje kursy raz na dzień roboczy TARGET ok. 16:00 CET, nie w weekendy i święta TARGET."
            }
        });
    }
    if pair.is_cross() {
        text.push_str(match language {
            Language::En => " Cross rate calculated by Frankfurter from the ECB's euro rates.",
            Language::Pl => " Kurs krzyżowy wyliczony przez Frankfurter z kursów euro EBC.",
        });
    }
    text.push(' ');
    text.push_str(provider.attribution());
    text
}

/// The outputs every rate action has about its pair and the data's origin.
fn with_origin(mut out: Output, served: &Served, pair: &Pair, provider: &dyn Provider) -> Output {
    out = out
        .with("pair", pair.label())
        .with("rate", numbers::rate(served.latest().r))
        .with("rateDate", served.latest().d.as_str())
        .with("cross", pair.is_cross())
        .with("delayed", provider.delayed())
        .with("stale", served.stale)
        .with("source", provider.source_id())
        .with("attribution", provider.attribution());
    if let Some(at) = served.fetched_at {
        out = out.with("fetchedAt", time::utc_text(at));
    }
    if let Some(age) = served.age {
        out = out.with("ageMinutes", i64::try_from(age / 60).unwrap_or(i64::MAX));
    }
    if let Some(problem) = &served.problem {
        out = out.with("problem", problem.as_str());
    }
    out
}

fn change_words(c: &Change, language: Language) -> String {
    let (diff, pct) = (
        numbers::signed_rate(c.difference),
        numbers::percent(c.percent),
    );
    match language {
        Language::En => format!("change since {}: {diff} ({pct}%)", c.from.d),
        Language::Pl => format!("zmiana od {}: {diff} ({pct}%)", c.from.d),
    }
}

/// What a key bound to `rate` would show for these rates.
pub fn snapshot(served: &Served, pair: &Pair) -> Snapshot {
    let latest = served.latest();
    Snapshot {
        pair: pair.label(),
        rate: numbers::rate(latest.r),
        date: latest.d.clone(),
        stale: served.stale,
        change_percent: history::change(&served.points, 1).map(|c| numbers::percent(c.percent)),
        series: history::series(&served.points),
    }
}

fn rate(ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let pair = pair(args)?;
    let amount: Option<String> = args.get("amount")?;
    let amount = match amount.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        None => None,
        Some(text) => Some(numbers::parse_positive(text).ok_or_else(|| {
            PluginError::new(
                INVALID_AMOUNT,
                "amount: expected a positive number such as 100 or 99.50",
            )
        })?),
    };
    let at = run_time(args)?;
    let language = language(ctx)?;
    let provider = Frankfurter;
    let served = served(ctx, &provider, &pair, at)?;
    let latest = served.latest().clone();
    let rate_text = numbers::rate(latest.r);
    let previous = history::change(&served.points, 1);
    let stats = history::stats(&served.points).expect("served rates are never empty");

    let mut sentence = match language {
        Language::En => format!(
            "{} {rate_text}: ECB reference rate of {} (a daily rate, not live).",
            pair.label(),
            dated(&latest.d, language)
        ),
        Language::Pl => format!(
            "{} {rate_text}: kurs referencyjny EBC z {} (kurs dzienny, nie bieżący).",
            pair.label(),
            dated(&latest.d, language)
        ),
    };
    let converted = amount.map(|a| (numbers::amount(a), numbers::amount(a * latest.r)));
    if let Some((a, c)) = &converted {
        sentence.push_str(&match language {
            Language::En => format!(
                " {a} {} = {c} {} (converted by the plugin).",
                pair.base, pair.quote
            ),
            Language::Pl => format!(
                " {a} {} = {c} {} (przeliczone przez wtyczkę).",
                pair.base, pair.quote
            ),
        });
    }
    if let Some(c) = &previous {
        let words = change_words(c, language);
        sentence.push(' ');
        sentence.push_str(&capitalized(&words));
        sentence.push('.');
    }
    if stats.count > 1 {
        let (min, max) = (numbers::rate(stats.min), numbers::rate(stats.max));
        sentence.push_str(&match language {
            Language::En => format!(
                " {} rates since {}: {min} to {max}, trend {}.",
                stats.count,
                stats.first_date,
                stats.trend.id()
            ),
            Language::Pl => format!(
                " {} kursów od {}: od {min} do {max}, trend {}.",
                stats.count,
                stats.first_date,
                stats.trend.id()
            ),
        });
    }
    let summary = format!("{}{sentence}", stale_mark(served.stale, language));
    let history_json = serde_json::Value::Array(
        served
            .points
            .iter()
            .map(|p| serde_json::json!({ "date": p.d, "rate": numbers::rate(p.r) }))
            .collect(),
    );

    publish(ctx, "pair", pair.label());
    publish(ctx, "rate", &rate_text);
    publish(ctx, "rateDate", &latest.d);
    publish(
        ctx,
        "changePercent",
        previous
            .as_ref()
            .map(|c| numbers::percent(c.percent))
            .unwrap_or_default(),
    );
    publish(ctx, "summary", &summary);
    publish(ctx, "stale", served.stale);
    publish(ctx, "history", &history_json);

    let mut out = Output::message(message(&sentence, &served, &pair, &provider, language))
        .with("summary", summary)
        .with("base", pair.base.as_str())
        .with("quote", pair.quote.as_str())
        .with("asOf", as_of(&latest.d, language))
        .with("min", numbers::rate(stats.min))
        .with("max", numbers::rate(stats.max))
        .with("trend", stats.trend.id())
        .with("historyCount", i64::try_from(stats.count).unwrap_or(0))
        .with("historyFrom", stats.first_date.as_str());
    if let Some((a, c)) = converted {
        out = out.with("amount", a).with("converted", c);
    }
    if let Some(c) = &previous {
        out = out
            .with("previousRate", numbers::rate(c.from.r))
            .with("previousDate", c.from.d.as_str())
            .with("change", numbers::signed_rate(c.difference))
            .with("changePercent", numbers::percent(c.percent))
            .with("direction", c.direction.id());
    }
    Ok(with_origin(out, &served, &pair, &provider))
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn change(ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let pair = pair(args)?;
    let days: i64 = args.get("days")?;
    let days = usize::try_from(days.clamp(1, MAX_CHANGE_DAYS)).unwrap_or(1);
    let at = run_time(args)?;
    let language = language(ctx)?;
    let provider = Frankfurter;
    let served = served(ctx, &provider, &pair, at)?;
    let Some(c) = history::change(&served.points, days) else {
        let known = served.points.len();
        return Err(PluginError::new(
            NOT_ENOUGH_HISTORY,
            format!(
                "Only {known} ECB rate(s) of {} are known; ask for at most {} working day(s)",
                pair.label(),
                known.saturating_sub(1)
            ),
        ));
    };
    let words = change_words(&c, language);
    let sentence = match language {
        Language::En => format!(
            "{} {}: {words}, {} over {days} ECB working day(s), from {} to {}.",
            pair.label(),
            numbers::rate(c.to.r),
            c.direction.id(),
            numbers::rate(c.from.r),
            numbers::rate(c.to.r)
        ),
        Language::Pl => format!(
            "{} {}: {words}, {} w ciągu {days} dni roboczych EBC, z {} na {}.",
            pair.label(),
            numbers::rate(c.to.r),
            c.direction.id(),
            numbers::rate(c.from.r),
            numbers::rate(c.to.r)
        ),
    };
    let summary = format!("{}{sentence}", stale_mark(served.stale, language));
    let out = Output::message(message(&sentence, &served, &pair, &provider, language))
        .with("summary", summary)
        .with("fromRate", numbers::rate(c.from.r))
        .with("fromDate", c.from.d.as_str())
        .with("days", i64::try_from(days).unwrap_or(1))
        .with("change", numbers::signed_rate(c.difference))
        .with("changePercent", numbers::percent(c.percent))
        .with("direction", c.direction.id());
    Ok(with_origin(out, &served, &pair, &provider))
}

/// How a notification ended.
enum Sent {
    /// Shown, or accepted by an agent that does not say where.
    Shown,
    /// Accepted, and shown nowhere: the user's settings say so.
    NotShown(String),
    /// Refused; the next check tries again.
    Refused(CapabilityError),
}

fn send(ctx: &mut Context<'_>, title: &str, body: &str) -> Sent {
    match ctx.notify_delivered(NotifySeverity::Info, title, body) {
        Ok(Some(delivery)) if !delivery.shown => Sent::NotShown(
            delivery
                .reason
                .unwrap_or_else(|| "the notification settings show it nowhere".to_owned()),
        ),
        Ok(_) => Sent::Shown,
        // An agent that predates severities refuses the request as
        // malformed; the plain form is the original one.
        Err(e) if e.kind() == CapabilityErrorKind::BadRequest => match ctx.notify(title, body) {
            Ok(()) => Sent::Shown,
            Err(e) => Sent::Refused(e),
        },
        Err(e) => Sent::Refused(e),
    }
}

fn check_alert(ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let pair = pair(args)?;
    let condition: String = args.get("condition")?;
    let above = condition != "below";
    let threshold: String = args.get("threshold")?;
    let threshold = numbers::parse_positive(&threshold).ok_or_else(|| {
        PluginError::new(
            INVALID_THRESHOLD,
            "threshold: expected a positive number such as 1.10 or 4,35",
        )
    })?;
    let threshold_text = numbers::canonical(threshold);
    let at = run_time(args)?;
    let language = language(ctx)?;
    let provider = Frankfurter;
    let served = served(ctx, &provider, &pair, at)?;
    let latest = served.latest().clone();
    let rate_text = numbers::rate(latest.r);
    let triggered = if above {
        latest.r >= threshold
    } else {
        latest.r <= threshold
    };
    let condition_words = match (above, language) {
        (true, Language::En) => "at or above",
        (false, Language::En) => "at or below",
        (true, Language::Pl) => "równy lub wyższy niż",
        (false, Language::Pl) => "równy lub niższy niż",
    };

    let key = cache::alert_key(&pair, &condition, &threshold_text);
    let mut notification: Option<String> = None;
    let (mut crossed, mut notified, evaluated) = (false, false, !served.stale);
    let sentence = if served.stale {
        // Old data neither fires nor re-arms an alert.
        match language {
            Language::En => format!(
                "{} {rate_text} ({}): not checked against your threshold, because the rate is not known to be current.",
                pair.label(),
                latest.d
            ),
            Language::Pl => format!(
                "{} {rate_text} ({}): nie porównano z Twoim progiem, bo kurs nie jest na pewno aktualny.",
                pair.label(),
                latest.d
            ),
        }
    } else {
        let before = cache::alert(ctx, &key)?.is_some_and(|e| e.triggered);
        crossed = triggered && !before;
        let mut remember = triggered;
        if crossed {
            let title = match language {
                Language::En => format!(
                    "{} {} {threshold_text}",
                    pair.label(),
                    if above { "above" } else { "below" }
                ),
                Language::Pl => format!(
                    "{} {} {threshold_text}",
                    pair.label(),
                    if above { "powyżej" } else { "poniżej" }
                ),
            };
            let body = match language {
                Language::En => format!(
                    "The ECB reference rate of {} is {rate_text}, {condition_words} your threshold {threshold_text}. A daily reference rate, not a live price. Source: ECB via Frankfurter.",
                    latest.d
                ),
                Language::Pl => format!(
                    "Kurs referencyjny EBC z {} wynosi {rate_text} i jest {condition_words} Twój próg {threshold_text}. Kurs dzienny, nie bieżąca cena. Źródło: EBC przez Frankfurter.",
                    latest.d
                ),
            };
            match send(ctx, &title, &body) {
                Sent::Shown => notified = true,
                Sent::NotShown(reason) => {
                    notified = true;
                    notification = Some(reason);
                }
                Sent::Refused(e) => {
                    // Not remembered as crossed: the next check tries again.
                    remember = false;
                    ctx.warn("the alert notification was refused; the next check tries again");
                    notification = Some(e.to_string());
                }
            }
        }
        cache::set_alert(ctx, &key, remember, &latest.d);
        publish(ctx, "alertTriggered", triggered);
        match (triggered, crossed, language) {
            (true, true, Language::En) => format!(
                "{} {rate_text} ({}) is {condition_words} your threshold {threshold_text}: it crossed it, {}.",
                pair.label(),
                latest.d,
                if notified {
                    "notified"
                } else {
                    "but the notification was refused; the next check tries again"
                }
            ),
            (true, true, Language::Pl) => format!(
                "{} {rate_text} ({}) jest {condition_words} Twój próg {threshold_text}: przekroczył go, {}.",
                pair.label(),
                latest.d,
                if notified {
                    "wysłano powiadomienie"
                } else {
                    "ale powiadomienie odrzucono; następne sprawdzenie spróbuje ponownie"
                }
            ),
            (true, false, Language::En) => format!(
                "{} {rate_text} ({}) is still {condition_words} your threshold {threshold_text}; already notified.",
                pair.label(),
                latest.d
            ),
            (true, false, Language::Pl) => format!(
                "{} {rate_text} ({}) nadal jest {condition_words} Twój próg {threshold_text}; powiadomienie już wysłano.",
                pair.label(),
                latest.d
            ),
            (false, _, Language::En) => format!(
                "{} {rate_text} ({}) is not {condition_words} your threshold {threshold_text}.",
                pair.label(),
                latest.d
            ),
            (false, _, Language::Pl) => format!(
                "{} {rate_text} ({}) nie jest {condition_words} Twój próg {threshold_text}.",
                pair.label(),
                latest.d
            ),
        }
    };
    let summary = format!("{}{sentence}", stale_mark(served.stale, language));
    let mut out = Output::message(message(&sentence, &served, &pair, &provider, language))
        .with("summary", summary)
        .with("condition", if above { "above" } else { "below" })
        .with("threshold", threshold_text)
        .with("triggered", triggered)
        .with("crossed", crossed)
        .with("notified", notified)
        .with("evaluated", evaluated);
    if let Some(reason) = notification {
        out = out.with("notification", reason);
    }
    Ok(with_origin(out, &served, &pair, &provider))
}

fn forget(ctx: &mut Context<'_>, _args: &Args<'_>) -> ActionResult {
    let language = language(ctx)?;
    cache::forget(ctx)?;
    if ctx.clear_variables().is_err() {
        ctx.warn("the variables could not be cleared");
    }
    Ok(Output::message(match language {
        Language::En => "The saved rates and the state of every alert were deleted.",
        Language::Pl => "Usunięto zapisane kursy i stan wszystkich alertów.",
    }))
}

#[cfg(test)]
mod tests;
