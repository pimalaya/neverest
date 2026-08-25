//! CardDAV wizard (contacts).
//!
//! A DAV server is addressed by URL and authenticates with HTTP Basic in the
//! common case, or with a bearer token where the provider fronts DAV with
//! OAuth 2.0. The discovered entry supplies the URL, so the wizard only
//! collects the login and its secret, then opens the session (which
//! discovers the address book home set) as the connection test.

use anyhow::Result;
use pimalaya_cli::{prompt, spinner::Spinner};

use crate::{
    client,
    config::{CarddavConfig, DavAuthConfig, SideBackendConfig, SideConfig},
    wizard::{search::Discovered, secret},
};

/// Runs the CardDAV wizard over a discovered entry, returning a tested
/// [`CarddavConfig`].
pub fn configure(account_name: &str, url: &str, choice: &Discovered) -> Result<CarddavConfig> {
    let server = prompt::text("CardDAV server URL:", Some(url))?;

    let auth = if choice.auth.token() && !choice.auth.basic {
        DavAuthConfig::Bearer {
            token: secret::configure_token(
                "CardDAV access token",
                &format!("{account_name}-carddav"),
                choice.auth.oauth,
            )?,
        }
    } else {
        let username = prompt::text("CardDAV username:", choice.login_default("").as_deref())?;
        let password =
            secret::configure_password("CardDAV password", &format!("{account_name}-carddav"))?;
        DavAuthConfig::Basic { username, password }
    };

    let config = CarddavConfig {
        server,
        tls: Default::default(),
        alpn: vec!["http/1.1".to_string()],
        auth,
        collection: Default::default(),
        flag: Default::default(),
        item: Default::default(),
        pool_size: None,
    };

    let spinner = Spinner::start("Testing CardDAV connection");
    if let Err(err) = client::open(SideConfig::new(SideBackendConfig::Carddav(config.clone()))) {
        spinner.failure("CardDAV connection failed");
        return Err(err);
    }
    spinner.success("CardDAV connection succeeded");

    Ok(config)
}
