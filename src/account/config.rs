//! # Account configuration
//!
//! Module dedicated to account configuration.

use anyhow::Result;
use email::{
    envelope::sync::config::EnvelopeSyncFilters, folder::sync::config::FolderSyncStrategy,
};
use serde::{Deserialize, Serialize};

use crate::backend::config::{BackendConfig, BackendGlobalConfig};

/// The account configuration.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct AccountConfig {
    /// The defaultness of the current account.
    ///
    /// When synchronizing, if no account name is explicitly given,
    /// this one will be used by default.
    pub default: Option<bool>,

    /// The account configuration dedicated to folders.
    pub folder: Option<FolderConfig>,

    /// The account configuration dedicated to envelopes.
    pub envelope: Option<EnvelopeConfig>,

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

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct FolderConfig {
    #[serde(default)]
    pub filter: FolderSyncStrategy,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct EnvelopeConfig {
    #[serde(default)]
    pub filter: EnvelopeSyncFilters,
}
