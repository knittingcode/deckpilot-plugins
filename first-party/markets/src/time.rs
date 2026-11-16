//! The little calendar arithmetic the plugin needs.
//!
//! A guest has no clock (plugin API 1.0). The plugin learns a time from two
//! places only, and both are parsed here: the `Date` header of an HTTP
//! answer (UTC, to the second) and the `at` parameter of an action, which a
//! scheduled workflow fills with the firing time of its trigger (the
//! computer's local time, to the minute, without a zone). The third kind of
//! date is the one a reference rate is for (`YYYY-MM-DD`), as days.

/// Days from 1970-01-01 to the given civil date (proleptic Gregorian).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date of a day number: the inverse of [`days_from_civil`].
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// A number written with exactly `digits` ASCII digits.
fn digits(text: &str, digits: usize) -> Option<i64> {
    (text.len() == digits && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

fn valid_date(year: i64, month: i64, day: i64) -> bool {
    (1970..10_000).contains(&year)
        && (1..=12).contains(&month)
        && (1..=days_in_month(year, month)).contains(&day)
}

/// An IMF-fixdate (RFC 9110 §5.6.7, `Sun, 06 Nov 1994 08:49:37 GMT`) as
/// Unix seconds. Anything else is `None`.
pub fn http_date(value: &str) -> Option<u64> {
    let (_, rest) = value.trim().split_once(", ")?;
    let mut parts = rest.split(' ');
    let (day, month, year, clock, zone) = (
        parts.next()?,
        parts.next()?,
        parts.next()?,
        parts.next()?,
        parts.next()?,
    );
    if zone != "GMT" || parts.next().is_some() {
        return None;
    }
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|m| *m == month)? as i64
        + 1;
    let (day, year) = (digits(day, 2)?, digits(year, 4)?);
    let mut clock = clock.split(':');
    let (hour, minute, second) = (
        digits(clock.next()?, 2)?,
        digits(clock.next()?, 2)?,
        digits(clock.next()?, 2)?,
    );
    if clock.next().is_some()
        || !valid_date(year, month, day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let seconds = days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second;
    u64::try_from(seconds).ok()
}

/// The firing time of a workflow trigger, `YYYY-MM-DD HH:MM` in the
/// computer's local time, as minutes on a local time line (minutes since
/// 1970-01-01 00:00 of that wall clock). Two such values of the same
/// computer can be subtracted; the result is wrong by the size of the
/// shift across a change of the UTC offset (daylight saving time).
pub fn local_minutes(value: &str) -> Option<i64> {
    let (date, clock) = value.trim().split_once(' ')?;
    let mut date = date.split('-');
    let (year, month, day) = (
        digits(date.next()?, 4)?,
        digits(date.next()?, 2)?,
        digits(date.next()?, 2)?,
    );
    let (hour, minute) = clock.split_once(':')?;
    let (hour, minute) = (digits(hour, 2)?, digits(minute, 2)?);
    if date.next().is_some() || !valid_date(year, month, day) || hour > 23 || minute > 59 {
        return None;
    }
    Some(days_from_civil(year, month, day) * 1_440 + hour * 60 + minute)
}

/// Unix seconds as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn utc_text(unix: u64) -> String {
    let seconds = i64::try_from(unix).unwrap_or(i64::MAX / 2);
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    let rest = seconds.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}

/// A date `YYYY-MM-DD` as days since 1970-01-01.
pub fn day_number(value: &str) -> Option<i64> {
    let mut parts = value.split('-');
    let (year, month, day) = (
        digits(parts.next()?, 4)?,
        digits(parts.next()?, 2)?,
        digits(parts.next()?, 2)?,
    );
    if parts.next().is_some() || !valid_date(year, month, day) {
        return None;
    }
    Some(days_from_civil(year, month, day))
}

/// Days since 1970-01-01 as `YYYY-MM-DD`.
pub fn date_text(days: i64) -> String {
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

/// The UTC day of Unix seconds.
pub fn utc_day(unix: u64) -> i64 {
    i64::try_from(unix / 86_400).unwrap_or(i64::MAX / 2)
}

/// The day of a local minute of [`local_minutes`].
pub fn local_day(minutes: i64) -> i64 {
    minutes.div_euclid(1_440)
}

/// Monday is 0, Sunday 6 (1970-01-01 was a Thursday).
pub fn weekday(days: i64) -> i64 {
    (days + 3).rem_euclid(7)
}

/// Saturday or Sunday.
pub fn is_weekend(days: i64) -> bool {
    weekday(days) >= 5
}
