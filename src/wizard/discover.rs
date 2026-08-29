//! # Discovery
//!
//! The half of the wizard deciding what the account is, what becomes of it
//! belonging to [`crate::cli::configure`].
//!
//! It asks for one input, an email address (a bare domain is accepted and
//! synthesized as `@domain`). That feeds io-pim-discovery's parallel search
//! (see [`super::search`]) and every reachable service becomes one selectable
//! configuration; only backends compiled into this build are proposed.
//!
//! It discovers one account with one source, in the direct-backend sugar
//! (`imap.server = …`): the offline replica, which is the common case and
//! reads offline with no further setting. A second kind, a mirror and a
//! fan-in are all written by hand against config.sample.toml, and so is a
//! change to an account already configured.

use anyhow::{Result, bail};
use pimalaya_cli::{prompt, spinner::Spinner};

#[cfg(any(feature = "imap", feature = "msgraph"))]
use crate::config::SourceBackendConfig;
#[cfg(feature = "dav")]
use crate::dav::client::DavKind;
#[cfg(feature = "dav")]
use crate::wizard::dav;
#[cfg(feature = "imap")]
use crate::wizard::imap_smtp;
#[cfg(feature = "msgraph")]
use crate::wizard::msgraph;
#[cfg(any(feature = "imap", feature = "msgraph", feature = "dav"))]
use crate::wizard::search::DiscoveredKind;
use crate::{
    config::{AccountConfig, SourceConfig},
    wizard::search::{self, Discovered},
};

/// The one prompt of the wizard. Discovery also accepts a bare domain, but
/// the label names what users actually have.
const EMAIL_PROMPT: &str = "Email address:";

/// The documented sample configuration, shown in the welcome banner and
/// pointed at when discovery finds nothing.
pub const CONFIG_SAMPLE_URL: &str =
    "https://github.com/pimalaya/neverest/blob/master/config.sample.toml";

/// Discovers one account from a single prompt, tests it, and hands back the
/// name it proposes beside the account itself.
///
/// What becomes of that account belongs to [`crate::cli::configure`]. This is
/// the discovery half alone.
pub fn run() -> Result<(String, AccountConfig)> {
    let email = prompt_email()?;

    // NOTE: the account name is just the TOML table key, so it is derived
    // from the input rather than prompted; the user renames it by hand.
    let account_name = default_account_name(&email);
    let source = configure(&account_name, &email)?;

    Ok((account_name, AccountConfig::with_source(source)))
}

/// Prompts the email address and normalizes it for discovery: a bare domain
/// becomes the `@domain` form the search understands.
fn prompt_email() -> Result<String> {
    let input = prompt::text(EMAIL_PROMPT, None)?;
    let input = input.trim();

    if input.is_empty() {
        bail!("Empty input: enter an email address");
    }

    Ok(match input.contains('@') {
        true => input.to_string(),
        false => format!("@{input}"),
    })
}

/// Searches the services reachable from `email`, keeps only those this build
/// supports, lets the user pick one, then configures its backend (its
/// authentication method being a second prompt) and tests the connection.
pub fn configure(account_name: &str, email: &str) -> Result<SourceConfig> {
    let spinner = Spinner::start("Searching for server settings");
    let mut found = search::search(email)?;
    search::retain_supported(&mut found);

    if found.is_empty() {
        spinner.failure("No configuration found");
        return stop_undiscovered(email);
    }
    spinner.success(format!("Found {} configuration(s)", found.len()));

    let default = found.first().cloned();
    let choice = prompt::item("Choose a configuration:", found, default)?;

    dispatch(account_name, email, choice)
}

/// Configures the backend behind a discovered entry.
#[cfg_attr(
    all(feature = "imap", feature = "msgraph"),
    allow(unreachable_patterns)
)]
#[cfg_attr(not(feature = "imap"), allow(unused_variables))]
fn dispatch(account_name: &str, email: &str, choice: Discovered) -> Result<SourceConfig> {
    match &choice.kind {
        #[cfg(feature = "imap")]
        DiscoveredKind::ImapSmtp { .. } => {
            let (imap, smtp) = imap_smtp::configure_discovered(account_name, email, &choice)?;
            Ok(SourceConfig {
                backend: SourceBackendConfig::Imap(imap),
                smtp,
            })
        }
        #[cfg(feature = "msgraph")]
        DiscoveredKind::Msgraph => Ok(SourceConfig::new(SourceBackendConfig::Msgraph(
            msgraph::configure(account_name)?,
        ))),
        #[cfg(feature = "dav")]
        DiscoveredKind::Carddav { url } => Ok(SourceConfig::new(dav::configure(
            account_name,
            DavKind::Card,
            url,
            &choice,
        )?)),
        #[cfg(feature = "dav")]
        DiscoveredKind::Caldav { url } => Ok(SourceConfig::new(dav::configure(
            account_name,
            DavKind::Cal,
            url,
            &choice,
        )?)),
        kind => bail!("Configuration {kind:?} is not supported by this build"),
    }
}

/// Stops the wizard when discovery found nothing for `email`, pointing at the
/// documented sample rather than dropping into a hand-entry flow: it only
/// ever configures what it can discover automatically.
fn stop_undiscovered(email: &str) -> Result<SourceConfig> {
    bail!(
        "Could not automatically discover a configuration for {email}.\n\n\
         Write your account configuration by hand instead, starting from the \
         documented sample:\n  {CONFIG_SAMPLE_URL}"
    )
}

/// Proposes a default account name from the email: the first label of
/// its domain, never the local part.
pub fn default_account_name(email: &str) -> String {
    let domain = match email.rsplit_once('@') {
        Some((_, domain)) => domain,
        None => email,
    };

    let label = domain.split('.').next().unwrap_or(domain);

    match label.is_empty() {
        true => String::from("default"),
        false => label.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_name_defaults_to_the_first_domain_label() {
        assert_eq!(default_account_name("clement.douin@posteo.net"), "posteo");
        assert_eq!(default_account_name("alice@mail.example.co.uk"), "mail");

        assert_eq!(default_account_name("@posteo.net"), "posteo");
        assert_eq!(default_account_name("posteo.net"), "posteo");

        assert_eq!(default_account_name("@"), "default");
    }
}
