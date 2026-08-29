//! # Configuration wizard
//!
//! Offered when no configuration file is found, by a bare `neverest` or by a
//! command that needs an account, and re-run over an existing account by
//! `neverest configure` (see [`super::edit`]). A configuration that already
//! exists is never met with it: a bare `neverest` gets the help instead.
//!
//! It asks for one input, an email address (a bare domain is accepted and
//! synthesized as `@domain`). That feeds io-pim-discovery's parallel search
//! (see [`super::search`]) and every reachable service becomes one selectable
//! configuration; only backends compiled into this build are proposed.
//!
//! The wizard writes one account with one source, in the direct-backend sugar
//! (`imap.server = …`): the offline replica, which is the common case and
//! reads offline with no further setting. A second kind, a mirror and a
//! fan-in are all written by hand against config.sample.toml.
//!
//! The generated configuration is offered for saving when writing to a
//! terminal; when stdout is redirected or in JSON mode it is emitted straight
//! to stdout, so scripts keep working.

use std::{collections::HashMap, fmt, io::IsTerminal, path::Path};

use anyhow::{Result, bail};
use log::info;
use pimalaya_cli::{printer::Printer, prompt, spinner::Spinner};
use pimalaya_config::toml as config_toml;
use serde::{Serialize, Serializer};

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
    config::{AccountConfig, Config, SourceConfig},
    wizard::search::{self, Discovered},
};

/// The one prompt of the wizard. Discovery also accepts a bare domain, but
/// the label names what users actually have.
const EMAIL_PROMPT: &str = "Email address:";

/// The documented sample configuration, shown in the welcome banner and
/// pointed at when discovery finds nothing.
pub const CONFIG_SAMPLE_URL: &str =
    "https://github.com/pimalaya/neverest/blob/master/config.sample.toml";

/// Offers to generate a configuration at `target`, running the wizard
/// when accepted, and reports whether it ran.
///
/// The only place the wizard introduces itself to someone who did not ask for
/// it. Callers guard the offer on a terminal and on the human output, since
/// neither a script nor a JSON consumer can answer a prompt.
pub fn offer_configuration(printer: &mut impl Printer, target: &Path) -> Result<bool> {
    let prompt = format!(
        "No configuration found, create one at {}?",
        target.display()
    );

    if !prompt::bool(&prompt, true)? {
        return Ok(false);
    }

    run(printer, target)?;

    Ok(true)
}

/// Runs the wizard and either saves the resulting [`Config`] to `target` or
/// prints it as a ready-to-save TOML document, then returns it.
pub fn run(printer: &mut impl Printer, target: &Path) -> Result<Config> {
    if !printer.is_json() {
        print_welcome();
    }

    let email = prompt_email()?;

    let account_name = default_account_name(&email);
    let source = configure(&account_name, &email)?;

    let account = AccountConfig::with_source(true, source);

    let config = Config {
        accounts: HashMap::from([(account_name.clone(), account)]),
    };

    if printer.is_json() || !std::io::stdout().is_terminal() {
        printer.out(GeneratedConfig(&config))?;
        return Ok(config);
    }

    save_or_print(printer, target, config)
}

/// Offers to save the generated config to `target`, printing it on stdout
/// when the user declines or an existing file must not be overwritten.
fn save_or_print(printer: &mut impl Printer, target: &Path, config: Config) -> Result<Config> {
    let prompt = format!("Save this configuration to {}?", target.display());

    let save = prompt::bool(&prompt, true)?
        && (!target.exists()
            || prompt::bool(
                format!("{} already exists. Overwrite it?", target.display()),
                false,
            )?);

    if !save {
        return printer.out(GeneratedConfig(&config)).map(|()| config);
    }

    config.write(target)?;
    info!("configuration written to {}", target.display());

    eprintln!();
    eprintln!("Configuration saved to {}.", target.display());
    eprintln!("Run `neverest init` to prepare the store, then `neverest sync`.");

    Ok(config)
}

/// The account the wizard produced, as a ready-to-save TOML document or, in
/// JSON mode, an object. Byte-for-byte what [`Config::write`] saves.
struct GeneratedConfig<'a>(&'a Config);

impl fmt::Display for GeneratedConfig<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let toml = config_toml::to_string(self.0).map_err(|_| fmt::Error)?;
        write!(f, "{toml}")
    }
}

impl Serialize for GeneratedConfig<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

/// Prints a welcome banner on stderr framing the project and the wizard,
/// so the first run explains itself before dropping into prompts.
fn print_welcome() {
    eprintln!();
    eprintln!("Welcome to Neverest, the CLI to synchronize PIM collections.");
    eprintln!();
    eprintln!("Neverest reconciles what you already have, mail over IMAP or");
    eprintln!("Microsoft Graph and contacts and calendar over DAV, with a local");
    eprintln!("pimdir store the apps read and edit. Before it can sync, it needs");
    eprintln!("to know about one account.");
    eprintln!();
    eprintln!("This wizard discovers a provider's settings from your email address,");
    eprintln!("tests the connection and generates a ready-to-use configuration it");
    eprintln!("can save for you.");
    eprintln!();
    eprintln!("Every field is documented in the sample configuration:");
    eprintln!("  {CONFIG_SAMPLE_URL}");
    eprintln!();
}

/// Prompts the email address and normalizes it for discovery: a bare domain
/// becomes the `@domain` form the search understands.
pub fn prompt_email() -> Result<String> {
    prompt_email_with(None)
}

/// [`prompt_email`] with a pre-filled default.
pub fn prompt_email_with(default: Option<&str>) -> Result<String> {
    let input = prompt::text(EMAIL_PROMPT, default)?;
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
