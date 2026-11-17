//! Memory: find the pairs among face-down cards on your keys.
//!
//! Every card is one key. Its **click** runs `card`, a read-only action
//! whose key state shows the card (face down, or its face once turned);
//! its **long press** runs `flip`, which turns it. A key can show state
//! only for a read-only action (the manifest rule of DeckPilot today), and
//! `flip` changes the board, so the two are separate actions on the same
//! key; one press per move waits for that rule to change (README).
//!
//! Capabilities: the shuffle's seed from the agent's random source
//! (`random.bytes`, plugin API 1.1), the time of the start and the finish
//! (`time.now`), the board in the plugin's storage, and key state (plugin
//! API 1.2, `device:state`). No network, no notifications.
#![deny(unsafe_code)]

pub mod game;

use deckpilot_plugin_sdk::prelude::*;
use deckpilot_plugin_sdk::{ApiLevel, Settings};

use game::{Best, Board, Face, Flip, MAX_PAIRS, MIN_PAIRS, Refused};

/// Must equal `manifest.json` (`id`, `version`).
pub const ID: &str = "deckpilot.memory";
pub const VERSION: &str = "0.1.0";
/// Key state, the time and random bytes: plugin API 1.2.
pub const API: ApiLevel = ApiLevel { major: 1, minor: 2 };

/// The storage key of the board.
const BOARD_KEY: &str = "board";
/// The storage key of the best game of a board size (`best.<pairs>`).
fn best_key(pairs: u8) -> String {
    format!("best.{pairs}")
}
/// The setting of the board size, and its default.
const PAIRS_SETTING: &str = "pairs";
const DEFAULT_PAIRS: u8 = 6;
/// The plugin's own variables.
const MOVES_VARIABLE: &str = "moves";
const FOUND_VARIABLE: &str = "pairsFound";

/// Error codes of this plugin, each declared in `contributes.errorCodes`.
pub const NO_GAME: &str = "no-game";
pub const NO_SUCH_CARD: &str = "no-such-card";
pub const GAME_OVER: &str = "game-over";
pub const PLUGIN_CODES: &[&str] = &[NO_GAME, NO_SUCH_CARD, GAME_OVER];

/// The faces, one per pair: a built-in glyph and its name.
pub const FACES: [(Glyph, &str); MAX_PAIRS as usize] = [
    (Glyph::Star, "Star"),
    (Glyph::Heart, "Heart"),
    (Glyph::Diamond, "Diamond"),
    (Glyph::Triangle, "Triangle"),
    (Glyph::Circle, "Circle"),
    (Glyph::Square, "Square"),
    (Glyph::Bell, "Bell"),
    (Glyph::Timer, "Clock"),
    (Glyph::Check, "Tick"),
    (Glyph::Cross, "Cross"),
    (Glyph::Play, "Play"),
    (Glyph::Pause, "Pause"),
    (Glyph::Stop, "Stop"),
    (Glyph::Up, "Up"),
    (Glyph::Down, "Down"),
    (Glyph::Dot, "Dot"),
];
/// The colours of the faces, by pair (the back of a card is blue).
const FACE_ACCENTS: [Accent; 5] = [
    Accent::Green,
    Accent::Amber,
    Accent::Red,
    Accent::Purple,
    Accent::Teal,
];

/// The plugin definition. The action ids, their order and their parameters
/// must equal `manifest.json`; a unit test compares them.
pub fn plugin() -> Plugin {
    let cell = || {
        Param::integer("cell")
            .min(1.0)
            .max(f64::from(MAX_PAIRS) * 2.0)
            .required()
    };
    Plugin::new(ID, VERSION)
        .requires_api(API)
        .action(Action::new("new-game", new_game))
        .action(Action::new("flip", flip).param(cell()))
        .action(Action::new("card", card).param(cell()).state(card_state))
        .action(Action::new("score", score).state(score_state))
}

deckpilot_plugin_sdk::plugin!(plugin);

/// The board size from the settings, within the bounds.
fn pairs(settings: &Settings) -> u8 {
    settings
        .integer(PAIRS_SETTING)
        .and_then(|n| u8::try_from(n).ok())
        .unwrap_or(DEFAULT_PAIRS)
        .clamp(MIN_PAIRS, MAX_PAIRS)
}

/// The board in storage, if there is one this version can play. A board
/// that does not parse or is not consistent is no board: the player
/// starts a new game.
fn load(ctx: &mut Context<'_>) -> Result<Option<Board>, CapabilityError> {
    let Some(value) = ctx.storage_get(BOARD_KEY)? else {
        return Ok(None);
    };
    Ok(serde_json::from_value::<Board>(value)
        .ok()
        .and_then(Board::checked))
}

fn save(ctx: &mut Context<'_>, board: &Board) -> Result<(), CapabilityError> {
    ctx.storage_set(BOARD_KEY, board)
}

fn publish(ctx: &mut Context<'_>, board: &Board) -> Result<(), CapabilityError> {
    ctx.publish_variable(MOVES_VARIABLE, i64::from(board.moves))?;
    ctx.publish_variable(FOUND_VARIABLE, i64::from(board.pairs_found()))
}

/// The cell parameter (1-based on keys) as an index into the board.
fn cell(args: &Args<'_>) -> Result<usize, PluginError> {
    let cell: i64 = args.get("cell")?;
    Ok(usize::try_from(cell - 1).unwrap_or(usize::MAX))
}

fn clock(seconds: u64) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn no_game() -> PluginError {
    PluginError::new(NO_GAME, "no game is running; start a new game")
}

fn new_game(ctx: &mut Context<'_>, _args: &Args<'_>) -> ActionResult {
    let pairs = pairs(&ctx.settings()?);
    let seed = ctx.random_u64()?;
    let now = ctx.now()?;
    let board = Board::new(pairs, seed, now.unix_ms);
    save(ctx, &board)?;
    publish(ctx, &board)?;
    let cells = board.cells();
    Ok(
        Output::message(format!("New game: {pairs} pairs on cards 1 to {cells}"))
            .with("pairs", i64::from(pairs))
            .with("cells", i64::try_from(cells).unwrap_or(0)),
    )
}

fn flip(ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let cell = cell(args)?;
    let mut board = load(ctx)?.ok_or_else(no_game)?;
    let now = ctx.now()?;
    let flipped = match board.flip(cell, now.unix_ms) {
        Ok(flipped) => flipped,
        Err(Refused::NoSuchCard) => {
            return Err(PluginError::new(
                NO_SUCH_CARD,
                format!(
                    "card {} is not on this board of {} cards",
                    cell.saturating_add(1),
                    board.cells()
                ),
            ));
        }
        Err(Refused::GameOver) => {
            return Err(PluginError::new(
                GAME_OVER,
                "every pair was found; start a new game",
            ));
        }
        Err(Refused::AlreadyUp) => {
            // A miss may have just gone face down: keep that.
            save(ctx, &board)?;
            return Ok(Output::skipped(format!(
                "card {} is already face up",
                cell + 1
            )));
        }
    };
    save(ctx, &board)?;
    publish(ctx, &board)?;
    let message = match flipped {
        Flip::Won => {
            let seconds = board.seconds().unwrap_or(0);
            let game = Best {
                moves: board.moves,
                seconds,
            };
            let key = best_key(board.pairs());
            let best: Option<Best> = ctx.storage_get_as(key.as_str())?;
            let record = best.is_none_or(|best| game.beats(&best));
            if record {
                ctx.storage_set(&key, game)?;
            }
            format!(
                "All {} pairs in {} moves and {}{}",
                board.pairs(),
                board.moves,
                clock(seconds),
                if record { ", a new best" } else { "" }
            )
        }
        Flip::First => format!("Card {} is up; turn another", cell + 1),
        Flip::Match => format!("A pair: {} of {} found", board.pairs_found(), board.pairs()),
        Flip::Miss => "No pair; turn the next card to go on".to_owned(),
    };
    Ok(Output::message(message)
        .with("result", flipped.name())
        .with("moves", i64::from(board.moves))
        .with("pairsFound", i64::from(board.pairs_found())))
}

fn card(ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let cell = cell(args)?;
    let board = load(ctx)?.ok_or_else(no_game)?;
    let shown = match board.face(cell) {
        Face::Off => {
            return Err(PluginError::new(
                NO_SUCH_CARD,
                format!(
                    "card {} is not on this board of {} cards",
                    cell.saturating_add(1),
                    board.cells()
                ),
            ));
        }
        Face::Down => format!("Card {} is face down; long-press to turn it", cell + 1),
        Face::Up { pair, matched } => format!(
            "Card {} is {}{}",
            cell + 1,
            FACES[usize::from(pair)].1,
            if matched { ", found" } else { "" }
        ),
    };
    Ok(Output::message(shown))
}

/// What a card's key shows: the back of the card, its face once turned,
/// and a mark once its pair was found. Off the board, or without a game,
/// the key stays as the user designed it.
fn card_state(ctx: &mut Context<'_>, args: &Args<'_>) -> Result<KeyState, PluginError> {
    let cell = cell(args)?;
    let Some(board) = load(ctx)? else {
        return Ok(KeyState::new());
    };
    Ok(card_face(board.face(cell)))
}

/// The key state of a card's face.
pub fn card_face(face: Face) -> KeyState {
    match face {
        Face::Off => KeyState::new(),
        Face::Down => KeyState::new().label("?").accent(Accent::Blue),
        Face::Up { pair, matched } => {
            let (glyph, name) = FACES[usize::from(pair)];
            let state = KeyState::new()
                .label(name)
                .glyph(glyph)
                .accent(FACE_ACCENTS[usize::from(pair) % FACE_ACCENTS.len()]);
            if matched {
                state.indicator(Indicator::Ok).toggle(true)
            } else {
                state
            }
        }
    }
}

fn score(ctx: &mut Context<'_>, _args: &Args<'_>) -> ActionResult {
    let board = load(ctx)?.ok_or_else(no_game)?;
    let message = match board.seconds() {
        Some(seconds) => format!(
            "Won: {} pairs in {} moves and {}",
            board.pairs(),
            board.moves,
            clock(seconds)
        ),
        None => format!(
            "{} of {} pairs, {} moves",
            board.pairs_found(),
            board.pairs(),
            board.moves
        ),
    };
    Ok(Output::message(message)
        .with("moves", i64::from(board.moves))
        .with("pairsFound", i64::from(board.pairs_found()))
        .with("pairs", i64::from(board.pairs())))
}

/// What the score key shows: pairs found and moves while playing, the
/// time once won (with a star for a best), and the best of this board
/// size when there is no game.
fn score_state(ctx: &mut Context<'_>, _args: &Args<'_>) -> Result<KeyState, PluginError> {
    let Some(board) = load(ctx)? else {
        let pairs = pairs(&ctx.settings()?);
        let best: Option<Best> = ctx.storage_get_as(best_key(pairs))?;
        let state = KeyState::new().label("Memory").detail("New game");
        return Ok(match best {
            Some(best) => state.badge(format!("{}", best.moves.min(9999))),
            None => state,
        });
    };
    let best: Option<Best> = ctx.storage_get_as(best_key(board.pairs()))?;
    Ok(score_face(&board, best))
}

/// The key state of the score key for `board`, with the best game of its
/// size.
pub fn score_face(board: &Board, best: Option<Best>) -> KeyState {
    let found = board.pairs_found();
    let pairs = board.pairs();
    let progress = u8::try_from(u32::from(found) * 100 / u32::from(pairs)).unwrap_or(100);
    match board.seconds() {
        Some(seconds) => {
            let game = Best {
                moves: board.moves,
                seconds,
            };
            let state = KeyState::new()
                .label(format!("{} moves", board.moves))
                .detail(clock(seconds))
                .progress(100)
                .accent(Accent::Green)
                .indicator(Indicator::Ok);
            // The best is this game when it set it.
            if best.is_none_or(|best| !best.beats(&game)) {
                state.glyph(Glyph::Star)
            } else {
                state
            }
        }
        None => KeyState::new()
            .label(format!("{found}/{pairs}"))
            .detail(format!("{} moves", board.moves))
            .progress(progress)
            .indicator(Indicator::Running),
    }
}

#[cfg(test)]
mod tests;
