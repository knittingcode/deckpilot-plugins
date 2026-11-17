//! Native unit tests: the scheduling logic as pure functions, and every
//! action with the agent replaced by a scripted `MockHost`.
//! `tools/first-party.sh` adds what a unit test cannot: the manifest against
//! the schema, the Polish texts, the wasm build and the package checks.

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
    Config::from_settings(settings.as_object().expect("an object")).expect("valid settings")
}

/// Break every 50, water every 60, away after 5: the defaults.
fn defaults() -> Config {
    Config::default()
}

// ----------------------------------------------------------------- time

#[test]
fn fired_at_is_parsed_as_the_agent_writes_it() {
    let a = at("2026-11-16 09:30");
    assert_eq!(a.minute_of_day(), 9 * 60 + 30);
    assert_eq!(a.text(), "2026-11-16 09:30");
    assert_eq!(at("2026-11-16 09:31").0 - a.0, 1);
    // Midnight, month and year ends are one minute apart.
    assert_eq!(at("2026-11-17 00:00").0 - at("2026-11-16 23:59").0, 1);
    assert_eq!(at("2026-12-01 00:00").0 - at("2026-11-30 23:59").0, 1);
    assert_eq!(at("2027-01-01 00:00").0 - at("2026-12-31 23:59").0, 1);
    // Leap days exist only in leap years.
    assert_eq!(
        at("2028-03-01 00:00").0 - at("2028-02-28 00:00").0,
        2 * 1440
    );
    assert_eq!(at("2027-03-01 00:00").0 - at("2027-02-28 00:00").0, 1440);
    assert_eq!(at("1970-01-01 00:00"), Stamp(0));
    for text in ["2000-02-29 12:00", "2026-01-31 23:59", "9999-12-31 23:59"] {
        assert_eq!(at(text).text(), text);
    }
}

#[test]
fn fired_at_tolerates_what_a_person_or_another_tool_writes() {
    let expected = at("2026-11-16 09:30");
    for text in [
        " 2026-11-16 09:30 ",
        "2026-11-16T09:30",
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

#[test]
fn times_of_day() {
    assert_eq!(parse_time_of_day("00:00"), Some(0));
    assert_eq!(parse_time_of_day("7:05"), Some(425));
    assert_eq!(parse_time_of_day(" 22:00 "), Some(1320));
    assert_eq!(parse_time_of_day("23:59"), Some(1439));
    for text in [
        "", "22", "24:00", "12:60", "12:5", "1200", "-1:00", "ab:cd", "12:00:00",
    ] {
        assert_eq!(parse_time_of_day(text), None, "{text:?}");
    }
}

// ------------------------------------------------------------- settings

#[test]
fn settings_have_defaults_and_bounds() {
    assert_eq!(config(json!({})), defaults());
    let c = config(json!({
        "breakEnabled": true, "breakMinutes": 25,
        "hydrationEnabled": false, "hydrationMinutes": 90,
        "awayMinutes": 15, "language": "pl",
        "quietStart": "22:00", "quietEnd": "07:00"
    }));
    assert_eq!(c.break_minutes, Some(25));
    assert_eq!(c.hydration_minutes, None);
    assert_eq!(c.away_minutes, 15);
    assert_eq!(c.language, Language::Polish);
    assert_eq!(
        c.quiet,
        Some(Quiet {
            start: 1320,
            end: 420
        })
    );
    // The agent validates saved values; values out of range or of another
    // type still cannot make a countdown of 0 or of a year.
    let c = config(json!({
        "breakMinutes": 0, "hydrationMinutes": 100_000, "awayMinutes": -4,
        "language": 7, "breakEnabled": "yes"
    }));
    assert_eq!(c.break_minutes, Some(BREAK_MINUTES.0));
    assert_eq!(c.hydration_minutes, Some(HYDRATION_MINUTES.1));
    assert_eq!(c.away_minutes, AWAY_MINUTES.0);
    assert_eq!(c.language, Language::English);
}

#[test]
fn quiet_hours_settings() {
    // Empty, blank and equal times: no quiet hours.
    for settings in [
        json!({ "quietStart": "", "quietEnd": "" }),
        json!({ "quietStart": " ", "quietEnd": "  " }),
        json!({ "quietStart": "12:00", "quietEnd": "12:00" }),
    ] {
        assert_eq!(config(settings).quiet, None);
    }
    // Half of them, or not a time: an error that names the setting and
    // does not repeat what was typed.
    for (settings, names) in [
        (json!({ "quietStart": "22:00" }), "both settings"),
        (
            json!({ "quietEnd": "07:00", "quietStart": "" }),
            "both settings",
        ),
        (
            json!({ "quietStart": "10pm", "quietEnd": "07:00" }),
            "Quiet hours from",
        ),
        (
            json!({ "quietStart": "22:00", "quietEnd": "25:00" }),
            "Quiet hours until",
        ),
    ] {
        let error = Config::from_settings(settings.as_object().unwrap()).unwrap_err();
        assert!(error.contains(names), "{error}");
        assert!(
            !error.contains("10pm") && !error.contains("25:00"),
            "{error}"
        );
    }
}

#[test]
fn quiet_hours_within_a_day_and_across_midnight() {
    let lunch = Quiet {
        start: 12 * 60,
        end: 13 * 60,
    };
    assert!(!lunch.contains(11 * 60 + 59));
    assert!(lunch.contains(12 * 60));
    assert!(lunch.contains(12 * 60 + 59));
    assert!(!lunch.contains(13 * 60), "the end is not included");
    let night = Quiet {
        start: 22 * 60,
        end: 7 * 60,
    };
    assert!(!night.contains(21 * 60 + 59));
    assert!(night.contains(22 * 60));
    assert!(night.contains(23 * 60 + 59));
    assert!(night.contains(0));
    assert!(night.contains(6 * 60 + 59));
    assert!(!night.contains(7 * 60));
    assert!(!night.contains(12 * 60));
}

// ------------------------------------------------- the scheduling logic

/// Ticks every minute from `from` for `minutes` minutes (the first tick is
/// one minute after `from`), sending what is due. Returns the minutes
/// after `from` at which something was due, and what.
fn run(state: &mut State, config: &Config, from: Stamp, minutes: i64) -> Vec<(i64, &'static str)> {
    let mut fired = Vec::new();
    for minute in 1..=minutes {
        let advance = state.advance(config, Stamp(from.0 + minute));
        state.shown(config, advance.due);
        if !advance.due.is_empty() {
            fired.push((minute, advance.due.name()));
        }
    }
    fired
}

/// A state whose first tick was at `start`.
fn started(config: &Config, start: Stamp) -> State {
    let mut state = State::new();
    let advance = state.advance(config, start);
    assert_eq!(advance.note, Note::First);
    assert!(
        advance.due.is_empty(),
        "the first tick only starts the countdowns"
    );
    state
}

#[test]
fn reminders_come_at_their_intervals() {
    let config = defaults();
    let start = at("2026-11-16 09:00");
    let mut state = started(&config, start);
    assert_eq!((state.break_.left, state.hydration.left), (50, 60));
    let fired = run(&mut state, &config, start, 300);
    assert_eq!(
        fired,
        [
            (50, "break"),
            (60, "hydration"),
            (100, "break"),
            (120, "hydration"),
            (150, "break"),
            (180, "hydration"),
            (200, "break"),
            (240, "hydration"),
            (250, "break"),
            // Both due in the same minute: one notification for both.
            (300, "both"),
        ]
    );
    assert_eq!((state.break_.shown, state.hydration.shown), (6, 5));
    assert_eq!(state.last_reminder, Which::BOTH);
}

#[test]
fn a_reminder_that_is_off_never_comes() {
    let config = config(json!({ "breakEnabled": false, "hydrationMinutes": 30 }));
    let start = at("2026-11-16 09:00");
    let mut state = started(&config, start);
    assert_eq!(
        run(&mut state, &config, start, 90),
        [(30, "hydration"), (60, "hydration"), (90, "hydration")]
    );
    let snapshot = Snapshot::of(&state, &config);
    assert_eq!(
        (snapshot.break_left, snapshot.hydration_left),
        (None, Some(30))
    );

    let none = config_off();
    let mut state = started(&none, start);
    assert_eq!(run(&mut state, &none, start, 600), []);
}

fn config_off() -> Config {
    config(json!({ "breakEnabled": false, "hydrationEnabled": false }))
}

#[test]
fn changing_an_interval_or_turning_a_reminder_on_starts_its_countdown_again() {
    let start = at("2026-11-16 09:00");
    let mut state = started(&defaults(), start);
    run(&mut state, &defaults(), start, 20);
    assert_eq!((state.break_.left, state.hydration.left), (30, 40));
    // Only the changed one starts again.
    let shorter = config(json!({ "breakMinutes": 25 }));
    state.sync(&shorter);
    assert_eq!((state.break_.left, state.hydration.left), (25, 40));
    // Off, then on again: a full interval, not what was left.
    state.sync(&config(
        json!({ "breakMinutes": 25, "hydrationEnabled": false }),
    ));
    assert_eq!(state.hydration.left, 0);
    state.sync(&shorter);
    assert_eq!(state.hydration.left, 60);
}

#[test]
fn a_tick_interval_longer_than_a_minute_works() {
    // A workflow that fires every 3 minutes: reminders come at the first
    // tick at or after their time.
    let config = config(json!({ "breakMinutes": 10, "hydrationEnabled": false }));
    let start = at("2026-11-16 09:00");
    let mut state = started(&config, start);
    let mut fired = Vec::new();
    for minute in (3..=30).step_by(3) {
        let advance = state.advance(&config, Stamp(start.0 + minute));
        state.shown(&config, advance.due);
        if !advance.due.is_empty() {
            fired.push(minute);
        }
    }
    assert_eq!(fired, [12, 24]);
}

#[test]
fn quiet_hours_hold_the_countdowns_and_show_nothing() {
    let config = config(json!({
        "breakMinutes": 30, "hydrationEnabled": false,
        "quietStart": "12:00", "quietEnd": "13:00"
    }));
    let start = at("2026-11-16 11:40");
    let mut state = started(&config, start);
    // 20 minutes pass before noon, then the hour of quiet does not count:
    // the reminder comes 10 working minutes after 13:00, at 13:10.
    let fired = run(&mut state, &config, start, 120);
    assert_eq!(fired, [(90, "break"), (120, "break")]);
    let mut probe = started(&config, at("2026-11-16 11:59"));
    assert!(!probe.quiet);
    probe.advance(&config, at("2026-11-16 12:00"));
    assert!(probe.quiet);
    assert_eq!(Snapshot::of(&probe, &config).phase, Phase::Quiet);
    probe.advance(&config, at("2026-11-16 12:02"));
    assert_eq!(
        probe.break_.left,
        30 - 1,
        "only the minute before noon counted"
    );
    // The minute that ends the quiet hours was a quiet one.
    let mut probe = started(&config, at("2026-11-16 12:59"));
    probe.advance(&config, at("2026-11-16 13:00"));
    assert_eq!((probe.quiet, probe.break_.left), (false, 30));
    probe.advance(&config, at("2026-11-16 13:01"));
    assert_eq!(probe.break_.left, 29);
}

#[test]
fn quiet_hours_across_midnight() {
    // Ticks every minute all night (the computer stays on): nothing is
    // shown between 22:00 and 07:00, on either side of midnight.
    let config = config(json!({
        "breakMinutes": 20, "hydrationMinutes": 45,
        "quietStart": "22:00", "quietEnd": "07:00"
    }));
    let start = at("2026-11-16 21:30");
    let mut state = started(&config, start);
    let fired = run(&mut state, &config, start, 11 * 60);
    let clock: Vec<String> = fired
        .iter()
        .map(|(minute, what)| format!("{} {what}", &Stamp(start.0 + minute).text()[11..]))
        .collect();
    // 21:50 is the only one before the night. 10 of the next 20 minutes
    // and 30 of the 45 pass before 22:00; the rest after 07:00.
    assert_eq!(
        clock,
        [
            "21:50 break",
            "07:10 break",
            "07:15 hydration",
            "07:30 break",
            "07:50 break",
            "08:00 hydration",
            "08:10 break",
            "08:30 break"
        ]
    );
    // A reminder that was due and could not be sent before the quiet
    // hours waits for their end; it is not sent during them.
    let begin = at("2026-11-16 21:39");
    let mut state = started(&config, begin);
    let mut due_at = Vec::new();
    for minute in 1..=(9 * 60 + 22) {
        // Never marked as shown: the notification fails each time.
        let now = Stamp(begin.0 + minute);
        if !state.advance(&config, now).due.is_empty() {
            due_at.push(now.text()[11..].to_owned());
        }
    }
    assert_eq!(due_at, ["21:59", "07:00", "07:01"]);
}

#[test]
fn a_long_gap_is_a_break_and_never_a_burst() {
    let config = defaults();
    let start = at("2026-11-16 09:00");
    let mut state = started(&config, start);
    run(&mut state, &config, start, 40);
    assert_eq!((state.break_.left, state.hydration.left), (10, 20));
    // The computer sleeps for nine hours: both reminders were due many
    // times over. The first tick after it shows nothing and starts again.
    let woke = at("2026-11-16 18:40");
    let advance = state.advance(&config, woke);
    assert_eq!(advance.note, Note::Away);
    assert!(advance.due.is_empty() && advance.passed_over.is_empty());
    assert_eq!((state.break_.left, state.hydration.left), (50, 60));
    // From there the usual rhythm: one reminder per interval.
    assert_eq!(
        run(&mut state, &config, woke, 60),
        [(50, "break"), (60, "hydration")]
    );
    // The same for several days.
    let advance = state.advance(&config, at("2026-11-20 08:00"));
    assert_eq!(advance.note, Note::Away);
    assert!(advance.due.is_empty());
}

#[test]
fn a_short_gap_counts_and_shows_at_most_one_reminder_for_both() {
    // Away after 30 minutes; two ticks were missed while both reminders
    // became due: one notification, not two and not one per missed tick.
    let config = config(json!({ "breakMinutes": 10, "hydrationMinutes": 12, "awayMinutes": 30 }));
    let start = at("2026-11-16 09:00");
    let mut state = started(&config, start);
    let advance = state.advance(&config, at("2026-11-16 09:29"));
    assert_eq!(advance.note, Note::Counted);
    assert_eq!(advance.due, Which::BOTH);
    state.shown(&config, advance.due);
    assert_eq!((state.break_.left, state.hydration.left), (10, 12));
    assert_eq!((state.break_.shown, state.hydration.shown), (1, 1));
    // The gap that is exactly the away time is a break.
    let advance = state.advance(&config, at("2026-11-16 09:59"));
    assert_eq!(advance.note, Note::Away);
    assert!(advance.due.is_empty());
}

#[test]
fn the_clock_going_backwards_takes_nothing_off_and_shows_nothing() {
    let config = defaults();
    let start = at("2026-10-25 02:00");
    let mut state = started(&config, start);
    run(&mut state, &config, start, 45);
    assert_eq!(state.break_.left, 5);
    // Daylight saving time ends: 02:45 is followed by 02:00.
    let advance = state.advance(&config, at("2026-10-25 02:00"));
    assert_eq!(advance.note, Note::ClockWentBack);
    assert!(advance.due.is_empty());
    assert_eq!((state.break_.left, state.hydration.left), (5, 15));
    // The countdowns go on from the new time.
    assert_eq!(
        run(&mut state, &config, at("2026-10-25 02:00"), 15),
        [(5, "break"), (15, "hydration")]
    );
    // A clock set back by years is the same.
    let advance = state.advance(&config, at("1999-01-01 00:00"));
    assert_eq!(advance.note, Note::ClockWentBack);
    assert!(advance.due.is_empty());
    assert_eq!(state.last_tick, Some(at("1999-01-01 00:00").0));
}

#[test]
fn the_same_minute_twice_counts_once() {
    let config = defaults();
    let start = at("2026-11-16 09:00");
    let mut state = started(&config, start);
    let first = state.advance(&config, at("2026-11-16 09:01"));
    let again = state.advance(&config, at("2026-11-16 09:01"));
    assert_eq!((first.note, again.note), (Note::Counted, Note::SameMinute));
    assert_eq!(state.break_.left, 49);
}

#[test]
fn a_reminder_that_was_not_sent_stays_due_and_is_one_reminder() {
    let config = config(json!({ "breakMinutes": 5, "hydrationEnabled": false }));
    let start = at("2026-11-16 09:00");
    let mut state = started(&config, start);
    assert_eq!(run(&mut state, &config, start, 4), []);
    // Due at 09:05; the notification is refused for three ticks.
    for minute in 5..8 {
        let advance = state.advance(&config, Stamp(start.0 + minute));
        assert_eq!(advance.due, Which::BREAK, "minute {minute}");
    }
    assert_eq!(state.break_.shown, 0);
    let advance = state.advance(&config, Stamp(start.0 + 8));
    state.shown(&config, advance.due);
    assert_eq!((state.break_.shown, state.break_.left), (1, 5));
    assert!(state.advance(&config, Stamp(start.0 + 9)).due.is_empty());
}

#[test]
fn snooze_makes_a_reminder_due_in_the_given_minutes() {
    let config = defaults();
    let start = at("2026-11-16 09:00");
    let mut state = started(&config, start);
    assert_eq!(run(&mut state, &config, start, 50), [(50, "break")]);
    // "Remind me again in 10 minutes", pressed after the reminder.
    assert_eq!(state.snooze(&config, state.last_reminder, 10), Which::BREAK);
    assert_eq!(state.break_.snoozed, 1);
    let later = Stamp(start.0 + 50);
    // 09:50 + 10: the snoozed break and the hydration reminder of 10:00
    // are due together.
    assert_eq!(run(&mut state, &config, later, 10), [(10, "both")]);
    // After a snoozed reminder the full interval applies again.
    assert_eq!(state.break_.left, 50);
    // A snooze longer than what was left postpones; a shorter one brings
    // the reminder forward. Longer than the interval is allowed.
    let short = config_with_break(15);
    let mut state = started(&short, start);
    state.snooze(&short, Which::BREAK, 40);
    assert_eq!(
        run(&mut state, &short, start, 60),
        [(40, "break"), (55, "break")]
    );
    // A reminder that is off cannot be snoozed.
    assert_eq!(state.snooze(&short, Which::HYDRATION, 5), Which::NONE);
    assert_eq!(state.snooze(&short, Which::BOTH, 5), Which::BREAK);
    // Bounds, whatever the caller passes.
    state.snooze(&short, Which::BREAK, 0);
    assert_eq!(state.break_.left, 1);
    state.snooze(&short, Which::BREAK, 100_000);
    assert_eq!(state.break_.left, MAX_SNOOZE);
}

fn config_with_break(minutes: u32) -> Config {
    config(json!({ "breakMinutes": minutes, "hydrationEnabled": false }))
}

#[test]
fn skip_passes_over_one_reminder() {
    let config = config_with_break(10);
    let start = at("2026-11-16 09:00");
    let mut state = started(&config, start);
    assert_eq!(state.skip(&config, Which::BOTH), Which::BREAK);
    // Twice is still one.
    state.skip(&config, Which::BREAK);
    assert_eq!(run(&mut state, &config, start, 9), []);
    let advance = state.advance(&config, Stamp(start.0 + 10));
    assert!(advance.due.is_empty());
    assert_eq!(advance.passed_over, Which::BREAK);
    assert_eq!(
        (state.break_.skipped, state.break_.shown, state.break_.left),
        (1, 0, 10)
    );
    // The one after it comes.
    assert_eq!(
        run(&mut state, &config, Stamp(start.0 + 10), 20),
        [(10, "break"), (20, "break")]
    );
    assert_eq!(state.skip(&config, Which::HYDRATION), Which::NONE);
}

#[test]
fn restart_begins_a_full_interval_and_forgets_a_skip() {
    let config = defaults();
    let start = at("2026-11-16 09:00");
    let mut state = started(&config, start);
    run(&mut state, &config, start, 45);
    state.skip(&config, Which::BREAK);
    assert_eq!(state.restart(&config, Which::BREAK), Which::BREAK);
    assert_eq!((state.break_.left, state.hydration.left), (50, 15));
    assert!(!state.break_.skip_next);
    assert_eq!(
        run(&mut state, &config, Stamp(start.0 + 45), 50),
        [(15, "hydration"), (50, "break")]
    );
}

#[test]
fn pause_holds_everything_until_resumed() {
    let config = defaults();
    let start = at("2026-11-16 09:00");
    let mut state = started(&config, start);
    run(&mut state, &config, start, 48);
    state.pause(0);
    assert_eq!(Snapshot::of(&state, &config).phase, Phase::Paused);
    // Paused for two hours of ticks, with a gap in them: nothing is shown
    // and what was left stays (the gap restarts, as always).
    assert_eq!(run(&mut state, &config, Stamp(start.0 + 48), 4), []);
    assert_eq!((state.break_.left, state.hydration.left), (2, 12));
    state.resume();
    assert_eq!(
        run(&mut state, &config, Stamp(start.0 + 52), 12),
        [(2, "break"), (12, "hydration")]
    );
    // A reminder that is due when the pause begins waits for its end.
    let mut state = started(&config_with_break(5), start);
    let short = config_with_break(5);
    state.advance(&short, Stamp(start.0 + 4));
    state.pause(0);
    for minute in 5..9 {
        assert!(
            state
                .advance(&short, Stamp(start.0 + minute))
                .due
                .is_empty()
        );
    }
    assert_eq!(state.break_.left, 1);
    state.resume();
    assert_eq!(state.advance(&short, Stamp(start.0 + 9)).due, Which::BREAK);
}

#[test]
fn a_timed_pause_ends_by_itself() {
    let config = config_with_break(10);
    let start = at("2026-11-16 09:00");
    let mut state = started(&config, start);
    run(&mut state, &config, start, 4);
    state.pause(3);
    assert_eq!(Snapshot::of(&state, &config).pause_left, Some(3));
    let later = Stamp(start.0 + 4);
    assert_eq!(run(&mut state, &config, later, 2), []);
    assert!(state.paused);
    assert_eq!(state.pause_left, Some(1));
    // The third minute ends the pause; it was still paused time.
    assert_eq!(run(&mut state, &config, Stamp(later.0 + 2), 1), []);
    assert!(!state.paused);
    assert_eq!(state.pause_left, None);
    assert_eq!(state.break_.left, 6);
    assert_eq!(
        run(&mut state, &config, Stamp(later.0 + 3), 6),
        [(6, "break")]
    );
    // A pause is never longer than a day.
    state.pause(1_000_000);
    assert_eq!(state.pause_left, Some(MAX_PAUSE));
    // A long gap ends a timed pause and starts the countdowns again.
    state.pause(30);
    let advance = state.advance(&config, at("2026-11-16 17:00"));
    assert_eq!(advance.note, Note::Away);
    assert!(!state.paused && advance.due.is_empty());
    assert_eq!(state.break_.left, 10);
}

#[test]
fn counters_of_the_day_start_again_at_midnight() {
    let config = config(json!({
        "breakMinutes": 10, "hydrationEnabled": false, "awayMinutes": 120
    }));
    let start = at("2026-11-16 23:30");
    let mut state = started(&config, start);
    run(&mut state, &config, start, 25);
    assert_eq!((state.break_.today, state.break_.shown), (2, 2));
    run(&mut state, &config, Stamp(start.0 + 25), 10);
    // 23:40 and 23:50 were yesterday's; 00:00 is the first of the new day.
    assert_eq!((state.break_.today, state.break_.shown), (1, 3));
}

#[test]
fn the_snapshot_says_what_a_user_sees() {
    let config = defaults();
    let mut state = State::new();
    state.sync(&config);
    let waiting = Snapshot::of(&state, &config);
    assert_eq!(waiting.phase, Phase::Waiting);
    assert!(waiting.summary().starts_with("Waiting for the first tick"));
    state.advance(&config, at("2026-11-16 09:00"));
    state.advance(&config, at("2026-11-16 09:03"));
    let running = Snapshot::of(&state, &config);
    assert_eq!(running.summary(), "Break in 47 min, water in 57 min");
    state.pause(15);
    assert_eq!(
        Snapshot::of(&state, &config).summary(),
        "Paused for 15 more min (break in 47 min, water in 57 min)"
    );
    state.pause(0);
    assert!(
        Snapshot::of(&state, &config)
            .summary()
            .starts_with("Paused (break")
    );
    let off = config_off();
    state.resume();
    state.sync(&off);
    assert_eq!(
        Snapshot::of(&state, &off).summary(),
        "Both reminders are turned off"
    );
    let names: Vec<&str> = [Phase::Waiting, Phase::Running, Phase::Paused, Phase::Quiet]
        .iter()
        .map(|p| p.name())
        .collect();
    assert_eq!(names, ["waiting", "running", "paused", "quiet"]);
}

#[test]
fn stored_state_round_trips_and_damage_is_survived() {
    let config = defaults();
    let start = at("2026-11-16 09:00");
    let mut state = started(&config, start);
    run(&mut state, &config, start, 70);
    state.pause(20);
    state.skip(&config, Which::HYDRATION);
    let stored = serde_json::to_value(&state).unwrap();
    assert_eq!(stored["version"], STATE_VERSION);
    assert_eq!(stored["break"]["left"], 30);
    assert_eq!(
        stored["lastReminder"],
        json!({ "break": false, "hydration": true })
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
    newer["break"].as_object_mut().unwrap().remove("skipNext");
    assert_eq!(State::from_stored(Some(newer)), (state, false));
    // Not a state, or one of another version: start again, and say so.
    for damaged in [
        json!("text"),
        json!(42),
        json!([]),
        json!({}),
        json!({ "version": 2, "lastTick": 5 }),
        json!({ "version": 1, "lastTick": "yesterday" }),
        json!({ "version": 1, "break": { "left": -5 } }),
        json!({ "version": 1, "paused": "no" }),
    ] {
        assert_eq!(
            State::from_stored(Some(damaged.clone())),
            (State::new(), true),
            "{damaged}"
        );
    }
    // Numbers that fit the type and not the plugin are bounded.
    let (state, dropped) = State::from_stored(Some(json!({
        "version": 1, "pauseLeft": 4_000_000_000_u32,
        "break": { "interval": 50, "left": 4_000_000_000_u32 }
    })));
    assert!(!dropped);
    assert_eq!(state.break_.left, HYDRATION_MINUTES.1);
    assert_eq!(state.pause_left, Some(MAX_PAUSE));
}

#[test]
fn notifications_fit_the_agents_limits_in_both_languages() {
    for language in [Language::English, Language::Polish] {
        let mut titles = Vec::new();
        for which in [Which::BREAK, Which::HYDRATION, Which::BOTH] {
            let (title, body) = notification(which, language);
            assert!((1..=64).contains(&title.chars().count()), "{title}");
            assert!((1..=300).contains(&body.chars().count()), "{body}");
            titles.push(title);
        }
        titles.dedup();
        assert_eq!(titles.len(), 3);
    }
    assert_ne!(
        notification(Which::BREAK, Language::English),
        notification(Which::BREAK, Language::Polish)
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
    // Notifications and key state (plugin API 1.2), and no network.
    let kinds: Vec<&str> = m["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["notifications", "device"]);
    assert_eq!(m["permissions"][1]["state"], true);
    assert!(m["permissions"][1].get("icons").is_none());
    assert_eq!(m["engines"]["pluginApi"], "^1.2");
    assert_eq!(plugin().api_level().to_string(), "1.2");
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
        ("breakMinutes", BREAK_MINUTES),
        ("hydrationMinutes", HYDRATION_MINUTES),
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
    assert_eq!(Config::from_settings(&defaults).unwrap(), Config::default());

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
    assert_eq!(variables, [VAR_STATE, VAR_BREAK, VAR_HYDRATION]);
    let actions = m["contributes"]["actions"].as_array().unwrap();
    let action = |id: &str| actions.iter().find(|a| a["id"] == id).unwrap();
    assert_eq!(
        action("snooze")["parameters"][0]["kind"]["max"],
        json!(MAX_SNOOZE)
    );
    assert_eq!(
        action("pause")["parameters"][0]["kind"]["max"],
        json!(MAX_PAUSE)
    );
    assert_eq!(action("status")["node"]["effect"], "readOnly");
}

/// Every output an action can return is declared: the agent drops the
/// others without a word.
fn assert_outputs_declared(action: &str, data: &deckpilot_plugin_sdk::Params) {
    let m = manifest();
    let declared: Vec<String> = m["contributes"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == action)
        .unwrap()["node"]["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["key"].as_str().unwrap().to_owned())
        .collect();
    for key in data.keys() {
        assert!(
            declared.contains(key),
            "{action} returns the undeclared output {key}"
        );
    }
    for output in m["contributes"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == action)
        .unwrap()["node"]["outputs"]
        .as_array()
        .unwrap()
    {
        if output["always"] == true {
            let key = output["key"].as_str().unwrap();
            assert!(
                data.contains_key(key),
                "{action} did not return {key}, declared `always`"
            );
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
            CapabilityRequest::VariablesPublish(_) | CapabilityRequest::VariablesClear(_)
                if a.fail_variables =>
            {
                CallResult::error(CallErrorCode::Throttled, "10 per second")
            }
            CapabilityRequest::VariablesPublish(v) => {
                a.variables.insert(v.id.clone(), v.value.clone());
                CallResult::Ok(Value::Null)
            }
            CapabilityRequest::VariablesClear(v) => {
                let cleared = serde_json::to_value(v).unwrap();
                match cleared.get("id").and_then(Value::as_str) {
                    Some(id) => {
                        a.variables.remove(id);
                    }
                    None => a.variables.clear(),
                }
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

fn skipped(host: &mut MockHost, action: &str, params: Value) -> String {
    match host.invoke(&plugin(), action, params) {
        ActionOutcome::Skipped { message } => message.unwrap_or_default(),
        other => panic!("{action}: {other:?}"),
    }
}

fn tick_at(host: &mut MockHost, time: &str) -> (String, deckpilot_plugin_sdk::Params) {
    ok(host, "tick", json!({ "firedAt": time }))
}

fn int(data: &deckpilot_plugin_sdk::Params, key: &str) -> i64 {
    match data.get(key) {
        Some(ParamValue::Integer(v)) => *v,
        other => panic!("{key}: {other:?}"),
    }
}

fn stored(agent: &Shared) -> State {
    let (state, dropped) = State::from_stored(agent.borrow().store.get(STATE_KEY).cloned());
    assert!(!dropped);
    state
}

/// Minutes `from..=to` of 2026-11-16 09:xx as ticks; returns the last.
fn ticks(host: &mut MockHost, from: u32, to: u32) -> (String, deckpilot_plugin_sdk::Params) {
    let mut last = None;
    for minute in from..=to {
        last = Some(tick_at(
            host,
            &format!("2026-11-16 {:02}:{:02}", 9 + minute / 60, minute % 60),
        ));
    }
    last.expect("at least one tick")
}

const SHORT: fn() -> Value =
    || json!({ "breakMinutes": 5, "hydrationMinutes": 7, "awayMinutes": 3 });

#[test]
fn tick_counts_down_shows_reminders_and_publishes() {
    let agent = agent(SHORT());
    let mut host = host(&agent);
    let (message, data) = tick_at(&mut host, "2026-11-16 09:00");
    assert_eq!(message, "Break in 5 min, water in 7 min");
    assert_eq!(data["state"], ParamValue::String("running".into()));
    assert_eq!(data["reminder"], ParamValue::String("none".into()));
    assert_eq!(data["notified"], ParamValue::Bool(false));
    assert_eq!(
        (
            int(&data, "breakRemaining"),
            int(&data, "hydrationRemaining")
        ),
        (5, 7)
    );
    assert!(!data.contains_key("reason"));

    let (_, data) = ticks(&mut host, 1, 4);
    assert_eq!(int(&data, "breakRemaining"), 1);
    assert!(agent.borrow().shown.is_empty());

    let (message, data) = ticks(&mut host, 5, 5);
    assert_eq!(
        message,
        "Showed the break reminder. Break in 5 min, water in 2 min"
    );
    assert_eq!(data["reminder"], ParamValue::String("break".into()));
    assert_eq!(data["notified"], ParamValue::Bool(true));
    assert_eq!(
        agent.borrow().shown,
        [(
            "Time for a break".to_owned(),
            "Stand up, stretch and rest your eyes for a few minutes.".to_owned()
        )]
    );
    let (_, data) = ticks(&mut host, 6, 7);
    assert_eq!(data["reminder"], ParamValue::String("hydration".into()));
    assert_eq!(agent.borrow().shown[1].0, "Time to drink water");
    assert_eq!(
        agent.borrow().variables,
        BTreeMap::from([
            ("state".to_owned(), json!("running")),
            ("breakRemaining".to_owned(), json!(3)),
            ("hydrationRemaining".to_owned(), json!(7)),
        ])
    );
    let state = stored(&agent);
    assert_eq!((state.break_.shown, state.hydration.shown), (1, 1));
    assert_eq!(state.last_tick, Some(at("2026-11-16 09:07").0));
    // 35 minutes: one notification per reminder, both in one when they
    // are due in the same minute (09:35).
    ticks(&mut host, 8, 35);
    let titles: Vec<String> = agent.borrow().shown.iter().map(|n| n.0.clone()).collect();
    assert_eq!(titles.len(), 7 + 5 - 1);
    assert_eq!(titles.last().unwrap(), "Break and water");
    // Nothing the plugin logs or stores is more than its own state.
    assert!(agent.borrow().logs.is_empty(), "{:?}", agent.borrow().logs);
    assert_eq!(agent.borrow().store.keys().collect::<Vec<_>>(), [STATE_KEY]);
}

#[test]
fn tick_makes_few_calls() {
    let agent = agent(SHORT());
    let mut host = host(&agent);
    ticks(&mut host, 0, 5);
    // The last tick showed a reminder: settings, state, save, notify and
    // three variables. Far below 256 a run, 5 notifications a minute and
    // 10 variables a second.
    let requests = host.requests();
    let last: Vec<&CapabilityRequest> = requests[requests.len() - 7..].iter().collect();
    assert!(matches!(last[0], CapabilityRequest::SettingsGet(_)));
    assert!(matches!(last[1], CapabilityRequest::StorageGet(_)));
    assert!(matches!(last[2], CapabilityRequest::StorageSet(_)));
    assert!(matches!(last[3], CapabilityRequest::Notify(_)));
    assert_eq!(requests.len(), 5 * 6 + 7);
    assert!(!requests.iter().any(|r| matches!(
        r,
        CapabilityRequest::HttpFetch(_) | CapabilityRequest::OpenUrl(_)
    )));
}

#[test]
fn reminders_are_polish_when_the_setting_says_so() {
    let agent = agent(json!({ "breakMinutes": 5, "hydrationMinutes": 5, "language": "pl" }));
    let mut host = host(&agent);
    ticks(&mut host, 0, 5);
    assert_eq!(
        agent.borrow().shown,
        [(
            "Przerwa i woda".to_owned(),
            "Wstań, rozciągnij się, daj oczom odpocząć i wypij szklankę wody.".to_owned()
        )]
    );
}

#[test]
fn a_bad_or_missing_fired_at_fails_cleanly_and_changes_nothing() {
    let agent = agent(SHORT());
    let mut host = host(&agent);
    ticks(&mut host, 0, 2);
    let before = agent.borrow().store.clone();
    let calls = host.requests().len();
    for value in [
        "tomorrow",
        "2026-11-16",
        "2026-02-30 10:00",
        "{{ steps.start.output.firedAt }}",
        "2026-11-16 09:03 and a secret",
    ] {
        let (code, message, retryable) = failed(&mut host, "tick", json!({ "firedAt": value }));
        assert_eq!(code, INVALID_FIRED_AT, "{value}");
        assert!(message.contains("YYYY-MM-DD HH:MM"), "{message}");
        assert!(
            !message.contains(value),
            "the message repeats the value: {message}"
        );
        assert!(!retryable);
    }
    // Missing, empty, of another type or too long: refused before the
    // handler runs.
    for params in [
        json!({}),
        json!({ "firedAt": 202611160903_i64 }),
        json!({ "firedAt": "2026-11-16 09:03                 " }),
        json!({ "firedAt": "2026-11-16 09:03", "extra": true }),
    ] {
        let (code, _, _) = failed(&mut host, "tick", params);
        assert_eq!(code, codes::INVALID_PARAMETERS);
    }
    assert_eq!(host.requests().len(), calls, "no capability call was made");
    assert_eq!(agent.borrow().store, before);
    assert!(agent.borrow().logs.is_empty());
}

#[test]
fn a_long_gap_between_ticks_shows_nothing() {
    let agent = agent(SHORT());
    let mut host = host(&agent);
    ticks(&mut host, 0, 4);
    // Asleep from 09:04 to 17:30.
    let (message, data) = tick_at(&mut host, "2026-11-16 17:30");
    assert_eq!(message, "Break in 5 min, water in 7 min");
    assert_eq!(data["reminder"], ParamValue::String("none".into()));
    assert!(agent.borrow().shown.is_empty());
    assert_eq!(agent.borrow().logs.len(), 1);
    assert!(agent.borrow().logs[0].contains("counts as a break"));
    // The next morning, the same; and then reminders as usual.
    tick_at(&mut host, "2026-11-17 08:00");
    for minute in 1..=5 {
        tick_at(&mut host, &format!("2026-11-17 08:0{minute}"));
    }
    assert_eq!(agent.borrow().shown.len(), 1);
}

#[test]
fn the_clock_going_backwards_is_survived() {
    let agent = agent(SHORT());
    let mut host = host(&agent);
    ticks(&mut host, 0, 4);
    let (_, data) = tick_at(&mut host, "2026-11-16 08:10");
    assert_eq!(int(&data, "breakRemaining"), 1);
    assert_eq!(data["reminder"], ParamValue::String("none".into()));
    assert!(agent.borrow().logs[0].contains("clock went back"));
    let (_, data) = tick_at(&mut host, "2026-11-16 08:11");
    assert_eq!(data["reminder"], ParamValue::String("break".into()));
    assert_eq!(agent.borrow().shown.len(), 1);
}

#[test]
fn quiet_hours_across_midnight_show_nothing() {
    let agent = agent(json!({
        "breakMinutes": 5, "hydrationEnabled": false,
        "quietStart": "23:58", "quietEnd": "00:03"
    }));
    let mut host = host(&agent);
    let mut seen = Vec::new();
    for time in [
        "2026-11-16 23:54",
        "2026-11-16 23:55",
        "2026-11-16 23:56",
        "2026-11-16 23:57",
        "2026-11-16 23:58",
        "2026-11-16 23:59",
        "2026-11-17 00:00",
        "2026-11-17 00:01",
        "2026-11-17 00:02",
        "2026-11-17 00:03",
        "2026-11-17 00:04",
        "2026-11-17 00:05",
    ] {
        let (message, data) = tick_at(&mut host, time);
        let ParamValue::String(state) = &data["state"] else {
            panic!()
        };
        seen.push(format!(
            "{} {state} {} {}",
            &time[11..],
            int(&data, "breakRemaining"),
            agent.borrow().shown.len()
        ));
        if state == "quiet" {
            assert!(message.starts_with("Quiet hours ("), "{message}");
        }
    }
    assert_eq!(
        seen,
        [
            "23:54 running 5 0",
            "23:55 running 4 0",
            "23:56 running 3 0",
            "23:57 running 2 0",
            "23:58 quiet 1 0",
            "23:59 quiet 1 0",
            "00:00 quiet 1 0",
            "00:01 quiet 1 0",
            "00:02 quiet 1 0",
            "00:03 running 1 0",
            "00:04 running 5 1",
            "00:05 running 4 1",
        ]
    );
    let (_, status) = ok(&mut host, "status", json!({}));
    assert_eq!(status["quietHours"], ParamValue::Bool(false));
    assert_eq!(int(&status, "breaksToday"), 1);
}

#[test]
fn invalid_quiet_hours_are_an_error_that_names_the_setting() {
    let agent = agent(json!({ "quietStart": "late", "quietEnd": "07:00" }));
    let mut host = host(&agent);
    for (action, params) in [
        ("tick", json!({ "firedAt": "2026-11-16 09:00" })),
        ("status", json!({})),
        ("snooze", json!({})),
        ("pause", json!({})),
    ] {
        let (code, message, _) = failed(&mut host, action, params);
        assert_eq!(code, INVALID_SETTINGS, "{action}");
        assert!(
            message.contains("Quiet hours from") && message.contains("22:00"),
            "{message}"
        );
        assert!(!message.contains("late"), "{message}");
    }
    assert!(agent.borrow().store.is_empty());
}

#[test]
fn a_throttled_notification_stays_due_and_is_sent_once() {
    let agent = agent(SHORT());
    let mut host = host(&agent);
    ticks(&mut host, 0, 4);
    agent.borrow_mut().notify = Notify::Throttled;
    for minute in 5..=6 {
        let (message, data) = ticks(&mut host, minute, minute);
        assert_eq!(data["reminder"], ParamValue::String("break".into()));
        assert_eq!(data["notified"], ParamValue::Bool(false));
        assert!(matches!(&data["reason"], ParamValue::String(r) if r.contains("5 notifications")));
        assert!(
            message.starts_with("The break reminder was not shown:"),
            "{message}"
        );
        assert_eq!(int(&data, "breakRemaining"), 0);
    }
    let state = stored(&agent);
    assert_eq!(
        (state.break_.left, state.break_.shown, state.break_.today),
        (0, 0, 0)
    );
    assert_eq!(state.last_reminder, Which::NONE);
    // The limit passes: one notification for the break and the hydration
    // reminder that became due meanwhile, and the usual rhythm after it.
    agent.borrow_mut().notify = Notify::Shown;
    let (_, data) = ticks(&mut host, 7, 7);
    assert_eq!(data["reminder"], ParamValue::String("both".into()));
    assert_eq!(data["notified"], ParamValue::Bool(true));
    assert_eq!(agent.borrow().shown.len(), 1);
    ticks(&mut host, 8, 11);
    assert_eq!(agent.borrow().shown.len(), 1);
    ticks(&mut host, 12, 12);
    assert_eq!(agent.borrow().shown.len(), 2);
    // The warnings name the operation and the limit, nothing else.
    for line in &agent.borrow().logs {
        assert!(
            line.starts_with("the reminder was not sent and stays due:"),
            "{line}"
        );
    }
    assert_eq!(agent.borrow().logs.len(), 2);
}

#[test]
fn a_notification_that_fails_otherwise_is_tried_again() {
    let agent = agent(SHORT());
    let mut host = host(&agent);
    ticks(&mut host, 0, 4);
    agent.borrow_mut().notify = Notify::Failed;
    let (_, data) = ticks(&mut host, 5, 5);
    assert_eq!(data["notified"], ParamValue::Bool(false));
    assert!(matches!(&data["reason"], ParamValue::String(r) if r.contains("tried again")));
    agent.borrow_mut().notify = Notify::Shown;
    let (_, data) = ticks(&mut host, 6, 6);
    assert_eq!(data["notified"], ParamValue::Bool(true));
}

#[test]
fn a_revoked_notification_permission_is_not_permitted_and_keeps_the_reminder() {
    let agent = agent(SHORT());
    let mut host = host(&agent);
    ticks(&mut host, 0, 4);
    agent.borrow_mut().notify = Notify::Denied;
    let (code, message, _) = failed(&mut host, "tick", json!({ "firedAt": "2026-11-16 09:05" }));
    assert_eq!(code, codes::NOT_PERMITTED);
    assert!(message.contains("`notifications` permission"), "{message}");
    assert_eq!(stored(&agent).break_.left, 0);
    assert_eq!(stored(&agent).break_.shown, 0);
    // Granted again: shown with the next tick.
    agent.borrow_mut().notify = Notify::Shown;
    let (_, data) = ticks(&mut host, 6, 6);
    assert_eq!(data["notified"], ParamValue::Bool(true));
}

#[test]
fn a_notification_the_settings_show_nowhere_is_not_repeated() {
    let agent = agent(SHORT());
    let mut host = host(&agent);
    ticks(&mut host, 0, 4);
    agent.borrow_mut().notify = Notify::Nowhere;
    let (message, data) = ticks(&mut host, 5, 5);
    assert_eq!(data["notified"], ParamValue::Bool(false));
    assert_eq!(data["reminder"], ParamValue::String("break".into()));
    assert!(
        matches!(&data["reason"], ParamValue::String(r) if r.contains("desktop notifications are off"))
    );
    assert!(message.contains("not shown"), "{message}");
    // The user chose that: the countdown starts again.
    assert_eq!(int(&data, "breakRemaining"), 5);
    assert_eq!(stored(&agent).break_.shown, 1);
}

#[test]
fn older_agents_are_served() {
    for mode in [Notify::Old, Notify::NoSeverity] {
        let agent = agent(SHORT());
        agent.borrow_mut().notify = mode;
        let mut host = host(&agent);
        let (_, data) = ticks(&mut host, 0, 5);
        assert_eq!(data["notified"], ParamValue::Bool(true), "{mode:?}");
        assert_eq!(agent.borrow().shown.len(), 1, "{mode:?}");
    }
}

#[test]
fn storage_that_cannot_be_read_fails_every_action_without_a_notification() {
    let agent = agent(SHORT());
    let mut host = host(&agent);
    ticks(&mut host, 0, 4);
    let before = agent.borrow().store.clone();
    agent.borrow_mut().fail_get = true;
    for (action, params) in [
        ("tick", json!({ "firedAt": "2026-11-16 09:05" })),
        ("snooze", json!({})),
        ("skip", json!({})),
        ("restart", json!({})),
        ("pause", json!({})),
        ("resume", json!({})),
        ("toggle-pause", json!({})),
        ("status", json!({})),
    ] {
        let (code, message, retryable) = failed(&mut host, action, params);
        assert_eq!(code, STORAGE_UNAVAILABLE, "{action}");
        assert_eq!(message, "the plugin's storage could not be read; try again");
        assert!(retryable);
    }
    assert!(agent.borrow().shown.is_empty());
    assert_eq!(agent.borrow().store, before);
    // It works again: the reminder that was due is shown, once.
    agent.borrow_mut().fail_get = false;
    ticks(&mut host, 6, 6);
    assert_eq!(agent.borrow().shown.len(), 1);
}

#[test]
fn storage_that_cannot_be_written_never_repeats_a_reminder() {
    let agent = agent(SHORT());
    let mut host = host(&agent);
    ticks(&mut host, 0, 4);
    agent.borrow_mut().sets_left = Some(0);
    // Due at 09:05; the state cannot be saved for three ticks. Nothing is
    // shown, so nothing can repeat every minute.
    for minute in 5..=7 {
        let (code, message, retryable) = failed(
            &mut host,
            "tick",
            json!({ "firedAt": format!("2026-11-16 09:0{minute}") }),
        );
        assert_eq!(code, STORAGE_UNAVAILABLE);
        assert_eq!(
            message,
            "the plugin's storage could not be written; try again"
        );
        assert!(retryable);
    }
    assert!(agent.borrow().shown.is_empty());
    agent.borrow_mut().sets_left = None;
    // 09:04 was the last tick saved; 09:08 is a gap of more than the away
    // time, so the countdowns start again.
    let (_, data) = ticks(&mut host, 8, 8);
    assert_eq!(int(&data, "breakRemaining"), 5);
    // The quota keeps its shared code.
    agent.borrow_mut().quota = true;
    let (code, message, _) = failed(&mut host, "pause", json!({}));
    assert_eq!(code, codes::STORAGE_QUOTA);
    assert!(message.contains("storage is full"), "{message}");

    // Refused notification and the state cannot be put back: an error,
    // and the reminder is lost rather than repeated.
    let agent = self::agent(SHORT());
    let mut host = self::host(&agent);
    ticks(&mut host, 0, 4);
    agent.borrow_mut().notify = Notify::Throttled;
    agent.borrow_mut().sets_left = Some(1);
    let (code, _, _) = failed(&mut host, "tick", json!({ "firedAt": "2026-11-16 09:05" }));
    assert_eq!(code, STORAGE_UNAVAILABLE);
    assert_eq!(stored(&agent).break_.left, 5);
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
    assert_eq!(message, "Break in 5 min, water in 7 min");
    assert_eq!(
        agent.borrow().logs,
        ["the stored reminder state could not be used; the reminders start again"]
    );
    assert_eq!(stored(&agent).version, STATE_VERSION);
}

#[test]
fn variables_that_cannot_be_published_do_not_fail_an_action() {
    let agent = agent(SHORT());
    agent.borrow_mut().fail_variables = true;
    let mut host = host(&agent);
    let (_, data) = ticks(&mut host, 0, 5);
    assert_eq!(data["notified"], ParamValue::Bool(true));
    assert!(agent.borrow().variables.is_empty());
}

#[test]
fn snooze_from_a_key() {
    let agent = agent(SHORT());
    let mut host = host(&agent);
    // Before any reminder there is nothing to snooze.
    assert!(skipped(&mut host, "snooze", json!({})).contains("nothing to snooze"));
    ticks(&mut host, 0, 5);
    assert_eq!(agent.borrow().shown.len(), 1);
    // The default: the reminder shown last, in 10 minutes.
    let (message, data) = ok(&mut host, "snooze", json!({}));
    assert_eq!(
        message,
        "Snoozed the break reminder for 10 min. Break in 10 min, water in 2 min"
    );
    assert_eq!(int(&data, "breakRemaining"), 10);
    let (_, data) = ok(
        &mut host,
        "snooze",
        json!({ "minutes": 3, "reminder": "both" }),
    );
    assert_eq!(
        (
            int(&data, "breakRemaining"),
            int(&data, "hydrationRemaining")
        ),
        (3, 3)
    );
    assert_eq!(agent.borrow().variables["breakRemaining"], json!(3));
    ticks(&mut host, 6, 8);
    assert_eq!(agent.borrow().shown.len(), 2);
    assert_eq!(agent.borrow().shown[1].0, "Break and water");
    assert_eq!(stored(&agent).break_.snoozed, 2);
    for params in [
        json!({ "minutes": 0 }),
        json!({ "minutes": 121 }),
        json!({ "minutes": "10" }),
        json!({ "reminder": "lunch" }),
    ] {
        let (code, _, _) = failed(&mut host, "snooze", params);
        assert_eq!(code, codes::INVALID_PARAMETERS);
    }
    // A reminder that is turned off.
    let agent = self::agent(json!({ "hydrationEnabled": false }));
    let mut host = self::host(&agent);
    let message = skipped(&mut host, "snooze", json!({ "reminder": "hydration" }));
    assert_eq!(
        message,
        "the hydration reminder is turned off in the plugin's settings"
    );
    assert!(agent.borrow().store.is_empty());
}

#[test]
fn skip_and_restart_from_a_key() {
    let agent = agent(SHORT());
    let mut host = host(&agent);
    ticks(&mut host, 0, 3);
    let (message, _) = ok(&mut host, "skip", json!({}));
    assert_eq!(
        message,
        "The next break reminder will be skipped. Break in 2 min, water in 4 min"
    );
    let (message, data) = ticks(&mut host, 4, 5);
    assert_eq!(
        message,
        "Skipped the break reminder. Break in 5 min, water in 2 min"
    );
    assert_eq!(data["reminder"], ParamValue::String("none".into()));
    assert!(agent.borrow().shown.is_empty());
    assert_eq!(stored(&agent).break_.skipped, 1);

    let (message, data) = ok(&mut host, "restart", json!({}));
    assert_eq!(
        message,
        "Started the break and hydration countdown again. Break in 5 min, water in 7 min"
    );
    assert_eq!(int(&data, "hydrationRemaining"), 7);
    let (_, data) = ok(&mut host, "restart", json!({ "reminder": "hydration" }));
    assert_eq!(int(&data, "hydrationRemaining"), 7);

    let agent = self::agent(json!({ "breakEnabled": false }));
    let mut host = self::host(&agent);
    assert!(skipped(&mut host, "skip", json!({})).contains("turned off"));
    assert!(skipped(&mut host, "restart", json!({ "reminder": "break" })).contains("turned off"));
    let (_, data) = ok(&mut host, "restart", json!({}));
    assert!(!data.contains_key("breakRemaining"));
}

#[test]
fn pause_resume_and_toggle_from_a_key() {
    let agent = agent(SHORT());
    let mut host = host(&agent);
    ticks(&mut host, 0, 3);
    let (message, data) = ok(&mut host, "pause", json!({}));
    assert_eq!(message, "Paused (break in 2 min, water in 4 min)");
    assert_eq!(data["paused"], ParamValue::Bool(true));
    assert_eq!(data["state"], ParamValue::String("paused".into()));
    let (_, data) = ticks(&mut host, 4, 9);
    assert_eq!(data["state"], ParamValue::String("paused".into()));
    assert_eq!(int(&data, "breakRemaining"), 2);
    assert!(agent.borrow().shown.is_empty());
    assert_eq!(agent.borrow().variables["state"], json!("paused"));

    let (message, data) = ok(&mut host, "resume", json!({}));
    assert_eq!(message, "Break in 2 min, water in 4 min");
    assert_eq!(data["paused"], ParamValue::Bool(false));
    // Resuming what is not paused is fine.
    ok(&mut host, "resume", json!({}));
    ticks(&mut host, 10, 11);
    assert_eq!(agent.borrow().shown.len(), 1);

    let (_, data) = ok(&mut host, "toggle-pause", json!({}));
    assert_eq!(data["paused"], ParamValue::Bool(true));
    let (_, data) = ok(&mut host, "toggle-pause", json!({}));
    assert_eq!(data["paused"], ParamValue::Bool(false));

    // A timed pause, counted by the ticks.
    let (message, _) = ok(&mut host, "pause", json!({ "minutes": 2 }));
    assert!(message.starts_with("Paused for 2 more min ("), "{message}");
    let (_, status) = ok(&mut host, "status", json!({}));
    assert_eq!(int(&status, "pauseRemaining"), 2);
    let (_, data) = ticks(&mut host, 12, 12);
    assert_eq!(data["state"], ParamValue::String("paused".into()));
    let (_, data) = ticks(&mut host, 13, 13);
    assert_eq!(data["state"], ParamValue::String("running".into()));
    for params in [json!({ "minutes": -1 }), json!({ "minutes": 1441 })] {
        let (code, _, _) = failed(&mut host, "pause", params);
        assert_eq!(code, codes::INVALID_PARAMETERS);
    }
}

#[test]
fn status_reports_and_stores_nothing() {
    let agent = agent(json!({}));
    let mut host = host(&agent);
    let (message, data) = ok(&mut host, "status", json!({}));
    assert!(
        message.starts_with("Waiting for the first tick"),
        "{message}"
    );
    assert_eq!(data["state"], ParamValue::String("waiting".into()));
    assert_eq!(
        (
            int(&data, "breakRemaining"),
            int(&data, "hydrationRemaining")
        ),
        (50, 60)
    );
    assert!(!data.contains_key("lastTick") && !data.contains_key("pauseRemaining"));
    assert!(agent.borrow().store.is_empty(), "status is read-only");
    assert_eq!(agent.borrow().variables["state"], json!("waiting"));

    ticks(&mut host, 0, 12);
    let saved = agent.borrow().store.clone();
    let (message, data) = ok(&mut host, "status", json!({}));
    assert_eq!(message, "Break in 38 min, water in 48 min");
    assert_eq!(
        data["lastTick"],
        ParamValue::String("2026-11-16 09:12".into())
    );
    assert_eq!(data["paused"], ParamValue::Bool(false));
    assert_eq!(data["quietHours"], ParamValue::Bool(false));
    assert_eq!(
        (int(&data, "breaksToday"), int(&data, "hydrationToday")),
        (0, 0)
    );
    assert_eq!(agent.borrow().store, saved);

    // A changed setting is seen at once, a turned-off reminder has no value.
    agent.borrow_mut().settings = json!({ "breakMinutes": 20, "hydrationEnabled": false });
    let (message, data) = ok(&mut host, "status", json!({}));
    assert_eq!(message, "Break in 20 min");
    assert!(!data.contains_key("hydrationRemaining"));
    assert!(!agent.borrow().variables.contains_key("hydrationRemaining"));
}

#[test]
fn a_disabled_plugin_fails_cleanly() {
    // A disabled plugin is denied everything: no panic, a shared code.
    for (action, params) in [
        ("tick", json!({ "firedAt": "2026-11-16 09:00" })),
        ("snooze", json!({})),
        ("skip", json!({})),
        ("restart", json!({})),
        ("pause", json!({})),
        ("resume", json!({})),
        ("toggle-pause", json!({})),
        ("status", json!({})),
    ] {
        let mut denied = MockHost::deny_all();
        let out = denied.invoke(&plugin(), action, params);
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
fn a_day_of_work_never_shows_more_than_the_intervals_allow() {
    // 09:00 to 17:00 with a lunch away from the desk, a pause, a snooze
    // and a skip, ticks every minute: at most one notification a minute,
    // never two break reminders closer than the snooze, and none while
    // paused or away.
    let agent = agent(json!({ "awayMinutes": 10 }));
    let mut host = host(&agent);
    let mut shown_at = Vec::new();
    for minute in 0..=480_u32 {
        if (180..225).contains(&minute) {
            continue; // lunch: the computer sleeps
        }
        match minute {
            60 => drop(ok(&mut host, "snooze", json!({ "minutes": 5 }))),
            100 => drop(ok(&mut host, "pause", json!({ "minutes": 15 }))),
            300 => drop(ok(&mut host, "skip", json!({ "reminder": "hydration" }))),
            _ => {}
        }
        let before = agent.borrow().shown.len();
        ticks(&mut host, minute, minute);
        let after = agent.borrow().shown.len();
        assert!(after - before <= 1, "minute {minute}");
        if after > before {
            shown_at.push((minute, agent.borrow().shown[after - 1].0.clone()));
        }
    }
    let minutes: Vec<u32> = shown_at.iter().map(|(m, _)| *m).collect();
    let breaks: Vec<u32> = shown_at
        .iter()
        .filter(|(_, title)| title != "Time to drink water")
        .map(|(m, _)| *m)
        .collect();
    assert_eq!(
        breaks[..2],
        [50, 64],
        "the snooze at 60 counts from the tick at 60"
    );
    assert!(breaks.windows(2).all(|w| w[1] - w[0] >= 5), "{shown_at:?}");
    assert!(
        minutes
            .iter()
            .all(|m| !(100..115).contains(m) && !(180..=225).contains(m))
    );
    // Seven and a quarter working hours: between 7 and 16 notifications.
    assert!((7..=16).contains(&shown_at.len()), "{shown_at:?}");
    let state = stored(&agent);
    assert_eq!(state.hydration.skipped, 1);
    assert_eq!(u64::from(state.break_.today), state.break_.shown);
}

// ------------------------------------------------------------ key state

/// The key of `state` under `config`, at the local time `now`.
fn key_at(state: &State, config: &Config, now: &str) -> KeyState {
    key::status(&Snapshot::of(state, config), state, config, at(now).0)
}

#[test]
fn only_status_shows_key_state() {
    let m = manifest();
    for action in m["contributes"]["actions"].as_array().unwrap() {
        let id = action["id"].as_str().unwrap();
        let declared = action.get("state").is_some();
        assert_eq!(declared, id == "status", "{id}");
        if declared {
            assert_eq!(action["node"]["effect"], "readOnly");
            assert_eq!(
                action["state"],
                json!({"refresh": "every", "intervalSeconds": 60})
            );
        }
        assert_eq!(plugin().find(id).unwrap().has_state(), declared, "{id}");
    }
}

#[test]
fn the_key_counts_down_with_the_ticks_and_says_when_they_stop() {
    let config = defaults();
    let start = at("2026-11-16 09:00");
    let mut state = started(&config, start);
    let fresh = key_at(&state, &config, "2026-11-16 09:00");
    assert_eq!(fresh.label.as_deref(), Some("Break 50 min"));
    assert_eq!(fresh.detail.as_deref(), Some("Water 60 min"));
    assert_eq!((fresh.progress, fresh.toggle), (Some(0), Some(true)));
    assert_eq!(
        (fresh.accent, fresh.indicator),
        (Some(Accent::Teal), Some(Indicator::Running))
    );
    assert_eq!(fresh.badge, None);
    fresh.validate().expect("within the bounds of key state");
    run(&mut state, &config, start, 10);
    let later = key_at(&state, &config, "2026-11-16 09:10");
    assert_eq!(
        (later.label.as_deref(), later.progress),
        (Some("Break 40 min"), Some(20))
    );
    assert_eq!(later.detail.as_deref(), Some("Water 50 min"));
    // Up to two minutes without a tick is normal; three or more is said.
    assert_eq!(key_at(&state, &config, "2026-11-16 09:12"), later);
    for (now, badge) in [("2026-11-16 09:13", "3m"), ("2026-11-16 12:10", "3h")] {
        let late = key_at(&state, &config, now);
        assert_eq!(late.label, later.label, "{now}");
        assert_eq!(
            (late.indicator, late.badge.as_deref()),
            (Some(Indicator::Warning), Some(badge))
        );
    }
    // A reminder due now, before it is shown: amber.
    let mut due = started(&config, start);
    for minute in 1..=50 {
        due.advance(&config, Stamp(start.0 + minute));
    }
    let now = key_at(&due, &config, "2026-11-16 09:50");
    assert_eq!(
        (now.label.as_deref(), now.accent),
        (Some("Break now"), Some(Accent::Amber))
    );
}

#[test]
fn paused_quiet_waiting_and_off_are_told_apart() {
    let config = defaults();
    let mut state = started(&config, at("2026-11-16 09:00"));
    state.pause(40);
    let paused = key_at(&state, &config, "2026-11-16 18:00");
    assert_eq!(paused.label.as_deref(), Some("Paused"));
    assert_eq!(paused.detail.as_deref(), Some("40 min left"));
    assert_eq!(
        (paused.accent, paused.indicator),
        (Some(Accent::Grey), Some(Indicator::Warning))
    );
    assert_eq!(
        (paused.badge, paused.toggle, paused.progress),
        (None, Some(false), None)
    );
    state.pause(0);
    assert_eq!(
        key_at(&state, &config, "2026-11-16 18:00")
            .detail
            .as_deref(),
        Some("until resumed")
    );

    let quiet_config = self::config(json!({ "quietStart": "12:00", "quietEnd": "13:00" }));
    let mut quiet = started(&quiet_config, at("2026-11-16 11:58"));
    run(&mut quiet, &quiet_config, at("2026-11-16 11:58"), 4);
    let shown = key_at(&quiet, &quiet_config, "2026-11-16 12:02");
    assert_eq!(shown.label.as_deref(), Some("Quiet hours"));
    assert_eq!(
        (shown.accent, shown.indicator),
        (Some(Accent::Purple), Some(Indicator::Unknown))
    );
    // Ticks run in quiet hours too: a late one is said there as well.
    let late = key_at(&quiet, &quiet_config, "2026-11-16 12:30");
    assert_eq!(late.badge.as_deref(), Some("28m"));

    let mut waiting = State::new();
    waiting.sync(&config);
    let shown = key_at(&waiting, &config, "2026-11-16 09:00");
    assert_eq!(shown.label.as_deref(), Some("Waiting"));
    assert_eq!(
        (shown.badge.as_deref(), shown.indicator),
        (Some("?"), Some(Indicator::Warning))
    );

    let off = config_off();
    let state = started(&off, at("2026-11-16 09:00"));
    let shown = key_at(&state, &off, "2026-11-16 09:00");
    assert_eq!(
        (shown.label.as_deref(), shown.detail, shown.progress),
        (Some("Off"), None, None)
    );
}

#[test]
fn polish_texts_are_ascii_on_the_key() {
    let config = config(json!({ "language": "pl" }));
    let mut state = started(&config, at("2026-11-16 09:00"));
    let shown = key_at(&state, &config, "2026-11-16 09:00");
    assert_eq!(shown.label.as_deref(), Some("Przerwa 50 min"));
    assert_eq!(shown.detail.as_deref(), Some("Woda 60 min"));
    state.pause(15);
    assert_eq!(
        key_at(&state, &config, "2026-11-16 09:00")
            .detail
            .as_deref(),
        Some("jeszcze 15 min")
    );
    let mut waiting = State::new();
    waiting.sync(&config);
    let texts = [
        key_at(&waiting, &config, "2026-11-16 09:00").label.unwrap(),
        key_at(&state, &config, "2026-11-16 09:00").label.unwrap(),
    ];
    assert_eq!(texts, ["Czeka na takt", "Pauza"]);
    assert!(texts.iter().all(|t| t.is_ascii()));
}

#[test]
fn the_state_call_reads_the_stored_reminders_and_writes_nothing() {
    let config = defaults();
    let state = started(&config, at("2026-11-16 09:00"));
    let stored = serde_json::to_value(&state).unwrap();
    let now_minute = at("2026-11-16 09:04").0;
    let mut host = MockHost::new(move |request| match request {
        CapabilityRequest::SettingsGet(_) => CallResult::Ok(json!({})),
        CapabilityRequest::StorageGet(k) if k.key == STATE_KEY => CallResult::Ok(stored.clone()),
        CapabilityRequest::TimeNow(_) => {
            CallResult::Ok(json!({ "unixMs": now_minute * 60_000, "utcOffsetMinutes": 0 }))
        }
        other => panic!("a state call may only read: {other:?}"),
    });
    let deckpilot_plugin_sdk::StateOutcome::Ok(shown) =
        host.key_state(&plugin(), "status", json!({}))
    else {
        panic!("no state")
    };
    assert_eq!(shown.label.as_deref(), Some("Break 50 min"));
    assert_eq!(shown.badge.as_deref(), Some("4m"));
}
