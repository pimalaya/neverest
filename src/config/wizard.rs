use std::{fs, path::PathBuf};

use color_eyre::eyre::Result;
use pimalaya_tui::{config::TomlConfig, print, prompt};

use super::Config;
use crate::account;

pub async fn configure(path: &PathBuf) -> Result<Config> {
    print::section("Configuring your default account");

    let mut config = Config::default();

    let (account_name, account_config) = account::wizard::configure().await?;
    config.accounts.insert(account_name, account_config);

    let path = prompt::path("Where to save the configuration?", Some(path))?;
    println!("Writing the configuration to {}…", path.display());

    let toml = config.pretty_serialize()?;
    fs::create_dir_all(path.parent().unwrap_or(&path))?;
    fs::write(path, toml)?;

    println!("Done! Exiting the wizard…");
    Ok(config)
}
