//! Re-runs the wizard over an existing account.
//!
//! `neverest configure` runs the same discovery flow as the first-run
//! wizard (see [`super::discover`]), seeding the email prompt from the
//! account's current side. It owns the `left` side and the send channel
//! only: the `default` flag, the store, the collection and item settings,
//! the connection budget and a hand-written `right` side are carried
//! over untouched, since remote-to-remote sides are configured by hand.

use std::path::Path;

use anyhow::Result;
use log::info;

use crate::{
    config::{AccountConfig, Config, JmapAuthConfig, SaslConfig, SideBackendConfig, SideConfig},
    wizard::discover,
};

/// Edits (or creates) `account_name`, then writes `config` to `target`.
pub fn edit_account(target: &Path, mut config: Config, account_name: &str) -> Result<Config> {
    let existing = config.accounts.remove(account_name);

    let default_email = existing
        .as_ref()
        .and_then(|account| account.left.as_ref().or(account.right.as_ref()))
        .and_then(side_email);

    if existing.as_ref().is_some_and(|a| a.right.is_some()) {
        eprintln!(
            "This account syncs two remotes; the wizard configures the `left` side, and keeps `right` as configured."
        );
    }

    let email = discover::prompt_email_with(default_email.as_deref())?;
    let mut side = discover::configure(account_name, &email)?;

    // The wizard never invents a submission server, so a run that
    // discovered none keeps the channel the side already carried.
    if side.smtp.is_none() {
        side.smtp = existing
            .as_ref()
            .and_then(|account| account.left.as_ref())
            .and_then(|left| left.smtp.clone());
    }

    // A fresh account is the default one when it is the only one; an
    // existing account keeps whatever it was.
    let is_first_account = config.accounts.is_empty() && existing.is_none();

    let account = match existing {
        Some(existing) => AccountConfig {
            left: Some(side),
            ..existing
        },
        None => AccountConfig {
            default: is_first_account,
            left: Some(side),
            right: None,
            store: Default::default(),
            collection: Default::default(),
            item: Default::default(),
            connections: None,
        },
    };

    config.accounts.insert(account_name.to_owned(), account);
    config.write(target)?;
    info!("configuration written to {}", target.display());

    Ok(config)
}

/// User-facing email for a side, seeding the email prompt when
/// extractable.
fn side_email(side: &SideConfig) -> Option<String> {
    match &side.backend {
        SideBackendConfig::Imap(c) => sasl_login(c.sasl.as_ref()),
        SideBackendConfig::Jmap(c) => match &c.auth {
            JmapAuthConfig::Basic { username, .. } if !username.is_empty() => {
                Some(username.clone())
            }
            _ => None,
        },
        SideBackendConfig::Gmail(_) | SideBackendConfig::Msgraph(_) => None,
        // A DAV account authenticates with a username, not an email address,
        // and the two differ often enough not to seed a prompt with one.
        SideBackendConfig::Carddav(_) => None,
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

    fn imap_side(sasl: Option<SaslConfig>) -> SideConfig {
        SideConfig::new(SideBackendConfig::Imap(ImapConfig {
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
    fn the_email_prompt_is_seeded_from_the_side_login() {
        let side = imap_side(Some(SaslConfig::Plain(SaslPlainConfig {
            authzid: None,
            authcid: "user@example.org".into(),
            passwd: Secret::Raw(String::from("pw").into()),
        })));
        assert_eq!(side_email(&side), Some("user@example.org".into()));

        // A credential-less side carries no login to seed with.
        let anonymous = imap_side(Some(SaslConfig::Anonymous(SaslAnonymousConfig::default())));
        assert_eq!(side_email(&anonymous), None);
        assert_eq!(side_email(&imap_side(None)), None);
    }
}
