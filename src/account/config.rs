//! # Account configuration
//!
//! Module dedicated to account configuration.

use anyhow::Result;
#[cfg(feature = "imap")]
use email::imap::config::ImapConfig;
#[cfg(feature = "maildir")]
use email::maildir::config::MaildirConfig;
#[cfg(feature = "notmuch")]
use email::notmuch::config::NotmuchConfig;
use email::{
    envelope::sync::config::EnvelopeSyncConfig, flag::sync::config::FlagSyncConfig,
    folder::sync::config::FolderSyncConfig, message::sync::config::MessageSyncConfig,
};
use serde::{Deserialize, Serialize};

/// The account configuration.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct AccountConfig {
    /// The defaultness of the current account.
    ///
    /// When synchronizing, if no account name is explicitly given,
    /// this one will be used by default.
    pub default: Option<bool>,

    /// The configuration of the left backend.
    ///
    /// The left backend can be seen as the source backend, except
    /// that there is not implicit difference between source and
    /// target. Hence left and right are used instead.
    pub left: BackendGlobalConfig,

    /// The configuration of the right backend.
    ///
    /// The right backend can be seen as the target backend, except
    /// that there is not implicit difference between source and
    /// target. Hence left and right are used instead.
    pub right: BackendGlobalConfig,
}

impl AccountConfig {
    /// Configure the current account configuration.
    ///
    /// This function is mostly used to replace undefined keyring
    /// entries by default ones, based on the given account name.
    pub fn configure(&mut self, account_name: &str) -> Result<()> {
        match &mut self.left.backend {
            #[cfg(feature = "imap")]
            BackendConfig::Imap(config) => {
                config
                    .auth
                    .replace_undefined_keyring_entries(&account_name)?;
            }
            _ => (),
        }

        match &mut self.right.backend {
            #[cfg(feature = "imap")]
            BackendConfig::Imap(config) => {
                config
                    .auth
                    .replace_undefined_keyring_entries(&account_name)?;
            }
            _ => (),
        }

        Ok(())
    }
}

/// The global backend configuration (left or right).
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct BackendGlobalConfig {
    /// The backend-specific configuration.
    pub backend: BackendConfig,

    /// The backend configuration dedicated to folders.
    pub folder: Option<FolderSyncConfig>,

    /// The backend configuration dedicated to envelopes.
    pub envelope: Option<EnvelopeSyncConfig>,

    /// The backend configuration dedicated to flags.
    pub flag: Option<FlagSyncConfig>,

    /// The backend configuration dedicated to messages.
    pub message: Option<MessageSyncConfig>,
}

/// The backend-specific configuration.
///
/// Represents all valid backends managed by Neverest with their
/// specific configuration.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", tag = "type")]
pub enum BackendConfig {
    /// The IMAP backend configuration.
    #[cfg(feature = "imap")]
    Imap(ImapConfig),

    /// The Maildir backend configuration.
    #[cfg(feature = "maildir")]
    Maildir(MaildirConfig),

    /// The Notmuch backend configuration.
    #[cfg(feature = "notmuch")]
    Notmuch(NotmuchConfig),
}
