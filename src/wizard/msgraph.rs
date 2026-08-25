//! Microsoft Graph API wizard (Microsoft accounts).
//!
//! The Graph API is bearer-token-only, and neverest never runs an OAuth
//! grant itself: the wizard collects the user id and a token secret,
//! typically an Ortie command since tokens expire and need refreshing.
//! The connection is tested before the configuration is written.

use anyhow::Result;
use pimalaya_cli::{prompt, spinner::Spinner};

use crate::{
    client,
    config::{MsgraphAuthConfig, MsgraphConfig, SourceBackendConfig, SourceConfig},
    wizard::secret,
};

/// Runs the Microsoft Graph wizard, returning a tested [`MsgraphConfig`].
pub fn configure(account_name: &str) -> Result<MsgraphConfig> {
    eprintln!(
        "Microsoft Graph uses OAuth 2.0 tokens; issue and refresh them with an external broker such as Ortie."
    );

    let user_id = prompt::text("Microsoft Graph user id:", Some("me"))?;
    let token = secret::configure_token(
        "Microsoft Graph access token",
        &format!("{account_name}-msgraph"),
        true,
    )?;

    let config = MsgraphConfig {
        user_id,
        tls: Default::default(),
        alpn: vec!["http/1.1".to_string()],
        auth: MsgraphAuthConfig { token },
        collection: Default::default(),
        flag: Default::default(),
        item: Default::default(),
        pool_size: None,
    };

    let spinner = Spinner::start("Testing Microsoft Graph connection");
    if let Err(err) = client::open(SourceConfig::new(SourceBackendConfig::Msgraph(
        config.clone(),
    ))) {
        spinner.failure("Microsoft Graph connection failed");
        return Err(err);
    }
    spinner.success("Microsoft Graph connection succeeded");

    Ok(config)
}
