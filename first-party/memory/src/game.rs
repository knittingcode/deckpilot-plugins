//! The game, without any capability: a board of face-down cards in pairs,
//! turned two at a time. Everything here is deterministic, so the tests
//! cover it exhaustively; the plugin adds randomness (the shuffle's seed),
//! the time and storage around it.

use serde::{Deserialize, Serialize};

/// The fewest pairs on a board.
pub const MIN_PAIRS: u8 = 2;
/// The most pairs on a board: one face per built-in glyph (16), so the
/// largest board is 32 cards, a Stream Deck XL.
pub const MAX_PAIRS: u8 = 16;
/// The version of the stored board; a board of another version is not
/// read (the player starts a new game).
pub const BOARD_VERSION: u32 = 1;

/// One game, as kept in the plugin's storage between key presses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Board {
    pub version: u32,
    /// The face of every card: a pair index `0..pairs`, each twice.
    pub cards: Vec<u8>,
    /// Cards whose pair was found.
    pub matched: Vec<bool>,
    /// Cards face up and not matched: none, one, or the two of a miss,
    /// which stay up until the next flip.
    pub up: Vec<usize>,
    /// Pairs of cards turned so far.
    pub moves: u32,
    /// When the game started, milliseconds since the Unix epoch (UTC).
    pub started_ms: u64,
    /// When the last pair was found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_ms: Option<u64>,
}

/// What one flip did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flip {
    /// The first card of a move is up.
    First,
    /// The second card matches the first; both stay up.
    Match,
    /// The second card does not match; both stay up until the next flip.
    Miss,
    /// The last pair was found.
    Won,
}

impl Flip {
    /// The name in the action's `result` output.
    pub fn name(self) -> &'static str {
        match self {
            Self::First => "first",
            Self::Match => "match",
            Self::Miss => "miss",
            Self::Won => "won",
        }
    }
}

/// Why a flip changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// The cell is not on this board.
    NoSuchCard,
    /// Every pair was found.
    GameOver,
    /// The card is already up or already matched: nothing to turn.
    AlreadyUp,
}

/// What a card shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    /// Not on this board.
    Off,
    /// Face down.
    Down,
    /// Face up, the pair index; `matched` once its pair was found.
    Up { pair: u8, matched: bool },
}

/// A small generator for the shuffle, seeded from the agent's random
/// source (SplitMix64). Not for anything secret: it only orders cards.
#[derive(Debug, Clone)]
pub struct Shuffle(u64);

impl Shuffle {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0..bound`, every value equally likely (`bound` > 0).
    fn below(&mut self, bound: u64) -> u64 {
        let zone = u64::MAX - (u64::MAX % bound + 1) % bound;
        loop {
            let draw = self.next();
            if draw <= zone {
                return draw % bound;
            }
        }
    }
}

impl Board {
    /// A new board of `pairs` pairs (brought within
    /// [`MIN_PAIRS`]..=[`MAX_PAIRS`]), shuffled with `seed`, started at
    /// `now_ms`.
    pub fn new(pairs: u8, seed: u64, now_ms: u64) -> Self {
        let pairs = pairs.clamp(MIN_PAIRS, MAX_PAIRS);
        let mut cards: Vec<u8> = (0..pairs).flat_map(|p| [p, p]).collect();
        // Fisher-Yates.
        let mut shuffle = Shuffle::new(seed);
        for i in (1..cards.len()).rev() {
            let j = usize::try_from(shuffle.below(i as u64 + 1)).unwrap_or(0);
            cards.swap(i, j);
        }
        let len = cards.len();
        Self {
            version: BOARD_VERSION,
            cards,
            matched: vec![false; len],
            up: Vec::new(),
            moves: 0,
            started_ms: now_ms,
            finished_ms: None,
        }
    }

    /// A stored board, if it is one this version can play: the right
    /// version, a whole number of pairs within the bounds, each face twice,
    /// and consistent marks. Anything else is `None` (start a new game).
    pub fn checked(self) -> Option<Self> {
        let len = self.cards.len();
        let pairs = len / 2;
        let sane = self.version == BOARD_VERSION
            && len.is_multiple_of(2)
            && (usize::from(MIN_PAIRS)..=usize::from(MAX_PAIRS)).contains(&pairs)
            && self.matched.len() == len
            && self.up.len() <= 2
            && self.up.iter().all(|&i| i < len && !self.matched[i])
            && (self.up.len() < 2 || self.up[0] != self.up[1])
            && (0..pairs).all(|p| self.cards.iter().filter(|&&c| usize::from(c) == p).count() == 2);
        sane.then_some(self)
    }

    pub fn pairs(&self) -> u8 {
        u8::try_from(self.cards.len() / 2).unwrap_or(MAX_PAIRS)
    }

    pub fn cells(&self) -> usize {
        self.cards.len()
    }

    pub fn pairs_found(&self) -> u8 {
        u8::try_from(self.matched.iter().filter(|m| **m).count() / 2).unwrap_or(0)
    }

    pub fn is_won(&self) -> bool {
        self.matched.iter().all(|m| *m)
    }

    /// Seconds from the start to the last pair, once won.
    pub fn seconds(&self) -> Option<u64> {
        self.finished_ms
            .map(|end| end.saturating_sub(self.started_ms) / 1000)
    }

    /// What `cell` shows.
    pub fn face(&self, cell: usize) -> Face {
        if cell >= self.cells() {
            return Face::Off;
        }
        if self.matched[cell] || self.up.contains(&cell) {
            Face::Up {
                pair: self.cards[cell],
                matched: self.matched[cell],
            }
        } else {
            Face::Down
        }
    }

    /// Turns `cell` at `now_ms`. The two cards of a miss go face down
    /// first, so a miss stays visible until the player's next flip.
    pub fn flip(&mut self, cell: usize, now_ms: u64) -> Result<Flip, Refused> {
        if cell >= self.cells() {
            return Err(Refused::NoSuchCard);
        }
        if self.is_won() {
            return Err(Refused::GameOver);
        }
        if self.up.len() == 2 {
            self.up.clear();
        }
        if self.matched[cell] || self.up.contains(&cell) {
            return Err(Refused::AlreadyUp);
        }
        self.up.push(cell);
        if self.up.len() == 1 {
            return Ok(Flip::First);
        }
        self.moves = self.moves.saturating_add(1);
        let (a, b) = (self.up[0], self.up[1]);
        if self.cards[a] != self.cards[b] {
            return Ok(Flip::Miss);
        }
        self.matched[a] = true;
        self.matched[b] = true;
        self.up.clear();
        if self.is_won() {
            self.finished_ms = Some(now_ms);
            return Ok(Flip::Won);
        }
        Ok(Flip::Match)
    }
}

/// The best finished game for one board size: fewest moves, then the
/// shortest time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Best {
    pub moves: u32,
    pub seconds: u64,
}

impl Best {
    /// Whether `self` beats `other`.
    pub fn beats(&self, other: &Self) -> bool {
        (self.moves, self.seconds) < (other.moves, other.seconds)
    }
}
