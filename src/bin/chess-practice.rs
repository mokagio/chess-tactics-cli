use anyhow::Result;
use clap::Parser;
use tactics_trainer_cli::{run_practice_loop, PracticeArgs};

#[tokio::main]
async fn main() -> Result<()> {
    let exit_code = run_practice_loop(PracticeArgs::parse()).await?;
    if exit_code != 0 {
        std::process::exit(exit_code);
    }

    return Ok(());
}
