use clap::Parser;
use color_eyre::eyre::Result;
use neverest::cli::Cli;

#[tokio::main]
async fn main() -> Result<()> {
    Cli::parse().execute().await
}
