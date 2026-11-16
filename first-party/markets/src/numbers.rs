//! Decimal numbers in and out.
//!
//! A manifest parameter has no kind for a decimal number (plugin API 1.0:
//! text, integer, boolean, choice), and an output has none either. So an
//! amount or a threshold is typed as text and parsed here, and a rate leaves
//! the plugin as a decimal text that a workflow can compare or show.

/// The largest amount or threshold accepted: far above any sum of money,
/// and small enough that a product with a rate cannot overflow.
pub const MAX_INPUT: f64 = 1e12;

/// A positive decimal number as a user types it: digits with at most one
/// decimal point or decimal comma, spaces, `_` and `'` between digit groups
/// (`1 000,50`, `1_000.5`, `0.95`). No sign, no exponent, no thousands
/// separator that is also a decimal separator. `None` for anything else,
/// zero, or more than [`MAX_INPUT`].
pub fn parse_positive(text: &str) -> Option<f64> {
    let compact: String = text
        .trim()
        .chars()
        .filter(|c| !matches!(c, ' ' | '_' | '\'' | '\u{a0}' | '\u{202f}'))
        .collect();
    if compact.is_empty() || compact.len() > 32 {
        return None;
    }
    let mut separators = 0;
    for c in compact.chars() {
        match c {
            '0'..='9' => {}
            '.' | ',' => separators += 1,
            _ => return None,
        }
    }
    if separators > 1 || compact.starts_with(['.', ',']) || compact.ends_with(['.', ',']) {
        return None;
    }
    let value: f64 = compact.replace(',', ".").parse().ok()?;
    (value.is_finite() && value > 0.0 && value <= MAX_INPUT).then_some(value)
}

/// Decimals that show `value` with at least `significant` significant
/// digits, and at least `min`, at most 8.
fn decimals_for(value: f64, significant: i32, min: usize) -> usize {
    let magnitude = value.abs();
    if magnitude == 0.0 || !magnitude.is_finite() {
        return min;
    }
    let integer_digits = magnitude.log10().floor() as i32 + 1;
    let needed = usize::try_from((significant - integer_digits).max(0)).unwrap_or(0);
    needed.clamp(min, 8)
}

/// `value` with `decimals` decimals, without trailing zeros beyond `keep`.
fn fixed(value: f64, decimals: usize, keep: usize) -> String {
    let mut text = format!("{value:.decimals$}");
    if text.contains('.') {
        while text.ends_with('0') && text.split('.').nth(1).is_some_and(|d| d.len() > keep) {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    if text == "-0" {
        text = "0".into();
    }
    text
}

/// A rate as text: at least five significant digits (the ECB publishes
/// four or five), no trailing zeros (`1.1014`, `171.23`, `0.0062543`).
pub fn rate(value: f64) -> String {
    fixed(value, decimals_for(value, 5, 0), 0)
}

/// A sum of money: two decimals, more for a small one (`1101.40`,
/// `0.0054321`).
pub fn amount(value: f64) -> String {
    fixed(value, decimals_for(value, 4, 2), 2)
}

/// A difference of two rates, with its sign (`+0.0038`, `-1.25`, `0`).
pub fn signed_rate(value: f64) -> String {
    let text = fixed(value.abs(), decimals_for(value, 3, 4).max(4), 0);
    match () {
        () if text == "0" => text,
        () if value > 0.0 => format!("+{text}"),
        () => format!("-{text}"),
    }
}

/// A percentage with two decimals and its sign (`+0.34`, `-1.20`,
/// `0.00`).
pub fn percent(value: f64) -> String {
    let text = format!("{:.2}", value.abs());
    if text == "0.00" {
        text
    } else if value > 0.0 {
        format!("+{text}")
    } else {
        format!("-{text}")
    }
}

/// A user's number back as text in the plugin's own form (`1.1`,
/// `1000.5`): what a threshold is stored and compared as.
pub fn canonical(value: f64) -> String {
    fixed(value, 8, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_numbers() {
        assert_eq!(parse_positive("1.10"), Some(1.1));
        assert_eq!(parse_positive(" 1,10 "), Some(1.1));
        assert_eq!(parse_positive("1 000,50"), Some(1000.5));
        assert_eq!(parse_positive("1_000.5"), Some(1000.5));
        assert_eq!(parse_positive("12"), Some(12.0));
        for bad in [
            "",
            " ",
            "0",
            "0.0",
            "-1",
            "+1",
            "1e3",
            "1.000,5",
            ".5",
            "5.",
            "abc",
            "1..2",
            "NaN",
            "inf",
            "10000000000000",
        ] {
            assert_eq!(parse_positive(bad), None, "{bad}");
        }
    }

    #[test]
    fn rates_and_amounts() {
        assert_eq!(rate(1.1014), "1.1014");
        assert_eq!(rate(4.403), "4.403");
        assert_eq!(rate(171.23), "171.23");
        assert_eq!(rate(1.0), "1");
        assert_eq!(rate(3.997_640_2), "3.9976");
        assert_eq!(rate(0.006_254_3), "0.0062543");
        assert_eq!(rate(26_543.2), "26543");
        assert_eq!(amount(1101.4), "1101.40");
        assert_eq!(amount(110.14), "110.14");
        assert_eq!(amount(0.005_432_1), "0.005432");
        assert_eq!(signed_rate(0.0038), "+0.0038");
        assert_eq!(signed_rate(-0.0038), "-0.0038");
        assert_eq!(signed_rate(-1.25), "-1.25");
        assert_eq!(signed_rate(0.0), "0");
        assert_eq!(percent(0.344), "+0.34");
        assert_eq!(percent(-1.2), "-1.20");
        assert_eq!(percent(0.001), "0.00");
        assert_eq!(percent(-0.001), "0.00");
        assert_eq!(canonical(1.1), "1.1");
        assert_eq!(canonical(1000.5), "1000.5");
    }
}
