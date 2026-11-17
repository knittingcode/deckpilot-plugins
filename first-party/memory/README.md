# Memory (`deckpilot.memory`)

The memory game on your keys: every key is a card, face down. Turn two;
if they match they stay up, if not they turn back at your next turn. Find
all the pairs in as few moves as you can. Every game is shuffled anew, and
the plugin keeps your best game for each board size.

Polish: [README.pl.md](README.pl.md).

> **Needs DeckPilot with plugin API 1.2** (key state). Older versions do
> not offer the plugin for installation.
>
> **Two gestures per card, for now.** A key shows a plugin's state only for
> an action that changes nothing, and turning a card changes the board. So
> a card is two actions on one key: a **click** shows the card (`Card`),
> a **long press** turns it (`Turn card`). Playing with one press per card
> waits for a change in DeckPilot that lets a turning action show the card
> too ([knittingcode/deckpilot#306](https://github.com/knittingcode/deckpilot/pull/306),
> under review). Once it lands, a later version of this plugin moves to
> one press per card ([What it does not do](#what-it-does-not-do)).

## Setup

1. Install and enable the plugin. Allow its one permission: showing state
   on its keys.
2. In the plugin's settings, choose the number of **Pairs** (default 6, so
   12 cards; from 2 to 16, which is 4 to 32 cards). It applies to the next
   new game.
3. Put **New game** on one key, and optionally **Score** on another.
4. For every card, numbered from 1 to the number of cards, set up one
   key:
   - **Click**: `Card`, with the parameter **Card** set to the card's
     number;
   - **Long press**: `Turn card`, with the same number.

   Give the key no title, or a short one such as its number: the card's
   face replaces it while the game runs.
5. Press **New game**.

Cards with a number above the board's size are left alone (their key shows
your title), so a 15-key deck can hold a board of 6 pairs and leave three
keys for New game, Score and anything else.

## How to play

- **Long-press** a card to turn it. Turn a second one:
  - if they match, both stay up with a check mark;
  - if not, both stay up so you can memorise them, and turn back when you
    turn your next card.
- A **click** on a card only tells you what it shows (in DeckPilot's status
  messages); it never changes the game.
- One move is two cards. When the last pair is found, the clock stops and
  the Score key shows your moves and time, with a star if it is your best
  game of that size.
- **New game** gives up the game in progress.

## What the keys show

| Key | Shows |
| --- | --- |
| A card, face down | `?` on blue |
| A card, face up | its picture (a star, a heart, a bell…) and its name, on its own colour |
| A card of a found pair | the same, with a check mark and a mark along the top |
| Score, while playing | pairs found, such as `3/6`, the moves, and a bar of the progress |
| Score, once won | the moves and the time, on green, with a star for a best |
| Score, without a game | `Memory`, and the fewest moves of your best game as a badge |

Sixteen pictures, one per pair, are drawn by DeckPilot itself at your
device's resolution; the plugin only names them. A device or a control
without a display shows nothing, and the game still works through the
status messages.

## Actions

| Action | Id | What it does | Effect |
| --- | --- | --- | --- |
| New game | `new-game` | Shuffles a board of the size in the settings and starts the clock | modifies |
| Turn card | `flip` | Turns one card (parameter **Card**, 1 to 32) | modifies |
| Card | `card` | Shows one card on its key; running it changes nothing | read-only |
| Score | `score` | Shows the score on its key; running it reports it | read-only |

Outputs, for workflows: New game gives `pairs` and `cells`; Turn card gives
`result` (`first`, `match`, `miss` or `won`), `moves` and `pairsFound`;
Score gives `moves`, `pairsFound` and `pairs`. Turning a card that is
already up is skipped, not an error.

Variables: `{{ plugins.deckpilot.memory.moves }}` and
`{{ plugins.deckpilot.memory.pairsFound }}`, for the current game.

## Settings

| Setting | Default | Meaning |
| --- | --- | --- |
| Pairs | 6 | How many pairs a new game has, from 2 to 16 (4 to 32 cards) |

## Permissions and data

- **Showing state on its keys** (`device`, `state`): to show each card and
  the score. The plugin describes a key (a text, a picture from
  DeckPilot's set, a colour, a mark); DeckPilot draws it. The plugin never
  learns which device or key you use.
- The shuffle takes 8 random bytes from DeckPilot's random source for each
  new game, and the clock is read at the start and at every turn. Neither
  needs a permission.
- The board and your best games are kept in the plugin's own storage on
  this computer, and deleted when you uninstall it. Nothing is sent
  anywhere: the plugin has no network access and shows no notifications.

## Errors

| Code | When |
| --- | --- |
| `no-game` | No game is running, or the stored one could not be read: press New game |
| `no-such-card` | The card's number is not on this board: bind cards from 1 to the number of cards |
| `game-over` | Every pair was found: press New game |

## What it does not do

- **One press per card.** Turning a card changes the board, and DeckPilot
  shows a plugin's state only on keys of actions that change nothing. Hence
  the click to show and the long press to turn. A change that allows state
  on any action, since showing state can never change anything,
  is waiting for review in DeckPilot
  ([#306](https://github.com/knittingcode/deckpilot/pull/306)). The
  plugin does not mark its turning action as read-only to get round this:
  that would tell you something untrue about it.
- **Hiding a miss after a moment.** A plugin has no timer; a miss stays up
  until your next turn.
- **Several players**, levels or a time limit.

## Building and testing

From `first-party/`:

```sh
cargo test --locked -p deckpilot-memory
cargo build --locked -p deckpilot-memory --target wasm32-unknown-unknown --profile guest
```

`tools/first-party.sh` checks the layout and the Polish texts, and
packages the plugin under the test publisher (first-party plugins install
only from the application bundle today). The tests cover the game
exhaustively (the shuffle's fairness, every kind of turn, damaged boards),
every action against a scripted agent, the key states and every error
code.
