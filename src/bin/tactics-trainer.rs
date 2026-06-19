use anyhow::Result;
use clap::Parser;
use tactics_trainer_cli::{run_single_puzzle, TrainerArgs};

#[tokio::main]
async fn main() -> Result<()> {
    return run_single_puzzle(TrainerArgs::parse()).await;
}
