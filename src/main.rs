use clap::{CommandFactory, Parser};
use color_eyre::eyre::Result;
use neverest::{cli::Cli, config::TomlConfig};
use pimalaya_tui::terminal::{
    cli::{printer::StdoutPrinter, tracing},
    config::TomlConfig as _,
};

#[tokio::main]
async fn main() -> Result<()> {
    let tracing = tracing::install()?;

    #[cfg(feature = "keyring")]
    secret::keyring::set_global_service_name("neverest-cli");

    let cli = Cli::parse();
    let mut printer = StdoutPrinter::new(cli.output);
    let res = match cli.command {
        Some(cmd) => cmd.execute(&mut printer, cli.config_paths.as_ref()).await,
        None => {
            TomlConfig::from_paths_or_default(cli.config_paths.as_ref()).await?;
            println!("{}", Cli::command().render_help());
            Ok(())
        }
    };

    tracing.with_debug_and_trace_notes(res)
}
