//! `deckpilot.break-reminder`: break and hydration reminders.
//!
//! **Tick-driven.** Plugin API 1.0 has no timer and no clock, and a plugin
//! runs only while one of its actions is invoked (`docs/plugin-factory.md`).
//! So the user adds one workflow with a schedule trigger ("every minute")
//! whose only node is this plugin's `tick`, and maps the trigger's `firedAt`
//! (`YYYY-MM-DD HH:MM`, local time) to the action's parameter. That text is
//! the plugin's only clock.
//!
//! **Countdowns, not deadlines.** The state keeps, per reminder, the minutes
//! left until it is due. A tick takes the minutes since the previous tick
//! off both countdowns; `snooze`, `skip`, `pause` and `restart` change the
//! countdowns or flags and need no clock at all, so they work from a key
//! between two ticks. The price is their precision: one tick.
//!
//! **Never a burst.** A countdown that reaches zero is one reminder, however
//! long ago it reached it, and a tick that finds both due sends one
//! notification for both. A pause between ticks of `awayMinutes` or more
//! (the computer slept, DeckPilot was locked or not running) counts as a
//! break: both countdowns start again and nothing is sent for the time
//! missed. A clock that went backwards takes nothing off.
//!
//! **Key state** (plugin API 1.2). [`Snapshot`] is the one place that
//! turns the stored state into what a user sees: the `status` outputs, the
//! variables and, through [`key`], a key bound to `status`, which shows
//! the countdowns as of the last tick and marks ticks that stopped.
#![deny(unsafe_code)]

pub mod key;

use deckpilot_plugin_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Must equal `manifest.json` (`id`, `version`).
pub const ID: &str = "deckpilot.break-reminder";
pub const VERSION: &str = "0.2.0";

/// The storage key of the whole state: one value, written once per action.
pub const STATE_KEY: &str = "state";
/// The version of the stored state. A value of another version, or one
/// that does not parse, is replaced by a fresh state.
pub const STATE_VERSION: u32 = 1;

/// Error codes of this plugin, each declared in `contributes.errorCodes`.
pub const INVALID_FIRED_AT: &str = "invalid-fired-at";
pub const INVALID_SETTINGS: &str = "invalid-settings";
pub const STORAGE_UNAVAILABLE: &str = "storage-unavailable";
pub const PLUGIN_CODES: &[&str] = &[INVALID_FIRED_AT, INVALID_SETTINGS, STORAGE_UNAVAILABLE];

/// The plugin's own variables (`{{ plugins.deckpilot.breakReminder.<id> }}`).
pub const VAR_STATE: &str = "state";
pub const VAR_BREAK: &str = "breakRemaining";
pub const VAR_HYDRATION: &str = "hydrationRemaining";

/// Bounds and defaults of the settings, as in `manifest.json`.
pub const BREAK_MINUTES: (u32, u32, u32) = (5, 240, 50);
pub const HYDRATION_MINUTES: (u32, u32, u32) = (5, 480, 60);
pub const AWAY_MINUTES: (u32, u32, u32) = (3, 120, 5);
/// The longest snooze and the longest timed pause, in minutes.
pub const MAX_SNOOZE: u32 = 120;
pub const MAX_PAUSE: u32 = 1440;

const WHICH: [&str; 3] = ["break", "hydration", "both"];
const WHICH_OR_LAST: [&str; 4] = ["last", "break", "hydration", "both"];

/// The plugin definition. The action ids, their order and their parameters
/// must equal `manifest.json`; a unit test compares them.
pub fn plugin() -> Plugin {
    Plugin::new(ID, VERSION)
        .requires_api(deckpilot_plugin_sdk::ApiLevel { major: 1, minor: 2 })
        .action(Action::new("tick", tick).param(Param::text("firedAt").required().max_length(32)))
        .action(
            Action::new("snooze", snooze)
                .param(
                    Param::integer("minutes")
                        .min(1.0)
                        .max(f64::from(MAX_SNOOZE))
                        .default(10),
                )
                .param(Param::choice("reminder", &WHICH_OR_LAST).default("last")),
        )
        .action(Action::new("skip", skip).param(Param::choice("reminder", &WHICH).default("break")))
        .action(
            Action::new("restart", restart)
                .param(Param::choice("reminder", &WHICH).default("both")),
        )
        .action(
            Action::new("pause", pause).param(
                Param::integer("minutes")
                    .min(0.0)
                    .max(f64::from(MAX_PAUSE))
                    .default(0),
            ),
        )
        .action(Action::new("resume", resume))
        .action(Action::new("toggle-pause", toggle_pause))
        .action(Action::new("status", status).state(key::status_state))
}

deckpilot_plugin_sdk::plugin!(plugin);

// ---------------------------------------------------------------------
// Time: the trigger's `firedAt`, the only clock there is.
// ---------------------------------------------------------------------

/// A local wall-clock minute: minutes since 1970-01-01 00:00 of the
/// computer's own time zone. Only differences and the time of day are used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Stamp(pub i64);

impl Stamp {
    /// Minutes after local midnight, 0-1439.
    pub fn minute_of_day(self) -> u16 {
        u16::try_from(self.0.rem_euclid(1440)).unwrap_or(0)
    }

    /// The local date as a day number.
    pub fn day(self) -> i64 {
        self.0.div_euclid(1440)
    }

    /// `YYYY-MM-DD HH:MM`, the form of `firedAt`.
    pub fn text(self) -> String {
        let (year, month, day) = civil_from_days(self.day());
        let minute = self.minute_of_day();
        format!(
            "{year:04}-{month:02}-{day:02} {:02}:{:02}",
            minute / 60,
            minute % 60
        )
    }
}

/// Days since 1970-01-01 of a date of the proleptic Gregorian calendar.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The inverse of [`days_from_civil`].
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        _ => 31,
    }
}

/// `n` ASCII digits as a number; anything else (a sign, a space, a
/// non-ASCII digit) is refused.
fn digits(text: &str, n: usize) -> Option<i64> {
    (text.len() == n && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

/// Parses a trigger's `firedAt`: `YYYY-MM-DD HH:MM`. Surrounding spaces,
/// a `T` instead of the space and trailing seconds (`:SS`) are accepted,
/// so a time written by hand or by another tool works too. `None` for
/// everything else, including dates that do not exist.
pub fn parse_fired_at(text: &str) -> Option<Stamp> {
    let text = text.trim();
    if !text.is_ascii() || !(16..=19).contains(&text.len()) {
        return None;
    }
    let (date, time) = text.split_at(10);
    let (separator, time) = time.split_at(1);
    if separator != " " && separator != "T" {
        return None;
    }
    let mut date = date.split('-');
    let (year, month, day) = (date.next()?, date.next()?, date.next()?);
    if date.next().is_some() {
        return None;
    }
    let (year, month, day) = (digits(year, 4)?, digits(month, 2)?, digits(day, 2)?);
    let mut time = time.split(':');
    let (hour, minute) = (digits(time.next()?, 2)?, digits(time.next()?, 2)?);
    if let Some(seconds) = time.next()
        && (!(0..60).contains(&digits(seconds, 2)?) || time.next().is_some())
    {
        return None;
    }
    let valid = (1970..=9999).contains(&year)
        && (1..=12).contains(&month)
        && (1..=days_in_month(year, month)).contains(&day)
        && (0..24).contains(&hour)
        && (0..60).contains(&minute);
    valid.then(|| Stamp(days_from_civil(year, month, day) * 1440 + hour * 60 + minute))
}

/// `HH:MM` (or `H:MM`) as minutes after midnight.
pub fn parse_time_of_day(text: &str) -> Option<u16> {
    let (hours, minutes) = text.trim().split_once(':')?;
    let hours = digits(hours, hours.len()).filter(|_| (1..=2).contains(&hours.len()))?;
    let minutes = digits(minutes, 2)?;
    ((0..24).contains(&hours) && (0..60).contains(&minutes))
        .then(|| u16::try_from(hours * 60 + minutes).ok())
        .flatten()
}

// ---------------------------------------------------------------------
// Preferences: the plugin's settings, read on every invocation.
// ---------------------------------------------------------------------

/// The language of the notifications. The plugin cannot learn the app's or
/// the system's language, so it is a setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    English,
    Polish,
}

/// Quiet hours: from `start` up to, not including, `end`, both minutes
/// after midnight. `start > end` crosses midnight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quiet {
    pub start: u16,
    pub end: u16,
}

impl Quiet {
    pub fn contains(self, minute_of_day: u16) -> bool {
        if self.start <= self.end {
            (self.start..self.end).contains(&minute_of_day)
        } else {
            minute_of_day >= self.start || minute_of_day < self.end
        }
    }
}

/// The settings as the plugin uses them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Minutes between break reminders; `None`: turned off.
    pub break_minutes: Option<u32>,
    /// Minutes between hydration reminders; `None`: turned off.
    pub hydration_minutes: Option<u32>,
    pub quiet: Option<Quiet>,
    /// A pause between two ticks of at least this many minutes is a break.
    pub away_minutes: u32,
    pub language: Language,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            break_minutes: Some(BREAK_MINUTES.2),
            hydration_minutes: Some(HYDRATION_MINUTES.2),
            quiet: None,
            away_minutes: AWAY_MINUTES.2,
            language: Language::English,
        }
    }
}

impl Config {
    /// From the settings object the agent returns (`settings.get`). The
    /// agent validates what the user saves and fills in defaults; a value
    /// that is absent or of another type still falls back to the default,
    /// and numbers are clamped to the manifest's bounds. Only quiet hours
    /// can be wrong, because they are free text: that is an error naming
    /// the setting, never a guess.
    pub fn from_settings(settings: &serde_json::Map<String, Value>) -> Result<Self, String> {
        let flag = |key: &str| settings.get(key).and_then(Value::as_bool).unwrap_or(true);
        let minutes = |key: &str, (min, max, default): (u32, u32, u32)| {
            settings
                .get(key)
                .and_then(Value::as_i64)
                .map_or(default, |v| {
                    u32::try_from(v.clamp(i64::from(min), i64::from(max))).unwrap_or(default)
                })
        };
        let time = |key: &str, label: &str| -> Result<Option<u16>, String> {
            match settings.get(key).and_then(Value::as_str).map(str::trim) {
                None | Some("") => Ok(None),
                Some(text) => parse_time_of_day(text).map(Some).ok_or_else(|| {
                    format!("the setting “{label}” must be a time such as 22:00 (HH:MM)")
                }),
            }
        };
        let quiet = match (
            time("quietStart", "Quiet hours from")?,
            time("quietEnd", "Quiet hours until")?,
        ) {
            (None, None) => None,
            (Some(start), Some(end)) if start == end => None,
            (Some(start), Some(end)) => Some(Quiet { start, end }),
            _ => {
                return Err(
                    "quiet hours need both settings, “Quiet hours from” and “Quiet hours until”, or neither"
                        .to_owned(),
                );
            }
        };
        Ok(Self {
            break_minutes: flag("breakEnabled").then(|| minutes("breakMinutes", BREAK_MINUTES)),
            hydration_minutes: flag("hydrationEnabled")
                .then(|| minutes("hydrationMinutes", HYDRATION_MINUTES)),
            quiet,
            away_minutes: minutes("awayMinutes", AWAY_MINUTES),
            language: match settings.get("language").and_then(Value::as_str) {
                Some("pl") => Language::Polish,
                _ => Language::English,
            },
        })
    }

    fn interval(&self, kind: Kind) -> Option<u32> {
        match kind {
            Kind::Break => self.break_minutes,
            Kind::Hydration => self.hydration_minutes,
        }
    }
}

// ---------------------------------------------------------------------
// State: what is stored, and the pure functions that change it.
// ---------------------------------------------------------------------

/// One of the two reminders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Break,
    Hydration,
}

impl Kind {
    pub const ALL: [Self; 2] = [Self::Break, Self::Hydration];
}

/// A set of reminders: what is due, what an action is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Which {
    #[serde(rename = "break")]
    pub break_: bool,
    pub hydration: bool,
}

impl Which {
    pub const NONE: Self = Self {
        break_: false,
        hydration: false,
    };
    pub const BREAK: Self = Self {
        break_: true,
        hydration: false,
    };
    pub const HYDRATION: Self = Self {
        break_: false,
        hydration: true,
    };
    pub const BOTH: Self = Self {
        break_: true,
        hydration: true,
    };

    pub fn has(self, kind: Kind) -> bool {
        match kind {
            Kind::Break => self.break_,
            Kind::Hydration => self.hydration,
        }
    }

    fn set(&mut self, kind: Kind) {
        match kind {
            Kind::Break => self.break_ = true,
            Kind::Hydration => self.hydration = true,
        }
    }

    pub fn is_empty(self) -> bool {
        self == Self::NONE
    }

    /// `none`, `break`, `hydration` or `both`: the action parameter's
    /// options and the `reminder` output.
    pub fn name(self) -> &'static str {
        match (self.break_, self.hydration) {
            (false, false) => "none",
            (true, false) => "break",
            (false, true) => "hydration",
            (true, true) => "both",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "break" => Some(Self::BREAK),
            "hydration" => Some(Self::HYDRATION),
            "both" => Some(Self::BOTH),
            _ => None,
        }
    }
}

/// The countdown of one reminder.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Timer {
    /// The interval the countdown was started with. When the setting
    /// differs (changed, or the reminder was off), the countdown starts
    /// again; 0 means it never started.
    pub interval: u32,
    /// Minutes until the reminder is due; 0: due now.
    pub left: u32,
    /// The next time it is due it is passed over, without a notification.
    pub skip_next: bool,
    /// Totals since the plugin was installed.
    pub shown: u64,
    pub skipped: u64,
    pub snoozed: u64,
    /// Reminders shown on the local date of the last tick.
    pub today: u32,
}

/// Everything the plugin stores, as one JSON value under [`STATE_KEY`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct State {
    pub version: u32,
    /// The `firedAt` of the last tick, as [`Stamp`] minutes.
    pub last_tick: Option<i64>,
    /// Whether the last tick was within the quiet hours.
    pub quiet: bool,
    pub paused: bool,
    /// Minutes left of a timed pause; `None`: until resumed.
    pub pause_left: Option<u32>,
    /// The reminders of the last notification, for `snooze` with `last`.
    pub last_reminder: Which,
    #[serde(rename = "break")]
    pub break_: Timer,
    pub hydration: Timer,
}

impl State {
    pub fn new() -> Self {
        Self {
            version: STATE_VERSION,
            ..Self::default()
        }
    }

    /// The stored value as a state. Anything that is not a state of this
    /// version (never written, damaged, written by a newer version before
    /// a rollback) is a fresh state: the reminders start again rather than
    /// fail on every tick. The flag says whether a stored value was
    /// dropped. Countdowns are bounded, whatever was stored.
    pub fn from_stored(value: Option<Value>) -> (Self, bool) {
        let Some(value) = value else {
            return (Self::new(), false);
        };
        match serde_json::from_value::<Self>(value) {
            Ok(mut state) if state.version == STATE_VERSION => {
                let bound = HYDRATION_MINUTES.1.max(MAX_SNOOZE);
                for timer in [&mut state.break_, &mut state.hydration] {
                    timer.left = timer.left.min(bound);
                }
                state.pause_left = state.pause_left.map(|left| left.min(MAX_PAUSE));
                (state, false)
            }
            _ => (Self::new(), true),
        }
    }

    pub fn timer(&self, kind: Kind) -> &Timer {
        match kind {
            Kind::Break => &self.break_,
            Kind::Hydration => &self.hydration,
        }
    }

    fn timer_mut(&mut self, kind: Kind) -> &mut Timer {
        match kind {
            Kind::Break => &mut self.break_,
            Kind::Hydration => &mut self.hydration,
        }
    }

    /// Brings the countdowns in line with the settings: a reminder that was
    /// turned on, or whose interval changed, starts a full interval; one
    /// that is off forgets its countdown. Every action does this first.
    pub fn sync(&mut self, config: &Config) {
        for kind in Kind::ALL {
            let timer = self.timer_mut(kind);
            match config.interval(kind) {
                Some(interval) if timer.interval != interval => {
                    timer.interval = interval;
                    timer.left = interval;
                }
                Some(_) => {}
                None => {
                    timer.interval = 0;
                    timer.left = 0;
                    timer.skip_next = false;
                }
            }
        }
    }

    /// Starts the countdowns of `which` again, as after a break.
    pub fn restart(&mut self, config: &Config, which: Which) -> Which {
        let mut done = Which::NONE;
        for kind in Kind::ALL {
            if let (true, Some(interval)) = (which.has(kind), config.interval(kind)) {
                let timer = self.timer_mut(kind);
                timer.left = interval;
                timer.skip_next = false;
                done.set(kind);
            }
        }
        done
    }

    /// Makes the reminders of `which` due in `minutes`, whatever their
    /// countdown was. Returns the ones that are on.
    pub fn snooze(&mut self, config: &Config, which: Which, minutes: u32) -> Which {
        let mut done = Which::NONE;
        for kind in Kind::ALL {
            if which.has(kind) && config.interval(kind).is_some() {
                let timer = self.timer_mut(kind);
                timer.left = minutes.clamp(1, MAX_SNOOZE);
                timer.snoozed = timer.snoozed.saturating_add(1);
                done.set(kind);
            }
        }
        done
    }

    /// Marks the next reminder of `which` to be passed over. Doing it twice
    /// skips one reminder, not two.
    pub fn skip(&mut self, config: &Config, which: Which) -> Which {
        let mut done = Which::NONE;
        for kind in Kind::ALL {
            if which.has(kind) && config.interval(kind).is_some() {
                self.timer_mut(kind).skip_next = true;
                done.set(kind);
            }
        }
        done
    }

    /// Pauses both reminders: until resumed, or for `minutes` when that is
    /// more than 0 (counted by the ticks).
    pub fn pause(&mut self, minutes: u32) {
        self.paused = true;
        self.pause_left = (minutes > 0).then(|| minutes.min(MAX_PAUSE));
    }

    pub fn resume(&mut self) {
        self.paused = false;
        self.pause_left = None;
    }

    /// One tick at `now`. Takes the time since the last tick off the
    /// countdowns and returns what is due. The due reminders are **not**
    /// restarted here: [`State::shown`] does that once the notification is
    /// on its way, so one that could not be sent stays due.
    pub fn advance(&mut self, config: &Config, now: Stamp) -> Advance {
        self.sync(config);
        let (elapsed, mut note) = match self.last_tick {
            None => (0, Note::First),
            Some(last) if now.0 < last => (0, Note::ClockWentBack),
            Some(last) if now.0 == last => (0, Note::SameMinute),
            Some(last) => (
                u32::try_from(now.0 - last).unwrap_or(u32::MAX),
                Note::Counted,
            ),
        };
        if self
            .last_tick
            .is_some_and(|last| Stamp(last).day() != now.day())
            || matches!(note, Note::First)
        {
            self.break_.today = 0;
            self.hydration.today = 0;
        }
        self.last_tick = Some(now.0);
        // The time since the last tick belongs to what the last tick was:
        // the minute that ends the quiet hours was a quiet one.
        let was_quiet = self.quiet;
        self.quiet = config
            .quiet
            .is_some_and(|q| q.contains(now.minute_of_day()));

        let away = elapsed >= config.away_minutes.max(1);
        if away {
            // The time away was the break: start again, send nothing.
            self.restart(config, Which::BOTH);
            note = Note::Away;
        }
        let mut counts = !away && !was_quiet;
        if self.paused {
            // The time since the last tick was paused time, also on the
            // tick that ends a timed pause.
            counts = false;
            if let Some(left) = self.pause_left {
                let left = left.saturating_sub(elapsed);
                if left == 0 {
                    self.resume();
                } else {
                    self.pause_left = Some(left);
                }
            }
        }
        if counts {
            for kind in Kind::ALL {
                if config.interval(kind).is_some() {
                    let timer = self.timer_mut(kind);
                    timer.left = timer.left.saturating_sub(elapsed);
                }
            }
        }

        let mut due = Which::NONE;
        let mut passed_over = Which::NONE;
        if !self.paused && !self.quiet {
            for kind in Kind::ALL {
                let Some(interval) = config.interval(kind) else {
                    continue;
                };
                let timer = self.timer_mut(kind);
                if timer.left > 0 {
                    continue;
                }
                if timer.skip_next {
                    timer.skip_next = false;
                    timer.skipped = timer.skipped.saturating_add(1);
                    timer.left = interval;
                    passed_over.set(kind);
                } else {
                    due.set(kind);
                }
            }
        }
        Advance {
            due,
            passed_over,
            note,
        }
    }

    /// The reminders of `which` were sent: count them and start again.
    pub fn shown(&mut self, config: &Config, which: Which) {
        if which.is_empty() {
            return;
        }
        for kind in Kind::ALL {
            if let (true, Some(interval)) = (which.has(kind), config.interval(kind)) {
                let timer = self.timer_mut(kind);
                timer.left = interval;
                timer.shown = timer.shown.saturating_add(1);
                timer.today = timer.today.saturating_add(1);
            }
        }
        self.last_reminder = which;
    }
}

/// What a tick found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Advance {
    /// Due now: to be sent as one notification.
    pub due: Which,
    /// Due now, and passed over because of `skip`.
    pub passed_over: Which,
    pub note: Note,
}

/// How the tick's time related to the previous tick's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Note {
    /// The first tick ever: the countdowns start.
    First,
    /// Later than the last tick, by less than the away time.
    Counted,
    /// The same minute again (a retry, a second workflow): nothing counted.
    SameMinute,
    /// Earlier than the last tick (the clock was set back, or daylight
    /// saving time ended): nothing counted, the countdowns go on from here.
    ClockWentBack,
    /// Later by the away time or more: both countdowns started again.
    Away,
}

/// What the user sees of the state: the outputs of every action and the
/// plugin's variables, and what a key bound to `status` shows ([`key`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub phase: Phase,
    /// Minutes until the next reminder; `None`: that reminder is off.
    pub break_left: Option<u32>,
    pub hydration_left: Option<u32>,
    /// Minutes left of a timed pause.
    pub pause_left: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// No tick was ever received: the workflow is not set up or has not
    /// fired yet.
    Waiting,
    Running,
    Paused,
    /// The last tick was within the quiet hours.
    Quiet,
}

impl Phase {
    pub fn name(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Quiet => "quiet",
        }
    }
}

impl Snapshot {
    /// Of a state that [`State::sync`] brought in line with `config`.
    pub fn of(state: &State, config: &Config) -> Self {
        let left = |kind| config.interval(kind).map(|_| state.timer(kind).left);
        Self {
            phase: if state.paused {
                Phase::Paused
            } else if state.last_tick.is_none() {
                Phase::Waiting
            } else if state.quiet {
                Phase::Quiet
            } else {
                Phase::Running
            },
            break_left: left(Kind::Break),
            hydration_left: left(Kind::Hydration),
            pause_left: state.paused.then_some(state.pause_left).flatten(),
        }
    }

    /// One line for the action's message.
    pub fn summary(&self) -> String {
        let countdowns = match (self.break_left, self.hydration_left) {
            (None, None) => "both reminders are turned off".to_owned(),
            (Some(b), None) => format!("break in {b} min"),
            (None, Some(h)) => format!("water in {h} min"),
            (Some(b), Some(h)) => format!("break in {b} min, water in {h} min"),
        };
        match (self.phase, self.pause_left) {
            (Phase::Waiting, _) => format!(
                "Waiting for the first tick ({countdowns}): add the every-minute workflow from the plugin's README"
            ),
            (Phase::Running, _) => {
                let mut text = countdowns;
                if let Some(first) = text.get_mut(..1) {
                    first.make_ascii_uppercase();
                }
                text
            }
            (Phase::Paused, Some(left)) => format!("Paused for {left} more min ({countdowns})"),
            (Phase::Paused, None) => format!("Paused ({countdowns})"),
            (Phase::Quiet, _) => format!("Quiet hours ({countdowns})"),
        }
    }
}

/// The title and text of the notification for `which` (not empty).
pub fn notification(which: Which, language: Language) -> (&'static str, &'static str) {
    match (language, which.break_, which.hydration) {
        (Language::English, true, false) => (
            "Time for a break",
            "Stand up, stretch and rest your eyes for a few minutes.",
        ),
        (Language::English, false, true) => (
            "Time to drink water",
            "Have a glass of water before you go on.",
        ),
        (Language::English, _, _) => (
            "Break and water",
            "Stand up, stretch, rest your eyes and have a glass of water.",
        ),
        (Language::Polish, true, false) => (
            "Czas na przerwę",
            "Wstań, rozciągnij się i daj oczom odpocząć przez kilka minut.",
        ),
        (Language::Polish, false, true) => (
            "Czas napić się wody",
            "Wypij szklankę wody, zanim wrócisz do pracy.",
        ),
        (Language::Polish, _, _) => (
            "Przerwa i woda",
            "Wstań, rozciągnij się, daj oczom odpocząć i wypij szklankę wody.",
        ),
    }
}

// ---------------------------------------------------------------------
// Actions.
// ---------------------------------------------------------------------

/// A failed storage call as this plugin's error. A denial (the plugin is
/// disabled) and the quota keep their shared codes.
fn storage_error(error: CapabilityError, doing: &str) -> PluginError {
    if error.is_denied() {
        return error.into();
    }
    if error.detail().starts_with("storage-quota") {
        return PluginError::new(
            codes::STORAGE_QUOTA,
            "the plugin's storage is full; the reminder state was not saved",
        );
    }
    PluginError::new(
        STORAGE_UNAVAILABLE,
        format!("the plugin's storage could not be {doing}; try again"),
    )
    .retryable()
}

/// The settings and the stored state, brought in line with each other.
fn load(ctx: &mut Context<'_>) -> Result<(Config, State), PluginError> {
    let settings = ctx.settings()?;
    let config = Config::from_settings(settings.as_map())
        .map_err(|problem| PluginError::new(INVALID_SETTINGS, problem))?;
    let stored = ctx
        .storage_get(STATE_KEY)
        .map_err(|e| storage_error(e, "read"))?;
    let (mut state, dropped) = State::from_stored(stored);
    if dropped {
        ctx.warn("the stored reminder state could not be used; the reminders start again");
    }
    state.sync(&config);
    Ok((config, state))
}

fn save(ctx: &mut Context<'_>, state: &State) -> Result<(), PluginError> {
    ctx.storage_set(STATE_KEY, state)
        .map_err(|e| storage_error(e, "written"))
}

/// The outputs every action has, and the plugin's variables. Variables
/// live in the agent's memory only, so every action publishes them again;
/// one that cannot be published is not worth failing the action for.
fn report(ctx: &mut Context<'_>, snapshot: &Snapshot, message: String) -> Output {
    let mut output = Output::message(message).with("state", snapshot.phase.name());
    let published = [
        ctx.publish_variable(VAR_STATE, snapshot.phase.name()),
        match snapshot.break_left {
            Some(left) => ctx.publish_variable(VAR_BREAK, left),
            None => ctx.clear_variable(VAR_BREAK),
        },
        match snapshot.hydration_left {
            Some(left) => ctx.publish_variable(VAR_HYDRATION, left),
            None => ctx.clear_variable(VAR_HYDRATION),
        },
    ];
    if published.iter().any(Result::is_err) {
        ctx.debug("the plugin's variables could not be published");
    }
    if let Some(left) = snapshot.break_left {
        output = output.with("breakRemaining", left);
    }
    if let Some(left) = snapshot.hydration_left {
        output = output.with("hydrationRemaining", left);
    }
    output
}

/// How a notification ended.
enum Sent {
    /// Shown, or accepted by an agent that does not say where.
    Shown,
    /// Accepted, and shown nowhere: the user's settings say so.
    NotShown(String),
    /// Refused; the reminder stays due.
    Refused(CapabilityError),
}

fn send(ctx: &mut Context<'_>, which: Which, language: Language) -> Sent {
    let (title, body) = notification(which, language);
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

fn tick(ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let fired_at: String = args.get("firedAt")?;
    let Some(now) = parse_fired_at(&fired_at) else {
        return Err(PluginError::new(
            INVALID_FIRED_AT,
            "firedAt must be the date and time of the schedule trigger, as YYYY-MM-DD HH:MM: \
             map the trigger's “Date and time” to a workflow input and that input to this parameter",
        ));
    };
    let (config, mut state) = load(ctx)?;
    let advance = state.advance(&config, now);
    match advance.note {
        Note::ClockWentBack => ctx.info("the clock went back; the countdowns go on from here"),
        Note::Away => {
            ctx.info("a long pause between ticks counts as a break; both countdowns start again")
        }
        _ => {}
    }

    // Saved as sent before sending: if the storage fails, nothing was
    // shown and nothing repeats; if the notification is refused, the
    // reminder is put back as due.
    let unsent = state.clone();
    state.shown(&config, advance.due);
    save(ctx, &state)?;

    let mut notified = false;
    let mut reason = None;
    if !advance.due.is_empty() {
        match send(ctx, advance.due, config.language) {
            Sent::Shown => notified = true,
            Sent::NotShown(why) => reason = Some(why),
            Sent::Refused(error) => {
                state = unsent;
                save(ctx, &state)?;
                if error.is_denied() {
                    return Err(error.into());
                }
                ctx.warn(format!("the reminder was not sent and stays due: {error}"));
                reason = Some(match error.kind() {
                    CapabilityErrorKind::Throttled => {
                        "more than 5 notifications a minute; it is sent with the next tick"
                            .to_owned()
                    }
                    _ => "the notification could not be sent; it is tried again with the next tick"
                        .to_owned(),
                });
            }
        }
    }

    let snapshot = Snapshot::of(&state, &config);
    let message = match (advance.due.name(), notified, &reason) {
        ("none", _, _) if !advance.passed_over.is_empty() => format!(
            "Skipped the {} reminder. {}",
            describe(advance.passed_over),
            snapshot.summary()
        ),
        ("none", _, _) => snapshot.summary(),
        (_, true, _) => format!(
            "Showed the {} reminder. {}",
            describe(advance.due),
            snapshot.summary()
        ),
        (_, false, why) => format!(
            "The {} reminder was not shown: {}",
            describe(advance.due),
            why.as_deref().unwrap_or("unknown")
        ),
    };
    let mut output = report(ctx, &snapshot, message)
        .with("reminder", advance.due.name())
        .with("notified", notified);
    if let Some(reason) = reason {
        output = output.with("reason", reason);
    }
    Ok(output)
}

/// `break`, `hydration`, or `break and hydration`, for messages.
fn describe(which: Which) -> &'static str {
    match (which.break_, which.hydration) {
        (true, true) => "break and hydration",
        (false, true) => "hydration",
        _ => "break",
    }
}

/// The message of an action that changed nothing because the reminders it
/// names are turned off.
fn turned_off(which: Which) -> Output {
    Output::skipped(format!(
        "the {} reminder is turned off in the plugin's settings",
        describe(which)
    ))
}

fn which(args: &Args<'_>) -> Result<Option<Which>, PluginError> {
    let name: String = args.get("reminder")?;
    Ok(Which::from_name(&name))
}

fn snooze(ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let minutes = u32::try_from(args.get::<i64>("minutes")?).unwrap_or(1);
    let named = which(args)?;
    let (config, mut state) = load(ctx)?;
    // `last`: the reminders of the last notification.
    let target = named.unwrap_or(state.last_reminder);
    if target.is_empty() {
        return Ok(Output::skipped(
            "no reminder was shown yet, so there is nothing to snooze",
        ));
    }
    let done = state.snooze(&config, target, minutes);
    if done.is_empty() {
        return Ok(turned_off(target));
    }
    save(ctx, &state)?;
    let snapshot = Snapshot::of(&state, &config);
    let message = format!(
        "Snoozed the {} reminder for {minutes} min. {}",
        describe(done),
        snapshot.summary()
    );
    Ok(report(ctx, &snapshot, message))
}

fn skip(ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let target = which(args)?.unwrap_or(Which::BREAK);
    let (config, mut state) = load(ctx)?;
    let done = state.skip(&config, target);
    if done.is_empty() {
        return Ok(turned_off(target));
    }
    save(ctx, &state)?;
    let snapshot = Snapshot::of(&state, &config);
    let message = format!(
        "The next {} reminder will be skipped. {}",
        describe(done),
        snapshot.summary()
    );
    Ok(report(ctx, &snapshot, message))
}

fn restart(ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let target = which(args)?.unwrap_or(Which::BOTH);
    let (config, mut state) = load(ctx)?;
    let done = state.restart(&config, target);
    if done.is_empty() {
        return Ok(turned_off(target));
    }
    save(ctx, &state)?;
    let snapshot = Snapshot::of(&state, &config);
    let message = format!(
        "Started the {} countdown again. {}",
        describe(done),
        snapshot.summary()
    );
    Ok(report(ctx, &snapshot, message))
}

/// Saves a state whose pause changed and reports it.
fn paused_changed(ctx: &mut Context<'_>, config: &Config, state: &State) -> ActionResult {
    save(ctx, state)?;
    let snapshot = Snapshot::of(state, config);
    let message = snapshot.summary();
    Ok(report(ctx, &snapshot, message).with("paused", state.paused))
}

fn pause(ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let minutes = u32::try_from(args.get::<i64>("minutes")?).unwrap_or(0);
    let (config, mut state) = load(ctx)?;
    state.pause(minutes);
    paused_changed(ctx, &config, &state)
}

fn resume(ctx: &mut Context<'_>, _args: &Args<'_>) -> ActionResult {
    let (config, mut state) = load(ctx)?;
    state.resume();
    paused_changed(ctx, &config, &state)
}

fn toggle_pause(ctx: &mut Context<'_>, _args: &Args<'_>) -> ActionResult {
    let (config, mut state) = load(ctx)?;
    if state.paused {
        state.resume();
    } else {
        state.pause(0);
    }
    paused_changed(ctx, &config, &state)
}

/// Reads and reports; stores nothing.
fn status(ctx: &mut Context<'_>, _args: &Args<'_>) -> ActionResult {
    let (config, state) = load(ctx)?;
    let snapshot = Snapshot::of(&state, &config);
    let message = snapshot.summary();
    let mut output = report(ctx, &snapshot, message)
        .with("paused", state.paused)
        .with("quietHours", state.quiet)
        .with("breaksToday", state.break_.today)
        .with("hydrationToday", state.hydration.today);
    if let Some(left) = snapshot.pause_left {
        output = output.with("pauseRemaining", left);
    }
    if let Some(last) = state.last_tick {
        output = output.with("lastTick", Stamp(last).text());
    }
    Ok(output)
}

#[cfg(test)]
mod tests;
