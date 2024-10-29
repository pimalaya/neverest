use clap::Parser;
use color_eyre::eyre::Result;
use neverest::cli::Cli;
use pimalaya_tui::terminal::cli::{printer::StdoutPrinter, tracing};

#[tokio::main]
async fn main() -> Result<()> {
    let tracing = tracing::install()?;

    #[cfg(feature = "keyring")]
    secret::keyring::set_global_service_name("neverest-cli");

    let cli = Cli::parse();
    let mut printer = StdoutPrinter::new(cli.output);
    let res = cli
        .command
        .execute(&mut printer, cli.config_paths.as_ref())
        .await;

    tracing.with_debug_and_trace_notes(res)
}
