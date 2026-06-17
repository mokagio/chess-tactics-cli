use std::env;

use anyhow::Result;
use clap::Parser;
use colored::*;
use serde::{Deserialize, Serialize};
use shakmaty::{
    fen::{self, Fen},
    san::San,
    uci::Uci,
    Board, CastlingMode, Chess, Color, Piece, Position, Setup, Square,
};

#[derive(Parser, Debug)]
#[clap(version = "1.0", author = "Marcus B. <me@mbuffett.com>")]
struct Args {
    #[clap(short, long)]
    /// The rating range of the tactics to fetch. Try 0-1200 for easy, 1200-1800 for
    /// intermediate, or 1800-3000 for difficult tactics.
    rating: Option<String>,
    #[clap(short, long)]
    /// Optionally specify a list of tags to get tactics for. Every tactic returned will have one
    /// of these tags
    tags: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let opts = Args::parse();
    // dbg!(&opts);
    let (rating_lower_bound, rating_upper_bound): (Option<i32>, Option<i32>) = {
        match opts.rating {
            Some(rating) => {
                let parts = rating.split("-").collect::<Vec<&str>>();
                match (parts.get(0), parts.get(1)) {
                    (Some(first), Some(second)) => {
                        let parse_rating = |s: &str| -> i32 {
                            s.parse::<i32>()
                                .expect(&format!("Failed to parse {} as a rating", s))
                        };
                        (Some(parse_rating(first)), Some(parse_rating(second)))
                    }
                    _ => {
                        panic!("Could not parse rating, make sure it's in the form '500-1200'")
                    }
                }
            }
            None => (None, None),
        }
    };
    let tactic = get_new_puzzle(ChessTacticRequest {
        rating_gte: rating_lower_bound,
        rating_lte: rating_upper_bound,
        tags: opts.tags,
    })
    .await
    .expect("Failed to get a new tactic from the server, exiting.");
    let fen = tactic.fen;
    // let fen = "r6k/pp2r2p/4Rp1Q/3p4/8/1N1P2R1/PqP2bPP/7K b - - 0 24";
    let moves = tactic.moves;
    let setup: Fen = fen.parse()?;
    let mut position: Chess = setup.position(CastlingMode::Standard)?;
    let mut continuation_moves = moves.iter().map(|m| -> Uci { m.parse().unwrap() });
    let first_move = &continuation_moves
        .next()
        .unwrap()
        .to_move(&position)
        .unwrap();
    let their_side = position.turn();
    position = position.play(first_move).unwrap();
    println!();
    print_board(&position);
    let mut next_move = continuation_moves
        .next()
        .unwrap()
        .to_move(&position)
        .unwrap();
    loop {
        println!();
        let san_move = San::from_move(&position, &next_move);
        // dbg!(&san_move.to_string());
        let reply = get_prompt_response(&position);
        println!();
        let mut correct = false;
        match reply {
            PromptResponse::ShowBoard => {
                print_board(&position);
                continue;
            }
            PromptResponse::Help => {
                print_help();
                continue;
            }
            PromptResponse::PrintFen => {
                println!("{}", fen::epd(&position).to_string());
                continue;
            }
            PromptResponse::NoResponse => {}
            PromptResponse::ShowRating => {
                println!("This tactic is rated {}.", tactic.rating);
                continue;
            }
            PromptResponse::Move(move_input) => {
                if move_input == san_move.to_string() {
                    correct = true;
                } else {
                    println!("{} is not the correct move", move_input);
                    continue;
                }
            }
        }
        let reply = san_move.to_move(&position).unwrap();
        position = position.play(&reply).unwrap();
        let response = continuation_moves.next();
        match response {
            Some(response) => {
                let prefix = if correct {
                    "Correct! ".to_string()
                } else {
                    format!("The correct move was {}. ", san_move.to_string())
                };
                let response = response.to_move(&position).unwrap();
                let response_san = San::from_move(&position, &response);
                println!(
                    "{}{} responds with {}",
                    prefix,
                    print_side(&their_side),
                    response_san.to_string()
                );
                position = position.play(&response).unwrap();
                next_move = continuation_moves
                    .next()
                    .unwrap()
                    .to_move(&position)
                    .unwrap();
            }
            None => {
                let prefix = if correct {
                    "Correct! ".to_string()
                } else {
                    "".to_string()
                };
                println!("{}Completed this tactic.", prefix);
                break;
            }
        };
    }
    return Ok(());
}

enum PromptResponse {
    ShowBoard,
    NoResponse,
    PrintFen,
    Help,
    ShowRating,
    Move(String),
}

fn get_prompt_response(position: &Chess) -> PromptResponse {
    let reply = rprompt::prompt_reply_stdout(&get_prompt(position)).unwrap();
    match reply.as_ref() {
        "s" | "show" => return PromptResponse::ShowBoard,
        "f" | "fen" => return PromptResponse::PrintFen,
        "?" | "help" => return PromptResponse::Help,
        "r" | "rating" => return PromptResponse::ShowRating,
        // "h" | "hint" => return PromptResponse::NoResponse,
        "" => return PromptResponse::NoResponse,
        x => return PromptResponse::Move(x.to_string()),
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ChessTactic {
    pub id: String,
    pub moves: Vec<String>,
    pub fen: String,
    pub popularity: i32,
    pub tags: Vec<String>,
    pub game_link: String,
    pub rating: i32,
    pub rating_deviation: i32,
    pub number_plays: i32,
}

#[derive(Serialize, Debug)]
struct ChessTacticRequest {
    rating_gte: Option<i32>,
    rating_lte: Option<i32>,
    tags: Vec<String>,
}

async fn get_new_puzzle(request: ChessTacticRequest) -> Result<ChessTactic> {
    let client = reqwest::Client::new();
    let tactic: ChessTactic = client
        .post(get_api_endpoint())
        .header("User-Agent", "tactics-trainer-cli")
        .json(&request)
        .send()
        .await?
        .json()
        .await?;
    // dbg!(&tactic);
    return Ok(tactic);
}

fn get_api_endpoint() -> String {
    return format!(
        "{}/api/v1/tactic",
        env::var("TACTICS_SERVER_URL").unwrap_or("https://chessmadra.com".to_string())
    );
}

fn print_side(side: &Color) -> String {
    if side == &Color::White {
        "White".to_string()
    } else {
        "Black".to_string()
    }
}

fn get_prompt(position: &Chess) -> String {
    let side = if position.turn() == Color::White {
        "White"
    } else {
        "Black"
    };
    return format!("{} to move, enter the best move, or '?' for help: ", side);
}

fn print_help() {
    let rows = [
        (
            "Any move, ex. Qxd7",
            "Attempt to solve the tactic with the given move.",
        ),
        (
            "No input",
            "Reveal the answer, and continue the tactic if there are more moves.",
        ),
        (
            "'f' or 'fen'",
            "Print out the current board, in FEN notation.",
        ),
        ("'s' or 'show'", "Show the current board."),
        ("'r' or 'rating'", "Show the rating of the current tactic."),
        ("'?' or 'help'", "Display this help."),
    ];
    let width = rows.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    for (key, desc) in rows {
        println!("  {:<width$}  {}", key, desc, width = width);
    }
}

fn print_board(position: &Chess) {
    let board: &Board = position.board();
    // Square colours (light, dark), chosen so both white and black pieces
    // stay legible on either square.
    let light = (152u8, 174, 196);
    let dark = (95u8, 122, 150);
    for row in 0..8 {
        print!("  {}  ", 8 - row);
        for col in 0..8 {
            let idx = 64 - (row + 1) * 8 + col;
            let square = Square::new(idx);
            let piece = board.piece_at(square);
            let square_is_light = (row + col) % 2 == 0;
            let (br, bg, bb) = if square_is_light { light } else { dark };
            // A square is a uniform two-cell block: glyph (or blank) + trailing space.
            let glyph = piece.map(|p| piece_unicode(&p)).unwrap_or(" ");
            let mut cell = format!("{} ", glyph).on_truecolor(br, bg, bb);
            if let Some(p) = piece {
                cell = if p.color == Color::White {
                    cell.truecolor(255, 255, 255).bold()
                } else {
                    cell.truecolor(18, 18, 18).bold()
                };
            }
            print!("{}", cell);
        }
        println!();
    }

    println!(
        "     {}",
        (b'a'..=b'h')
            .map(char::from)
            .map(|c| c.to_string())
            .collect::<Vec<String>>()
            .join(" ")
    )
}

fn piece_unicode(piece: &Piece) -> &'static str {
    match (piece.role, piece.color) {
        (shakmaty::Role::Pawn, shakmaty::Color::Black) => "♟︎",
        (shakmaty::Role::Pawn, shakmaty::Color::White) => "♟︎",
        (shakmaty::Role::Knight, shakmaty::Color::Black) => "♞",
        (shakmaty::Role::Knight, shakmaty::Color::White) => "♞",
        (shakmaty::Role::Bishop, shakmaty::Color::Black) => "♝",
        (shakmaty::Role::Bishop, shakmaty::Color::White) => "♝",
        (shakmaty::Role::Rook, shakmaty::Color::Black) => "♜",
        (shakmaty::Role::Rook, shakmaty::Color::White) => "♜",
        (shakmaty::Role::Queen, shakmaty::Color::Black) => "♛",
        (shakmaty::Role::Queen, shakmaty::Color::White) => "♛",
        (shakmaty::Role::King, shakmaty::Color::Black) => "♚",
        (shakmaty::Role::King, shakmaty::Color::White) => "♚",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use shakmaty::Role;
    use std::env;
    use std::ffi::OsString;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvVarGuard {
        key: &'static str,
        original: Option<OsString>,
    }

    impl EnvVarGuard {
        fn remove(key: &'static str) -> Self {
            let original = env::var_os(key);
            env::remove_var(key);
            Self { key, original }
        }

        fn set(key: &'static str, value: &str) -> Self {
            let original = env::var_os(key);
            env::set_var(key, value);
            Self { key, original }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.original {
                Some(value) => env::set_var(self.key, value),
                None => env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn default_api_endpoint_uses_chessmadra() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _env = EnvVarGuard::remove("TACTICS_SERVER_URL");

        assert_eq!(get_api_endpoint(), "https://chessmadra.com/api/v1/tactic");
    }

    #[test]
    fn configured_api_endpoint_uses_tactics_server_url() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _env = EnvVarGuard::set("TACTICS_SERVER_URL", "http://localhost:3000");

        assert_eq!(get_api_endpoint(), "http://localhost:3000/api/v1/tactic");
    }

    #[test]
    fn print_side_labels_white() {
        assert_eq!(print_side(&Color::White), "White");
    }

    #[test]
    fn print_side_labels_black() {
        assert_eq!(print_side(&Color::Black), "Black");
    }

    #[test]
    fn prompt_names_white_to_move() {
        assert_eq!(
            get_prompt(&Chess::default()),
            "White to move, enter the best move, or '?' for help: "
        );
    }

    #[test]
    fn prompt_names_black_to_move() {
        let position = Chess::default();
        let white_move = "e2e4".parse::<Uci>().unwrap().to_move(&position).unwrap();
        let position = position.play(&white_move).unwrap();

        assert_eq!(
            get_prompt(&position),
            "Black to move, enter the best move, or '?' for help: "
        );
    }

    #[test]
    fn piece_unicode_returns_role_glyphs() {
        let cases = [
            (
                Piece {
                    role: Role::Pawn,
                    color: Color::White,
                },
                "♟︎",
            ),
            (
                Piece {
                    role: Role::Knight,
                    color: Color::White,
                },
                "♞",
            ),
            (
                Piece {
                    role: Role::Bishop,
                    color: Color::White,
                },
                "♝",
            ),
            (
                Piece {
                    role: Role::Rook,
                    color: Color::White,
                },
                "♜",
            ),
            (
                Piece {
                    role: Role::Queen,
                    color: Color::White,
                },
                "♛",
            ),
            (
                Piece {
                    role: Role::King,
                    color: Color::White,
                },
                "♚",
            ),
            (
                Piece {
                    role: Role::Pawn,
                    color: Color::Black,
                },
                "♟︎",
            ),
            (
                Piece {
                    role: Role::Knight,
                    color: Color::Black,
                },
                "♞",
            ),
            (
                Piece {
                    role: Role::Bishop,
                    color: Color::Black,
                },
                "♝",
            ),
            (
                Piece {
                    role: Role::Rook,
                    color: Color::Black,
                },
                "♜",
            ),
            (
                Piece {
                    role: Role::Queen,
                    color: Color::Black,
                },
                "♛",
            ),
            (
                Piece {
                    role: Role::King,
                    color: Color::Black,
                },
                "♚",
            ),
        ];

        for (piece, glyph) in cases {
            assert_eq!(piece_unicode(&piece), glyph);
        }
    }

    #[test]
    fn chess_tactic_deserializes_camel_case_fields() {
        let json = r#"{
            "id": "puzzle-1",
            "moves": ["e2e4", "e7e5"],
            "fen": "startpos",
            "popularity": 91,
            "tags": ["mateIn1"],
            "gameLink": "https://example.test/game",
            "rating": 1200,
            "ratingDeviation": 75,
            "numberPlays": 42
        }"#;

        let tactic: ChessTactic = serde_json::from_str(json).unwrap();

        assert_eq!(tactic.game_link, "https://example.test/game");
        assert_eq!(tactic.rating_deviation, 75);
        assert_eq!(tactic.number_plays, 42);
    }
}
