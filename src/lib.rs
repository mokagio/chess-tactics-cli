use std::{
    convert::{TryFrom, TryInto},
    env, fmt,
    fs::{self, OpenOptions},
    future::Future,
    io::{self, BufRead, Write},
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, Result};
use clap::Parser;
use colored::*;
use serde::{Deserialize, Serialize};
use shakmaty::{
    fen::{self, Fen},
    san::San,
    uci::Uci,
    Board, CastlingMode, Chess, Color, Piece, Position, Setup, Square,
};

#[derive(Parser, Clone, Debug)]
#[clap(version = "1.0", author = "Marcus B. <me@mbuffett.com>")]
pub struct TrainerArgs {
    #[clap(short, long)]
    /// The rating range of the tactics to fetch. Try 0-1200 for easy, 1200-1800 for
    /// intermediate, or 1800-3000 for difficult tactics.
    pub rating: Option<String>,
    #[clap(short, long)]
    /// Optionally specify a list of tags to get tactics for. Every tactic returned will have one
    /// of these tags
    pub tags: Vec<String>,
    #[clap(short, long, value_name = "PUZZLE_ID", conflicts_with_all = &["rating", "tags"])]
    /// Replay a specific Lichess puzzle by ID.
    pub id: Option<String>,
}

#[derive(Parser, Clone, Debug)]
#[clap(version = "1.0", author = "Marcus B. <me@mbuffett.com>")]
pub struct PracticeArgs {
    #[clap(short, long)]
    /// The rating range to start from. If omitted, chess-practice calibrates from the practice log.
    pub rating: Option<String>,
    #[clap(short, long)]
    /// Optionally specify a list of tags to get tactics for. Every tactic returned will have one
    /// of these tags
    pub tags: Vec<String>,
    #[clap(long = "tag", value_name = "TAG")]
    /// Alias for --tags.
    pub tag_aliases: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RatingRange {
    pub lower: i32,
    pub upper: i32,
}

impl RatingRange {
    const MIN: i32 = 0;
    const MAX: i32 = 3000;

    fn parse(input: &str) -> Result<Self> {
        let (lower, upper) = input.split_once('-').ok_or_else(|| {
            anyhow!("Could not parse rating, make sure it's in the form '500-1200'")
        })?;
        let lower = lower
            .parse::<i32>()
            .map_err(|_| anyhow!("Failed to parse {} as a rating", lower))?;
        let upper = upper
            .parse::<i32>()
            .map_err(|_| anyhow!("Failed to parse {} as a rating", upper))?;

        return Ok(Self { lower, upper });
    }

    fn width(&self) -> i32 {
        return self.upper - self.lower;
    }

    fn with_center(&self, center: i32) -> Self {
        let width = self.width();
        let mut lower = center - width / 2;
        let mut upper = lower + width;

        if lower < Self::MIN {
            lower = Self::MIN;
            upper = lower + width;
        }

        if upper > Self::MAX {
            upper = Self::MAX;
            lower = upper - width;
        }

        return Self { lower, upper };
    }
}

impl fmt::Display for RatingRange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(formatter, "{}-{}", self.lower, self.upper);
    }
}

#[derive(Clone, Debug)]
pub struct PracticeConfig {
    trainer_args: TrainerArgs,
    calibrate_rating: bool,
    default_rating_range: RatingRange,
    once: bool,
    max_runs: Option<usize>,
    retry_delay: Duration,
    clear_screen: bool,
}

impl PracticeConfig {
    pub fn from_args(args: PracticeArgs) -> Result<Self> {
        let mut tags = args.tags;
        tags.extend(args.tag_aliases);

        return Ok(Self {
            calibrate_rating: args.rating.is_none(),
            trainer_args: TrainerArgs {
                rating: args.rating,
                tags,
                id: None,
            },
            default_rating_range: RatingRange {
                lower: 600,
                upper: 1200,
            },
            once: env_flag("CHESS_PRACTICE_ONCE"),
            max_runs: env_usize("CHESS_PRACTICE_MAX_RUNS")?,
            retry_delay: Duration::from_secs(env_u64("CHESS_PRACTICE_RETRY_DELAY")?.unwrap_or(1)),
            clear_screen: !env_flag("CHESS_PRACTICE_NO_CLEAR"),
        });
    }

    fn trainer_args_for_attempts(&self, attempts: &[PuzzleAttempt]) -> TrainerArgs {
        let mut args = self.trainer_args.clone();
        if self.calibrate_rating {
            args.rating =
                Some(calibrate_rating_range(self.default_rating_range, attempts).to_string());
        }
        return args;
    }
}

pub async fn run_single_puzzle(opts: TrainerArgs) -> Result<()> {
    let puzzle = match opts.id.as_deref() {
        Some(id) => get_puzzle_by_id(id).await?,
        None => get_new_puzzle(tactic_request(&opts)?).await?.try_into()?,
    };
    println!("{}", puzzle_reference(&puzzle));
    let mut position = puzzle.position.clone();
    let their_side = opposite_color(position.turn());
    let mut continuation_moves = puzzle.moves.iter().map(|m| -> Uci { m.parse().unwrap() });
    println!();
    print_board(&position);
    let mut next_move = continuation_moves
        .next()
        .unwrap()
        .to_move(&position)
        .unwrap();
    let mut solved_correctly = true;
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
            PromptResponse::NoResponse => {
                solved_correctly = false;
            }
            PromptResponse::ShowRating => {
                println!("This tactic is rated {}.", puzzle.rating);
                continue;
            }
            PromptResponse::Move(move_input) => {
                if move_input == san_move.to_string() {
                    correct = true;
                } else {
                    solved_correctly = false;
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
                if let Err(error) = log_puzzle_attempt(&puzzle_attempt(&puzzle, solved_correctly)) {
                    eprintln!("Failed to log puzzle attempt: {}", error);
                }
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

#[derive(Clone, Debug)]
struct Puzzle {
    id: String,
    moves: Vec<String>,
    position: Chess,
    rating: i32,
    tags: Vec<String>,
}

impl TryFrom<ChessTactic> for Puzzle {
    type Error = anyhow::Error;

    fn try_from(tactic: ChessTactic) -> Result<Self> {
        let ChessTactic {
            id,
            moves,
            fen,
            rating,
            tags,
            ..
        } = tactic;
        let setup: Fen = fen.parse()?;
        let mut position: Chess = setup.position(CastlingMode::Standard)?;
        let mut moves = moves.into_iter();
        let first_move = moves
            .next()
            .ok_or_else(|| anyhow!("Puzzle {} has no moves", id))?;
        let first_move = first_move.parse::<Uci>()?.to_move(&position)?;
        position = position.play(&first_move)?;
        let moves = moves.collect::<Vec<_>>();
        if moves.is_empty() {
            return Err(anyhow!("Puzzle {} has no solution moves", id));
        }

        return Ok(Self {
            id,
            moves,
            position,
            rating,
            tags,
        });
    }
}

#[derive(Deserialize, Debug)]
struct LichessPuzzleResponse {
    puzzle: LichessPuzzle,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct LichessPuzzle {
    id: String,
    rating: i32,
    solution: Vec<String>,
    themes: Vec<String>,
    fen: String,
}

impl TryFrom<LichessPuzzleResponse> for Puzzle {
    type Error = anyhow::Error;

    fn try_from(response: LichessPuzzleResponse) -> Result<Self> {
        let puzzle = response.puzzle;
        let setup: Fen = puzzle.fen.parse()?;
        let position: Chess = setup.position(CastlingMode::Standard)?;
        if puzzle.solution.is_empty() {
            return Err(anyhow!("Puzzle {} has no solution moves", puzzle.id));
        }

        return Ok(Self {
            id: puzzle.id,
            moves: puzzle.solution,
            position,
            rating: puzzle.rating,
            tags: puzzle.themes,
        });
    }
}

#[derive(Serialize, Debug)]
struct ChessTacticRequest {
    rating_gte: Option<i32>,
    rating_lte: Option<i32>,
    tags: Vec<String>,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize, Debug)]
struct PuzzleAttempt {
    puzzle_id: String,
    rating: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    timestamp: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tags: Vec<String>,
    correct: bool,
}

fn tactic_request(args: &TrainerArgs) -> Result<ChessTacticRequest> {
    let rating_range = args.rating.as_deref().map(RatingRange::parse).transpose()?;

    return Ok(ChessTacticRequest {
        rating_gte: rating_range.map(|range| range.lower),
        rating_lte: rating_range.map(|range| range.upper),
        tags: args.tags.clone(),
    });
}

pub async fn run_practice_loop(args: PracticeArgs) -> Result<i32> {
    let config = PracticeConfig::from_args(args)?;
    let mut stderr = io::stderr();

    return run_practice_loop_with_hooks(
        config,
        |trainer_args| run_single_puzzle(trainer_args),
        clear_practice_screen,
        || read_puzzle_attempts(&puzzle_log_path()),
        std::thread::sleep,
        &mut stderr,
    )
    .await;
}

async fn run_practice_loop_with_hooks<Run, Fut, Clear, ReadAttempts, Sleep>(
    config: PracticeConfig,
    mut run_once: Run,
    mut clear_screen: Clear,
    mut read_attempts: ReadAttempts,
    mut sleep: Sleep,
    stderr: &mut dyn Write,
) -> Result<i32>
where
    Run: FnMut(TrainerArgs) -> Fut,
    Fut: Future<Output = Result<()>>,
    Clear: FnMut() -> Result<()>,
    ReadAttempts: FnMut() -> Result<Vec<PuzzleAttempt>>,
    Sleep: FnMut(Duration),
{
    let mut run_count = 0;

    loop {
        let attempts = read_attempts()?;
        let trainer_args = config.trainer_args_for_attempts(&attempts);

        if config.clear_screen {
            clear_screen()?;
        }

        let result = run_once(trainer_args).await;
        run_count += 1;

        let should_stop = config.once
            || config
                .max_runs
                .map(|max_runs| run_count >= max_runs)
                .unwrap_or(false);

        match result {
            Ok(()) => {
                if should_stop {
                    return Ok(0);
                }
            }
            Err(error) => {
                if should_stop {
                    writeln!(stderr, "chess-practice: puzzle run failed: {}", error)?;
                    return Ok(1);
                }

                writeln!(
                    stderr,
                    "chess-practice: puzzle run failed: {}; retrying...",
                    error
                )?;
                sleep(config.retry_delay);
            }
        }
    }
}

fn calibrate_rating_range(base: RatingRange, attempts: &[PuzzleAttempt]) -> RatingRange {
    const WINDOW_SIZE: usize = 10;
    const MIN_ATTEMPTS: usize = 3;
    const STEP: i32 = 100;

    let recent = attempts.iter().rev().take(WINDOW_SIZE).collect::<Vec<_>>();
    if recent.len() < MIN_ATTEMPTS {
        return base;
    }

    let correct_count = recent.iter().filter(|attempt| attempt.correct).count();
    let accuracy = correct_count as f32 / recent.len() as f32;
    let rating_sum = recent.iter().map(|attempt| attempt.rating).sum::<i32>();
    let average_rating = (rating_sum as f32 / recent.len() as f32).round() as i32;
    let center = if accuracy >= 0.70 {
        average_rating + STEP
    } else if accuracy <= 0.40 {
        average_rating - STEP
    } else {
        average_rating
    };

    return base.with_center(center);
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

async fn get_puzzle_by_id(id: &str) -> Result<Puzzle> {
    let client = reqwest::Client::new();
    let response: LichessPuzzleResponse = client
        .get(get_lichess_puzzle_endpoint(id))
        .header("User-Agent", "tactics-trainer-cli")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    return response.try_into();
}

fn get_api_endpoint() -> String {
    return format!(
        "{}/api/v1/tactic",
        env::var("TACTICS_SERVER_URL").unwrap_or("https://chessmadra.com".to_string())
    );
}

fn get_lichess_puzzle_endpoint(id: &str) -> String {
    return format!(
        "{}/api/puzzle/{}",
        env::var("LICHESS_SERVER_URL").unwrap_or("https://lichess.org".to_string()),
        id
    );
}

fn puzzle_reference(puzzle: &Puzzle) -> String {
    return format!("Puzzle URL: https://lichess.org/training/{}", puzzle.id);
}

fn puzzle_attempt(puzzle: &Puzzle, correct: bool) -> PuzzleAttempt {
    return puzzle_attempt_at(puzzle, correct, current_unix_timestamp());
}

fn puzzle_attempt_at(puzzle: &Puzzle, correct: bool, timestamp: u64) -> PuzzleAttempt {
    return PuzzleAttempt {
        puzzle_id: puzzle.id.clone(),
        rating: puzzle.rating,
        timestamp: Some(timestamp),
        tags: puzzle.tags.clone(),
        correct,
    };
}

fn current_unix_timestamp() -> u64 {
    return SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time is before Unix epoch")
        .as_secs();
}

fn puzzle_log_path() -> PathBuf {
    if let Some(path) = env::var_os("CHESS_PRACTICE_LOG") {
        return PathBuf::from(path);
    }

    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    return home.join(".chess-practice").join("puzzles.jsonl");
}

fn log_puzzle_attempt(attempt: &PuzzleAttempt) -> Result<()> {
    let path = puzzle_log_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    serde_json::to_writer(&mut file, attempt)?;
    writeln!(file)?;
    return Ok(());
}

fn read_puzzle_attempts(path: &PathBuf) -> Result<Vec<PuzzleAttempt>> {
    if !path.exists() {
        return Ok(vec![]);
    }

    let file = fs::File::open(path)?;
    let reader = io::BufReader::new(file);
    let mut attempts = vec![];
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        attempts.push(serde_json::from_str(&line)?);
    }

    return Ok(attempts);
}

fn clear_practice_screen() -> Result<()> {
    print!("\x1B[2J\x1B[H");
    io::stdout().flush()?;
    return Ok(());
}

fn env_flag(key: &str) -> bool {
    return env::var(key).map(|value| value == "1").unwrap_or(false);
}

fn env_usize(key: &str) -> Result<Option<usize>> {
    return env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            value
                .parse::<usize>()
                .map_err(|_| anyhow!("Failed to parse {} as a number", key))
        })
        .transpose();
}

fn env_u64(key: &str) -> Result<Option<u64>> {
    return env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| anyhow!("Failed to parse {} as a number", key))
        })
        .transpose();
}

fn print_side(side: &Color) -> String {
    if side == &Color::White {
        "White".to_string()
    } else {
        "Black".to_string()
    }
}

fn opposite_color(color: Color) -> Color {
    if color == Color::White {
        return Color::Black;
    }

    return Color::White;
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
    let light = (191u8, 167, 111);
    let dark = (132u8, 97, 48);
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

    use clap::Parser;
    use shakmaty::Role;
    use std::env;
    use std::ffi::OsString;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

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

    fn sample_puzzle() -> Puzzle {
        return Puzzle {
            id: "puzzle-1".to_string(),
            moves: vec!["e7e5".to_string()],
            position: Chess::default(),
            rating: 1200,
            tags: vec!["mateIn1".to_string()],
        };
    }

    fn temp_log_path(name: &str) -> PathBuf {
        return env::temp_dir().join(format!(
            "tactics-trainer-{}-{}.jsonl",
            std::process::id(),
            name
        ));
    }

    fn sample_attempt(rating: i32, correct: bool) -> PuzzleAttempt {
        return PuzzleAttempt {
            puzzle_id: format!("puzzle-{}", rating),
            rating,
            timestamp: Some(1_719_000_000),
            tags: vec![],
            correct,
        };
    }

    fn practice_args(
        rating: Option<&str>,
        tags: Vec<&str>,
        tag_aliases: Vec<&str>,
    ) -> PracticeArgs {
        return PracticeArgs {
            rating: rating.map(String::from),
            tags: tags.into_iter().map(String::from).collect(),
            tag_aliases: tag_aliases.into_iter().map(String::from).collect(),
        };
    }

    fn test_practice_config(max_runs: usize, clear_screen: bool) -> PracticeConfig {
        return PracticeConfig {
            trainer_args: TrainerArgs {
                rating: None,
                tags: vec![],
                id: None,
            },
            calibrate_rating: true,
            default_rating_range: RatingRange {
                lower: 600,
                upper: 1200,
            },
            once: false,
            max_runs: Some(max_runs),
            retry_delay: Duration::from_secs(0),
            clear_screen,
        };
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
    fn default_lichess_puzzle_endpoint_uses_lichess() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _env = EnvVarGuard::remove("LICHESS_SERVER_URL");

        assert_eq!(
            get_lichess_puzzle_endpoint("zZG03"),
            "https://lichess.org/api/puzzle/zZG03"
        );
    }

    #[test]
    fn configured_lichess_puzzle_endpoint_uses_lichess_server_url() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _env = EnvVarGuard::set("LICHESS_SERVER_URL", "http://localhost:4000");

        assert_eq!(
            get_lichess_puzzle_endpoint("zZG03"),
            "http://localhost:4000/api/puzzle/zZG03"
        );
    }

    #[test]
    fn trainer_args_parse_replay_id() {
        let args = TrainerArgs::try_parse_from(["tactics-trainer", "--id", "zZG03"]).unwrap();

        assert_eq!(args.id, Some("zZG03".to_string()));
        assert_eq!(args.rating, None);
        assert_eq!(args.tags, Vec::<String>::new());
    }

    #[test]
    fn trainer_args_replay_id_conflicts_with_filters() {
        let result =
            TrainerArgs::try_parse_from(["tactics-trainer", "--id", "zZG03", "--rating", "0-1200"]);

        assert!(result.is_err());
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

    #[test]
    fn chess_madra_tactic_converts_to_playable_puzzle() {
        let tactic = ChessTactic {
            id: "puzzle-1".to_string(),
            moves: vec!["e2e4".to_string(), "e7e5".to_string()],
            fen: "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1".to_string(),
            popularity: 91,
            tags: vec!["opening".to_string()],
            game_link: "https://example.test/game".to_string(),
            rating: 1200,
            rating_deviation: 75,
            number_plays: 42,
        };

        let puzzle = Puzzle::try_from(tactic).unwrap();

        assert_eq!(puzzle.id, "puzzle-1");
        assert_eq!(puzzle.moves, vec!["e7e5".to_string()]);
        assert_eq!(puzzle.position.turn(), Color::Black);
        assert_eq!(puzzle.tags, vec!["opening".to_string()]);
    }

    #[test]
    fn lichess_response_converts_to_playable_puzzle() {
        let json = r#"{
            "puzzle": {
                "id": "zZG03",
                "rating": 1203,
                "solution": ["c1h1", "h2g3"],
                "themes": ["endgame", "fork"],
                "fen": "7k/1p4p1/1p1R3p/4N3/1P6/P6P/5nPK/2r5 b - - 1 1"
            }
        }"#;
        let response: LichessPuzzleResponse = serde_json::from_str(json).unwrap();

        let puzzle = Puzzle::try_from(response).unwrap();

        assert_eq!(puzzle.id, "zZG03");
        assert_eq!(puzzle.moves, vec!["c1h1".to_string(), "h2g3".to_string()]);
        assert_eq!(puzzle.position.turn(), Color::Black);
        assert_eq!(puzzle.rating, 1203);
        assert_eq!(puzzle.tags, vec!["endgame".to_string(), "fork".to_string()]);
    }

    #[test]
    fn puzzle_reference_includes_puzzle_url() {
        let puzzle = sample_puzzle();

        assert_eq!(
            puzzle_reference(&puzzle),
            "Puzzle URL: https://lichess.org/training/puzzle-1"
        );
    }

    #[test]
    fn puzzle_attempt_captures_id_rating_and_result() {
        let attempt = puzzle_attempt(&sample_puzzle(), true);

        assert_eq!(attempt.puzzle_id, "puzzle-1");
        assert_eq!(attempt.rating, 1200);
        assert!(attempt.timestamp.is_some());
        assert_eq!(attempt.tags, vec!["mateIn1".to_string()]);
        assert!(attempt.correct);
    }

    #[test]
    fn log_puzzle_attempt_appends_json_line() {
        let _guard = ENV_LOCK.lock().unwrap();
        let path = temp_log_path("attempt");
        let _ = fs::remove_file(&path);
        let _env = EnvVarGuard::set("CHESS_PRACTICE_LOG", path.to_str().unwrap());

        log_puzzle_attempt(&puzzle_attempt_at(&sample_puzzle(), false, 1_719_000_000)).unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "{\"puzzle_id\":\"puzzle-1\",\"rating\":1200,\"timestamp\":1719000000,\"tags\":[\"mateIn1\"],\"correct\":false}\n"
        );

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn read_puzzle_attempts_accepts_old_log_lines() {
        let path = temp_log_path("old-attempt");
        let _ = fs::remove_file(&path);
        fs::write(
            &path,
            "{\"puzzle_id\":\"puzzle-1\",\"rating\":1200,\"correct\":true}\n",
        )
        .unwrap();

        let attempts = read_puzzle_attempts(&path).unwrap();

        assert_eq!(attempts[0].timestamp, None);
        assert_eq!(attempts[0].tags, Vec::<String>::new());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn practice_config_defaults_to_beginner_rating_range() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _once = EnvVarGuard::remove("CHESS_PRACTICE_ONCE");
        let _max_runs = EnvVarGuard::remove("CHESS_PRACTICE_MAX_RUNS");
        let _retry_delay = EnvVarGuard::remove("CHESS_PRACTICE_RETRY_DELAY");
        let _no_clear = EnvVarGuard::remove("CHESS_PRACTICE_NO_CLEAR");

        let config = PracticeConfig::from_args(practice_args(None, vec![], vec![])).unwrap();

        assert_eq!(
            config.trainer_args_for_attempts(&[]).rating.unwrap(),
            "600-1200"
        );
    }

    #[test]
    fn practice_config_combines_tags_and_tag_aliases() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _once = EnvVarGuard::remove("CHESS_PRACTICE_ONCE");
        let _max_runs = EnvVarGuard::remove("CHESS_PRACTICE_MAX_RUNS");
        let _retry_delay = EnvVarGuard::remove("CHESS_PRACTICE_RETRY_DELAY");
        let _no_clear = EnvVarGuard::remove("CHESS_PRACTICE_NO_CLEAR");

        let config =
            PracticeConfig::from_args(practice_args(None, vec!["fork"], vec!["mateIn1"])).unwrap();

        assert_eq!(
            config.trainer_args_for_attempts(&[]).tags,
            vec!["fork".to_string(), "mateIn1".to_string()]
        );
    }

    #[test]
    fn practice_config_preserves_explicit_rating() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _once = EnvVarGuard::remove("CHESS_PRACTICE_ONCE");
        let _max_runs = EnvVarGuard::remove("CHESS_PRACTICE_MAX_RUNS");
        let _retry_delay = EnvVarGuard::remove("CHESS_PRACTICE_RETRY_DELAY");
        let _no_clear = EnvVarGuard::remove("CHESS_PRACTICE_NO_CLEAR");
        let attempts = vec![
            sample_attempt(900, true),
            sample_attempt(900, true),
            sample_attempt(900, true),
        ];

        let config =
            PracticeConfig::from_args(practice_args(Some("1400-1800"), vec![], vec![])).unwrap();

        assert_eq!(
            config.trainer_args_for_attempts(&attempts).rating.unwrap(),
            "1400-1800"
        );
    }

    #[test]
    fn calibration_steps_up_after_recent_successes() {
        let base = RatingRange {
            lower: 600,
            upper: 1200,
        };
        let attempts = vec![
            sample_attempt(900, true),
            sample_attempt(900, true),
            sample_attempt(900, true),
        ];

        assert_eq!(
            calibrate_rating_range(base, &attempts),
            RatingRange {
                lower: 700,
                upper: 1300
            }
        );
    }

    #[test]
    fn calibration_steps_down_after_recent_misses() {
        let base = RatingRange {
            lower: 600,
            upper: 1200,
        };
        let attempts = vec![
            sample_attempt(900, false),
            sample_attempt(900, false),
            sample_attempt(900, false),
        ];

        assert_eq!(
            calibrate_rating_range(base, &attempts),
            RatingRange {
                lower: 500,
                upper: 1100
            }
        );
    }

    #[tokio::test]
    async fn practice_loop_reloads_attempts_between_runs() {
        let run_args = Arc::new(Mutex::new(Vec::<TrainerArgs>::new()));
        let read_count = Arc::new(Mutex::new(0usize));
        let mut stderr = Vec::new();
        let attempts = vec![
            sample_attempt(900, true),
            sample_attempt(900, true),
            sample_attempt(900, true),
        ];

        let exit_code = run_practice_loop_with_hooks(
            test_practice_config(2, false),
            {
                let run_args = Arc::clone(&run_args);
                move |args| {
                    run_args.lock().unwrap().push(args);
                    async { Ok(()) }
                }
            },
            || Ok(()),
            {
                let read_count = Arc::clone(&read_count);
                move || {
                    let mut read_count = read_count.lock().unwrap();
                    *read_count += 1;
                    if *read_count == 1 {
                        return Ok(vec![]);
                    }

                    return Ok(attempts.clone());
                }
            },
            |_| {},
            &mut stderr,
        )
        .await
        .unwrap();

        let ratings = run_args
            .lock()
            .unwrap()
            .iter()
            .map(|args| args.rating.clone().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(exit_code, 0);
        assert_eq!(
            ratings,
            vec!["600-1200".to_string(), "700-1300".to_string()]
        );
    }

    #[tokio::test]
    async fn practice_loop_clears_before_each_run() {
        let events = Arc::new(Mutex::new(Vec::<String>::new()));
        let mut stderr = Vec::new();

        run_practice_loop_with_hooks(
            test_practice_config(2, true),
            {
                let events = Arc::clone(&events);
                move |_| {
                    events.lock().unwrap().push("run".to_string());
                    async { Ok(()) }
                }
            },
            {
                let events = Arc::clone(&events);
                move || {
                    events.lock().unwrap().push("clear".to_string());
                    Ok(())
                }
            },
            || Ok(vec![]),
            |_| {},
            &mut stderr,
        )
        .await
        .unwrap();

        assert_eq!(
            *events.lock().unwrap(),
            vec![
                "clear".to_string(),
                "run".to_string(),
                "clear".to_string(),
                "run".to_string()
            ]
        );
    }

    #[tokio::test]
    async fn practice_loop_continues_after_failure() {
        let run_count = Arc::new(Mutex::new(0usize));
        let mut stderr = Vec::new();

        let exit_code = run_practice_loop_with_hooks(
            test_practice_config(2, false),
            {
                let run_count = Arc::clone(&run_count);
                move |_| {
                    let run_count = Arc::clone(&run_count);
                    async move {
                        let mut run_count = run_count.lock().unwrap();
                        *run_count += 1;
                        if *run_count == 1 {
                            return Err(anyhow::anyhow!("fetch failed"));
                        }

                        return Ok(());
                    }
                }
            },
            || Ok(()),
            || Ok(vec![]),
            |_| {},
            &mut stderr,
        )
        .await
        .unwrap();

        assert_eq!(exit_code, 0);
        assert_eq!(*run_count.lock().unwrap(), 2);
        assert!(String::from_utf8(stderr).unwrap().contains("retrying"));
    }
}
