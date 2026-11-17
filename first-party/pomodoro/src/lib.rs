//! `deckpilot.pomodoro`: a focus timer with short and long breaks.
//!
//! **Tick-driven.** Plugin API 1.0 has no timer and no clock, and a plugin
//! runs only while one of its actions is invoked (`docs/plugin-factory.md`).
//! So the user adds one workflow with a schedule trigger ("every minute")
//! whose only node is this plugin's `tick`, and maps the trigger's `firedAt`
//! (`YYYY-MM-DD HH:MM`, local time) to the action's parameter. That text is
//! the plugin's only clock, and it has no seconds.
//!
//! **A countdown, not a deadline.** The state keeps the minutes left of the
//! current phase. A tick takes the minutes since the previous tick off a
//! running countdown; `start`, `pause`, `resume`, `toggle`, `skip` and
//! `reset` change the phase or a flag and need no clock at all, so they
//! work from a key between two ticks. The price is their precision: one
//! tick, a minute. Seconds are not possible with this API.
//!
//! **One phase end per tick, never a burst.** A tick ends at most one
//! phase; time left over is not carried into the next one. A pause between
//! ticks of `awayMinutes` or more (the computer slept, DeckPilot was locked
//! or not running) is not counted at all: a running timer is paused where
//! it was, rather than several phases finishing unseen. A clock that went
//! backwards takes nothing off.
//!
//! **Key state** (plugin API 1.2). [`Snapshot`] is the one place that
//! turns the stored state into what a user sees: the outputs, the
//! variables and, through [`Snapshot::key`] and [`key`], a key bound to
//! `status`, which shows the minutes left as of the last tick and marks a
//! running countdown whose ticks stopped.
#![deny(unsafe_code)]

pub mod key;

use deckpilot_plugin_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Must equal `manifest.json` (`id`, `version`).
pub const ID: &str = "deckpilot.pomodoro";
pub const VERSION: &str = "0.2.0";

/// The storage key of the whole state: one value, written once per action.
pub const STATE_KEY: &str = "state";
/// The version of the stored state. A value of another version, or one
/// that does not parse, is replaced by a fresh state.
pub const STATE_VERSION: u32 = 1;

/// Error codes of this plugin, each declared in `contributes.errorCodes`.
pub const INVALID_FIRED_AT: &str = "invalid-fired-at";
pub const STORAGE_UNAVAILABLE: &str = "storage-unavailable";
pub const PLUGIN_CODES: &[&str] = &[INVALID_FIRED_AT, STORAGE_UNAVAILABLE];

/// The plugin's own variables (`{{ plugins.deckpilot.pomodoro.<id> }}`).
/// The first four are published by every action, the statistics by `tick`,
/// `status` and `reset`: at most 10 variables a second may be published.
pub const TIMER_VARIABLES: [&str; 4] = ["state", "phase", "remaining", "session"];
pub const STATS_VARIABLES: [&str; 4] = [
    "completedToday",
    "focusMinutesToday",
    "completedTotal",
    "focusMinutesTotal",
];

/// Bounds and defaults of the settings, as in `manifest.json`.
pub const FOCUS_MINUTES: (u32, u32, u32) = (1, 180, 25);
pub const SHORT_BREAK_MINUTES: (u32, u32, u32) = (1, 60, 5);
pub const LONG_BREAK_MINUTES: (u32, u32, u32) = (1, 120, 15);
pub const SESSIONS: (u32, u32, u32) = (1, 12, 4);
pub const AWAY_MINUTES: (u32, u32, u32) = (3, 120, 5);
/// The longest phase there can be, in minutes.
pub const MAX_PHASE: u32 = FOCUS_MINUTES.1;
/// A notification that could not be sent is tried with the following ticks
/// for this many minutes, then dropped: later it would only confuse.
pub const MAX_NOTICE_AGE: u32 = 5;

const SCOPES: [&str; 4] = ["phase", "cycle", "statistics", "all"];

/// The plugin definition. The action ids, their order and their parameters
/// must equal `manifest.json`; a unit test compares them.
pub fn plugin() -> Plugin {
    Plugin::new(ID, VERSION)
        .requires_api(deckpilot_plugin_sdk::ApiLevel { major: 1, minor: 2 })
        .action(Action::new("tick", tick).param(Param::text("firedAt").required().max_length(32)))
        .action(Action::new("start", start))
        .action(Action::new("pause", pause))
        .action(Action::new("resume", resume))
        .action(Action::new("toggle", toggle))
        .action(Action::new("skip", skip))
        .action(Action::new("reset", reset).param(Param::choice("scope", &SCOPES).default("phase")))
        .action(Action::new("status", status).state(key::status_state))
}

deckpilot_plugin_sdk::plugin!(plugin);

// ---------------------------------------------------------------------
// Time: the trigger's `firedAt`, the only clock there is.
// ---------------------------------------------------------------------

/// A local wall-clock minute: minutes since 1970-01-01 00:00 of the
/// computer's own time zone. Only differences and the date are used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Stamp(pub i64);

impl Stamp {
    /// The local date as a day number.
    pub fn day(self) -> i64 {
        self.0.div_euclid(1440)
    }

    /// `YYYY-MM-DD HH:MM`, the form of `firedAt`.
    pub fn text(self) -> String {
        let minute = self.0.rem_euclid(1440);
        format!(
            "{} {:02}:{:02}",
            date_text(self.day()),
            minute / 60,
            minute % 60
        )
    }
}

/// `YYYY-MM-DD` of a day number.
pub fn date_text(day: i64) -> String {
    let (year, month, day) = civil_from_days(day);
    format!("{year:04}-{month:02}-{day:02}")
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
/// a `T` instead of the space and trailing seconds (`:SS`, ignored) are
/// accepted, so a time written by hand or by another tool works too.
/// `None` for everything else, including dates that do not exist.
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

// ---------------------------------------------------------------------
// Preferences: the plugin's settings, read on every invocation.
// ---------------------------------------------------------------------

/// The language of the notifications and of what a key would show. The
/// plugin cannot learn the app's or the system's language, so it is a
/// setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    English,
    Polish,
}

/// The settings as the plugin uses them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub focus_minutes: u32,
    pub short_break_minutes: u32,
    pub long_break_minutes: u32,
    /// The break after this many focus sessions is the long one.
    pub sessions: u32,
    /// A break starts by itself when a focus session ends.
    pub auto_start_breaks: bool,
    /// A focus session starts by itself when a break ends.
    pub auto_start_focus: bool,
    /// A pause between two ticks of at least this many minutes is not
    /// counted and pauses a running timer.
    pub away_minutes: u32,
    pub language: Language,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            focus_minutes: FOCUS_MINUTES.2,
            short_break_minutes: SHORT_BREAK_MINUTES.2,
            long_break_minutes: LONG_BREAK_MINUTES.2,
            sessions: SESSIONS.2,
            auto_start_breaks: true,
            auto_start_focus: false,
            away_minutes: AWAY_MINUTES.2,
            language: Language::English,
        }
    }
}

impl Config {
    /// From the settings object the agent returns (`settings.get`). The
    /// agent validates what the user saves and fills in defaults; a value
    /// that is absent or of another type still falls back to the default,
    /// and numbers are clamped to the manifest's bounds. No setting is
    /// free text, so no setting can be wrong.
    pub fn from_settings(settings: &serde_json::Map<String, Value>) -> Self {
        let flag = |key: &str, default: bool| {
            settings
                .get(key)
                .and_then(Value::as_bool)
                .unwrap_or(default)
        };
        let number = |key: &str, (min, max, default): (u32, u32, u32)| {
            settings
                .get(key)
                .and_then(Value::as_i64)
                .map_or(default, |v| {
                    u32::try_from(v.clamp(i64::from(min), i64::from(max))).unwrap_or(default)
                })
        };
        let default = Self::default();
        Self {
            focus_minutes: number("focusMinutes", FOCUS_MINUTES),
            short_break_minutes: number("shortBreakMinutes", SHORT_BREAK_MINUTES),
            long_break_minutes: number("longBreakMinutes", LONG_BREAK_MINUTES),
            sessions: number("sessionsBeforeLongBreak", SESSIONS),
            auto_start_breaks: flag("autoStartBreaks", default.auto_start_breaks),
            auto_start_focus: flag("autoStartFocus", default.auto_start_focus),
            away_minutes: number("awayMinutes", AWAY_MINUTES),
            language: match settings.get("language").and_then(Value::as_str) {
                Some("pl") => Language::Polish,
                _ => Language::English,
            },
        }
    }

    /// The full length of a phase.
    pub fn minutes(&self, phase: Phase) -> u32 {
        match phase {
            Phase::Focus => self.focus_minutes,
            Phase::ShortBreak => self.short_break_minutes,
            Phase::LongBreak => self.long_break_minutes,
        }
    }

    /// Whether `phase` starts by itself when the phase before it ends.
    fn auto_start(&self, phase: Phase) -> bool {
        match phase {
            Phase::Focus => self.auto_start_focus,
            Phase::ShortBreak | Phase::LongBreak => self.auto_start_breaks,
        }
    }
}

// ---------------------------------------------------------------------
// State: what is stored, and the pure functions that change it.
// ---------------------------------------------------------------------

/// What the timer counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    #[default]
    Focus,
    ShortBreak,
    LongBreak,
}

impl Phase {
    /// `focus`, `shortBreak` or `longBreak`: the `phase` output and variable.
    pub fn name(self) -> &'static str {
        match self {
            Self::Focus => "focus",
            Self::ShortBreak => "shortBreak",
            Self::LongBreak => "longBreak",
        }
    }

    /// For messages: `focus session`, `short break`, `long break`.
    fn words(self) -> &'static str {
        match self {
            Self::Focus => "focus session",
            Self::ShortBreak => "short break",
            Self::LongBreak => "long break",
        }
    }
}

/// Whether the countdown of the phase runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Timer {
    /// The phase has not started: its countdown is full.
    #[default]
    Idle,
    Running,
    Paused,
}

impl Timer {
    /// `idle`, `running` or `paused`: the `state` output and variable.
    pub fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "running",
            Self::Paused => "paused",
        }
    }
}

/// What a notification is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Notice {
    /// A focus session ended; the state's phase is the break after it.
    FocusEnded,
    /// A break ended; the state's phase is the focus session after it.
    BreakEnded,
    /// A long pause between ticks paused the running timer.
    Away,
}

/// A notification that is to be sent with this tick, or that could not be
/// sent and waits for the next one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    pub notice: Notice,
    /// Minutes since it became due, counted by the ticks.
    #[serde(default)]
    pub age: u32,
}

/// Completed focus sessions. Only a session whose countdown ran out counts;
/// a skipped or reset one does not, and neither do its minutes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Stats {
    /// The local date (a day number) that `sessions_today` and
    /// `minutes_today` are of: the date of the last tick.
    pub day: Option<i64>,
    pub sessions_today: u32,
    pub minutes_today: u32,
    /// Totals since the plugin was installed or the statistics were reset.
    pub sessions_total: u64,
    pub minutes_total: u64,
}

/// Everything the plugin stores, as one JSON value under [`STATE_KEY`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct State {
    pub version: u32,
    /// The `firedAt` of the last tick, as [`Stamp`] minutes.
    pub last_tick: Option<i64>,
    pub phase: Phase,
    pub timer: Timer,
    /// Paused by a long pause between ticks, not by the user.
    pub away: bool,
    /// The minutes the phase began with: what a completed focus session
    /// adds to the statistics, and the whole of the progress bar.
    pub length: u32,
    /// Minutes left of the phase.
    pub left: u32,
    /// The number of the focus session in the cycle, from 1: the one that
    /// runs, the one the break follows, or the next one.
    pub session: u32,
    pub pending: Option<Pending>,
    pub stats: Stats,
}

impl State {
    pub fn new() -> Self {
        Self {
            version: STATE_VERSION,
            session: 1,
            ..Self::default()
        }
    }

    /// The stored value as a state. Anything that is not a state of this
    /// version (never written, damaged, written by a newer version before
    /// a rollback) is a fresh state: the timer starts again rather than
    /// fail on every tick. The flag says whether a stored value was
    /// dropped. Numbers are bounded, whatever was stored.
    pub fn from_stored(value: Option<Value>) -> (Self, bool) {
        let Some(value) = value else {
            return (Self::new(), false);
        };
        match serde_json::from_value::<Self>(value) {
            Ok(mut state) if state.version == STATE_VERSION => {
                state.length = state.length.min(MAX_PHASE);
                state.left = state.left.min(MAX_PHASE);
                state.session = state.session.clamp(1, SESSIONS.1);
                (state, false)
            }
            _ => (Self::new(), true),
        }
    }

    /// Brings the state in line with the settings. A phase that has not
    /// started takes its length from the settings at once; one that runs or
    /// is paused keeps the length it began with, and the changed setting
    /// applies from the next phase. Every action does this first.
    pub fn sync(&mut self, config: &Config) {
        self.session = self.session.clamp(1, config.sessions);
        if self.timer == Timer::Idle || self.length == 0 {
            self.length = config.minutes(self.phase);
            if self.timer == Timer::Idle {
                self.left = self.length;
            }
        }
        self.left = self.left.min(self.length);
        if self.timer != Timer::Paused {
            self.away = false;
        }
    }

    /// Makes `phase` the current one, with a full countdown.
    fn begin(&mut self, config: &Config, phase: Phase, timer: Timer) {
        self.phase = phase;
        self.timer = timer;
        self.away = false;
        self.length = config.minutes(phase);
        self.left = self.length;
    }

    /// The phase after the current one: the break after a focus session
    /// (the long one after the last session of a cycle), the next focus
    /// session after a break.
    pub fn following(&self, config: &Config) -> Phase {
        match self.phase {
            Phase::Focus if self.session >= config.sessions => Phase::LongBreak,
            Phase::Focus => Phase::ShortBreak,
            Phase::ShortBreak | Phase::LongBreak => Phase::Focus,
        }
    }

    /// Ends the current phase and makes the following one current. Returns
    /// the phase that ended.
    fn next_phase(&mut self, config: &Config, timer: Timer) -> Phase {
        let ended = self.phase;
        let next = self.following(config);
        match ended {
            Phase::Focus => {}
            Phase::ShortBreak => self.session = (self.session + 1).min(config.sessions),
            Phase::LongBreak => self.session = 1,
        }
        self.begin(config, next, timer);
        ended
    }

    /// Starts the phase, or continues a paused one. `false`: it was
    /// running already.
    pub fn start(&mut self) -> bool {
        if self.timer == Timer::Running {
            return false;
        }
        self.timer = Timer::Running;
        self.away = false;
        // The user is back: a waiting "paused" notification is pointless.
        if self.pending.is_some_and(|p| p.notice == Notice::Away) {
            self.pending = None;
        }
        true
    }

    /// Pauses a running countdown. `false`: it was not running.
    pub fn pause(&mut self) -> bool {
        if self.timer != Timer::Running {
            return false;
        }
        self.timer = Timer::Paused;
        self.away = false;
        true
    }

    /// Continues a paused countdown. `false`: it was not paused.
    pub fn resume(&mut self) -> bool {
        self.timer == Timer::Paused && self.start()
    }

    /// Ends the current phase now and goes on to the next one, which runs
    /// if the timer was running. A skipped focus session is not counted in
    /// the statistics; the cycle goes on as if it had ended. Returns the
    /// phase that was skipped.
    pub fn skip(&mut self, config: &Config) -> Phase {
        let timer = match self.timer {
            Timer::Running => Timer::Running,
            Timer::Idle | Timer::Paused => Timer::Idle,
        };
        self.pending = None;
        self.next_phase(config, timer)
    }

    /// Stops the timer and puts the current phase back to its full length.
    pub fn reset_phase(&mut self, config: &Config) {
        self.pending = None;
        self.begin(config, self.phase, Timer::Idle);
    }

    /// Stops the timer and goes back to the first focus session.
    pub fn reset_cycle(&mut self, config: &Config) {
        self.pending = None;
        self.session = 1;
        self.begin(config, Phase::Focus, Timer::Idle);
    }

    /// Forgets the statistics; the date of "today" stays.
    pub fn reset_statistics(&mut self) {
        self.stats = Stats {
            day: self.stats.day,
            ..Stats::default()
        };
    }

    /// One tick at `now`. Takes the time since the last tick off a running
    /// countdown and ends the phase when nothing is left. What is to be
    /// shown is left in [`State::pending`] for the caller to send.
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
        self.last_tick = Some(now.0);
        // "Today" is the date of the tick. Another date, later or (after
        // the clock was set back across midnight) earlier, starts the
        // counters of the day again; the totals are not touched.
        if self.stats.day != Some(now.day()) {
            self.stats.day = Some(now.day());
            self.stats.sessions_today = 0;
            self.stats.minutes_today = 0;
        }
        let mut expired = false;
        if let Some(pending) = &mut self.pending {
            pending.age = pending.age.saturating_add(elapsed);
            if pending.age > MAX_NOTICE_AGE {
                self.pending = None;
                expired = true;
            }
        }

        let mut ended = None;
        if self.timer == Timer::Running {
            if elapsed >= config.away_minutes.max(1) {
                // Nobody saw that time pass. Counting it could finish
                // several phases unseen, so none of it is counted and the
                // timer waits, paused, for the user.
                self.timer = Timer::Paused;
                self.away = true;
                self.pending = Some(Pending {
                    notice: Notice::Away,
                    age: 0,
                });
                note = Note::Away;
            } else {
                self.left = self.left.saturating_sub(elapsed);
                if self.left == 0 {
                    ended = Some(self.complete(config));
                }
            }
        }
        Advance {
            note,
            ended,
            expired,
        }
    }

    /// The countdown of the current phase ran out.
    fn complete(&mut self, config: &Config) -> Phase {
        if self.phase == Phase::Focus {
            let stats = &mut self.stats;
            stats.sessions_today = stats.sessions_today.saturating_add(1);
            stats.minutes_today = stats.minutes_today.saturating_add(self.length);
            stats.sessions_total = stats.sessions_total.saturating_add(1);
            stats.minutes_total = stats.minutes_total.saturating_add(u64::from(self.length));
        }
        let timer = if config.auto_start(self.following(config)) {
            Timer::Running
        } else {
            Timer::Idle
        };
        let ended = self.next_phase(config, timer);
        self.pending = Some(Pending {
            notice: match ended {
                Phase::Focus => Notice::FocusEnded,
                Phase::ShortBreak | Phase::LongBreak => Notice::BreakEnded,
            },
            age: 0,
        });
        ended
    }
}

/// What a tick found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Advance {
    pub note: Note,
    /// The phase whose countdown ran out with this tick.
    pub ended: Option<Phase>,
    /// A notification that could not be sent for [`MAX_NOTICE_AGE`]
    /// minutes was dropped.
    pub expired: bool,
}

/// How the tick's time related to the previous tick's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Note {
    /// The first tick ever.
    First,
    /// Later than the last tick; the minutes were taken off a running
    /// countdown.
    Counted,
    /// The same minute again (a retry, a second workflow): nothing counted.
    SameMinute,
    /// Earlier than the last tick (the clock was set back, or daylight
    /// saving time ended): nothing counted, the countdown goes on from here.
    ClockWentBack,
    /// Later by the away time or more while the timer ran: nothing
    /// counted, the timer was paused.
    Away,
}

/// What the user sees of the state: the outputs of every action and the
/// plugin's variables today, and, through [`Snapshot::key`], what a key
/// shows ([`key`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub phase: Phase,
    pub timer: Timer,
    /// Paused by a long pause between ticks.
    pub away: bool,
    /// No tick was ever received: the workflow is not set up or has not
    /// fired yet, and nothing counts down.
    pub waiting: bool,
    /// Minutes left of the phase.
    pub remaining: u32,
    /// Minutes the phase began with.
    pub length: u32,
    /// The focus session in the cycle, from 1, and the sessions of a cycle.
    pub session: u32,
    pub sessions: u32,
    pub stats: Stats,
    pub language: Language,
}

/// What a key would show: the members of DeckPilot's `ButtonState`, which
/// the proposed key-state call for plugins returns
/// (`docs/plugin-factory.md`, 6.1), returned from `status`'s state call by
/// [`key`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyFace {
    /// The title: the minutes left, at most 32 characters.
    pub label: String,
    /// The second line: the phase, at most 32 characters.
    pub detail: String,
    /// `running`, `warning` (paused) or `unknown` (not started).
    pub indicator: &'static str,
    /// How much of the phase is over, 0-100.
    pub progress: u8,
    /// Whether the countdown runs.
    pub toggle: bool,
}

impl Snapshot {
    /// Of a state that [`State::sync`] brought in line with `config`.
    pub fn of(state: &State, config: &Config) -> Self {
        Self {
            phase: state.phase,
            timer: state.timer,
            away: state.away && state.timer == Timer::Paused,
            waiting: state.last_tick.is_none(),
            remaining: state.left,
            length: state.length,
            session: state.session,
            sessions: config.sessions,
            stats: state.stats,
            language: config.language,
        }
    }

    /// How much of the phase is over, 0-100.
    pub fn progress(&self) -> u8 {
        if self.length == 0 {
            return 0;
        }
        let done = self.length - self.remaining.min(self.length);
        u8::try_from(u64::from(done) * 100 / u64::from(self.length)).unwrap_or(100)
    }

    /// What a key would show, in the language of the settings.
    pub fn key(&self) -> KeyFace {
        let polish = self.language == Language::Polish;
        let phase = match (self.phase, polish) {
            (Phase::Focus, false) => format!("Focus {}/{}", self.session, self.sessions),
            (Phase::Focus, true) => format!("Skupienie {}/{}", self.session, self.sessions),
            (Phase::ShortBreak, false) => "Short break".to_owned(),
            (Phase::ShortBreak, true) => "Krótka przerwa".to_owned(),
            (Phase::LongBreak, false) => "Long break".to_owned(),
            (Phase::LongBreak, true) => "Długa przerwa".to_owned(),
        };
        let detail = match (self.timer, polish) {
            (Timer::Running, _) => phase,
            (Timer::Paused, false) => format!("{phase} · paused"),
            (Timer::Paused, true) => format!("{phase} · pauza"),
            (Timer::Idle, false) => format!("{phase} · ready"),
            (Timer::Idle, true) => format!("{phase} · gotowe"),
        };
        KeyFace {
            label: format!("{} min", self.remaining),
            detail,
            indicator: match self.timer {
                Timer::Running => "running",
                Timer::Paused => "warning",
                Timer::Idle => "unknown",
            },
            progress: self.progress(),
            toggle: self.timer == Timer::Running,
        }
    }

    /// `Focus 2 of 4`, `Short break`, `Long break`.
    fn phase_words(&self) -> String {
        match self.phase {
            Phase::Focus => format!("Focus {} of {}", self.session, self.sessions),
            Phase::ShortBreak => "Short break".to_owned(),
            Phase::LongBreak => "Long break".to_owned(),
        }
    }

    /// One line for the action's message.
    pub fn summary(&self) -> String {
        let phase = self.phase_words();
        let minutes = self.remaining;
        let text = match (self.timer, self.away) {
            (Timer::Running, _) => format!("{phase}: {minutes} min left"),
            (Timer::Paused, true) => {
                format!("{phase} paused after a long pause between ticks: {minutes} min left")
            }
            (Timer::Paused, false) => format!("{phase} paused: {minutes} min left"),
            (Timer::Idle, _) => format!("{phase} not started: {minutes} min"),
        };
        if self.waiting {
            format!(
                "{text} (no tick received yet: add the every-minute workflow from the plugin's README)"
            )
        } else {
            text
        }
    }
}

/// The title and text of a notification, for the state after the tick
/// that made it due.
pub fn notification(notice: Notice, state: &State, config: &Config) -> (String, String) {
    let polish = config.language == Language::Polish;
    let started = state.timer == Timer::Running;
    let (minutes, session, sessions) = (state.length, state.session, config.sessions);
    let long = state.phase == Phase::LongBreak;
    let (title, body) = match (notice, polish) {
        (Notice::FocusEnded, false) => {
            let kind = if long { "long" } else { "short" };
            (
                "Focus session finished",
                if started {
                    format!("Time for a {kind} break: {minutes} min.")
                } else {
                    format!("Next is a {kind} break of {minutes} min. Start it when you are ready.")
                },
            )
        }
        (Notice::FocusEnded, true) => {
            let (kind, next) = if long {
                ("długą przerwę", "długa przerwa")
            } else {
                ("krótką przerwę", "krótka przerwa")
            };
            (
                "Sesja skupienia zakończona",
                if started {
                    format!("Czas na {kind}: {minutes} min.")
                } else {
                    format!("Następna jest {next} ({minutes} min). Uruchom ją, gdy zechcesz.")
                },
            )
        }
        (Notice::BreakEnded, false) => (
            "Break is over",
            if started {
                format!("Focus session {session} of {sessions} has started: {minutes} min.")
            } else {
                format!(
                    "Start focus session {session} of {sessions} ({minutes} min) when you are ready."
                )
            },
        ),
        (Notice::BreakEnded, true) => (
            "Koniec przerwy",
            if started {
                format!("Zaczęła się sesja skupienia {session} z {sessions}: {minutes} min.")
            } else {
                format!(
                    "Uruchom sesję skupienia {session} z {sessions} ({minutes} min), gdy zechcesz."
                )
            },
        ),
        (Notice::Away, false) => (
            "Pomodoro paused",
            format!(
                "The timer was not counted for a while (the computer slept or DeckPilot was locked), \
                 so it is paused with {} min left. Resume it when you are ready.",
                state.left
            ),
        ),
        (Notice::Away, true) => (
            "Pomodoro wstrzymane",
            format!(
                "Przez pewien czas nic nie odliczało czasu (komputer spał albo DeckPilot był \
                 zablokowany), więc odliczanie jest wstrzymane; zostało {} min. Wznów je, gdy zechcesz.",
                state.left
            ),
        ),
    };
    (title.to_owned(), body)
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
            "the plugin's storage is full; the timer state was not saved",
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
    let config = Config::from_settings(settings.as_map());
    let stored = ctx
        .storage_get(STATE_KEY)
        .map_err(|e| storage_error(e, "read"))?;
    let (mut state, dropped) = State::from_stored(stored);
    if dropped {
        ctx.warn(
            "the stored timer state could not be used; the timer and its statistics start again",
        );
    }
    state.sync(&config);
    Ok((config, state))
}

fn save(ctx: &mut Context<'_>, state: &State) -> Result<(), PluginError> {
    ctx.storage_set(STATE_KEY, state)
        .map_err(|e| storage_error(e, "written"))
}

fn integer(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// The outputs every action has, and the plugin's variables. Variables
/// live in the agent's memory only, so they are published again and again:
/// the timer's four by every action, the statistics too when `statistics`
/// is set (`tick`, `status`, `reset`). One that cannot be published (the
/// limit is 10 a second) is not worth failing the action for; the next
/// tick publishes it.
fn report(ctx: &mut Context<'_>, snapshot: &Snapshot, message: String, statistics: bool) -> Output {
    let stats = &snapshot.stats;
    let numbers = [
        ("remaining", i64::from(snapshot.remaining)),
        ("session", i64::from(snapshot.session)),
        ("completedToday", i64::from(stats.sessions_today)),
        ("focusMinutesToday", i64::from(stats.minutes_today)),
        ("completedTotal", integer(stats.sessions_total)),
        ("focusMinutesTotal", integer(stats.minutes_total)),
    ];
    let mut failed = ctx
        .publish_variable("state", snapshot.timer.name())
        .is_err()
        | ctx
            .publish_variable("phase", snapshot.phase.name())
            .is_err();
    for (id, value) in numbers {
        if statistics || TIMER_VARIABLES.contains(&id) {
            failed |= ctx.publish_variable(id, value).is_err();
        }
    }
    if failed {
        ctx.debug("some of the plugin's variables could not be published");
    }
    let mut output = Output::message(message)
        .with("state", snapshot.timer.name())
        .with("phase", snapshot.phase.name());
    for (key, value) in numbers {
        output = output.with(key, value);
    }
    output
}

/// How a notification ended.
enum Sent {
    /// Shown, or accepted by an agent that does not say where.
    Shown,
    /// Accepted, and shown nowhere: the user's settings say so.
    NotShown(String),
    /// Refused; it is tried again with the next tick.
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
        Note::ClockWentBack => ctx.info("the clock went back; the countdown goes on from here"),
        Note::Away => {
            ctx.info("a long pause between ticks was not counted; the timer is paused where it was")
        }
        _ => {}
    }
    if advance.expired {
        ctx.warn("a notification could not be sent for several minutes and was dropped");
    }

    // Saved as sent before sending: if the storage fails, nothing was
    // shown and nothing repeats; if the notification is refused, it is put
    // back to be tried with the next tick. The countdown and the
    // statistics are saved either way.
    let notice = state.pending.take();
    save(ctx, &state)?;

    let mut notified = false;
    let mut reason = None;
    if let Some(pending) = notice {
        let (title, body) = notification(pending.notice, &state, &config);
        match send(ctx, &title, &body) {
            Sent::Shown => notified = true,
            Sent::NotShown(why) => reason = Some(why),
            Sent::Refused(error) => {
                state.pending = Some(pending);
                save(ctx, &state)?;
                if error.is_denied() {
                    return Err(error.into());
                }
                ctx.warn(format!(
                    "the notification was not sent and is tried again: {error}"
                ));
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
    let mut message = match advance.ended {
        Some(ended) => format!("The {} ended. {}", ended.words(), snapshot.summary()),
        None => snapshot.summary(),
    };
    if let Some(why) = &reason {
        message = format!("{message}. The notification was not shown: {why}");
    }
    let mut output = report(ctx, &snapshot, message, true)
        .with("ended", advance.ended.map_or("none", Phase::name))
        .with("notified", notified)
        .with("away", snapshot.away);
    if let Some(reason) = reason {
        output = output.with("reason", reason);
    }
    Ok(output)
}

/// Saves a state a key changed (`changed`), or leaves the stored one alone,
/// and reports it either way.
fn changed_by_key(
    ctx: &mut Context<'_>,
    config: &Config,
    state: &State,
    changed: bool,
    unchanged: &str,
) -> ActionResult {
    if changed {
        save(ctx, state)?;
    }
    let snapshot = Snapshot::of(state, config);
    let message = if changed {
        snapshot.summary()
    } else {
        format!("{unchanged}. {}", snapshot.summary())
    };
    Ok(report(ctx, &snapshot, message, false).with("changed", changed))
}

fn start(ctx: &mut Context<'_>, _args: &Args<'_>) -> ActionResult {
    let (config, mut state) = load(ctx)?;
    let changed = state.start();
    changed_by_key(ctx, &config, &state, changed, "Already running")
}

fn pause(ctx: &mut Context<'_>, _args: &Args<'_>) -> ActionResult {
    let (config, mut state) = load(ctx)?;
    let changed = state.pause();
    changed_by_key(
        ctx,
        &config,
        &state,
        changed,
        "Not running, nothing to pause",
    )
}

fn resume(ctx: &mut Context<'_>, _args: &Args<'_>) -> ActionResult {
    let (config, mut state) = load(ctx)?;
    let changed = state.resume();
    changed_by_key(
        ctx,
        &config,
        &state,
        changed,
        "Not paused, nothing to resume",
    )
}

/// One key for everything: starts, pauses, resumes.
fn toggle(ctx: &mut Context<'_>, _args: &Args<'_>) -> ActionResult {
    let (config, mut state) = load(ctx)?;
    let changed = match state.timer {
        Timer::Running => state.pause(),
        Timer::Idle | Timer::Paused => state.start(),
    };
    changed_by_key(ctx, &config, &state, changed, "Nothing changed")
}

fn skip(ctx: &mut Context<'_>, _args: &Args<'_>) -> ActionResult {
    let (config, mut state) = load(ctx)?;
    let skipped = state.skip(&config);
    save(ctx, &state)?;
    let snapshot = Snapshot::of(&state, &config);
    let message = format!("Skipped the {}. {}", skipped.words(), snapshot.summary());
    Ok(report(ctx, &snapshot, message, false).with("skipped", skipped.name()))
}

fn reset(ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let scope: String = args.get("scope")?;
    let (config, mut state) = load(ctx)?;
    let what = match scope.as_str() {
        "cycle" => {
            state.reset_cycle(&config);
            "cycle"
        }
        "statistics" => {
            state.reset_statistics();
            "statistics"
        }
        "all" => {
            state.reset_cycle(&config);
            state.reset_statistics();
            "cycle and the statistics"
        }
        _ => {
            state.reset_phase(&config);
            "phase"
        }
    };
    save(ctx, &state)?;
    let snapshot = Snapshot::of(&state, &config);
    let message = format!("Reset the {what}. {}", snapshot.summary());
    Ok(report(ctx, &snapshot, message, true))
}

/// Reads and reports; stores nothing.
fn status(ctx: &mut Context<'_>, _args: &Args<'_>) -> ActionResult {
    let (config, state) = load(ctx)?;
    let snapshot = Snapshot::of(&state, &config);
    let key = snapshot.key();
    let message = snapshot.summary();
    let mut output = report(ctx, &snapshot, message, true)
        .with("away", snapshot.away)
        .with("sessionsPerCycle", i64::from(snapshot.sessions))
        .with("phaseMinutes", i64::from(snapshot.length))
        .with("progress", i64::from(key.progress))
        .with("keyLabel", key.label)
        .with("keyDetail", key.detail);
    if let Some(day) = snapshot.stats.day {
        output = output.with("statisticsDate", date_text(day));
    }
    if let Some(last) = state.last_tick {
        output = output.with("lastTick", Stamp(last).text());
    }
    Ok(output)
}

#[cfg(test)]
mod tests;
