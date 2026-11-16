//! What the stored rates of a pair say: the change over some days, the
//! lowest and highest rate, the trend, and what a key would show.
//!
//! **No chart.** Plugin API 1.0 cannot put anything on a key
//! (`docs/plugin-factory.md`, "Dynamic key images"). The history kept for
//! each pair is what a sparkline would be drawn from; [`Snapshot::key`]
//! turns it into the members of the proposed key state (6.1), `series`
//! included, so that a later version can return it from a state call
//! without changing what is stored. This version does not deliver it to a
//! key.

use crate::provider::Point;

/// A change below this percentage either way is `flat`.
pub const FLAT_PERCENT: f64 = 0.1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Flat,
}

impl Direction {
    pub fn of(percent: f64) -> Self {
        if percent >= FLAT_PERCENT {
            Self::Up
        } else if percent <= -FLAT_PERCENT {
            Self::Down
        } else {
            Self::Flat
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Flat => "flat",
        }
    }
}

/// From one rate to a later one.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub from: Point,
    pub to: Point,
    pub difference: f64,
    pub percent: f64,
    pub direction: Direction,
}

impl Change {
    pub fn between(from: &Point, to: &Point) -> Self {
        let difference = to.r - from.r;
        let percent = difference / from.r * 100.0;
        Self {
            from: from.clone(),
            to: to.clone(),
            difference,
            percent,
            direction: Direction::of(percent),
        }
    }
}

/// The change over the last `days` rates (ECB working days): from the rate
/// `days` before the newest to the newest. `None` with fewer rates.
pub fn change(points: &[Point], days: usize) -> Option<Change> {
    let to = points.last()?;
    let from = points.get(points.len().checked_sub(days + 1)?)?;
    Some(Change::between(from, to))
}

/// The stored rates of a pair at a glance.
#[derive(Debug, Clone, PartialEq)]
pub struct Stats {
    pub count: usize,
    pub first_date: String,
    pub min: f64,
    pub max: f64,
    /// From the oldest stored rate to the newest.
    pub trend: Direction,
}

pub fn stats(points: &[Point]) -> Option<Stats> {
    let (first, last) = (points.first()?, points.last()?);
    let (min, max) = points
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
            (lo.min(p.r), hi.max(p.r))
        });
    Some(Stats {
        count: points.len(),
        first_date: first.d.clone(),
        min,
        max,
        trend: Change::between(first, last).direction,
    })
}

/// The rates scaled to 0-100 between their lowest and highest, oldest
/// first: the `series` of the proposed key state (at most 32 values). All
/// equal: 50.
pub fn series(points: &[Point]) -> Vec<u8> {
    let Some(stats) = stats(points) else {
        return Vec::new();
    };
    let span = stats.max - stats.min;
    points
        .iter()
        .rev()
        .take(32)
        .rev()
        .map(|p| {
            if span <= 0.0 {
                50
            } else {
                ((p.r - stats.min) / span * 100.0).round().clamp(0.0, 100.0) as u8
            }
        })
        .collect()
}

/// What a key bound to `rate` would show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// `EUR/USD`.
    pub pair: String,
    /// The newest rate as text.
    pub rate: String,
    /// Its date, `YYYY-MM-DD`.
    pub date: String,
    /// Not known to be current.
    pub stale: bool,
    /// The change since the previous rate in percent, as text.
    pub change_percent: Option<String>,
    pub series: Vec<u8>,
}

/// The members of DeckPilot's `ButtonState` plus `series`, which the
/// proposed key-state call for plugins returns (`docs/plugin-factory.md`,
/// 6.1). Not delivered to a key by this version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyFace {
    /// The title: the rate, at most 32 characters.
    pub label: String,
    /// The second line: the pair and the date, at most 32 characters.
    pub detail: String,
    /// `ok` or `warning` (stale).
    pub indicator: &'static str,
    /// A sparkline of the stored rates, 0-100, at most 32 values.
    pub series: Vec<u8>,
}

impl Snapshot {
    pub fn key(&self) -> KeyFace {
        let mut label = self.rate.clone();
        if let Some(change) = &self.change_percent {
            label = format!("{label} {change}%");
        }
        // `MM-DD` keeps the pair and the day within 32 characters.
        let day = self.date.get(5..).unwrap_or(&self.date);
        let detail = format!("{} {day}", self.pair);
        KeyFace {
            label: label.chars().take(32).collect(),
            detail: detail.chars().take(32).collect(),
            indicator: if self.stale { "warning" } else { "ok" },
            series: self.series.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn points(rates: &[f64]) -> Vec<Point> {
        rates
            .iter()
            .enumerate()
            .map(|(i, r)| Point {
                d: format!("2026-11-{:02}", i + 1),
                r: *r,
            })
            .collect()
    }

    #[test]
    fn change_over_days() {
        let p = points(&[1.10, 1.12, 1.11]);
        let c = change(&p, 1).unwrap();
        assert_eq!(c.from.d, "2026-11-02");
        assert!((c.difference + 0.01).abs() < 1e-12);
        assert_eq!(c.direction, Direction::Down);
        let c = change(&p, 2).unwrap();
        assert_eq!(c.from.d, "2026-11-01");
        assert_eq!(c.direction, Direction::Up);
        assert!(change(&p, 3).is_none());
        assert!(change(&p[..1], 1).is_none());
        assert_eq!(Direction::of(0.05), Direction::Flat);
        assert_eq!(Direction::of(-0.1), Direction::Down);
    }

    #[test]
    fn stats_series_and_key() {
        let p = points(&[1.0, 2.0, 1.5]);
        let s = stats(&p).unwrap();
        assert_eq!((s.count, s.min, s.max), (3, 1.0, 2.0));
        assert_eq!(s.first_date, "2026-11-01");
        assert_eq!(s.trend, Direction::Up);
        assert_eq!(series(&p), vec![0, 100, 50]);
        assert_eq!(series(&points(&[3.0, 3.0])), vec![50, 50]);
        assert!(series(&[]).is_empty());
        let many: Vec<f64> = (0..40).map(f64::from).collect();
        assert_eq!(series(&points(&many)).len(), 32);

        let snapshot = Snapshot {
            pair: "EUR/USD".into(),
            rate: "1.1014".into(),
            date: "2026-11-16".into(),
            stale: true,
            change_percent: Some("-0.34".into()),
            series: vec![1, 2],
        };
        let key = snapshot.key();
        assert_eq!(key.label, "1.1014 -0.34%");
        assert_eq!(key.detail, "EUR/USD 11-16");
        assert_eq!(key.indicator, "warning");
        assert_eq!(key.series, vec![1, 2]);
    }
}
