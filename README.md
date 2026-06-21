

<!-- PROJECT LOGO -->
<br />
<p align="center">

  <h3 align="center">Chess Tactics CLI</h3>
</p>



![Screen shot](./assets/usage.gif)

Practice some chess tactics in your terminal while you wait for your code to
compile. Fetches tactics from [Chess Madra](https://chessmadra.com).


### Built With

* Rust
* [The Lichess Puzzles Database](https://database.lichess.org/#puzzles)
* [Shakmaty](https://github.com/niklasf/shakmaty)

## Installation

```sh
cargo install tactics-trainer-cli
```

<!-- USAGE EXAMPLES -->
## Usage

Usage is straightforward, just run `tactics-trainer`

```sh
tactics-trainer
```
Or specify some tags (See [this
file](https://github.com/ornicar/lila/blob/master/translation/source/puzzleTheme.xml) for all tags):
```sh
tactics-trainer --tags mateIn1
```

Or specify a rating range:
```sh
tactics-trainer --rating=600-1200
```

To keep fetching new puzzles after each solved tactic:
```sh
chess-practice --rating=600-1200 --tag mateIn1
```
From a checkout, run it with Cargo:
```sh
cargo run --bin chess-practice -- --tag mateIn1
```
The `scripts/chess-practice` checkout shim calls the Rust command.
When no `--rating` is supplied, `chess-practice` starts from `--rating=600-1200` and calibrates that range from recent entries in the practice log.
Each puzzle prints its Lichess training URL before the board.
The screen is cleared before each new puzzle.
Completed puzzles are logged to `~/.chess-practice/puzzles.jsonl` with puzzle id, rating, Unix timestamp, and correctness.
A move with incorrect syntax is logged as incorrect, even if the intended move was right.
Set `CHESS_PRACTICE_LOG` to use a different log file.

## Puzzle API

Puzzles are fetched with `POST https://chessmadra.com/api/v1/tactic`.
Set `TACTICS_SERVER_URL` to override the host; the path remains `/api/v1/tactic`.

The request body is JSON:

```json
{
  "ratingGte": 600,
  "ratingLte": 1200,
  "tags": ["mateIn1"]
}
```

The tag ids come from the Lichess puzzle theme source:
`https://raw.githubusercontent.com/lichess-org/lila/master/translation/source/puzzleTheme.xml`.

Available tag ids:

```text
advancedPawn
advantage
anastasiaMate
arabianMate
attackingF2F7
attraction
backRankMate
balestraMate
blindSwineMate
bishopEndgame
bodenMate
castling
capturingDefender
clearance
collinearMove
cornerMate
crushing
defensiveMove
deflection
discoveredAttack
discoveredCheck
doubleBishopMate
doubleCheck
dovetailMate
endgame
enPassant
epauletteMate
equality
exposedKing
fork
hangingPiece
hookMate
interference
intermezzo
killBoxMate
kingsideAttack
knightEndgame
long
master
masterVsMaster
mate
mateIn1
mateIn2
mateIn3
mateIn4
mateIn5
middlegame
morphysMate
oneMove
opening
operaMate
pawnEndgame
pillsburysMate
pin
promotion
queenEndgame
queenRookEndgame
queensideAttack
quietMove
rookEndgame
sacrifice
short
skewer
smotheredMate
superGM
swallowstailMate
trappedPiece
triangleMate
underPromotion
veryLong
vukovicMate
xRayAttack
zugzwang
```

<!-- ROADMAP -->
## Roadmap

- [ ] Sessions
- [ ] Spaced repetition of failed puzzles
- [ ] AND queries for themes

<!-- LICENSE -->
## License

Distributed under the MIT License. See `LICENSE` for more information.


<!-- CONTACT -->
## Contact

Marcus Bufett - [@marcusbuffett](https://twitter.com/marcusbuffett) - me@mbuffett.com
