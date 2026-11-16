//! The language of the plugin's texts.

/// The language of messages and of the index categories (the `language`
/// setting). Error messages are English.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    En,
    Pl,
}

impl Language {
    pub fn from_setting(value: Option<&str>) -> Self {
        match value {
            Some("pl") => Self::Pl,
            _ => Self::En,
        }
    }

    /// Picks the text of this language.
    pub fn pick<'a>(self, en: &'a str, pl: &'a str) -> &'a str {
        match self {
            Self::En => en,
            Self::Pl => pl,
        }
    }
}

/// `µg/m³`: the unit of every concentration the plugin reports.
pub const UNIT: &str = "µg/m³";

/// A provider's text as one line with single spaces, at most 80
/// characters.
pub fn one_line(text: &str) -> String {
    let line: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
    line.chars().take(80).collect()
}

/// A text as it is searched: lower case, Polish letters without their
/// marks, everything that is not a letter or a digit as a space.
pub fn fold(text: &str) -> String {
    let folded: String = text
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'ą' => 'a',
            'ć' => 'c',
            'ę' => 'e',
            'ł' => 'l',
            'ń' => 'n',
            'ó' => 'o',
            'ś' => 's',
            'ź' | 'ż' => 'z',
            c if c.is_alphanumeric() => c,
            _ => ' ',
        })
        .collect();
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}
