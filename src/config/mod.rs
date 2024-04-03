//! # Configuration
//!
//! Module dedicated to the main configuration of Neverest CLI.

#[cfg(feature = "wizard")]
pub mod wizard;

use anyhow::{anyhow, bail, Context, Result};
use dirs::{config_dir, home_dir};
use log::debug;
use serde::{Deserialize, Serialize};
use serde_toml_merge::merge;
use shellexpand_utils::{canonicalize, expand};
use std::{collections::HashMap, fs, path::PathBuf};
use toml::Value;

use crate::account::config::AccountConfig;

/// The main configuration.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Config {
    /// The configuration of all the accounts.
    pub accounts: HashMap<String, AccountConfig>,
}

impl Config {
    /// Read and parse the TOML configuration at the given paths.
    ///
    /// Returns an error if a configuration file cannot be read or if
    /// a content cannot be parsed.
    fn from_paths(paths: &[PathBuf]) -> Result<Self> {
        match paths.len() {
            0 => {
                // should never happen
                bail!("cannot read config file from empty paths");
            }
            1 => {
                let path = &paths[0];

                let ref content = fs::read_to_string(path)
                    .context(format!("cannot read config file at {path:?}"))?;

                toml::from_str(content).context(format!("cannot parse config file at {path:?}"))
            }
            _ => {
                let path = &paths[0];

                let mut merged_content = fs::read_to_string(path)
                    .context(format!("cannot read config file at {path:?}"))?
                    .parse::<Value>()?;

                for path in &paths[1..] {
                    match fs::read_to_string(path) {
                        Ok(content) => {
                            merged_content = merge(merged_content, content.parse()?).unwrap();
                        }
                        Err(err) => {
                            debug!("skipping subconfig file at {path:?}: {err}");
                            continue;
                        }
                    }
                }

                merged_content
                    .try_into()
                    .context(format!("cannot parse merged config file at {path:?}"))
            }
        }
    }

    /// Create and save a TOML configuration using the wizard.
    ///
    /// If the user accepts the confirmation, the wizard starts and
    /// help him to create his configuration file. Otherwise the
    /// program stops.
    ///
    /// NOTE: the wizard can only be used with interactive shells.
    #[cfg(feature = "wizard")]
    async fn from_wizard(path: &PathBuf) -> Result<Self> {
        use dialoguer::Confirm;
        use std::process;

        use crate::{wizard_prompt, wizard_warn};

        wizard_warn!("Cannot find existing configuration at {path:?}.");

        let confirm = Confirm::new()
            .with_prompt(wizard_prompt!(
                "Would you like to create one with the wizard?"
            ))
            .default(true)
            .interact_opt()?
            .unwrap_or_default();

        if !confirm {
            process::exit(0);
        }

        wizard::configure(path).await
    }

    /// Read and parse the TOML configuration from default paths.
    pub async fn from_default_paths() -> Result<Self> {
        match Self::first_valid_default_path() {
            Some(path) => Self::from_paths(&[path]),
            #[cfg(feature = "wizard")]
            None => Self::from_wizard(&Self::default_path()?).await,
            #[cfg(not(feature = "wizard"))]
            None => anyhow::bail!("cannot find config file from default paths"),
        }
    }

    /// Read and parse the TOML configuration at the optional given
    /// path.
    ///
    /// If the given path exists, then read and parse the TOML
    /// configuration from it.
    ///
    /// If the given path does not exist, then create it using the
    /// wizard.
    ///
    /// If no path is given, then either read and parse the TOML
    /// configuration at the first valid default path, otherwise
    /// create it using the wizard.  wizard.
    pub async fn from_paths_or_default(paths: &[PathBuf]) -> Result<Self> {
        match paths.len() {
            0 => Self::from_default_paths().await,
            _ if paths[0].exists() => Self::from_paths(paths),
            #[cfg(feature = "wizard")]
            _ => Self::from_wizard(&paths[0]).await,
            #[cfg(not(feature = "wizard"))]
            _ => anyhow::bail!("cannot find config file from default paths"),
        }
    }

    /// Get the default configuration path.
    ///
    /// Returns an error if the XDG configuration directory cannot be
    /// found.
    pub fn default_path() -> Result<PathBuf> {
        Ok(config_dir()
            .ok_or(anyhow!("cannot get XDG config directory"))?
            .join("neverest")
            .join("config.toml"))
    }

    /// Get the first default configuration path that points to a
    /// valid file.
    ///
    /// Tries paths in this order:
    ///
    /// - `$XDG_CONFIG_DIR/neverest/config.toml` (or equivalent to
    ///   `$XDG_CONFIG_DIR` in other OSes.)
    /// - `$HOME/.config/neverest/config.toml`
    /// - `$HOME/.neverestrc`
    pub fn first_valid_default_path() -> Option<PathBuf> {
        Self::default_path()
            .ok()
            .filter(|p| p.exists())
            .or_else(|| home_dir().map(|p| p.join(".config").join("neverest").join("config.toml")))
            .filter(|p| p.exists())
            .or_else(|| home_dir().map(|p| p.join(".neverestrc")))
            .filter(|p| p.exists())
    }

    pub fn into_account_config(
        &self,
        account_name: Option<&str>,
    ) -> Result<(String, AccountConfig)> {
        #[allow(unused_mut)]
        let (account_name, mut account_config) = match account_name {
            Some("default") | Some("") | None => self
                .accounts
                .iter()
                .find_map(|(name, config)| {
                    config
                        .default
                        .filter(|default| *default)
                        .map(|_| (name.to_owned(), config.clone()))
                })
                .ok_or_else(|| anyhow!("cannot find default account")),
            Some(name) => self
                .accounts
                .get(name)
                .map(|config| (name.to_owned(), config.clone()))
                .ok_or_else(|| anyhow!("cannot find account {name}")),
        }?;

        account_config.configure(account_name.as_str())?;

        Ok((account_name, account_config))
    }
}

/// Parse a configuration file path as [`PathBuf`].
///
/// The path is shell-expanded then canonicalized (if applicable).
pub fn path_parser(path: &str) -> Result<PathBuf, String> {
    expand::try_path(path)
        .map(canonicalize::path)
        .map_err(|err| err.to_string())
}
