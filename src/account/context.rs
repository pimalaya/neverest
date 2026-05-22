//! Merged runtime account: per-account view consumed by sync,
//! doctor, and configure. Built by `cli::take_account_for_side`:
//!
//! 1. start with [`Account::default`];
//! 2. fold the global [`Config`] via `Account::from(config)`;
//! 3. fold the selected `[accounts.<name>]` via [`Account::merge`].

use std::collections::HashMap;

use crate::config::{AccountConfig, Config};

#[derive(Clone, Debug, Default)]
pub struct Account {
    /// Mailbox aliases, keys lowercased. Currently sourced only from
    /// the per-account `[accounts.<name>.mailbox.alias]` table since
    /// neverest has no global mailbox section. The map shape mirrors
    /// himalaya's so future global aliases can be folded in without
    /// reshuffling callers.
    pub mailbox_alias: HashMap<String, String>,
}

impl Account {
    /// Folds `other`'s set fields on top of `self`.
    pub fn merge(self, other: Self) -> Self {
        let mut mailbox_alias = self.mailbox_alias;
        mailbox_alias.extend(other.mailbox_alias);
        Self { mailbox_alias }
    }

    /// Resolves `name` through the alias map. Lookup is
    /// case-insensitive; falls back to `name` itself when no alias
    /// matches so callers can pass either form transparently.
    #[allow(dead_code)]
    pub fn resolve_mailbox<'a>(&'a self, name: &'a str) -> &'a str {
        let key = name.to_lowercase();
        self.mailbox_alias
            .get(&key)
            .map(String::as_str)
            .unwrap_or(name)
    }
}

impl From<Config> for Account {
    fn from(_config: Config) -> Self {
        Self::default()
    }
}

impl From<AccountConfig> for Account {
    fn from(config: AccountConfig) -> Self {
        Self {
            mailbox_alias: lowercase_alias_keys(config.mailbox.alias),
        }
    }
}

fn lowercase_alias_keys(aliases: HashMap<String, String>) -> HashMap<String, String> {
    aliases
        .into_iter()
        .map(|(k, v)| (k.to_lowercase(), v))
        .collect()
}
