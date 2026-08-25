//! Re-runs the wizard over an existing account.
//!
//! `neverest configure` runs the same discovery flow as the first-run
//! wizard (see [`super::discover`]), seeding the email prompt from the
//! account's current direct backend. It owns that backend and the send
//! channel only: the `default` flag, the store, the item settings, the
//! connection budget and a hand-written `sources` table are carried over
//! untouched, since a mirror or a second kind is configured by hand.

use std::path::Path;

use anyhow::Result;
use log::info;

use crate::{
    config::{
        AccountConfig, Config, JmapAuthConfig, SaslConfig, SourceBackendConfig, SourceConfig,
    },
    wizard::discover,
};

/// Edits (or creates) `account_name`, then writes `config` to `target`.
pub fn edit_account(target: &Path, mut config: Config, account_name: &str) -> Result<Config> {
    let existing = config.accounts.remove(account_name);

    let existing_sources = existing.as_ref().map(AccountConfig::direct_sources);

    let default_email = existing_sources
        .as_ref()
        .and_then(|sources| sources.iter().find_map(source_email));

    if existing.as_ref().is_some_and(|a| !a.sources.is_empty()) {
        eprintln!(
            "This account names sources by hand; the wizard configures the direct backend only, and keeps the `sources` table as configured."
        );
    }

    let email = discover::prompt_email_with(default_email.as_deref())?;
    let mut source = discover::configure(account_name, &email)?;

    // NOTE: the wizard never invents a submission server, so a run that
    // discovered none keeps the channel the account already carried.
    if source.smtp.is_none() {
        source.smtp = existing.as_ref().and_then(|account| account.smtp.clone());
    }

    let is_first_account = config.accounts.is_empty() && existing.is_none();

    let account = match existing {
        Some(mut existing) => {
            existing.set_direct_source(source);
            existing
        }
        None => AccountConfig::with_source(is_first_account, source),
    };

    config.accounts.insert(account_name.to_owned(), account);
    config.write(target)?;
    info!("configuration written to {}", target.display());

    Ok(config)
}

/// User-facing email for a source, seeding the email prompt when
/// extractable.
fn source_email(source: &SourceConfig) -> Option<String> {
    match &source.backend {
        SourceBackendConfig::Imap(c) => sasl_login(c.sasl.as_ref()),
        SourceBackendConfig::Jmap(c) => match &c.auth {
            JmapAuthConfig::Basic { username, .. } if !username.is_empty() => {
                Some(username.clone())
            }
            _ => None,
        },
        SourceBackendConfig::Gmail(_) | SourceBackendConfig::Msgraph(_) => None,
        // NOTE: a DAV account authenticates with a username rather than an
        // email address, and the two differ often enough not to seed a prompt.
        SourceBackendConfig::Carddav(_) => None,
    }
}

/// The user-facing login of a SASL block, when it carries one.
fn sasl_login(sasl: Option<&SaslConfig>) -> Option<String> {
    let login = match sasl? {
        SaslConfig::Plain(c) => c.authcid.clone(),
        SaslConfig::Login(c) => c.username.clone(),
        SaslConfig::Oauthbearer(c) => c.username.clone(),
        SaslConfig::Xoauth2(c) => c.username.clone(),
        SaslConfig::ScramSha256(c) => c.username.clone(),
        SaslConfig::Anonymous(_) => return None,
    };

    Some(login).filter(|login| !login.is_empty())
}

#[cfg(test)]
mod tests {
    use pimalaya_config::secret::Secret;

    use super::*;
    use crate::config::{ImapConfig, SaslAnonymousConfig, SaslPlainConfig};

    fn imap_source(sasl: Option<SaslConfig>) -> SourceConfig {
        SourceConfig::new(SourceBackendConfig::Imap(ImapConfig {
            server: "imaps://imap.example.org:993".into(),
            tls: Default::default(),
            starttls: false,
            alpn: None,
            sasl,
            collection: Default::default(),
            flag: Default::default(),
            item: Default::default(),
            pool_size: None,
        }))
    }

    #[test]
    fn the_email_prompt_is_seeded_from_the_source_login() {
        let source = imap_source(Some(SaslConfig::Plain(SaslPlainConfig {
            authzid: None,
            authcid: "user@example.org".into(),
            passwd: Secret::Raw(String::from("pw").into()),
        })));
        assert_eq!(source_email(&source), Some("user@example.org".into()));

        let anonymous = imap_source(Some(SaslConfig::Anonymous(SaslAnonymousConfig::default())));
        assert_eq!(source_email(&anonymous), None);
        assert_eq!(source_email(&imap_source(None)), None);
    }
}
