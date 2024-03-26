use anyhow::Result;
use clap::Parser;
use env_logger::{Builder as LoggerBuilder, Env, DEFAULT_FILTER_ENV};
use log::{debug, trace};
use neverest::{cli::Cli, printer::StdoutPrinter};

#[tokio::main]
async fn main() -> Result<()> {
    #[cfg(not(target_os = "windows"))]
    if let Err((_, err)) = coredump::register_panic_handler() {
        debug!("cannot register coredump panic handler: {err}");
        trace!("{err:?}");
    }

    LoggerBuilder::new()
        .parse_env(Env::new().filter_or(DEFAULT_FILTER_ENV, "warn"))
        .format_timestamp(None)
        .init();

    let cli = Cli::parse();
    let mut printer = StdoutPrinter::new(cli.output, cli.color);

    cli.command
        .execute(&mut printer, cli.config_paths.as_ref())
        .await
}
