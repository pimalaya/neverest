use anyhow::Result;
use clap::Parser;
#[cfg(feature = "imap")]
use email::imap::config::ImapAuthConfig;
#[cfg(feature = "smtp")]
use email::smtp::config::SmtpAuthConfig;
use log::{debug, info, warn};

use crate::{
    account::arg::name::OptionalAccountNameArg, backend::config::BackendConfig, config::Config,
    printer::Printer, ui::prompt,
};

/// Configure an account.
///
/// This command is mostly used to define or reset passwords managed
/// by your global keyring. If you do not use the keyring system, you
/// can skip this command.
#[derive(Debug, Parser)]
pub struct ConfigureAccountCommand {
    #[command(flatten)]
    pub account: OptionalAccountNameArg,

    /// Reset keyring passwords.
    ///
    /// This argument will force passwords to be prompted again, then
    /// saved to your global keyring.
    #[arg(long, short)]
    pub reset: bool,
}

impl ConfigureAccountCommand {
    pub async fn execute(self, printer: &mut impl Printer, config: &Config) -> Result<()> {
        info!("executing configure account command");

        let (name, config) = config.into_account_config(self.account.name.as_deref())?;

        if self.reset {
            let reset = match &config.left.backend {
                #[cfg(feature = "imap")]
                BackendConfig::Imap(config) => config.auth.reset().await,
                _ => Ok(()),
            };

            if let Err(err) = reset {
                warn!("cannot reset left imap secrets: {err}");
                debug!("{err:?}");
            }

            let reset = match &config.right.backend {
                #[cfg(feature = "imap")]
                BackendConfig::Imap(config) => config.auth.reset().await,
                _ => Ok(()),
            };

            if let Err(err) = reset {
                warn!("cannot reset right imap secrets: {err}");
                debug!("{err:?}");
            }
        }

        match &config.left.backend {
            #[cfg(feature = "imap")]
            BackendConfig::Imap(config) => match &config.auth {
                ImapAuthConfig::Passwd(config) => {
                    config
                        .configure(|| prompt::passwd("Left IMAP password"))
                        .await?;
                }
                ImapAuthConfig::OAuth2(config) => {
                    config
                        .configure(|| prompt::secret("Left IMAP OAuth 2.0 client secret"))
                        .await?;
                }
            },
            _ => (),
        };

        match &config.right.backend {
            #[cfg(feature = "imap")]
            BackendConfig::Imap(config) => match &config.auth {
                ImapAuthConfig::Passwd(config) => {
                    config
                        .configure(|| prompt::passwd("Right IMAP password"))
                        .await?;
                }
                ImapAuthConfig::OAuth2(config) => {
                    config
                        .configure(|| prompt::secret("Right IMAP OAuth 2.0 client secret"))
                        .await?;
                }
            },
            _ => (),
        };

        let re = if self.reset { "re" } else { "" };
        printer.print(format!("Account {name} successfully {re}configured!"))
    }
}
