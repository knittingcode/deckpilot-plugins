//! Units. The provider is always asked for metric values (°C, km/h); the
//! plugin converts, so the cache does not depend on the setting.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Units {
    Metric,
    Imperial,
}

impl Units {
    /// The `units` setting; anything unknown is metric (its default).
    pub fn from_setting(value: Option<&str>) -> Self {
        match value {
            Some("imperial") => Self::Imperial,
            _ => Self::Metric,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Metric => "metric",
            Self::Imperial => "imperial",
        }
    }

    pub fn temperature_unit(self) -> &'static str {
        match self {
            Self::Metric => "°C",
            Self::Imperial => "°F",
        }
    }

    pub fn wind_unit(self) -> &'static str {
        match self {
            Self::Metric => "km/h",
            Self::Imperial => "mph",
        }
    }

    /// A temperature in °C as a whole number of the chosen unit.
    pub fn temperature(self, celsius: f64) -> i64 {
        whole(match self {
            Self::Metric => celsius,
            Self::Imperial => celsius * 9.0 / 5.0 + 32.0,
        })
    }

    /// A wind speed in km/h as a whole number of the chosen unit.
    pub fn wind(self, kmh: f64) -> i64 {
        whole(match self {
            Self::Metric => kmh,
            Self::Imperial => kmh / 1.609_344,
        })
    }
}

/// Rounded half away from zero; "-0" is 0. The provider's values are
/// checked to be small and finite before they get here.
fn whole(value: f64) -> i64 {
    let rounded = value.round();
    if rounded.is_finite() {
        rounded as i64
    } else {
        0
    }
}

/// The compass point (of eight) the wind blows from.
pub fn compass(degrees: i64) -> &'static str {
    const POINTS: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    let sector = (degrees.rem_euclid(360) * 2 + 45) / 90 % 8;
    POINTS[sector as usize]
}
