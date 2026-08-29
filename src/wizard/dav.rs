//! # DAV wizard
//!
//! Contacts and calendar. A DAV server authenticates with HTTP Basic, or with
//! a bearer token where the provider fronts DAV with OAuth 2.0; the discovered
//! entry supplies the URL, so the wizard collects the login and its secret and
//! opens a session, which discovers the home set, as the connection test.
//!
//! One flow serves CardDAV and CalDAV: they ask the same questions and write
//! the same fields under a different table, so [`DavKind`] decides only what
//! the prompts are called and which config the answers land in.

use anyhow::Result;
use pimalaya_cli::{prompt, spinner::Spinner};

use crate::{
    account::SourceAccount,
    client,
    config::{CaldavConfig, CarddavConfig, DavAuthConfig, SourceBackendConfig, SourceConfig},
    dav::client::DavKind,
    wizard::{search::Discovered, secret},
};

/// Runs the DAV wizard over a discovered entry, returning a tested backend
/// config of the given kind.
pub fn configure(
    account_name: &str,
    kind: DavKind,
    url: &str,
    choice: &Discovered,
) -> Result<SourceBackendConfig> {
    let protocol = kind.protocol();
    let server = prompt::text(format!("{kind} server URL:").as_str(), Some(url))?;

    let auth = if choice.auth.token() && !choice.auth.basic {
        DavAuthConfig::Bearer {
            token: secret::configure_token(
                &format!("{kind} access token"),
                &format!("{account_name}-{protocol}"),
                choice.auth.oauth,
            )?,
        }
    } else {
        let username = prompt::text(
            format!("{kind} username:").as_str(),
            choice.login_default("").as_deref(),
        )?;
        let password = secret::configure_password(
            &format!("{kind} password"),
            &format!("{account_name}-{protocol}"),
        )?;
        DavAuthConfig::Basic { username, password }
    };

    let alpn = vec!["http/1.1".to_string()];
    let backend = match kind {
        DavKind::Card => SourceBackendConfig::Carddav(CarddavConfig {
            server,
            tls: Default::default(),
            alpn,
            auth,
            collection: Default::default(),
            flag: Default::default(),
            item: Default::default(),
            pool_size: None,
        }),
        DavKind::Cal => SourceBackendConfig::Caldav(CaldavConfig {
            server,
            tls: Default::default(),
            alpn,
            auth,
            collection: Default::default(),
            flag: Default::default(),
            item: Default::default(),
            pool_size: None,
        }),
    };

    let spinner = Spinner::start(format!("Testing {kind} connection"));
    if let Err(err) = SourceAccount::resolve(&kind.to_string(), &SourceConfig::new(backend.clone()))
        .and_then(|account| client::open(&account))
    {
        spinner.failure(format!("{kind} connection failed"));
        return Err(err);
    }
    spinner.success(format!("{kind} connection succeeded"));

    Ok(backend)
}
