//! WMO weather interpretation codes (WMO code table 4677, the subset
//! Open-Meteo reports as `weather_code`) as a short text in English and
//! Polish and a symbolic name.
//!
//! The symbolic name is what a key icon would be chosen by once plugins can
//! set key content; together with `is_day` it tells a sun from a moon.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    En,
    Pl,
}

impl Language {
    /// The `language` setting; anything unknown is English (its default).
    pub fn from_setting(value: Option<&str>) -> Self {
        match value {
            Some("pl") => Self::Pl,
            _ => Self::En,
        }
    }

    /// The code the geocoding API takes.
    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Pl => "pl",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Condition {
    /// One of [`NAMES`].
    pub name: &'static str,
    pub en: &'static str,
    pub pl: &'static str,
}

impl Condition {
    pub fn text(&self, language: Language) -> &'static str {
        match language {
            Language::En => self.en,
            Language::Pl => self.pl,
        }
    }
}

/// Every symbolic condition name, in the order of the codes.
pub const NAMES: &[&str] = &[
    "clear",
    "mostly-clear",
    "partly-cloudy",
    "overcast",
    "fog",
    "drizzle",
    "freezing-drizzle",
    "rain",
    "freezing-rain",
    "snow",
    "rain-showers",
    "snow-showers",
    "thunderstorm",
    "thunderstorm-hail",
    "unknown",
];

/// Every code with a meaning of its own: `(code, name, English, Polish)`.
pub const CODES: &[(i64, &str, &str, &str)] = &[
    (0, "clear", "Clear sky", "Bezchmurnie"),
    (1, "mostly-clear", "Mainly clear", "Przeważnie bezchmurnie"),
    (
        2,
        "partly-cloudy",
        "Partly cloudy",
        "Częściowe zachmurzenie",
    ),
    (3, "overcast", "Overcast", "Pochmurno"),
    (45, "fog", "Fog", "Mgła"),
    (48, "fog", "Depositing rime fog", "Mgła osadzająca szadź"),
    (51, "drizzle", "Light drizzle", "Słaba mżawka"),
    (53, "drizzle", "Moderate drizzle", "Umiarkowana mżawka"),
    (55, "drizzle", "Dense drizzle", "Gęsta mżawka"),
    (
        56,
        "freezing-drizzle",
        "Light freezing drizzle",
        "Słaba marznąca mżawka",
    ),
    (
        57,
        "freezing-drizzle",
        "Dense freezing drizzle",
        "Gęsta marznąca mżawka",
    ),
    (61, "rain", "Slight rain", "Słaby deszcz"),
    (63, "rain", "Moderate rain", "Umiarkowany deszcz"),
    (65, "rain", "Heavy rain", "Silny deszcz"),
    (
        66,
        "freezing-rain",
        "Light freezing rain",
        "Słaby marznący deszcz",
    ),
    (
        67,
        "freezing-rain",
        "Heavy freezing rain",
        "Silny marznący deszcz",
    ),
    (71, "snow", "Slight snowfall", "Słabe opady śniegu"),
    (73, "snow", "Moderate snowfall", "Umiarkowane opady śniegu"),
    (75, "snow", "Heavy snowfall", "Silne opady śniegu"),
    (77, "snow", "Snow grains", "Śnieg ziarnisty"),
    (
        80,
        "rain-showers",
        "Slight rain showers",
        "Słabe przelotne opady deszczu",
    ),
    (
        81,
        "rain-showers",
        "Moderate rain showers",
        "Umiarkowane przelotne opady deszczu",
    ),
    (
        82,
        "rain-showers",
        "Violent rain showers",
        "Gwałtowne przelotne opady deszczu",
    ),
    (
        85,
        "snow-showers",
        "Slight snow showers",
        "Słabe przelotne opady śniegu",
    ),
    (
        86,
        "snow-showers",
        "Heavy snow showers",
        "Silne przelotne opady śniegu",
    ),
    (95, "thunderstorm", "Thunderstorm", "Burza"),
    (
        96,
        "thunderstorm-hail",
        "Thunderstorm with slight hail",
        "Burza ze słabym gradem",
    ),
    (97, "thunderstorm", "Heavy thunderstorm", "Silna burza"),
    (
        99,
        "thunderstorm-hail",
        "Thunderstorm with heavy hail",
        "Burza z silnym gradem",
    ),
];

const UNKNOWN: Condition = Condition {
    name: "unknown",
    en: "Unknown conditions",
    pl: "Nieznane warunki",
};

/// The condition of a WMO code; a code without a meaning here is
/// [`UNKNOWN`], never a guess.
pub fn condition(code: i64) -> Condition {
    CODES
        .iter()
        .find(|(c, ..)| *c == code)
        .map_or(UNKNOWN, |&(_, name, en, pl)| Condition { name, en, pl })
}
