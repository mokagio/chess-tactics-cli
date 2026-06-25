use anyhow::Result;
use clap::Parser;
use tactics_trainer_cli::{run_trainer, TrainerArgs};

#[tokio::main]
async fn main() -> Result<()> {
    return run_trainer(TrainerArgs::parse()).await;
}
