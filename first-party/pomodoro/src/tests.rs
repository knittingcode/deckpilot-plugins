//! Native unit tests: the timer as pure functions, and every action with
//! the agent replaced by a scripted `MockHost`. `tools/first-party.sh` adds
//! what a unit test cannot: the manifest against the schema, the Polish
//! texts, the wasm build and the package checks.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use deckpilot_plugin_sdk::testing::{MockHost, manifest_mismatches};
use deckpilot_plugin_sdk::{
    ActionOutcome, CallErrorCode, CallResult, CapabilityRequest, ParamValue,
};
use serde_json::{Value, json};

use super::*;

const MANIFEST: &str = include_str!("../manifest.json");

fn manifest() -> Value {
    serde_json::from_str(MANIFEST).expect("manifest.json is JSON")
}

fn at(text: &str) -> Stamp {
    parse_fired_at(text).unwrap_or_else(|| panic!("{text} is a time"))
}

fn config(settings: Value) -> Config {
    Config::from_settings(settings.as_object().expect("an object"))
}

/// 25, 5 and 15 minutes, 4 sessions, away after 5, and every phase starts
/// by itself: the timer runs through its cycle without a key.
fn automatic() -> Config {
    Config {
        auto_start_focus: true,
        ..Config::default()
    }
}

// ----------------------------------------------------------------- time

#[test]
fn fired_at_is_parsed_as_the_agent_writes_it() {
    let a = at("2026-11-16 09:30");
    assert_eq!(a.text(), "2026-11-16 09:30");
    assert_eq!(at("2026-11-16 09:31").0 - a.0, 1);
    // Midnight, month and year ends are one minute apart.
    assert_eq!(at("2026-11-17 00:00").0 - at("2026-11-16 23:59").0, 1);
    assert_eq!(at("2026-12-01 00:00").0 - at("2026-11-30 23:59").0, 1);
    assert_eq!(at("2027-01-01 00:00").0 - at("2026-12-31 23:59").0, 1);
    assert_eq!(
        at("2026-11-17 00:00").day() - at("2026-11-16 23:59").day(),
        1
    );
    // Leap days exist only in leap years.
    assert_eq!(
        at("2028-03-01 00:00").0 - at("2028-02-28 00:00").0,
        2 * 1440
    );
    assert_eq!(at("2027-03-01 00:00").0 - at("2027-02-28 00:00").0, 1440);
    assert_eq!(at("1970-01-01 00:00"), Stamp(0));
    for text in ["2000-02-29 12:00", "2026-01-31 23:59", "9999-12-31 23:59"] {
        assert_eq!(at(text).text(), text);
        assert_eq!(date_text(at(text).day()), text[..10]);
    }
}

#[test]
fn fired_at_tolerates_what_a_person_or_another_tool_writes() {
    let expected = at("2026-11-16 09:30");
    for text in [
        " 2026-11-16 09:30 ",
        "2026-11-16T09:30",
        // Seconds are accepted and ignored: the plugin counts minutes.
        "2026-11-16 09:30:59",
        "2026-11-16T09:30:00\n",
    ] {
        assert_eq!(parse_fired_at(text), Some(expected), "{text:?}");
    }
}

#[test]
fn malformed_fired_at_is_refused() {
    for text in [
        "",
        " ",
        "now",
        "09:30",
        "2026-11-16",
        "2026-11-16 9:30",
        "2026-11-16 24:00",
        "2026-11-16 09:60",
        "2026-11-16 09:30:60",
        "2026-11-16 09:30:00:00",
        "2026-13-01 09:30",
        "2026-00-10 09:30",
        "2026-11-31 09:30",
        "2026-11-00 09:30",
        "2027-02-29 09:30",
        "1900-02-28 09:30",
        "2026/11/16 09:30",
        "2026-11-16_09:30",
        "16-11-2026 09:30",
        "2026-11-16 09-30",
        "+026-11-16 09:30",
        "2026-11-16 -9:30",
        "2026-11-16 09:30Z",
        "2026-11-16 09:30 +01:00",
        "{{ steps.start.output.firedAt }}",
        "２０２６-11-16 09:30",
        "2026-11-16 09:30\u{0}",
    ] {
        assert_eq!(parse_fired_at(text), None, "{text:?}");
    }
}

// ------------------------------------------------------------- settings

#[test]
fn settings_have_defaults_and_bounds() {
    assert_eq!(config(json!({})), Config::default());
    let d = Config::default();
    assert_eq!(
        (
            d.focus_minutes,
            d.short_break_minutes,
            d.long_break_minutes,
            d.sessions
        ),
        (25, 5, 15, 4)
    );
    assert!(d.auto_start_breaks && !d.auto_start_focus);
    let c = config(json!({
        "focusMinutes": 50, "shortBreakMinutes": 10, "longBreakMinutes": 30,
        "sessionsBeforeLongBreak": 2, "autoStartBreaks": false, "autoStartFocus": true,
        "awayMinutes": 15, "language": "pl"
    }));
    assert_eq!(
        c,
        Config {
            focus_minutes: 50,
            short_break_minutes: 10,
            long_break_minutes: 30,
            sessions: 2,
            auto_start_breaks: false,
            auto_start_focus: true,
            away_minutes: 15,
            language: Language::Polish,
        }
    );
    assert_eq!(
        (
            c.minutes(Phase::Focus),
            c.minutes(Phase::ShortBreak),
            c.minutes(Phase::LongBreak)
        ),
        (50, 10, 30)
    );
    // The agent validates saved values; values out of range or of another
    // type still cannot make a phase of 0 minutes or of a year.
    let c = config(json!({
        "focusMinutes": 0, "shortBreakMinutes": 100_000, "longBreakMinutes": -3,
        "sessionsBeforeLongBreak": 0, "awayMinutes": -4, "language": 7,
        "autoStartBreaks": "yes", "autoStartFocus": 1
    }));
    assert_eq!(c.focus_minutes, FOCUS_MINUTES.0);
    assert_eq!(c.short_break_minutes, SHORT_BREAK_MINUTES.1);
    assert_eq!(c.long_break_minutes, LONG_BREAK_MINUTES.0);
    assert_eq!(c.sessions, SESSIONS.0);
    assert_eq!(c.away_minutes, AWAY_MINUTES.0);
    assert_eq!(c.language, Language::English);
    assert!(c.auto_start_breaks && !c.auto_start_focus);
}

// ------------------------------------------------------------ the timer

/// Ticks every minute from `from` for `minutes` minutes (the first tick is
/// one minute after `from`); what is to be shown counts as sent. Returns
/// the minutes after `from` at which a phase ended, and which.
fn run(state: &mut State, config: &Config, from: Stamp, minutes: i64) -> Vec<(i64, &'static str)> {
    let mut ended = Vec::new();
    for minute in 1..=minutes {
        let advance = state.advance(config, Stamp(from.0 + minute));
        state.pending = None;
        if let Some(phase) = advance.ended {
            ended.push((minute, phase.name()));
        }
    }
    ended
}

/// A state whose first tick was at `start`, not started.
fn ticked(config: &Config, start: Stamp) -> State {
    let mut state = State::new();
    let advance = state.advance(config, start);
    assert_eq!(advance.note, Note::First);
    assert_eq!(advance.ended, None);
    assert_eq!(state.timer, Timer::Idle, "a tick starts nothing");
    state
}

/// The same, with the first focus session started by a key.
fn started(config: &Config, start: Stamp) -> State {
    let mut state = ticked(config, start);
    assert!(state.start());
    state
}

#[test]
fn a_cycle_is_focus_sessions_with_short_breaks_and_a_long_one() {
    let config = automatic();
    let start = at("2026-11-16 09:00");
    let mut state = started(&config, start);
    assert_eq!(
        (state.phase, state.session, state.left, state.length),
        (Phase::Focus, 1, 25, 25)
    );
    let ended = run(&mut state, &config, start, 155);
    assert_eq!(
        ended,
        [
            (25, "focus"),
            (30, "shortBreak"),
            (55, "focus"),
            (60, "shortBreak"),
            (85, "focus"),
            (90, "shortBreak"),
            (115, "focus"),
            // After the fourth session: the long break, then the cycle
            // starts again with session 1.
            (130, "longBreak"),
            (155, "focus"),
        ]
    );
    assert_eq!(
        (state.phase, state.session, state.timer),
        (Phase::ShortBreak, 1, Timer::Running)
    );
    assert_eq!(
        state.stats,
        Stats {
            day: Some(start.day()),
            sessions_today: 5,
            minutes_today: 125,
            sessions_total: 5,
            minutes_total: 125,
        }
    );
}

#[test]
fn the_session_number_follows_the_cycle() {
    let config = config(json!({
        "focusMinutes": 2, "shortBreakMinutes": 1, "longBreakMinutes": 3,
        "sessionsBeforeLongBreak": 3, "autoStartFocus": true
    }));
    let start = at("2026-11-16 09:00");
    let mut state = started(&config, start);
    let mut seen = Vec::new();
    for minute in 1..=22 {
        state.advance(&config, Stamp(start.0 + minute));
        state.pending = None;
        seen.push((state.phase.name(), state.session));
    }
    // Each entry is the state after that minute's tick: a cycle is
    // 2 + 1 + 2 + 1 + 2 + 3 = 11 minutes, and its last tick begins the next.
    let cycle = [
        ("focus", 1),
        ("shortBreak", 1),
        ("focus", 2),
        ("focus", 2),
        ("shortBreak", 2),
        ("focus", 3),
        ("focus", 3),
        ("longBreak", 3),
        ("longBreak", 3),
        ("longBreak", 3),
        ("focus", 1),
    ];
    assert_eq!(seen[..11], cycle);
    assert_eq!(seen[11..], cycle, "the second cycle is the first again");
    // One session a cycle: every break is the long one.
    let single = self::config(json!({ "sessionsBeforeLongBreak": 1, "autoStartFocus": true }));
    let mut state = started(&single, start);
    assert_eq!(
        run(&mut state, &single, start, 80),
        [
            (25, "focus"),
            (40, "longBreak"),
            (65, "focus"),
            (80, "longBreak")
        ]
    );
    assert_eq!(state.session, 1);
}

#[test]
fn nothing_counts_down_until_the_timer_is_started() {
    let config = Config::default();
    let start = at("2026-11-16 09:00");
    let mut state = ticked(&config, start);
    assert_eq!(run(&mut state, &config, start, 120), []);
    assert_eq!((state.timer, state.left), (Timer::Idle, 25));
    assert_eq!(state.stats.sessions_total, 0);
}

#[test]
fn without_auto_start_the_next_phase_waits_for_start() {
    let config = config(json!({ "autoStartBreaks": false, "autoStartFocus": false }));
    let start = at("2026-11-16 09:00");
    let mut state = started(&config, start);
    assert_eq!(run(&mut state, &config, start, 60), [(25, "focus")]);
    // The break is ready, full, and does not move.
    assert_eq!(
        (state.phase, state.timer, state.left),
        (Phase::ShortBreak, Timer::Idle, 5)
    );
    assert!(state.start());
    let from = Stamp(start.0 + 60);
    assert_eq!(run(&mut state, &config, from, 30), [(5, "shortBreak")]);
    assert_eq!(
        (state.phase, state.timer, state.left, state.session),
        (Phase::Focus, Timer::Idle, 25, 2)
    );
}

#[test]
fn by_default_breaks_start_by_themselves_and_focus_sessions_do_not() {
    let config = Config::default();
    let start = at("2026-11-16 09:00");
    let mut state = started(&config, start);
    assert_eq!(
        run(&mut state, &config, start, 90),
        [(25, "focus"), (30, "shortBreak")]
    );
    assert_eq!(
        (state.phase, state.timer, state.left, state.session),
        (Phase::Focus, Timer::Idle, 25, 2)
    );
}

#[test]
fn pause_holds_the_countdown_and_resume_continues_it() {
    let config = automatic();
    let start = at("2026-11-16 09:00");
    let mut state = started(&config, start);
    run(&mut state, &config, start, 10);
    assert_eq!(state.left, 15);
    assert!(state.pause());
    assert!(!state.pause(), "pausing twice changes nothing");
    assert_eq!(run(&mut state, &config, Stamp(start.0 + 10), 4), []);
    assert_eq!((state.timer, state.left), (Timer::Paused, 15));
    assert!(state.resume());
    assert!(!state.resume(), "it is running now");
    assert_eq!(
        run(&mut state, &config, Stamp(start.0 + 14), 15),
        [(15, "focus")]
    );
    // Resume is for a paused timer only; Start also starts a phase.
    let mut idle = ticked(&config, start);
    assert!(!idle.resume());
    assert!(!idle.pause());
    assert_eq!(idle.timer, Timer::Idle);
    assert!(idle.start());
    assert!(!idle.start(), "starting twice changes nothing");
}

#[test]
fn keys_work_without_a_clock_and_are_precise_to_one_tick() {
    // Start, pause and resume only set a flag: the minutes are counted by
    // the ticks, whenever within the minute the key was pressed.
    let config = automatic();
    let mut state = State::new();
    state.sync(&config);
    assert!(state.start(), "start works before the first tick");
    assert_eq!(
        (state.timer, state.left, state.last_tick),
        (Timer::Running, 25, None)
    );
    // The first tick counts nothing, there is no tick before it.
    let start = at("2026-11-16 09:00");
    assert_eq!(state.advance(&config, start).note, Note::First);
    assert_eq!(state.left, 25);
    // A key pressed between 09:00 and 09:01 loses up to a minute at the
    // next tick; that is the precision.
    state.advance(&config, Stamp(start.0 + 1));
    assert_eq!(state.left, 24);
}

#[test]
fn skip_goes_on_without_counting_the_session() {
    let config = Config::default();
    let start = at("2026-11-16 09:00");
    let mut state = started(&config, start);
    run(&mut state, &config, start, 10);
    // Running: the next phase runs.
    assert_eq!(state.skip(&config), Phase::Focus);
    assert_eq!(
        (state.phase, state.timer, state.left, state.session),
        (Phase::ShortBreak, Timer::Running, 5, 1)
    );
    assert_eq!(
        state.stats.sessions_total, 0,
        "a skipped session is not completed"
    );
    assert_eq!(state.stats.minutes_total, 0);
    // A skipped break moves on to the next session.
    assert_eq!(state.skip(&config), Phase::ShortBreak);
    assert_eq!(
        (state.phase, state.timer, state.session),
        (Phase::Focus, Timer::Running, 2)
    );
    // Paused or not started: the next phase waits.
    state.pause();
    assert_eq!(state.skip(&config), Phase::Focus);
    assert_eq!((state.phase, state.timer), (Phase::ShortBreak, Timer::Idle));
    assert_eq!(state.skip(&config), Phase::ShortBreak);
    assert_eq!(
        (state.phase, state.timer, state.session),
        (Phase::Focus, Timer::Idle, 3)
    );
    // The cycle goes on as if the sessions had ended: after the fourth,
    // the long break, then session 1.
    state.skip(&config);
    state.skip(&config);
    assert_eq!((state.phase, state.session), (Phase::Focus, 4));
    state.skip(&config);
    assert_eq!((state.phase, state.left), (Phase::LongBreak, 15));
    state.skip(&config);
    assert_eq!((state.phase, state.session), (Phase::Focus, 1));
    assert_eq!(state.stats.sessions_total, 0);
    // A notification that waited is about a phase that is over.
    state.pending = Some(Pending {
        notice: Notice::FocusEnded,
        age: 1,
    });
    state.skip(&config);
    assert_eq!(state.pending, None);
}

#[test]
fn reset_stops_the_timer_and_starts_the_phase_the_cycle_or_the_statistics_again() {
    let config = automatic();
    let start = at("2026-11-16 09:00");
    let mut state = started(&config, start);
    run(&mut state, &config, start, 40);
    // 25 focus, 5 break, 10 minutes into session 2.
    assert_eq!(
        (state.phase, state.session, state.left),
        (Phase::Focus, 2, 15)
    );
    let stats = state.stats;
    assert_eq!(stats.sessions_total, 1);

    let mut phase = state.clone();
    phase.reset_phase(&config);
    assert_eq!(
        (phase.phase, phase.session, phase.left, phase.timer),
        (Phase::Focus, 2, 25, Timer::Idle)
    );
    assert_eq!(phase.stats, stats);

    let mut cycle = state.clone();
    cycle.reset_cycle(&config);
    assert_eq!(
        (cycle.phase, cycle.session, cycle.left, cycle.timer),
        (Phase::Focus, 1, 25, Timer::Idle)
    );
    assert_eq!(cycle.stats, stats);

    let mut statistics = state.clone();
    statistics.reset_statistics();
    assert_eq!(
        statistics.stats,
        Stats {
            day: stats.day,
            ..Stats::default()
        }
    );
    // The timer is not touched.
    assert_eq!(
        (statistics.phase, statistics.left, statistics.timer),
        (Phase::Focus, 15, Timer::Running)
    );

    // Reset in a break: the break is full again and waits.
    run(&mut state, &config, Stamp(start.0 + 40), 17);
    assert_eq!((state.phase, state.left), (Phase::ShortBreak, 3));
    state.pending = Some(Pending {
        notice: Notice::FocusEnded,
        age: 0,
    });
    state.reset_phase(&config);
    assert_eq!(
        (state.phase, state.left, state.timer, state.pending),
        (Phase::ShortBreak, 5, Timer::Idle, None)
    );
}

#[test]
fn a_long_gap_pauses_the_timer_instead_of_finishing_phases() {
    let config = automatic();
    let start = at("2026-11-16 09:00");
    let mut state = started(&config, start);
    run(&mut state, &config, start, 23);
    assert_eq!(state.left, 2);
    // The computer sleeps for three hours: six phases could have ended.
    let back = Stamp(start.0 + 23 + 180);
    let advance = state.advance(&config, back);
    assert_eq!(
        advance,
        Advance {
            note: Note::Away,
            ended: None,
            expired: false
        }
    );
    assert_eq!(
        (state.phase, state.timer, state.away, state.left),
        (Phase::Focus, Timer::Paused, true, 2),
        "paused where it was"
    );
    assert_eq!(
        state.pending,
        Some(Pending {
            notice: Notice::Away,
            age: 0
        })
    );
    assert_eq!(state.stats.sessions_total, 0);
    // It stays paused, and says so once.
    state.pending = None;
    assert_eq!(run(&mut state, &config, back, 30), []);
    assert_eq!(
        (state.timer, state.left, state.pending),
        (Timer::Paused, 2, None)
    );
    assert!(Snapshot::of(&state, &config).away);
    // The user resumes: the two minutes that were left are counted.
    assert!(state.resume());
    assert!(!state.away);
    assert_eq!(
        run(&mut state, &config, Stamp(back.0 + 30), 2),
        [(2, "focus")]
    );

    // Exactly the away time is a gap; a minute less is counted.
    for (gap, note, left) in [(5, Note::Away, 20), (4, Note::Counted, 16)] {
        let mut state = started(&config, start);
        run(&mut state, &config, start, 5);
        let advance = state.advance(&config, Stamp(start.0 + 5 + gap));
        assert_eq!((advance.note, state.left), (note, left), "a gap of {gap}");
    }
}

#[test]
fn a_gap_changes_nothing_when_the_timer_does_not_run() {
    let config = automatic();
    let start = at("2026-11-16 09:00");
    // Paused by the user.
    let mut state = started(&config, start);
    run(&mut state, &config, start, 5);
    state.pause();
    let advance = state.advance(&config, Stamp(start.0 + 600));
    assert_eq!((advance.note, advance.ended), (Note::Counted, None));
    assert_eq!(
        (state.timer, state.away, state.left, state.pending),
        (Timer::Paused, false, 20, None)
    );
    // Not started.
    let mut state = ticked(&config, start);
    state.advance(&config, Stamp(start.0 + 600));
    assert_eq!(
        (state.timer, state.left, state.pending),
        (Timer::Idle, 25, None)
    );
}

#[test]
fn a_waiting_away_notice_is_dropped_when_the_user_is_back() {
    let config = automatic();
    let start = at("2026-11-16 09:00");
    let mut state = started(&config, start);
    state.advance(&config, Stamp(start.0 + 60));
    assert!(state.pending.is_some_and(|p| p.notice == Notice::Away));
    assert!(state.start());
    assert_eq!(state.pending, None);
    // Another notice is kept: it is still true.
    state.pause();
    state.pending = Some(Pending {
        notice: Notice::BreakEnded,
        age: 2,
    });
    state.resume();
    assert!(state.pending.is_some());
}

#[test]
fn a_tick_ends_one_phase_at_most_and_carries_nothing_over() {
    let config = automatic();
    let start = at("2026-11-16 09:00");
    let mut state = started(&config, start);
    run(&mut state, &config, start, 23);
    assert_eq!(state.left, 2);
    // Four minutes later (less than the away time): the session ended two
    // minutes ago. The break starts now, with all of its five minutes.
    let advance = state.advance(&config, Stamp(start.0 + 27));
    assert_eq!(
        (advance.note, advance.ended),
        (Note::Counted, Some(Phase::Focus))
    );
    assert_eq!(
        (state.phase, state.left, state.timer),
        (Phase::ShortBreak, 5, Timer::Running)
    );
    assert_eq!(state.stats.sessions_total, 1);
    // Even when the away time is long and whole phases fit into the gap.
    let patient = self::config(json!({
        "focusMinutes": 2, "shortBreakMinutes": 1, "awayMinutes": 120, "autoStartFocus": true
    }));
    let mut state = started(&patient, start);
    let advance = state.advance(&patient, Stamp(start.0 + 100));
    assert_eq!(advance.ended, Some(Phase::Focus));
    assert_eq!((state.phase, state.left), (Phase::ShortBreak, 1));
    assert_eq!(state.stats.sessions_total, 1);
}

#[test]
fn a_tick_interval_longer_than_a_minute_works() {
    // A workflow that fires every 3 minutes: a phase ends at the first
    // tick at or after its time.
    let config = config(json!({ "focusMinutes": 10, "autoStartFocus": true }));
    let start = at("2026-11-16 09:00");
    let mut state = started(&config, start);
    let mut ended = Vec::new();
    for minute in (3..=30).step_by(3) {
        let advance = state.advance(&config, Stamp(start.0 + minute));
        if let Some(phase) = advance.ended {
            ended.push((minute, phase.name()));
        }
    }
    assert_eq!(ended, [(12, "focus"), (18, "shortBreak"), (30, "focus")]);
}

#[test]
fn the_clock_going_backwards_takes_nothing_off() {
    let config = automatic();
    let start = at("2026-10-25 02:30");
    let mut state = started(&config, start);
    run(&mut state, &config, start, 20);
    assert_eq!(state.left, 5);
    // Daylight saving time ends: 02:50 is followed by 02:00.
    let back = at("2026-10-25 02:00");
    let advance = state.advance(&config, back);
    assert_eq!((advance.note, advance.ended), (Note::ClockWentBack, None));
    assert_eq!((state.left, state.timer), (5, Timer::Running));
    assert_eq!(state.last_tick, Some(back.0));
    // It goes on from the new time: no hour is lost or gained.
    assert_eq!(run(&mut state, &config, back, 5), [(5, "focus")]);
}

#[test]
fn the_same_minute_twice_counts_once() {
    let config = automatic();
    let start = at("2026-11-16 09:00");
    let mut state = started(&config, start);
    run(&mut state, &config, start, 24);
    assert_eq!(state.left, 1);
    let again = state.advance(&config, Stamp(start.0 + 24));
    assert_eq!((again.note, again.ended), (Note::SameMinute, None));
    assert_eq!(state.left, 1);
    let advance = state.advance(&config, Stamp(start.0 + 25));
    assert_eq!(advance.ended, Some(Phase::Focus));
    // And a session is not completed twice by a repeated tick.
    let again = state.advance(&config, Stamp(start.0 + 25));
    assert_eq!(again.ended, None);
    assert_eq!(state.stats.sessions_total, 1);
}

#[test]
fn statistics_of_the_day_start_again_at_midnight() {
    let config = automatic();
    let start = at("2026-11-16 23:00");
    let mut state = started(&config, start);
    // 23:25 and 23:55: two sessions on the 16th.
    run(&mut state, &config, start, 59);
    assert_eq!(
        state.stats,
        Stats {
            day: Some(start.day()),
            sessions_today: 2,
            minutes_today: 50,
            sessions_total: 2,
            minutes_total: 50
        }
    );
    // Midnight: the day's counters start again, the totals stay.
    let midnight = at("2026-11-17 00:00");
    state.advance(&config, midnight);
    assert_eq!(
        state.stats,
        Stats {
            day: Some(midnight.day()),
            sessions_today: 0,
            minutes_today: 0,
            sessions_total: 2,
            minutes_total: 50
        }
    );
    assert_eq!(date_text(state.stats.day.unwrap()), "2026-11-17");
    // A session that began before midnight counts on the day it ended.
    let mut late = started(&config, at("2026-11-16 23:50"));
    assert_eq!(
        run(&mut late, &config, at("2026-11-16 23:50"), 25),
        [(25, "focus")]
    );
    assert_eq!(
        (
            late.stats.day,
            late.stats.sessions_today,
            late.stats.minutes_today,
            late.stats.sessions_total
        ),
        (Some(midnight.day()), 1, 25, 1)
    );
    // Days without a tick: the first tick of a new date starts the day,
    // also when the timer is paused or was never started.
    state.pause();
    state.advance(&config, at("2026-11-20 08:00"));
    assert_eq!(
        (
            state.stats.day,
            state.stats.sessions_today,
            state.stats.sessions_total
        ),
        (Some(at("2026-11-20 08:00").day()), 0, 2)
    );
    // The clock set back across midnight is another date too: the day's
    // counters start again, the totals are never touched.
    let mut state = started(&config, at("2026-11-17 00:01"));
    state.stats.sessions_today = 3;
    state.stats.sessions_total = 9;
    let advance = state.advance(&config, at("2026-11-16 23:58"));
    assert_eq!(advance.note, Note::ClockWentBack);
    assert_eq!(
        (
            state.stats.day,
            state.stats.sessions_today,
            state.stats.sessions_total
        ),
        (Some(start.day()), 0, 9)
    );
}

#[test]
fn changed_settings_apply_to_a_phase_that_has_not_started() {
    let start = at("2026-11-16 09:00");
    let mut state = ticked(&Config::default(), start);
    assert_eq!((state.left, state.length), (25, 25));
    let longer = config(json!({ "focusMinutes": 50 }));
    state.sync(&longer);
    assert_eq!((state.left, state.length), (50, 50));
    // A phase that runs or is paused keeps the length it began with.
    state.start();
    run(&mut state, &longer, start, 10);
    for other in [json!({ "focusMinutes": 20 }), json!({ "focusMinutes": 90 })] {
        state.sync(&config(other));
        assert_eq!((state.left, state.length), (40, 50));
    }
    state.pause();
    state.sync(&config(json!({ "focusMinutes": 20 })));
    assert_eq!((state.left, state.length), (40, 50));
    // The next phase has the new length.
    let shorter = config(json!({ "focusMinutes": 20, "shortBreakMinutes": 3 }));
    state.resume();
    assert_eq!(
        run(&mut state, &shorter, Stamp(start.0 + 10), 40),
        [(40, "focus")]
    );
    assert_eq!(
        (state.phase, state.left, state.length),
        (Phase::ShortBreak, 3, 3)
    );
    assert_eq!(state.stats.minutes_total, 50, "the minutes it really had");
    // Fewer sessions a cycle than the current number: the current one is
    // the last, and the long break follows it.
    let mut state = ticked(&Config::default(), start);
    state.session = 4;
    let two = config(json!({ "sessionsBeforeLongBreak": 2 }));
    state.sync(&two);
    assert_eq!(state.session, 2);
    assert_eq!(state.following(&two), Phase::LongBreak);
}

#[test]
fn a_notification_that_cannot_be_sent_waits_a_few_minutes_and_no_longer() {
    let config = Config {
        short_break_minutes: 30,
        ..automatic()
    };
    let start = at("2026-11-16 09:00");
    let mut state = started(&config, start);
    for minute in 1..=25 {
        state.advance(&config, Stamp(start.0 + minute));
    }
    assert_eq!(
        state.pending,
        Some(Pending {
            notice: Notice::FocusEnded,
            age: 0
        })
    );
    // The caller never sends it: it grows older with every tick.
    for age in 1..=MAX_NOTICE_AGE {
        let advance = state.advance(&config, Stamp(start.0 + 25 + i64::from(age)));
        assert!(!advance.expired);
        assert_eq!(
            state.pending,
            Some(Pending {
                notice: Notice::FocusEnded,
                age
            })
        );
    }
    // A phase that ends meanwhile replaces it: one notice, the newer one.
    let mut replaced = state.clone();
    replaced.left = 1;
    let advance = replaced.advance(&config, Stamp(start.0 + 31));
    assert_eq!(advance.ended, Some(Phase::ShortBreak));
    assert_eq!(
        replaced.pending,
        Some(Pending {
            notice: Notice::BreakEnded,
            age: 0
        })
    );
    // Otherwise it is dropped, and the timer goes on.
    let advance = state.advance(&config, Stamp(start.0 + 31));
    assert!(advance.expired);
    assert_eq!((state.pending, state.left), (None, 24));
    // Ticks far apart age it at once.
    let patient = Config {
        away_minutes: 120,
        ..config
    };
    state.pending = Some(Pending {
        notice: Notice::FocusEnded,
        age: 0,
    });
    assert!(state.advance(&patient, Stamp(start.0 + 31 + 6)).expired);
}

#[test]
fn the_snapshot_says_what_a_user_and_a_key_see() {
    let config = Config::default();
    let start = at("2026-11-16 09:00");
    let mut state = State::new();
    state.sync(&config);
    let s = Snapshot::of(&state, &config);
    assert!(s.waiting);
    assert_eq!(
        s.summary(),
        "Focus 1 of 4 not started: 25 min (no tick received yet: add the every-minute workflow from the plugin's README)"
    );
    assert_eq!(
        s.key(),
        KeyFace {
            label: "25 min".into(),
            detail: "Focus 1/4 · ready".into(),
            indicator: "unknown",
            progress: 0,
            toggle: false
        }
    );

    let mut state = started(&config, start);
    run(&mut state, &config, start, 10);
    let s = Snapshot::of(&state, &config);
    assert_eq!(
        (
            s.phase,
            s.timer,
            s.remaining,
            s.length,
            s.session,
            s.sessions
        ),
        (Phase::Focus, Timer::Running, 15, 25, 1, 4)
    );
    assert_eq!(s.summary(), "Focus 1 of 4: 15 min left");
    assert_eq!(
        s.key(),
        KeyFace {
            label: "15 min".into(),
            detail: "Focus 1/4".into(),
            indicator: "running",
            progress: 40,
            toggle: true
        }
    );

    state.pause();
    let s = Snapshot::of(&state, &config);
    assert_eq!(s.summary(), "Focus 1 of 4 paused: 15 min left");
    assert_eq!(s.key().detail, "Focus 1/4 · paused");
    assert_eq!((s.key().indicator, s.key().toggle), ("warning", false));

    state.resume();
    state.advance(&config, Stamp(start.0 + 300));
    let s = Snapshot::of(&state, &config);
    assert!(s.away);
    assert_eq!(
        s.summary(),
        "Focus 1 of 4 paused after a long pause between ticks: 15 min left"
    );

    state.resume();
    run(&mut state, &config, Stamp(start.0 + 300), 17);
    let s = Snapshot::of(&state, &config);
    assert_eq!(s.summary(), "Short break: 3 min left");
    assert_eq!((s.key().detail.as_str(), s.progress()), ("Short break", 40));
    assert_eq!(s.stats.sessions_today, 1);

    // Polish on the key when the setting says so.
    let polish = Config {
        language: Language::Polish,
        ..Config::default()
    };
    assert_eq!(Snapshot::of(&state, &polish).key().detail, "Krótka przerwa");
    state.pause();
    assert_eq!(
        Snapshot::of(&state, &polish).key().detail,
        "Krótka przerwa · pauza"
    );
}

#[test]
fn what_a_key_would_show_fits_a_key_in_every_state() {
    for language in [Language::English, Language::Polish] {
        let config = Config {
            language,
            sessions: SESSIONS.1,
            ..Config::default()
        };
        for phase in [Phase::Focus, Phase::ShortBreak, Phase::LongBreak] {
            for timer in [Timer::Idle, Timer::Running, Timer::Paused] {
                for (left, length) in [(MAX_PHASE, MAX_PHASE), (0, MAX_PHASE), (1, 1), (0, 0)] {
                    let state = State {
                        phase,
                        timer,
                        left,
                        length,
                        session: SESSIONS.1,
                        ..State::new()
                    };
                    let key = Snapshot::of(&state, &config).key();
                    // DeckPilot's ButtonState: 32 characters, 0-100.
                    assert!((1..=32).contains(&key.label.chars().count()), "{key:?}");
                    assert!((1..=32).contains(&key.detail.chars().count()), "{key:?}");
                    assert!(key.progress <= 100);
                    assert!(["running", "warning", "unknown"].contains(&key.indicator));
                    assert_eq!(key.toggle, timer == Timer::Running);
                }
            }
        }
    }
    // The bar fills as the phase goes by.
    let config = Config::default();
    let progress = |left| {
        let state = State {
            timer: Timer::Running,
            left,
            length: 25,
            ..State::new()
        };
        Snapshot::of(&state, &config).progress()
    };
    assert_eq!([25, 24, 13, 1, 0].map(progress), [0, 4, 48, 96, 100]);
}

#[test]
fn stored_state_round_trips_and_damage_is_survived() {
    let config = automatic();
    let start = at("2026-11-16 09:00");
    let mut state = started(&config, start);
    for minute in 1..=65 {
        state.advance(&config, Stamp(start.0 + minute));
    }
    state.pause();
    let stored = serde_json::to_value(&state).unwrap();
    assert_eq!(stored["version"], STATE_VERSION);
    assert_eq!(stored["phase"], "focus");
    assert_eq!(stored["timer"], "paused");
    assert_eq!(stored["left"], 20);
    assert_eq!(stored["session"], 3);
    assert_eq!(
        stored["pending"],
        json!({ "notice": "breakEnded", "age": 5 })
    );
    assert_eq!(
        stored["stats"],
        json!({
            "day": start.day(), "sessionsToday": 2, "minutesToday": 50,
            "sessionsTotal": 2, "minutesTotal": 50
        })
    );
    assert_eq!(
        State::from_stored(Some(stored.clone())),
        (state.clone(), false)
    );
    // Small: far below one storage value (64 KiB) and the quota.
    assert!(
        stored.to_string().len() < 512,
        "{}",
        stored.to_string().len()
    );

    assert_eq!(State::from_stored(None), (State::new(), false));
    // Fields a later version adds are ignored, missing ones are defaults.
    let mut newer = stored.clone();
    newer["somethingNew"] = json!([1, 2, 3]);
    newer.as_object_mut().unwrap().remove("away");
    assert_eq!(State::from_stored(Some(newer)), (state, false));
    // Not a state, or one of another version: start again, and say so.
    for damaged in [
        json!("text"),
        json!(42),
        json!([]),
        json!({}),
        json!({ "version": 2, "lastTick": 5 }),
        json!({ "version": 1, "lastTick": "yesterday" }),
        json!({ "version": 1, "left": -5 }),
        json!({ "version": 1, "phase": "nap" }),
        json!({ "version": 1, "timer": true }),
        json!({ "version": 1, "pending": { "notice": "lunch" } }),
        json!({ "version": 1, "stats": { "sessionsTotal": -1 } }),
    ] {
        assert_eq!(
            State::from_stored(Some(damaged.clone())),
            (State::new(), true),
            "{damaged}"
        );
    }
    // Numbers that fit the type and not the plugin are bounded, and a
    // state that makes no sense still cannot stop the timer.
    let (mut state, dropped) = State::from_stored(Some(json!({
        "version": 1, "timer": "running", "left": 4_000_000_000_u32,
        "length": 4_000_000_000_u32, "session": 77
    })));
    assert!(!dropped);
    assert_eq!(
        (state.left, state.length, state.session),
        (MAX_PHASE, MAX_PHASE, SESSIONS.1)
    );
    state.sync(&config);
    assert_eq!(state.session, 4);
    let (mut state, _) = State::from_stored(Some(json!({
        "version": 1, "timer": "running", "left": 0, "length": 0, "session": 0, "away": true
    })));
    state.sync(&config);
    assert_eq!(
        (state.session, state.length, state.left, state.away),
        (1, 25, 0, false)
    );
    // Running with nothing left: the next tick ends the phase.
    assert_eq!(state.advance(&config, start).ended, Some(Phase::Focus));
}

#[test]
fn notifications_fit_the_agents_limits_in_both_languages() {
    let mut seen = Vec::new();
    for language in [Language::English, Language::Polish] {
        let config = Config {
            language,
            focus_minutes: FOCUS_MINUTES.1,
            short_break_minutes: SHORT_BREAK_MINUTES.1,
            long_break_minutes: LONG_BREAK_MINUTES.1,
            sessions: SESSIONS.1,
            ..Config::default()
        };
        for (notice, phase) in [
            (Notice::FocusEnded, Phase::ShortBreak),
            (Notice::FocusEnded, Phase::LongBreak),
            (Notice::BreakEnded, Phase::Focus),
            (Notice::Away, Phase::Focus),
            (Notice::Away, Phase::LongBreak),
        ] {
            for timer in [Timer::Idle, Timer::Running, Timer::Paused] {
                let mut state = State {
                    phase,
                    timer,
                    session: SESSIONS.1,
                    ..State::new()
                };
                state.length = config.minutes(phase);
                state.left = state.length;
                let (title, body) = notification(notice, &state, &config);
                assert!((1..=64).contains(&title.chars().count()), "{title}");
                assert!((1..=300).contains(&body.chars().count()), "{body}");
                assert!(!body.contains("  "), "{body}");
                seen.push((title, body));
            }
        }
    }
    seen.sort();
    seen.dedup();
    // Per language: a short and a long break, started or waiting (4), a
    // focus session, started or waiting (2), and away (1: both phases are
    // at their longest, 180 and 120 min, so 2).
    assert_eq!(seen.len(), 2 * 8, "{seen:#?}");

    let english = Config::default();
    let mut state = State::new();
    state.sync(&english);
    state.phase = Phase::LongBreak;
    state.length = 15;
    state.timer = Timer::Running;
    assert_eq!(
        notification(Notice::FocusEnded, &state, &english),
        (
            "Focus session finished".to_owned(),
            "Time for a long break: 15 min.".to_owned()
        )
    );
    state.timer = Timer::Idle;
    assert_eq!(
        notification(Notice::FocusEnded, &state, &english).1,
        "Next is a long break of 15 min. Start it when you are ready."
    );
    let polish = Config {
        language: Language::Polish,
        ..Config::default()
    };
    assert_eq!(
        notification(Notice::FocusEnded, &state, &polish),
        (
            "Sesja skupienia zakończona".to_owned(),
            "Następna jest długa przerwa (15 min). Uruchom ją, gdy zechcesz.".to_owned()
        )
    );
}

// ------------------------------------------------ the manifest and files

#[test]
fn definition_is_valid_and_matches_the_manifest() {
    plugin().validate().expect("the definition is valid");
    assert_eq!(
        manifest_mismatches(&plugin(), &manifest()),
        Vec::<String>::new()
    );
}

#[test]
fn the_manifest_declares_what_the_code_uses() {
    let m = manifest();
    assert_eq!(m["id"], ID);
    assert_eq!(m["version"], VERSION);
    assert_eq!(m["publisher"]["id"], "deckpilot");
    // One permission, and no network.
    let kinds: Vec<&str> = m["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["notifications"]);
    let fields = m["contributes"]["settings"]["fields"].as_array().unwrap();
    let field = |key: &str| {
        fields
            .iter()
            .find(|f| f["key"] == key)
            .unwrap_or_else(|| panic!("the setting {key}"))
    };
    // The routing names boolean settings of the plugin.
    for (_, setting) in m["permissions"][0]["routing"].as_object().unwrap() {
        assert_eq!(field(setting.as_str().unwrap())["kind"]["type"], "boolean");
    }
    // The bounds and defaults the code falls back to are the manifest's.
    for (key, (min, max, default)) in [
        ("focusMinutes", FOCUS_MINUTES),
        ("shortBreakMinutes", SHORT_BREAK_MINUTES),
        ("longBreakMinutes", LONG_BREAK_MINUTES),
        ("sessionsBeforeLongBreak", SESSIONS),
        ("awayMinutes", AWAY_MINUTES),
    ] {
        let f = field(key);
        assert_eq!(
            (&f["kind"]["min"], &f["kind"]["max"], &f["default"]["value"]),
            (&json!(min), &json!(max), &json!(default)),
            "{key}"
        );
    }
    assert_eq!(field("language")["kind"]["options"], json!(["en", "pl"]));
    // The settings' defaults are the code's defaults.
    let defaults: serde_json::Map<String, Value> = fields
        .iter()
        .filter(|f| f.get("default").is_some())
        .map(|f| {
            (
                f["key"].as_str().unwrap().to_owned(),
                f["default"]["value"].clone(),
            )
        })
        .collect();
    assert_eq!(defaults.len(), fields.len(), "every setting has a default");
    assert_eq!(Config::from_settings(&defaults), Config::default());

    let declared = m["contributes"]["errorCodes"]
        .as_object()
        .expect("errorCodes");
    assert_eq!(declared.len(), PLUGIN_CODES.len());
    for code in PLUGIN_CODES {
        assert!(declared.contains_key(*code), "{code} is not in errorCodes");
        assert!(!codes::is_shared(code), "{code} redeclares a shared code");
    }
    let variables: Vec<&str> = m["contributes"]["variables"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["id"].as_str().unwrap())
        .collect();
    assert_eq!(variables, [TIMER_VARIABLES, STATS_VARIABLES].concat());
    let actions = m["contributes"]["actions"].as_array().unwrap();
    let action = |id: &str| actions.iter().find(|a| a["id"] == id).unwrap();
    assert_eq!(
        action("reset")["parameters"][0]["kind"]["options"],
        json!(SCOPES)
    );
    assert_eq!(action("status")["node"]["effect"], "readOnly");
    // The description says what the plugin cannot do yet.
    let description = m["description"].as_str().unwrap();
    assert!(description.contains("every minute"), "{description}");
    assert!(
        description.contains("cannot show the countdown"),
        "{description}"
    );
}

/// Every output an action can return is declared: the agent drops the
/// others without a word.
fn assert_outputs_declared(action: &str, data: &deckpilot_plugin_sdk::Params) {
    let m = manifest();
    let outputs = m["contributes"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == action)
        .unwrap()["node"]["outputs"]
        .as_array()
        .unwrap()
        .clone();
    for key in data.keys() {
        assert!(
            outputs.iter().any(|o| o["key"] == key.as_str()),
            "{action} returns the undeclared output {key}"
        );
    }
    for output in &outputs {
        let key = output["key"].as_str().unwrap();
        if output["always"] == true {
            assert!(
                data.contains_key(key),
                "{action} did not return {key}, declared `always`"
            );
        }
        if let Some(value) = data.get(key) {
            let kind = match value {
                ParamValue::Bool(_) => "boolean",
                ParamValue::Integer(_) => "integer",
                ParamValue::String(_) => "text",
                other => panic!("{action}.{key}: {other:?}"),
            };
            assert_eq!(output["kind"], kind, "{action}.{key}");
        }
    }
}

#[test]
fn icon_is_a_128px_png() {
    let png = include_bytes!("../assets/icon-128.png");
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let side = |at: usize| u32::from_be_bytes([png[at], png[at + 1], png[at + 2], png[at + 3]]);
    assert_eq!((side(16), side(20)), (128, 128));
    assert!(png.len() <= 256 * 1024);
}

// ------------------------------------------------------------ the actions

/// How the scripted agent answers `notify`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Notify {
    /// Shown on the desktop.
    Shown,
    /// Accepted; the user's settings show it nowhere.
    Nowhere,
    /// An agent that predates deliveries: `null`.
    Old,
    /// An agent that predates severities: refuses them, takes the plain form.
    NoSeverity,
    Throttled,
    Denied,
    Failed,
}

/// The scripted agent: settings, an in-memory storage, and switches for
/// what can go wrong.
#[derive(Debug)]
struct Agent {
    settings: Value,
    store: BTreeMap<String, Value>,
    notify: Notify,
    fail_get: bool,
    /// The number of `storage.set` calls that still succeed.
    sets_left: Option<u32>,
    quota: bool,
    fail_variables: bool,
    /// Notifications that were accepted: (title, body).
    shown: Vec<(String, String)>,
    variables: BTreeMap<String, Value>,
    logs: Vec<String>,
}

type Shared = Rc<RefCell<Agent>>;

fn agent(settings: Value) -> Shared {
    Rc::new(RefCell::new(Agent {
        settings,
        store: BTreeMap::new(),
        notify: Notify::Shown,
        fail_get: false,
        sets_left: None,
        quota: false,
        fail_variables: false,
        shown: Vec::new(),
        variables: BTreeMap::new(),
        logs: Vec::new(),
    }))
}

fn host(agent: &Shared) -> MockHost {
    let agent = Rc::clone(agent);
    MockHost::new(move |request| {
        let mut a = agent.borrow_mut();
        match request {
            CapabilityRequest::SettingsGet(_) => CallResult::Ok(a.settings.clone()),
            CapabilityRequest::StorageGet(_) if a.fail_get => {
                CallResult::error(CallErrorCode::Failed, "the database is locked")
            }
            CapabilityRequest::StorageGet(k) => {
                CallResult::Ok(a.store.get(&k.key).cloned().unwrap_or(Value::Null))
            }
            CapabilityRequest::StorageSet(_) if a.quota => {
                CallResult::error(CallErrorCode::Failed, "storage-quota: 262144 bytes")
            }
            CapabilityRequest::StorageSet(s) => {
                if let Some(left) = a.sets_left {
                    if left == 0 {
                        return CallResult::error(CallErrorCode::Failed, "the disk is full");
                    }
                    a.sets_left = Some(left - 1);
                }
                a.store.insert(s.key.clone(), s.value.clone());
                CallResult::Ok(Value::Null)
            }
            CapabilityRequest::Notify(n) => {
                let accept = |a: &mut Agent| a.shown.push((n.title.clone(), n.body.clone()));
                match a.notify {
                    Notify::Shown => {
                        accept(&mut a);
                        CallResult::Ok(
                            json!({ "shown": true, "desktop": true, "statusBar": false }),
                        )
                    }
                    Notify::Nowhere => CallResult::Ok(json!({
                        "shown": false,
                        "reason": "DeckPilot is not open and desktop notifications are off"
                    })),
                    Notify::Old => {
                        accept(&mut a);
                        CallResult::Ok(Value::Null)
                    }
                    Notify::NoSeverity if n.severity.is_some() => {
                        CallResult::error(CallErrorCode::BadRequest, "unknown field `severity`")
                    }
                    Notify::NoSeverity => {
                        accept(&mut a);
                        CallResult::Ok(Value::Null)
                    }
                    Notify::Throttled => {
                        CallResult::error(CallErrorCode::Throttled, "5 notifications per minute")
                    }
                    Notify::Denied => CallResult::error(CallErrorCode::NotPermitted, "not granted"),
                    Notify::Failed => {
                        CallResult::error(CallErrorCode::Unavailable, "the run was cancelled")
                    }
                }
            }
            CapabilityRequest::VariablesPublish(_) if a.fail_variables => {
                CallResult::error(CallErrorCode::Throttled, "10 per second")
            }
            CapabilityRequest::VariablesPublish(v) => {
                a.variables.insert(v.id.clone(), v.value.clone());
                CallResult::Ok(Value::Null)
            }
            CapabilityRequest::Log(l) => {
                a.logs.push(l.message.clone());
                CallResult::Ok(Value::Null)
            }
            other => panic!("the plugin made a call it has no reason to make: {other:?}"),
        }
    })
}

/// The result of an action that succeeded: its message and outputs,
/// checked against the manifest.
fn ok(host: &mut MockHost, action: &str, params: Value) -> (String, deckpilot_plugin_sdk::Params) {
    match host.invoke(&plugin(), action, params) {
        ActionOutcome::Ok { message, data } => {
            assert_outputs_declared(action, &data);
            (message.expect("every action has a message"), data)
        }
        other => panic!("{action}: {other:?}"),
    }
}

fn failed(host: &mut MockHost, action: &str, params: Value) -> (String, String, bool) {
    match host.invoke(&plugin(), action, params) {
        ActionOutcome::Failed {
            code,
            message,
            retryable,
        } => (code, message.unwrap_or_default(), retryable),
        other => panic!("{action}: {other:?}"),
    }
}

/// A key press: an action without parameters.
fn press(host: &mut MockHost, action: &str) -> (String, deckpilot_plugin_sdk::Params) {
    ok(host, action, json!({}))
}

fn minute_text(minute: u32) -> String {
    format!("2026-11-16 {:02}:{:02}", 9 + minute / 60, minute % 60)
}

fn tick_at(host: &mut MockHost, time: &str) -> (String, deckpilot_plugin_sdk::Params) {
    ok(host, "tick", json!({ "firedAt": time }))
}

/// Minutes `from..=to` after 2026-11-16 09:00 as ticks; returns the last.
fn ticks(host: &mut MockHost, from: u32, to: u32) -> (String, deckpilot_plugin_sdk::Params) {
    let mut last = None;
    for minute in from..=to {
        last = Some(tick_at(host, &minute_text(minute)));
    }
    last.expect("at least one tick")
}

fn int(data: &deckpilot_plugin_sdk::Params, key: &str) -> i64 {
    match data.get(key) {
        Some(ParamValue::Integer(v)) => *v,
        other => panic!("{key}: {other:?}"),
    }
}

fn text(data: &deckpilot_plugin_sdk::Params, key: &str) -> String {
    match data.get(key) {
        Some(ParamValue::String(v)) => v.clone(),
        other => panic!("{key}: {other:?}"),
    }
}

fn flag(data: &deckpilot_plugin_sdk::Params, key: &str) -> bool {
    match data.get(key) {
        Some(ParamValue::Bool(v)) => *v,
        other => panic!("{key}: {other:?}"),
    }
}

fn stored(agent: &Shared) -> State {
    let (state, dropped) = State::from_stored(agent.borrow().store.get(STATE_KEY).cloned());
    assert!(!dropped);
    state
}

fn sets(host: &MockHost) -> usize {
    host.requests()
        .iter()
        .filter(|r| matches!(r, CapabilityRequest::StorageSet(_)))
        .count()
}

/// Focus 3, short break 2, long break 4, two sessions a cycle, away after
/// 3, everything starts by itself.
const SHORT: fn() -> Value = || {
    json!({
        "focusMinutes": 3, "shortBreakMinutes": 2, "longBreakMinutes": 4,
        "sessionsBeforeLongBreak": 2, "awayMinutes": 3,
        "autoStartBreaks": true, "autoStartFocus": true
    })
};

/// [`SHORT`] with other values for some settings.
fn short_with(other: Value) -> Value {
    let mut settings = SHORT();
    for (key, value) in other.as_object().unwrap() {
        settings[key] = value.clone();
    }
    settings
}

/// A host whose timer got its first tick at 09:00 and was started.
fn running(agent: &Shared) -> MockHost {
    let mut host = host(agent);
    ticks(&mut host, 0, 0);
    press(&mut host, "start");
    host
}

#[test]
fn tick_counts_down_ends_phases_notifies_and_publishes() {
    let agent = agent(SHORT());
    let mut host = host(&agent);
    let (message, data) = tick_at(&mut host, "2026-11-16 09:00");
    assert_eq!(message, "Focus 1 of 2 not started: 3 min");
    assert_eq!(text(&data, "state"), "idle");
    assert_eq!(text(&data, "ended"), "none");
    assert!(!flag(&data, "notified"));
    assert!(!data.contains_key("reason"));

    let (message, data) = press(&mut host, "start");
    assert_eq!(message, "Focus 1 of 2: 3 min left");
    assert!(flag(&data, "changed"));
    let (message, data) = ticks(&mut host, 1, 2);
    assert_eq!(message, "Focus 1 of 2: 1 min left");
    assert_eq!(
        (text(&data, "state"), int(&data, "remaining")),
        ("running".into(), 1)
    );
    assert!(agent.borrow().shown.is_empty());

    // 09:03: the session ends, the break begins.
    let (message, data) = ticks(&mut host, 3, 3);
    assert_eq!(message, "The focus session ended. Short break: 2 min left");
    assert_eq!(text(&data, "ended"), "focus");
    assert!(flag(&data, "notified"));
    assert_eq!(text(&data, "phase"), "shortBreak");
    assert_eq!(
        (
            int(&data, "remaining"),
            int(&data, "session"),
            int(&data, "completedToday"),
            int(&data, "focusMinutesToday"),
            int(&data, "completedTotal"),
            int(&data, "focusMinutesTotal")
        ),
        (2, 1, 1, 3, 1, 3)
    );
    // 09:05 the break ends, 09:08 session 2, 09:12 the long break.
    let (message, _) = ticks(&mut host, 4, 5);
    assert_eq!(message, "The short break ended. Focus 2 of 2: 3 min left");
    let (message, _) = ticks(&mut host, 6, 8);
    assert_eq!(message, "The focus session ended. Long break: 4 min left");
    let (message, data) = ticks(&mut host, 9, 12);
    assert_eq!(message, "The long break ended. Focus 1 of 2: 3 min left");
    assert_eq!(text(&data, "ended"), "longBreak");
    let own = |a: &str, b: &str| (a.to_owned(), b.to_owned());
    assert_eq!(
        agent.borrow().shown,
        [
            own("Focus session finished", "Time for a short break: 2 min."),
            own("Break is over", "Focus session 2 of 2 has started: 3 min."),
            own("Focus session finished", "Time for a long break: 4 min."),
            own("Break is over", "Focus session 1 of 2 has started: 3 min."),
        ]
    );
    assert_eq!(
        agent.borrow().variables,
        BTreeMap::from([
            ("state".to_owned(), json!("running")),
            ("phase".to_owned(), json!("focus")),
            ("remaining".to_owned(), json!(3)),
            ("session".to_owned(), json!(1)),
            ("completedToday".to_owned(), json!(2)),
            ("focusMinutesToday".to_owned(), json!(6)),
            ("completedTotal".to_owned(), json!(2)),
            ("focusMinutesTotal".to_owned(), json!(6)),
        ])
    );
    // One storage key, and nothing waits to be shown.
    assert_eq!(agent.borrow().store.keys().collect::<Vec<_>>(), [STATE_KEY]);
    assert_eq!(stored(&agent).pending, None);
    assert!(agent.borrow().logs.is_empty());
}

#[test]
fn without_auto_start_the_notification_asks_for_start() {
    let agent = agent(short_with(
        json!({ "autoStartBreaks": false, "autoStartFocus": false }),
    ));
    let mut host = running(&agent);
    let (message, data) = ticks(&mut host, 1, 3);
    assert_eq!(
        message,
        "The focus session ended. Short break not started: 2 min"
    );
    assert_eq!(text(&data, "state"), "idle");
    // Nothing moves until Start.
    let (_, data) = ticks(&mut host, 4, 9);
    assert_eq!(
        (text(&data, "state"), int(&data, "remaining")),
        ("idle".into(), 2)
    );
    press(&mut host, "toggle");
    let (message, _) = ticks(&mut host, 10, 11);
    assert_eq!(
        message,
        "The short break ended. Focus 2 of 2 not started: 3 min"
    );
    let own = |a: &str, b: &str| (a.to_owned(), b.to_owned());
    assert_eq!(
        agent.borrow().shown,
        [
            own(
                "Focus session finished",
                "Next is a short break of 2 min. Start it when you are ready."
            ),
            own(
                "Break is over",
                "Start focus session 2 of 2 (3 min) when you are ready."
            ),
        ]
    );
}

#[test]
fn actions_make_few_calls() {
    let agent = agent(SHORT());
    let mut host = host(&agent);
    let count = |host: &MockHost, from: usize| -> (usize, usize, usize) {
        let requests = &host.requests()[from..];
        let of = |f: fn(&CapabilityRequest) -> bool| requests.iter().filter(|r| f(r)).count();
        (
            requests.len(),
            of(|r| matches!(r, CapabilityRequest::VariablesPublish(_))),
            of(|r| matches!(r, CapabilityRequest::Notify(_))),
        )
    };
    // A tick: settings, state, save and the eight variables.
    ticks(&mut host, 0, 0);
    assert_eq!(count(&host, 0), (11, 8, 0));
    // A key: settings, state, save and the timer's four variables, so a
    // key pressed in the second of a tick stays near 10 variables a second.
    let before = host.requests().len();
    press(&mut host, "start");
    assert_eq!(count(&host, before), (7, 4, 0));
    // A key that changes nothing does not write.
    let before = host.requests().len();
    press(&mut host, "start");
    assert_eq!(count(&host, before), (6, 4, 0));
    // The tick that ends a phase adds one notification.
    ticks(&mut host, 1, 2);
    let before = host.requests().len();
    ticks(&mut host, 3, 3);
    assert_eq!(count(&host, before), (12, 8, 1));
    let requests = &host.requests()[before..];
    assert!(matches!(requests[0], CapabilityRequest::SettingsGet(_)));
    assert!(matches!(requests[1], CapabilityRequest::StorageGet(_)));
    assert!(matches!(requests[2], CapabilityRequest::StorageSet(_)));
    assert!(matches!(requests[3], CapabilityRequest::Notify(_)));
    // Status reads and publishes everything.
    let before = host.requests().len();
    press(&mut host, "status");
    assert_eq!(count(&host, before), (10, 8, 0));
    assert!(!host.requests().iter().any(|r| matches!(
        r,
        CapabilityRequest::HttpFetch(_)
            | CapabilityRequest::OpenUrl(_)
            | CapabilityRequest::VariablesClear(_)
    )));
}

#[test]
fn notifications_are_polish_when_the_setting_says_so() {
    let agent = agent(short_with(json!({ "language": "pl" })));
    let mut host = running(&agent);
    ticks(&mut host, 1, 5);
    let own = |a: &str, b: &str| (a.to_owned(), b.to_owned());
    assert_eq!(
        agent.borrow().shown,
        [
            own(
                "Sesja skupienia zakończona",
                "Czas na krótką przerwę: 2 min."
            ),
            own(
                "Koniec przerwy",
                "Zaczęła się sesja skupienia 2 z 2: 3 min."
            ),
        ]
    );
    // Away, in Polish too.
    tick_at(&mut host, "2026-11-16 12:00");
    let (title, body) = agent.borrow().shown[2].clone();
    assert_eq!(title, "Pomodoro wstrzymane");
    assert!(body.contains("zostało 3 min"), "{body}");
    // The key's texts follow the setting; the action's message does not.
    let (message, data) = press(&mut host, "status");
    assert_eq!(text(&data, "keyDetail"), "Skupienie 2/2 · pauza");
    assert!(message.starts_with("Focus 2 of 2 paused"), "{message}");
}

#[test]
fn a_bad_or_missing_fired_at_fails_cleanly_and_changes_nothing() {
    let agent = agent(SHORT());
    let mut host = running(&agent);
    ticks(&mut host, 1, 2);
    let before = agent.borrow().store.clone();
    let calls = host.requests().len();
    for bad in [
        "",
        "soon",
        "{{ steps.start.output.firedAt }}",
        "2026-11-31 09:03",
        "09:03",
        "1763280180",
    ] {
        let (code, message, retryable) = failed(&mut host, "tick", json!({ "firedAt": bad }));
        assert_eq!(code, INVALID_FIRED_AT, "{bad:?}");
        assert!(message.contains("YYYY-MM-DD HH:MM"), "{message}");
        assert!(!message.contains("soon"), "the value is not repeated");
        assert!(!retryable);
    }
    // Missing, of another type, too long: the SDK refuses the parameters.
    for params in [
        json!({}),
        json!({ "firedAt": 5 }),
        json!({ "firedAt": "2026-11-16 09:03 and a lot more text than a time" }),
    ] {
        let (code, _, _) = failed(&mut host, "tick", params);
        assert_eq!(code, codes::INVALID_PARAMETERS);
    }
    assert_eq!(host.requests().len(), calls, "no capability was called");
    assert_eq!(agent.borrow().store, before);
    assert!(agent.borrow().shown.is_empty());
    // The next good tick goes on: the session ends at 09:03.
    let (_, data) = ticks(&mut host, 3, 3);
    assert_eq!(text(&data, "ended"), "focus");
}

#[test]
fn a_long_gap_between_ticks_pauses_the_timer_and_says_so_once() {
    let agent = agent(SHORT());
    let mut host = running(&agent);
    ticks(&mut host, 1, 1);
    // Lunch with the lid closed: 09:01 to 10:30.
    let (message, data) = ticks(&mut host, 90, 90);
    assert_eq!(
        message,
        "Focus 1 of 2 paused after a long pause between ticks: 2 min left"
    );
    assert_eq!(text(&data, "state"), "paused");
    assert_eq!(text(&data, "ended"), "none");
    assert!(flag(&data, "away") && flag(&data, "notified"));
    assert_eq!(int(&data, "completedTotal"), 0);
    let shown = agent.borrow().shown.clone();
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].0, "Pomodoro paused");
    assert!(
        shown[0].1.contains("paused with 2 min left"),
        "{}",
        shown[0].1
    );
    assert_eq!(
        agent.borrow().logs,
        ["a long pause between ticks was not counted; the timer is paused where it was"]
    );
    // It stays paused and quiet.
    let (_, data) = ticks(&mut host, 91, 100);
    assert_eq!(
        (text(&data, "state"), int(&data, "remaining")),
        ("paused".into(), 2)
    );
    assert!(flag(&data, "away"));
    assert_eq!(agent.borrow().shown.len(), 1);
    // Resume: the rest of the session.
    let (message, _) = press(&mut host, "resume");
    assert_eq!(message, "Focus 1 of 2: 2 min left");
    let (_, data) = ticks(&mut host, 101, 102);
    assert_eq!(text(&data, "ended"), "focus");
    assert!(!flag(&data, "away"));
    assert_eq!(int(&data, "completedTotal"), 1);
}

#[test]
fn a_night_between_ticks_starts_the_statistics_of_the_day_again() {
    let agent = agent(SHORT());
    let mut host = running(&agent);
    let (_, data) = ticks(&mut host, 1, 4);
    assert_eq!(
        (
            int(&data, "completedToday"),
            int(&data, "focusMinutesToday")
        ),
        (1, 3)
    );
    let (_, data) = press(&mut host, "status");
    assert_eq!(text(&data, "statisticsDate"), "2026-11-16");
    let (_, data) = tick_at(&mut host, "2026-11-17 08:00");
    assert_eq!(
        (
            int(&data, "completedToday"),
            int(&data, "focusMinutesToday"),
            int(&data, "completedTotal"),
            int(&data, "focusMinutesTotal")
        ),
        (0, 0, 1, 3)
    );
    assert_eq!(text(&data, "state"), "paused", "the break was running");
    let (_, data) = press(&mut host, "status");
    assert_eq!(text(&data, "statisticsDate"), "2026-11-17");
    assert_eq!(text(&data, "lastTick"), "2026-11-17 08:00");
}

#[test]
fn the_clock_going_backwards_is_survived() {
    let agent = agent(SHORT());
    let mut host = running(&agent);
    ticks(&mut host, 1, 2);
    let (_, data) = tick_at(&mut host, "2026-11-16 08:02");
    assert_eq!(
        (text(&data, "ended"), int(&data, "remaining")),
        ("none".into(), 1)
    );
    assert_eq!(
        agent.borrow().logs,
        ["the clock went back; the countdown goes on from here"]
    );
    let (_, data) = tick_at(&mut host, "2026-11-16 08:03");
    assert_eq!(text(&data, "ended"), "focus");
    assert_eq!(agent.borrow().shown.len(), 1);
}

#[test]
fn a_throttled_notification_is_sent_once_with_a_later_tick() {
    let agent = agent(short_with(json!({ "shortBreakMinutes": 5 })));
    let mut host = running(&agent);
    ticks(&mut host, 1, 2);
    agent.borrow_mut().notify = Notify::Throttled;
    let (message, data) = ticks(&mut host, 3, 3);
    assert_eq!(
        message,
        "The focus session ended. Short break: 5 min left. The notification was not shown: \
         more than 5 notifications a minute; it is sent with the next tick"
    );
    assert_eq!(text(&data, "ended"), "focus");
    assert!(!flag(&data, "notified"));
    assert!(text(&data, "reason").contains("5 notifications"));
    // The timer does not wait for the notification.
    assert_eq!(int(&data, "completedTotal"), 1);
    assert_eq!(
        stored(&agent).pending,
        Some(Pending {
            notice: Notice::FocusEnded,
            age: 0
        })
    );
    let (_, data) = ticks(&mut host, 4, 4);
    assert_eq!(
        (text(&data, "ended"), int(&data, "remaining")),
        ("none".into(), 4)
    );
    assert!(!flag(&data, "notified"));
    assert!(data.contains_key("reason"));
    assert!(agent.borrow().shown.is_empty());
    // The limit passes: shown once, and never again.
    agent.borrow_mut().notify = Notify::Shown;
    let (message, data) = ticks(&mut host, 5, 5);
    assert_eq!(message, "Short break: 3 min left");
    assert!(flag(&data, "notified"));
    assert!(!data.contains_key("reason"));
    ticks(&mut host, 6, 7);
    assert_eq!(
        agent.borrow().shown,
        [(
            "Focus session finished".to_owned(),
            "Time for a short break: 5 min.".to_owned()
        )]
    );
    assert_eq!(stored(&agent).pending, None);
    // The warnings name the operation and the limit, nothing else.
    let logs = agent.borrow().logs.clone();
    assert_eq!(logs.len(), 2);
    for line in &logs {
        assert!(
            line.starts_with("the notification was not sent and is tried again:"),
            "{line}"
        );
    }
}

#[test]
fn a_notification_that_is_refused_for_minutes_is_dropped() {
    let agent = agent(short_with(json!({ "shortBreakMinutes": 30 })));
    let mut host = running(&agent);
    ticks(&mut host, 1, 2);
    agent.borrow_mut().notify = Notify::Throttled;
    // Due at 09:03; tried at 09:03 and with the five ticks after it.
    for minute in 3..=8 {
        let (_, data) = ticks(&mut host, minute, minute);
        assert!(data.contains_key("reason"), "minute {minute}");
    }
    let (message, data) = ticks(&mut host, 9, 9);
    assert_eq!(message, "Short break: 24 min left");
    assert!(!flag(&data, "notified"));
    assert!(!data.contains_key("reason"));
    assert_eq!(stored(&agent).pending, None);
    assert_eq!(
        agent.borrow().logs.last().map(String::as_str),
        Some("a notification could not be sent for several minutes and was dropped")
    );
    // Allowed again: there is nothing old left to show.
    agent.borrow_mut().notify = Notify::Shown;
    ticks(&mut host, 10, 12);
    assert!(agent.borrow().shown.is_empty());
}

#[test]
fn a_notification_that_fails_otherwise_is_tried_again() {
    let agent = agent(SHORT());
    let mut host = running(&agent);
    ticks(&mut host, 1, 2);
    agent.borrow_mut().notify = Notify::Failed;
    let (_, data) = ticks(&mut host, 3, 3);
    assert!(!flag(&data, "notified"));
    assert!(text(&data, "reason").contains("tried again"));
    agent.borrow_mut().notify = Notify::Shown;
    let (_, data) = ticks(&mut host, 4, 4);
    assert!(flag(&data, "notified"));
    assert_eq!(agent.borrow().shown.len(), 1);
}

#[test]
fn a_revoked_notification_permission_is_not_permitted_and_the_timer_goes_on() {
    let agent = agent(SHORT());
    let mut host = running(&agent);
    ticks(&mut host, 1, 2);
    agent.borrow_mut().notify = Notify::Denied;
    let (code, message, _) = failed(&mut host, "tick", json!({ "firedAt": minute_text(3) }));
    assert_eq!(code, codes::NOT_PERMITTED);
    assert!(message.contains("`notifications` permission"), "{message}");
    // The session ended and was counted; only the notification waits.
    let state = stored(&agent);
    assert_eq!(
        (
            state.phase,
            state.timer,
            state.left,
            state.stats.sessions_total
        ),
        (Phase::ShortBreak, Timer::Running, 2, 1)
    );
    assert!(state.pending.is_some());
    // Granted again: shown with the next tick.
    agent.borrow_mut().notify = Notify::Shown;
    let (_, data) = ticks(&mut host, 4, 4);
    assert!(flag(&data, "notified"));
    assert_eq!(int(&data, "remaining"), 1);
    assert_eq!(agent.borrow().shown.len(), 1);
    // The keys never need the permission.
    agent.borrow_mut().notify = Notify::Denied;
    for action in [
        "pause", "resume", "toggle", "skip", "reset", "status", "start",
    ] {
        press(&mut host, action);
    }
}

#[test]
fn a_notification_the_settings_show_nowhere_is_not_repeated() {
    let agent = agent(SHORT());
    let mut host = running(&agent);
    ticks(&mut host, 1, 2);
    agent.borrow_mut().notify = Notify::Nowhere;
    let (message, data) = ticks(&mut host, 3, 3);
    assert!(!flag(&data, "notified"));
    assert_eq!(text(&data, "ended"), "focus");
    assert!(text(&data, "reason").contains("desktop notifications are off"));
    assert!(message.contains("not shown"), "{message}");
    // The user chose that: nothing waits.
    assert_eq!(stored(&agent).pending, None);
    let (_, data) = ticks(&mut host, 4, 4);
    assert!(!data.contains_key("reason"));
}

#[test]
fn older_agents_are_served() {
    for mode in [Notify::Old, Notify::NoSeverity] {
        let agent = agent(SHORT());
        agent.borrow_mut().notify = mode;
        let mut host = running(&agent);
        let (_, data) = ticks(&mut host, 1, 3);
        assert!(flag(&data, "notified"), "{mode:?}");
        assert_eq!(agent.borrow().shown.len(), 1, "{mode:?}");
    }
}

const ALL_ACTIONS: [&str; 8] = [
    "tick", "start", "pause", "resume", "toggle", "skip", "reset", "status",
];

fn params_of(action: &str) -> Value {
    if action == "tick" {
        json!({ "firedAt": minute_text(3) })
    } else {
        json!({})
    }
}

#[test]
fn storage_that_cannot_be_read_fails_every_action_and_changes_nothing() {
    let agent = agent(SHORT());
    let mut host = running(&agent);
    ticks(&mut host, 1, 2);
    let before = agent.borrow().store.clone();
    agent.borrow_mut().fail_get = true;
    for action in ALL_ACTIONS {
        let (code, message, retryable) = failed(&mut host, action, params_of(action));
        assert_eq!(code, STORAGE_UNAVAILABLE, "{action}");
        assert_eq!(message, "the plugin's storage could not be read; try again");
        assert!(retryable);
    }
    assert!(agent.borrow().shown.is_empty());
    assert_eq!(agent.borrow().store, before);
    assert!(agent.borrow().variables["remaining"] == json!(1));
    // It works again: the session ends, once.
    agent.borrow_mut().fail_get = false;
    let (_, data) = ticks(&mut host, 3, 3);
    assert_eq!(text(&data, "ended"), "focus");
    assert_eq!(agent.borrow().shown.len(), 1);
}

#[test]
fn storage_that_cannot_be_written_never_repeats_a_notification() {
    let agent = agent(short_with(json!({ "awayMinutes": 10 })));
    let mut host = running(&agent);
    ticks(&mut host, 1, 2);
    agent.borrow_mut().sets_left = Some(0);
    // The session ends at 09:03; the state cannot be saved for three
    // ticks. Nothing is shown, so nothing can repeat every minute.
    for minute in 3..=5 {
        let (code, message, retryable) =
            failed(&mut host, "tick", json!({ "firedAt": minute_text(minute) }));
        assert_eq!(code, STORAGE_UNAVAILABLE);
        assert_eq!(
            message,
            "the plugin's storage could not be written; try again"
        );
        assert!(retryable);
    }
    assert!(agent.borrow().shown.is_empty());
    assert_eq!(stored(&agent).stats.sessions_total, 0);
    // A key cannot be saved either, and says so.
    let (code, _, _) = failed(&mut host, "pause", json!({}));
    assert_eq!(code, STORAGE_UNAVAILABLE);
    // It works again. 09:02 was the last tick saved, so the minutes since
    // are counted now: the session ends once and is counted once.
    agent.borrow_mut().sets_left = None;
    let (_, data) = ticks(&mut host, 6, 6);
    assert_eq!(text(&data, "ended"), "focus");
    assert_eq!(int(&data, "completedTotal"), 1);
    assert_eq!(agent.borrow().shown.len(), 1);
    // The quota keeps its shared code.
    agent.borrow_mut().quota = true;
    let (code, message, _) = failed(&mut host, "pause", json!({}));
    assert_eq!(code, codes::STORAGE_QUOTA);
    assert!(message.contains("storage is full"), "{message}");

    // A refused notification and the state cannot be put back: an error,
    // and the notification is lost rather than repeated.
    let agent = self::agent(SHORT());
    let mut host = running(&agent);
    ticks(&mut host, 1, 2);
    agent.borrow_mut().notify = Notify::Throttled;
    let saved = sets(&host);
    agent.borrow_mut().sets_left = Some(1);
    let (code, _, _) = failed(&mut host, "tick", json!({ "firedAt": minute_text(3) }));
    assert_eq!(code, STORAGE_UNAVAILABLE);
    assert_eq!(sets(&host), saved + 2);
    let state = stored(&agent);
    assert_eq!((state.phase, state.pending), (Phase::ShortBreak, None));
    assert_eq!(state.stats.sessions_total, 1);
}

#[test]
fn damaged_stored_state_starts_again_with_a_warning() {
    let agent = agent(SHORT());
    agent.borrow_mut().store.insert(
        STATE_KEY.to_owned(),
        json!({ "version": 99, "secret": "do not log me" }),
    );
    let mut host = host(&agent);
    let (message, _) = tick_at(&mut host, "2026-11-16 09:00");
    assert_eq!(message, "Focus 1 of 2 not started: 3 min");
    assert_eq!(
        agent.borrow().logs,
        ["the stored timer state could not be used; the timer and its statistics start again"]
    );
    assert_eq!(stored(&agent).version, STATE_VERSION);
}

#[test]
fn variables_that_cannot_be_published_do_not_fail_an_action() {
    let agent = agent(SHORT());
    agent.borrow_mut().fail_variables = true;
    let mut host = running(&agent);
    let (_, data) = ticks(&mut host, 1, 3);
    assert!(flag(&data, "notified"));
    for action in [
        "pause", "resume", "toggle", "skip", "reset", "status", "start",
    ] {
        press(&mut host, action);
    }
    assert!(agent.borrow().variables.is_empty());
}

#[test]
fn start_pause_resume_and_toggle_from_a_key() {
    let agent = agent(SHORT());
    let mut host = host(&agent);
    // Before any tick: the key works, and the message says what is missing.
    let (message, data) = press(&mut host, "start");
    assert_eq!(
        message,
        "Focus 1 of 2: 3 min left (no tick received yet: add the every-minute workflow from the plugin's README)"
    );
    assert!(flag(&data, "changed"));
    assert_eq!(text(&data, "state"), "running");
    assert_eq!(stored(&agent).timer, Timer::Running);
    ticks(&mut host, 0, 1);

    let (message, data) = press(&mut host, "start");
    assert_eq!(message, "Already running. Focus 1 of 2: 2 min left");
    assert!(!flag(&data, "changed"));

    let (message, data) = press(&mut host, "pause");
    assert_eq!(message, "Focus 1 of 2 paused: 2 min left");
    assert!(flag(&data, "changed"));
    assert_eq!(agent.borrow().variables["state"], json!("paused"));
    let (message, data) = press(&mut host, "pause");
    assert_eq!(
        message,
        "Not running, nothing to pause. Focus 1 of 2 paused: 2 min left"
    );
    assert!(!flag(&data, "changed"));
    let (_, data) = ticks(&mut host, 2, 6);
    assert_eq!(
        (text(&data, "state"), int(&data, "remaining")),
        ("paused".into(), 2)
    );

    let (message, data) = press(&mut host, "resume");
    assert_eq!(message, "Focus 1 of 2: 2 min left");
    assert!(flag(&data, "changed"));
    let (message, data) = press(&mut host, "resume");
    assert_eq!(
        message,
        "Not paused, nothing to resume. Focus 1 of 2: 2 min left"
    );
    assert!(!flag(&data, "changed"));
    // Start also continues a paused timer.
    press(&mut host, "pause");
    let (_, data) = press(&mut host, "start");
    assert!(flag(&data, "changed"));
    assert_eq!(text(&data, "state"), "running");

    // One key: pause, resume, and start a phase that waits.
    let writes = sets(&host);
    let states: Vec<String> = (0..4)
        .map(|_| text(&press(&mut host, "toggle").1, "state"))
        .collect();
    assert_eq!(states, ["paused", "running", "paused", "running"]);
    assert_eq!(sets(&host), writes + 4);
    press(&mut host, "reset");
    let (message, data) = press(&mut host, "toggle");
    assert_eq!(message, "Focus 1 of 2: 3 min left");
    assert!(flag(&data, "changed"));

    // Resume does not start a phase that waits; Start does.
    press(&mut host, "reset");
    let writes = sets(&host);
    let (message, data) = press(&mut host, "resume");
    assert_eq!(
        message,
        "Not paused, nothing to resume. Focus 1 of 2 not started: 3 min"
    );
    assert!(!flag(&data, "changed"));
    assert_eq!(
        sets(&host),
        writes,
        "a key that changes nothing writes nothing"
    );
    // The keys publish the timer's variables, not the statistics.
    agent.borrow_mut().variables.clear();
    press(&mut host, "start");
    assert_eq!(
        agent
            .borrow()
            .variables
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["phase", "remaining", "session", "state"]
    );
}

#[test]
fn skip_and_reset_from_a_key() {
    let agent = agent(SHORT());
    let mut host = running(&agent);
    ticks(&mut host, 1, 1);
    let (message, data) = press(&mut host, "skip");
    assert_eq!(
        message,
        "Skipped the focus session. Short break: 2 min left"
    );
    assert_eq!(text(&data, "skipped"), "focus");
    assert_eq!(
        (
            text(&data, "phase"),
            text(&data, "state"),
            int(&data, "completedTotal")
        ),
        ("shortBreak".into(), "running".into(), 0)
    );
    let (message, data) = press(&mut host, "skip");
    assert_eq!(message, "Skipped the short break. Focus 2 of 2: 3 min left");
    assert_eq!(text(&data, "skipped"), "shortBreak");
    // Complete session 2, then skip the long break.
    let (_, data) = ticks(&mut host, 2, 4);
    assert_eq!(text(&data, "ended"), "focus");
    assert_eq!(text(&data, "phase"), "longBreak");
    let (message, _) = press(&mut host, "skip");
    assert_eq!(message, "Skipped the long break. Focus 1 of 2: 3 min left");

    // Reset: the phase by default.
    ticks(&mut host, 5, 6);
    let (message, data) = press(&mut host, "reset");
    assert_eq!(message, "Reset the phase. Focus 1 of 2 not started: 3 min");
    assert_eq!(
        (text(&data, "state"), int(&data, "remaining")),
        ("idle".into(), 3)
    );
    assert_eq!(int(&data, "completedTotal"), 1);
    // The cycle.
    press(&mut host, "start");
    ticks(&mut host, 7, 9);
    press(&mut host, "skip");
    let (_, data) = press(&mut host, "status");
    assert_eq!(
        (text(&data, "phase"), int(&data, "session")),
        ("focus".into(), 2)
    );
    let (message, data) = ok(&mut host, "reset", json!({ "scope": "cycle" }));
    assert_eq!(message, "Reset the cycle. Focus 1 of 2 not started: 3 min");
    assert_eq!(
        (int(&data, "session"), int(&data, "completedTotal")),
        (1, 2)
    );
    // The statistics: the timer is not touched.
    press(&mut host, "start");
    ticks(&mut host, 10, 10);
    let (message, data) = ok(&mut host, "reset", json!({ "scope": "statistics" }));
    assert_eq!(message, "Reset the statistics. Focus 1 of 2: 2 min left");
    assert_eq!(
        (
            int(&data, "completedToday"),
            int(&data, "focusMinutesToday"),
            int(&data, "completedTotal"),
            int(&data, "focusMinutesTotal")
        ),
        (0, 0, 0, 0)
    );
    assert_eq!(agent.borrow().variables["completedTotal"], json!(0));
    // Everything.
    ticks(&mut host, 11, 12);
    let (message, data) = ok(&mut host, "reset", json!({ "scope": "all" }));
    assert_eq!(
        message,
        "Reset the cycle and the statistics. Focus 1 of 2 not started: 3 min"
    );
    assert_eq!(
        (int(&data, "completedTotal"), int(&data, "session")),
        (0, 1)
    );
    // An unknown scope is refused by the SDK and changes nothing.
    let before = agent.borrow().store.clone();
    let (code, _, _) = failed(&mut host, "reset", json!({ "scope": "everything" }));
    assert_eq!(code, codes::INVALID_PARAMETERS);
    assert_eq!(agent.borrow().store, before);
}

#[test]
fn status_reports_what_a_key_would_show_and_stores_nothing() {
    let agent = agent(SHORT());
    let mut host = host(&agent);
    let (message, data) = press(&mut host, "status");
    assert!(
        message.starts_with("Focus 1 of 2 not started: 3 min (no tick"),
        "{message}"
    );
    assert_eq!(text(&data, "state"), "idle");
    assert!(!data.contains_key("lastTick") && !data.contains_key("statisticsDate"));
    assert!(agent.borrow().store.is_empty());

    ticks(&mut host, 0, 0);
    press(&mut host, "start");
    ticks(&mut host, 1, 4);
    let before = agent.borrow().store.clone();
    let writes = sets(&host);
    let (message, data) = press(&mut host, "status");
    assert_eq!(message, "Short break: 1 min left");
    assert_eq!(
        (
            text(&data, "state"),
            text(&data, "phase"),
            int(&data, "remaining")
        ),
        ("running".into(), "shortBreak".into(), 1)
    );
    assert_eq!(
        (
            int(&data, "session"),
            int(&data, "sessionsPerCycle"),
            int(&data, "phaseMinutes"),
            int(&data, "progress")
        ),
        (1, 2, 2, 50)
    );
    assert_eq!(
        (text(&data, "keyLabel"), text(&data, "keyDetail")),
        ("1 min".into(), "Short break".into())
    );
    assert_eq!(
        (text(&data, "lastTick"), text(&data, "statisticsDate")),
        ("2026-11-16 09:04".into(), "2026-11-16".into())
    );
    assert!(!flag(&data, "away"));
    assert_eq!(
        (
            int(&data, "completedToday"),
            int(&data, "focusMinutesTotal")
        ),
        (1, 3)
    );
    assert_eq!(agent.borrow().store, before);
    assert_eq!(sets(&host), writes);
    // A changed setting shows at once for a phase that waits.
    press(&mut host, "reset");
    agent.borrow_mut().settings = short_with(json!({ "shortBreakMinutes": 9 }));
    let (message, _) = press(&mut host, "status");
    assert_eq!(message, "Short break not started: 9 min");
}

#[test]
fn a_disabled_plugin_fails_cleanly() {
    // A disabled plugin is denied everything: no panic, a shared code.
    for action in ALL_ACTIONS {
        let mut denied = MockHost::deny_all();
        let out = denied.invoke(&plugin(), action, params_of(action));
        assert!(
            matches!(&out, ActionOutcome::Failed { code, .. } if code == codes::NOT_PERMITTED),
            "{action}: {out:?}"
        );
    }
    // The settings answer and the storage is denied: the same.
    let mut host = MockHost::new(|request| match request {
        CapabilityRequest::SettingsGet(_) => CallResult::Ok(json!({})),
        _ => CallResult::error(CallErrorCode::NotPermitted, "the plugin is disabled"),
    });
    let out = host.invoke(&plugin(), "status", json!({}));
    assert!(
        matches!(&out, ActionOutcome::Failed { code, .. } if code == codes::NOT_PERMITTED),
        "{out:?}"
    );
}

#[test]
fn a_day_of_work_is_counted_as_it_was_done() {
    // 09:00 to 17:00 with the default settings (25/5/15, 4 sessions, breaks
    // start by themselves, focus sessions wait for Start), away after 10.
    // The user starts every focus session a minute after it is ready,
    // pauses for ten minutes, closes the lid over lunch and resumes after
    // it, and skips one session.
    let agent = agent(json!({ "awayMinutes": 10 }));
    let mut host = host(&agent);
    let mut shown_at: Vec<(u32, String)> = Vec::new();
    for minute in 0..=480_u32 {
        if (180..225).contains(&minute) {
            continue; // lunch: the computer sleeps
        }
        let before = agent.borrow().shown.len();
        ticks(&mut host, minute, minute);
        let after = agent.borrow().shown.len();
        assert!(after - before <= 1, "minute {minute}");
        if after > before {
            shown_at.push((minute, agent.borrow().shown[after - 1].0.clone()));
        }
        let state = stored(&agent);
        match minute {
            100 => drop(press(&mut host, "pause")),
            110 => drop(press(&mut host, "resume")),
            226 => {
                assert!(state.away, "the lunch paused the timer");
                press(&mut host, "resume");
            }
            300 => drop(press(&mut host, "skip")),
            _ if state.timer == Timer::Idle && state.phase == Phase::Focus => {
                press(&mut host, "start");
            }
            _ => {}
        }
    }
    let state = stored(&agent);
    let count = |title: &str| shown_at.iter().filter(|(_, t)| t == title).count();
    let finished = count("Focus session finished");
    // Every completed session was announced and counted, with its minutes.
    assert_eq!(state.stats.sessions_today as usize, finished);
    assert_eq!(state.stats.minutes_today as usize, finished * 25);
    assert_eq!(
        state.stats.sessions_total,
        u64::from(state.stats.sessions_today)
    );
    // 480 minutes less lunch (45), the pause (10) and the skipped part:
    // between 11 and 14 sessions of 25 minutes with their breaks.
    assert!((11..=14).contains(&finished), "{shown_at:?}");
    assert_eq!(count("Pomodoro paused"), 1);
    assert_eq!(
        finished + count("Break is over") + 1,
        shown_at.len(),
        "{shown_at:?}"
    );
    // Nothing ended while paused or away, and the first thing after lunch
    // is the notice that the timer is paused.
    assert!(
        shown_at
            .iter()
            .all(|(m, _)| !(101..=110).contains(m) && !(180..225).contains(m)),
        "{shown_at:?}"
    );
    assert!(shown_at.contains(&(225, "Pomodoro paused".to_owned())));
    // Two phases never end closer than the shortest phase.
    assert!(
        shown_at.windows(2).all(|w| w[1].0 - w[0].0 >= 1),
        "{shown_at:?}"
    );
    assert!(agent.borrow().logs.len() == 1, "{:?}", agent.borrow().logs);
}
