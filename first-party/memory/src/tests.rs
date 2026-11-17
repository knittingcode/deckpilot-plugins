//! Native unit tests: the definition against `manifest.json`, the game
//! exhaustively, every action against a scripted `MockHost` (storage, the
//! time, random bytes), the key states, and every error code.
//! `tools/first-party.sh` adds the manifest against the schema, the Polish
//! texts, the wasm build and the package checks.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use deckpilot_plugin_sdk::testing::{MockHost, manifest_mismatches};
use deckpilot_plugin_sdk::{
    ActionOutcome, CallErrorCode, CallResult, CapabilityRequest, ParamValue, StateOutcome, codes,
};
use serde_json::{Value, json};

use super::game::{BOARD_VERSION, Best, Board, Face, Flip, MAX_PAIRS, MIN_PAIRS, Refused};
use super::*;

const MANIFEST: &str = include_str!("../manifest.json");

fn manifest() -> Value {
    serde_json::from_str(MANIFEST).expect("manifest.json is JSON")
}

// ------------------------------------------------------------ the host

/// What the scripted agent holds: storage, settings, the clock and the
/// random source (counting bytes).
#[derive(Default)]
struct Agent {
    store: RefCell<BTreeMap<String, Value>>,
    settings: RefCell<Value>,
    now_ms: Cell<u64>,
    seed: Cell<u64>,
    random_calls: Cell<u32>,
}

type Shared = Rc<Agent>;

fn new_agent(pairs: Option<u8>) -> Shared {
    let a = Agent::default();
    *a.settings.borrow_mut() = pairs.map_or(json!({}), |p| json!({ "pairs": p }));
    a.now_ms.set(1_794_900_000_000);
    a.seed.set(7);
    Rc::new(a)
}

fn host_of(agent: &Shared) -> MockHost {
    let agent = Rc::clone(agent);
    MockHost::new(move |request| match request {
        CapabilityRequest::SettingsGet(_) => CallResult::Ok(agent.settings.borrow().clone()),
        CapabilityRequest::StorageGet(k) => CallResult::Ok(
            agent
                .store
                .borrow()
                .get(&k.key)
                .cloned()
                .unwrap_or(Value::Null),
        ),
        CapabilityRequest::StorageSet(s) => {
            agent
                .store
                .borrow_mut()
                .insert(s.key.clone(), s.value.clone());
            CallResult::Ok(Value::Null)
        }
        CapabilityRequest::TimeNow(_) => {
            CallResult::Ok(json!({ "unixMs": agent.now_ms.get(), "utcOffsetMinutes": 60 }))
        }
        CapabilityRequest::RandomBytes(r) => {
            agent.random_calls.set(agent.random_calls.get() + 1);
            let bytes = agent.seed.get().to_le_bytes();
            assert_eq!(r.length, 8, "one seed per game");
            CallResult::Ok(json!({ "bytes": base64(&bytes) }))
        }
        _ => CallResult::Ok(Value::Null),
    })
}

/// Padded standard base64, as the agent sends random bytes.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0_u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn board(agent: &Shared) -> Board {
    let value = agent.store.borrow()["board"].clone();
    serde_json::from_value(value).expect("a stored board")
}

fn ok_data(out: &ActionOutcome) -> (&str, &deckpilot_plugin_sdk::Params) {
    match out {
        ActionOutcome::Ok { message, data } => (message.as_deref().unwrap_or(""), data),
        other => panic!("expected ok, got {other:?}"),
    }
}

fn failed_code(out: &ActionOutcome) -> &str {
    match out {
        ActionOutcome::Failed { code, .. } => code,
        other => panic!("expected failed, got {other:?}"),
    }
}

/// The cell (1-based) of each face of `board`: `pair -> [cell, cell]`.
fn pairs_of(board: &Board) -> BTreeMap<u8, Vec<i64>> {
    let mut out: BTreeMap<u8, Vec<i64>> = BTreeMap::new();
    for (i, card) in board.cards.iter().enumerate() {
        out.entry(*card)
            .or_default()
            .push(i64::try_from(i).unwrap() + 1);
    }
    out
}

fn turn(host: &mut MockHost, cell: i64) -> ActionOutcome {
    host.invoke(&plugin(), "flip", json!({ "cell": cell }))
}

fn state(host: &mut MockHost, action: &str, params: Value) -> KeyState {
    match host.key_state(&plugin(), action, params) {
        StateOutcome::Ok(state) => state,
        StateOutcome::Error(e) => panic!("{action}: {e:?}"),
    }
}

// ------------------------------------------------------ the definition

#[test]
fn definition_is_valid_and_matches_the_manifest() {
    plugin().validate().expect("the definition is valid");
    assert_eq!(
        manifest_mismatches(&plugin(), &manifest()),
        Vec::<String>::new()
    );
    assert_eq!(plugin().api_level(), API);
    assert_eq!(manifest()["engines"]["pluginApi"], "^1.2");
}

#[test]
fn the_manifest_declares_what_the_code_uses_and_nothing_more() {
    let m = manifest();
    assert_eq!(m["id"], ID);
    assert_eq!(m["version"], VERSION);
    assert_eq!(m["publisher"]["id"], "deckpilot");
    // Key state, and nothing else: no network, no notifications.
    assert_eq!(
        m["permissions"],
        json!([{
            "kind": "device", "state": true,
            "justification": m["permissions"][0]["justification"]
        }])
    );
    let declared = m["contributes"]["errorCodes"]
        .as_object()
        .expect("errorCodes");
    let declared: BTreeSet<&str> = declared.keys().map(String::as_str).collect();
    assert_eq!(declared, PLUGIN_CODES.iter().copied().collect());
    for code in PLUGIN_CODES {
        assert!(!codes::is_shared(code), "{code} redeclares a shared code");
    }
    let variables: Vec<&Value> = m["contributes"]["variables"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| &v["id"])
        .collect();
    assert_eq!(variables, [&json!(MOVES_VARIABLE), &json!(FOUND_VARIABLE)]);
    let setting = &m["contributes"]["settings"]["fields"][0];
    assert_eq!(setting["key"], PAIRS_SETTING);
    assert_eq!(setting["kind"]["min"], MIN_PAIRS);
    assert_eq!(setting["kind"]["max"], MAX_PAIRS);
    assert_eq!(setting["default"]["value"], DEFAULT_PAIRS);
}

/// The manifest rule of DeckPilot today: only a read-only action may show
/// state. `card` and `score` show it and change nothing; `flip` and
/// `new-game` change the board and declare so, and show none.
#[test]
fn only_read_only_actions_carry_state_and_the_effects_are_honest() {
    let m = manifest();
    let actions = m["contributes"]["actions"].as_array().unwrap();
    let effect = |id: &str| {
        let a = actions.iter().find(|a| a["id"] == id).unwrap();
        (a["node"]["effect"].clone(), a.get("state").is_some())
    };
    assert_eq!(effect("card"), (json!("readOnly"), true));
    assert_eq!(effect("score"), (json!("readOnly"), true));
    assert_eq!(effect("flip"), (json!("modifies"), false));
    assert_eq!(effect("new-game"), (json!("modifies"), false));
    for action in plugin().actions() {
        let declared = effect(action.id()).1;
        assert_eq!(action.has_state(), declared, "{}", action.id());
    }
    // A state handler only reads: the scripted agent records any write.
    let agent = new_agent(None);
    let mut host = host_of(&agent);
    host.invoke(&plugin(), "new-game", json!({}));
    let before = agent.store.borrow().clone();
    state(&mut host, "card", json!({ "cell": 1 }));
    state(&mut host, "score", json!({}));
    assert_eq!(*agent.store.borrow(), before);
}

#[test]
fn icon_is_a_128px_png() {
    let png = include_bytes!("../assets/icon-128.png");
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let side = |at: usize| u32::from_be_bytes([png[at], png[at + 1], png[at + 2], png[at + 3]]);
    assert_eq!((side(16), side(20)), (128, 128));
}

#[test]
fn there_is_a_face_and_a_name_for_every_pair() {
    let glyphs: BTreeSet<String> = FACES.iter().map(|(g, _)| format!("{g:?}")).collect();
    let names: BTreeSet<&str> = FACES.iter().map(|(_, n)| *n).collect();
    assert_eq!(glyphs.len(), usize::from(MAX_PAIRS));
    assert_eq!(names.len(), usize::from(MAX_PAIRS));
    assert!(names.iter().all(|n| n.chars().count() <= 32));
}

// ------------------------------------------------------------ the game

#[test]
fn a_new_board_has_every_face_twice_and_nothing_up() {
    for pairs in [0, 1, MIN_PAIRS, 6, MAX_PAIRS, 17, u8::MAX] {
        let b = Board::new(pairs, 42, 1_000);
        let expected = pairs.clamp(MIN_PAIRS, MAX_PAIRS);
        assert_eq!(b.pairs(), expected);
        assert_eq!(b.cells(), usize::from(expected) * 2);
        for p in 0..expected {
            assert_eq!(b.cards.iter().filter(|c| **c == p).count(), 2);
        }
        assert!(b.up.is_empty() && b.matched.iter().all(|m| !m));
        assert_eq!((b.moves, b.started_ms, b.finished_ms), (0, 1_000, None));
        assert_eq!(b.clone().checked(), Some(b));
    }
}

#[test]
fn the_shuffle_depends_on_the_seed_and_is_fair() {
    // Different seeds give different orders; the same seed the same one.
    let a = Board::new(8, 1, 0).cards;
    assert_eq!(Board::new(8, 1, 0).cards, a);
    let orders: BTreeSet<Vec<u8>> = (0..200).map(|seed| Board::new(8, seed, 0).cards).collect();
    assert!(orders.len() > 190, "{} distinct orders", orders.len());
    // Every position holds every face about as often (2 cards of 4 pairs:
    // each face expected 1/4 of the time at each of the 8 positions).
    let games = 8_000_u32;
    let mut counts = [[0_u32; 4]; 8];
    for seed in 0..u64::from(games) {
        for (pos, face) in Board::new(4, seed * 2_654_435_761, 0)
            .cards
            .iter()
            .enumerate()
        {
            counts[pos][usize::from(*face)] += 1;
        }
    }
    for row in counts {
        for count in row {
            let share = f64::from(count) / f64::from(games);
            assert!((0.22..0.28).contains(&share), "{share}");
        }
    }
}

#[test]
fn a_whole_game_with_every_kind_of_turn() {
    let mut b = Board::new(3, 9, 10_000);
    let pairs = {
        let mut by: BTreeMap<u8, Vec<usize>> = BTreeMap::new();
        for (i, c) in b.cards.iter().enumerate() {
            by.entry(*c).or_default().push(i);
        }
        by
    };
    let (a0, a1) = (pairs[&0][0], pairs[&0][1]);
    let (b0, b1) = (pairs[&1][0], pairs[&1][1]);
    let (c0, c1) = (pairs[&2][0], pairs[&2][1]);

    assert_eq!(b.face(a0), Face::Down);
    assert_eq!(b.flip(a0, 10_100), Ok(Flip::First));
    assert_eq!(
        b.face(a0),
        Face::Up {
            pair: 0,
            matched: false
        }
    );
    // The same card again: nothing to turn.
    assert_eq!(b.flip(a0, 10_200), Err(Refused::AlreadyUp));
    // A miss: both stay up, one move.
    assert_eq!(b.flip(b0, 10_300), Ok(Flip::Miss));
    assert_eq!((b.moves, b.up.len()), (1, 2));
    assert_eq!(
        b.face(b0),
        Face::Up {
            pair: 1,
            matched: false
        }
    );
    // The next flip turns them back first, then turns the new card.
    assert_eq!(b.flip(a0, 10_400), Ok(Flip::First));
    assert_eq!(b.face(b0), Face::Down);
    assert_eq!(b.flip(a1, 10_500), Ok(Flip::Match));
    assert_eq!(
        b.face(a1),
        Face::Up {
            pair: 0,
            matched: true
        }
    );
    assert_eq!((b.moves, b.pairs_found()), (2, 1));
    // A matched card cannot be turned again.
    assert_eq!(b.flip(a0, 10_600), Err(Refused::AlreadyUp));
    assert_eq!(b.flip(b0, 10_700), Ok(Flip::First));
    assert_eq!(b.flip(b1, 10_800), Ok(Flip::Match));
    assert_eq!(b.flip(c0, 10_900), Ok(Flip::First));
    assert_eq!(b.seconds(), None);
    assert_eq!(b.flip(c1, 75_000), Ok(Flip::Won));
    assert!(b.is_won());
    assert_eq!((b.moves, b.pairs_found(), b.seconds()), (4, 3, Some(65)));
    // Over: nothing turns, off-board is off-board.
    assert_eq!(b.flip(c0, 80_000), Err(Refused::GameOver));
    assert_eq!(b.flip(6, 80_000), Err(Refused::NoSuchCard));
    assert_eq!(b.face(6), Face::Off);
    assert_eq!(b.clone().checked(), Some(b));
}

#[test]
fn a_damaged_or_foreign_board_is_no_board() {
    let good = Board::new(3, 5, 0);
    let mut cases: Vec<Board> = Vec::new();
    let mut f = |change: &dyn Fn(&mut Board)| {
        let mut b = good.clone();
        change(&mut b);
        cases.push(b);
    };
    f(&|b| b.version = BOARD_VERSION + 1);
    f(&|b| {
        b.cards.pop();
    });
    f(&|b| b.cards[0] = 9);
    f(&|b| {
        let other = b.cards.iter().position(|c| *c != b.cards[0]).unwrap();
        b.cards[other] = b.cards[0];
    });
    f(&|b| {
        b.matched.pop();
    });
    f(&|b| b.up = vec![0, 1, 2]);
    f(&|b| b.up = vec![1, 1]);
    f(&|b| b.up = vec![99]);
    f(&|b| {
        b.matched[0] = true;
        b.up = vec![0];
    });
    f(&|b| {
        b.cards = vec![0, 0];
        b.matched = vec![false; 2];
    });
    for (i, case) in cases.into_iter().enumerate() {
        assert_eq!(case.checked(), None, "case {i}");
    }
}

#[test]
fn the_best_game_is_the_fewest_moves_then_the_fastest() {
    let best = Best {
        moves: 10,
        seconds: 90,
    };
    assert!(
        Best {
            moves: 9,
            seconds: 500
        }
        .beats(&best)
    );
    assert!(
        Best {
            moves: 10,
            seconds: 89
        }
        .beats(&best)
    );
    assert!(
        !Best {
            moves: 10,
            seconds: 90
        }
        .beats(&best)
    );
    assert!(
        !Best {
            moves: 11,
            seconds: 1
        }
        .beats(&best)
    );
}

// --------------------------------------------------------- the actions

#[test]
fn a_new_game_uses_one_random_seed_and_the_clock() {
    let agent = new_agent(Some(4));
    let mut host = host_of(&agent);
    let out = host.invoke(&plugin(), "new-game", json!({}));
    let (message, data) = ok_data(&out);
    assert_eq!(message, "New game: 4 pairs on cards 1 to 8");
    assert_eq!(data["pairs"], ParamValue::Integer(4));
    assert_eq!(data["cells"], ParamValue::Integer(8));
    assert_eq!(agent.random_calls.get(), 1);
    let b = board(&agent);
    assert_eq!(b, Board::new(4, 7, 1_794_900_000_000));
    // A new seed shuffles anew, and the variables start at zero.
    agent.seed.set(8);
    host.invoke(&plugin(), "new-game", json!({}));
    assert_ne!(board(&agent).cards, b.cards);
    let published: Vec<(String, Value)> = host
        .requests()
        .iter()
        .filter_map(|r| match r {
            CapabilityRequest::VariablesPublish(v) => Some((v.id.clone(), v.value.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        published[..2],
        [("moves".into(), json!(0)), ("pairsFound".into(), json!(0))]
    );
    // The board size comes from the settings, default 6, within 2-16.
    for (setting, pairs) in [(None, 6), (Some(2), 2), (Some(16), 16)] {
        let agent = new_agent(setting);
        let mut host = host_of(&agent);
        host.invoke(&plugin(), "new-game", json!({}));
        assert_eq!(board(&agent).pairs(), pairs);
    }
}

#[test]
fn a_whole_game_through_the_actions_keeps_the_best() {
    let agent = new_agent(Some(2));
    let mut host = host_of(&agent);
    // Before any game.
    assert_eq!(failed_code(&turn(&mut host, 1)), NO_GAME);
    assert_eq!(
        failed_code(&host.invoke(&plugin(), "score", json!({}))),
        NO_GAME
    );
    host.invoke(&plugin(), "new-game", json!({}));
    let pairs = pairs_of(&board(&agent));
    let (a, b) = (&pairs[&0], &pairs[&1]);

    let out = turn(&mut host, a[0]);
    assert_eq!(
        ok_data(&out).1["result"],
        ParamValue::String("first".into())
    );
    let out = turn(&mut host, b[0]);
    let (message, data) = ok_data(&out);
    assert_eq!(message, "No pair; turn the next card to go on");
    assert_eq!(data["result"], ParamValue::String("miss".into()));
    assert_eq!(data["moves"], ParamValue::Integer(1));
    // The same card as the last one: skipped, the miss turned back.
    let out = turn(&mut host, a[0]);
    assert_eq!(
        ok_data(&out).1["result"],
        ParamValue::String("first".into())
    );
    let out = turn(&mut host, a[0]);
    assert!(matches!(out, ActionOutcome::Skipped { .. }), "{out:?}");
    let out = turn(&mut host, a[1]);
    assert_eq!(ok_data(&out).0, "A pair: 1 of 2 found");
    turn(&mut host, b[0]);
    agent.now_ms.set(agent.now_ms.get() + 83_000);
    let out = turn(&mut host, b[1]);
    let (message, data) = ok_data(&out);
    assert_eq!(message, "All 2 pairs in 3 moves and 1:23, a new best");
    assert_eq!(data["result"], ParamValue::String("won".into()));
    assert_eq!(data["pairsFound"], ParamValue::Integer(2));
    assert_eq!(
        agent.store.borrow()["best.2"],
        json!({ "moves": 3, "seconds": 83 })
    );
    assert_eq!(failed_code(&turn(&mut host, a[0])), GAME_OVER);
    let out = host.invoke(&plugin(), "score", json!({}));
    assert_eq!(ok_data(&out).0, "Won: 2 pairs in 3 moves and 1:23");

    // A game of more moves does not replace the best, however fast.
    host.invoke(&plugin(), "new-game", json!({}));
    let pairs = pairs_of(&board(&agent));
    let (a, b) = (&pairs[&0], &pairs[&1]);
    for cell in [a[0], b[0], a[0], b[0], a[0], a[1], b[0], b[1]] {
        turn(&mut host, cell);
    }
    assert_eq!(board(&agent).moves, 4);
    assert!(board(&agent).is_won());
    assert_eq!(
        agent.store.borrow()["best.2"],
        json!({ "moves": 3, "seconds": 83 })
    );
}

#[test]
fn cards_off_the_board_and_bad_parameters_are_refused() {
    let agent = new_agent(Some(2));
    let mut host = host_of(&agent);
    host.invoke(&plugin(), "new-game", json!({}));
    let before = agent.store.borrow().clone();
    // On the plugin's range but not on this board of 4 cards.
    for cell in [5, 32] {
        assert_eq!(failed_code(&turn(&mut host, cell)), NO_SUCH_CARD, "{cell}");
        let out = host.invoke(&plugin(), "card", json!({ "cell": cell }));
        assert_eq!(failed_code(&out), NO_SUCH_CARD, "{cell}");
    }
    // Outside the declared range, or of another kind: refused by the SDK.
    for params in [
        json!({ "cell": 0 }),
        json!({ "cell": 33 }),
        json!({ "cell": "1" }),
        json!({}),
    ] {
        assert_eq!(
            failed_code(&host.invoke(&plugin(), "flip", params.clone())),
            codes::INVALID_PARAMETERS,
            "{params}"
        );
    }
    assert_eq!(*agent.store.borrow(), before);
}

#[test]
fn a_damaged_stored_board_asks_for_a_new_game() {
    for stored in [json!("garbage"), json!({ "version": 99 }), json!(17)] {
        let agent = new_agent(None);
        agent
            .store
            .borrow_mut()
            .insert("board".into(), stored.clone());
        let mut host = host_of(&agent);
        assert_eq!(failed_code(&turn(&mut host, 1)), NO_GAME, "{stored}");
        assert_eq!(
            state(&mut host, "card", json!({ "cell": 1 })),
            KeyState::new()
        );
        let out = host.invoke(&plugin(), "new-game", json!({}));
        assert!(matches!(out, ActionOutcome::Ok { .. }), "{out:?}");
    }
}

#[test]
fn a_denied_or_older_agent_fails_cleanly() {
    let mut denied = MockHost::deny_all();
    for (action, params) in [("new-game", json!({})), ("flip", json!({ "cell": 1 }))] {
        let out = denied.invoke(&plugin(), action, params);
        assert_eq!(failed_code(&out), codes::NOT_PERMITTED, "{action}");
    }
    // An agent without random bytes (below plugin API 1.1): no game is
    // started, and nothing is stored.
    let store: Rc<RefCell<Vec<String>>> = Rc::default();
    let writes = Rc::clone(&store);
    let mut old = MockHost::new(move |request| match request {
        CapabilityRequest::RandomBytes(_) | CapabilityRequest::TimeNow(_) => {
            CallResult::error(CallErrorCode::UnsupportedOp, "unsupported operation")
        }
        CapabilityRequest::StorageSet(s) => {
            writes.borrow_mut().push(s.key.clone());
            CallResult::Ok(Value::Null)
        }
        CapabilityRequest::SettingsGet(_) => CallResult::Ok(json!({})),
        _ => CallResult::Ok(Value::Null),
    });
    let out = old.invoke(&plugin(), "new-game", json!({}));
    assert!(matches!(out, ActionOutcome::Failed { .. }), "{out:?}");
    assert!(store.borrow().is_empty());
}

// ------------------------------------------------------- the key states

#[test]
fn a_card_key_shows_its_back_its_face_and_a_found_pair() {
    assert_eq!(card_face(Face::Off), KeyState::new());
    assert_eq!(
        card_face(Face::Down),
        KeyState::new().label("?").accent(Accent::Blue)
    );
    let up = card_face(Face::Up {
        pair: 0,
        matched: false,
    });
    assert_eq!(
        up,
        KeyState::new()
            .label("Star")
            .glyph(Glyph::Star)
            .accent(Accent::Green)
    );
    let found = card_face(Face::Up {
        pair: 0,
        matched: true,
    });
    assert_eq!(found.indicator, Some(Indicator::Ok));
    assert_eq!(found.toggle, Some(true));
    // Every face is a valid state, distinct from every other.
    let faces: BTreeSet<String> = (0..MAX_PAIRS)
        .map(|pair| {
            let s = card_face(Face::Up {
                pair,
                matched: false,
            });
            s.validate().expect("within the bounds");
            format!("{s:?}")
        })
        .collect();
    assert_eq!(faces.len(), usize::from(MAX_PAIRS));
}

#[test]
fn card_keys_follow_the_game_through_key_state() {
    let agent = new_agent(Some(2));
    let mut host = host_of(&agent);
    // No game: every key stays as the user designed it.
    assert_eq!(
        state(&mut host, "card", json!({ "cell": 1 })),
        KeyState::new()
    );
    host.invoke(&plugin(), "new-game", json!({}));
    let pairs = pairs_of(&board(&agent));
    let a = &pairs[&0];
    let shown = |host: &mut MockHost, cell: i64| state(host, "card", json!({ "cell": cell }));
    assert_eq!(shown(&mut host, a[0]).label.as_deref(), Some("?"));
    turn(&mut host, a[0]);
    assert_eq!(shown(&mut host, a[0]).glyph, Some(Glyph::Star));
    assert_eq!(shown(&mut host, a[1]).label.as_deref(), Some("?"));
    turn(&mut host, a[1]);
    assert_eq!(shown(&mut host, a[1]).indicator, Some(Indicator::Ok));
    // Off this board: the key is left alone.
    assert_eq!(shown(&mut host, 9), KeyState::new());
}

#[test]
fn the_score_key_shows_progress_then_the_result() {
    let mut b = Board::new(2, 3, 0);
    let s = score_face(&b, None);
    assert_eq!(s.label.as_deref(), Some("0/2"));
    assert_eq!(
        (s.progress, s.indicator),
        (Some(0), Some(Indicator::Running))
    );
    let by = pairs_of(&b);
    let idx = |c: i64| usize::try_from(c - 1).unwrap();
    b.flip(idx(by[&0][0]), 0).unwrap();
    b.flip(idx(by[&0][1]), 0).unwrap();
    assert_eq!(score_face(&b, None).progress, Some(50));
    b.flip(idx(by[&1][0]), 0).unwrap();
    b.flip(idx(by[&1][1]), 61_000).unwrap();
    let won = score_face(&b, None);
    assert_eq!(won.label.as_deref(), Some("2 moves"));
    assert_eq!(won.detail.as_deref(), Some("1:01"));
    assert_eq!((won.progress, won.accent), (Some(100), Some(Accent::Green)));
    // A star for a best (none before, or this one), none when beaten.
    assert_eq!(won.glyph, Some(Glyph::Star));
    let this = Best {
        moves: 2,
        seconds: 61,
    };
    assert_eq!(score_face(&b, Some(this)).glyph, Some(Glyph::Star));
    let better = Best {
        moves: 2,
        seconds: 10,
    };
    assert_eq!(score_face(&b, Some(better)).glyph, None);

    // Through the host: without a game, the best of the size as a badge.
    let agent = new_agent(Some(2));
    agent
        .store
        .borrow_mut()
        .insert("best.2".into(), json!({ "moves": 14, "seconds": 70 }));
    let mut host = host_of(&agent);
    let idle = state(&mut host, "score", json!({}));
    assert_eq!(idle.label.as_deref(), Some("Memory"));
    assert_eq!(idle.badge.as_deref(), Some("14"));
}
