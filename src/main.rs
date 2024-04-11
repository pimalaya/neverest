use clap::Parser;
use color_eyre::eyre::Result;
use neverest::{cli::Cli, printer::StdoutPrinter};

#[tokio::main]
async fn main() -> Result<()> {
    neverest::tracing::install()?;

    let cli = Cli::parse();
    let mut printer = StdoutPrinter::new(cli.output, cli.color);

    cli.command
        .execute(&mut printer, cli.config_paths.as_ref())
        .await
}
