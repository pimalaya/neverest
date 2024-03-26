//! # Preset configuration
//!
//! Module dedicated to the TOML representation of a preset.

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

/// The TOML configuration of a preset.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct TomlPresetConfig {
    /// The defaultness of the current preset.
    ///
    /// When synchronizing, if no preset is explicitly given, this one
    /// will be used by default.
    pub default: Option<bool>,

    /// The configuration of the left backend.
    ///
    /// The left backend can be seen as the source backend, except
    /// that there is not implicit difference between source and
    /// target. Hence left and right are used instead.
    pub left: TomlBackendConfig,

    /// The configuration of the right backend.
    ///
    /// The right backend can be seen as the target backend, except
    /// that there is not implicit difference between source and
    /// target. Hence left and right are used instead.
    pub right: TomlBackendConfig,
}

impl TomlPresetConfig {
    pub fn configure(&mut self, preset_name: &str) -> Result<()> {
        match &mut self.left.backend {
            #[cfg(feature = "imap")]
            TomlBackend::Imap(config) => {
                config
                    .auth
                    .replace_undefined_keyring_entries(&preset_name)?;
            }
            _ => (),
        }

        match &mut self.right.backend {
            #[cfg(feature = "imap")]
            TomlBackend::Imap(config) => {
                config
                    .auth
                    .replace_undefined_keyring_entries(&preset_name)?;
            }
            _ => (),
        }

        Ok(())
    }
}

/// The TOML configuration of a preset backend.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct TomlBackendConfig {
    /// The backend kind and configuration.
    pub backend: TomlBackend,

    /// The backend configuration dedicated to folders.
    pub folder: Option<FolderSyncConfig>,

    /// The backend configuration dedicated to envelopes.
    pub envelope: Option<EnvelopeSyncConfig>,

    /// The backend configuration dedicated to flags.
    pub flag: Option<FlagSyncConfig>,

    /// The backend configuration dedicated to messages.
    pub message: Option<MessageSyncConfig>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", tag = "type")]
pub enum TomlBackend {
    /// The IMAP backend TOML configuration.
    #[cfg(feature = "imap")]
    Imap(ImapConfig),

    /// The Maildir backend TOML configuration.
    #[cfg(feature = "maildir")]
    Maildir(MaildirConfig),

    /// The Notmuch backend TOML configuration.
    #[cfg(feature = "notmuch")]
    Notmuch(NotmuchConfig),
}
