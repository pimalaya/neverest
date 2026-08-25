//! Configuration wizard.
//!
//! Run on bare `neverest` (no subcommand), proposed when no
//! configuration file is found (see
//! [`crate::config::Config::load_or_wizard`]), and re-run over an
//! existing account by `neverest configure` (see [`super::edit`]). It
//! opens with a welcome banner on stderr, then asks for **one** input:
//! an email address. A bare domain is accepted too (it is synthesized as
//! `@domain`); a side is always a remote, so there is no server URL or
//! folder path to type here.
//!
//! The address feeds io-pim-discovery's parallel discovery (see
//! [`super::search`]) and every reachable service becomes one selectable
//! configuration; picking one then prompts its authentication method and
//! credentials, and tests the connection. Only backends compiled into
//! this build are proposed, so the wizard never writes a side
//! [`crate::client::open`] would refuse. When discovery finds nothing the
//! wizard stops and points at the documented sample, rather than
//! prompting for a hand-entered config.
//!
//! The wizard configures a **local sync** only: the discovered remote as
//! `left`, reconciled against the implicit pimdir store. A
//! remote-to-remote mirror (a `right` side) is written by hand.
//!
//! The generated configuration is offered for saving when writing to a
//! terminal; when stdout is redirected (`neverest > config.toml`) or in
//! JSON mode it is emitted straight to stdout, so scripts keep working.

use std::{collections::HashMap, fmt, io::IsTerminal, path::Path};

use anyhow::{Result, bail};
use log::info;
use pimalaya_cli::{printer::Printer, prompt, spinner::Spinner};
use pimalaya_config::toml as config_toml;
use serde::{Serialize, Serializer};

#[cfg(any(feature = "imap", feature = "msgraph", feature = "carddav"))]
use crate::config::SideBackendConfig;
#[cfg(feature = "carddav")]
use crate::wizard::carddav;
#[cfg(feature = "imap")]
use crate::wizard::imap_smtp;
#[cfg(feature = "msgraph")]
use crate::wizard::msgraph;
#[cfg(any(feature = "imap", feature = "msgraph", feature = "carddav"))]
use crate::wizard::search::DiscoveredKind;
use crate::{
    config::{AccountConfig, Config, SideConfig},
    wizard::search::{self, Discovered},
};

/// The one prompt of the wizard. Discovery also accepts a bare domain,
/// but the label names what users actually have.
const EMAIL_PROMPT: &str = "Email address:";

/// The documented sample configuration, shown in the welcome banner and
/// pointed at when discovery finds nothing to configure automatically.
const CONFIG_SAMPLE_URL: &str =
    "https://github.com/pimalaya/neverest/blob/master/config.sample.toml";

/// Runs the wizard and either saves the resulting [`Config`] to
/// `target` or prints it as a ready-to-save TOML document, then returns
/// it. Run on bare `neverest`, and proposed by
/// [`crate::config::Config::load_or_wizard`] on first run.
pub fn run(printer: &mut impl Printer, target: &Path) -> Result<Config> {
    if !printer.is_json() {
        print_welcome();
    }

    let email = prompt_email()?;

    // NOTE: the account name is just the TOML table key, so it is derived
    // from the email rather than prompted; the user renames it by hand.
    let account_name = default_account_name(&email);
    let side = configure(&account_name, &email)?;

    let account = AccountConfig {
        default: true,
        left: Some(side),
        right: None,
        store: Default::default(),
        collection: Default::default(),
        item: Default::default(),
        connections: None,
    };

    let config = Config {
        accounts: HashMap::from([(account_name.clone(), account)]),
    };

    // JSON mode and a redirected stdout stay non-interactive: emit the
    // document straight to stdout so scripts and `neverest > config.toml`
    // keep working. Only offer to save when writing to a terminal.
    if printer.is_json() || !std::io::stdout().is_terminal() {
        printer.out(GeneratedConfig(&config))?;
        return Ok(config);
    }

    save_or_print(printer, target, config)
}

/// Offers to save the generated config to `target`, falling back to
/// printing it on stdout when the user declines or an existing file must
/// not be overwritten. Prompts and confirmations render on stderr.
fn save_or_print(printer: &mut impl Printer, target: &Path, config: Config) -> Result<Config> {
    let prompt = format!("Save this configuration to {}?", target.display());

    // Bare `neverest` runs the wizard even when a config already exists,
    // so never clobber without confirmation, and fall back to printing so
    // the generated config is never lost.
    let save = prompt::bool(&prompt, true)?
        && (!target.exists()
            || prompt::bool(
                format!("`{}` already exists. Overwrite it?", target.display()),
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

/// The account produced by the wizard, rendered as a ready-to-save TOML
/// document (for stdout), or serialized as an object in JSON mode. The
/// document is byte-for-byte what [`Config::write`] saves.
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
    eprintln!("Neverest reconciles your existing mailbox, over IMAP or Microsoft");
    eprintln!("Graph, with a local pimdir store the apps read and edit. Before it");
    eprintln!("can sync, it needs to know about one account.");
    eprintln!();
    eprintln!("This wizard discovers a provider's settings from your email address,");
    eprintln!("tests the connection and generates a ready-to-use configuration it");
    eprintln!("can save for you.");
    eprintln!();
    eprintln!("Every field is documented in the sample configuration:");
    eprintln!("  {CONFIG_SAMPLE_URL}");
    eprintln!();
}

/// Prompts the email address (seeded with `default` when re-running over
/// an existing account) and normalizes it for discovery: a bare domain
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

/// Searches the services reachable from `email`, keeps only those this
/// build supports, lets the user pick one, then configures its backend
/// (the authentication method is picked in a second, service-specific
/// prompt) and tests the connection.
pub fn configure(account_name: &str, email: &str) -> Result<SideConfig> {
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
fn dispatch(account_name: &str, email: &str, choice: Discovered) -> Result<SideConfig> {
    match &choice.kind {
        #[cfg(feature = "imap")]
        DiscoveredKind::ImapSmtp { .. } => {
            // The submission endpoint belongs to the side it was
            // discovered with: IMAP cannot send, so its companion SMTP
            // server is what performs this side's submit intents.
            let (imap, smtp) = imap_smtp::configure_discovered(account_name, email, &choice)?;
            Ok(SideConfig {
                backend: SideBackendConfig::Imap(imap),
                smtp,
            })
        }
        #[cfg(feature = "msgraph")]
        // Graph sends through its own `sendMail` action, so this side
        // needs no channel of its own.
        DiscoveredKind::Msgraph => Ok(SideConfig::new(SideBackendConfig::Msgraph(
            msgraph::configure(account_name)?,
        ))),
        // Contacts never submit anything, so a CardDAV side carries no send
        // channel at all (the config refuses one).
        #[cfg(feature = "carddav")]
        DiscoveredKind::Carddav { url } => Ok(SideConfig::new(SideBackendConfig::Carddav(
            carddav::configure(account_name, url, &choice)?,
        ))),
        kind => bail!("Configuration `{kind:?}` is not supported by this build"),
    }
}

/// Stops the wizard when discovery found nothing to configure for
/// `email`: it prints where to go next (a hand-written config, seeded
/// from the documented sample) and errors out, rather than dropping into
/// a hand-entry flow. The wizard only ever configures what it can
/// discover automatically.
fn stop_undiscovered(email: &str) -> Result<SideConfig> {
    bail!(
        "Could not automatically discover a configuration for `{email}`.\n\n\
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
        // Email: the domain's first label, never the local part.
        assert_eq!(default_account_name("clement.douin@posteo.net"), "posteo");
        assert_eq!(default_account_name("alice@mail.example.co.uk"), "mail");
        // Bare domain, as the email prompt synthesizes it.
        assert_eq!(default_account_name("@posteo.net"), "posteo");
        assert_eq!(default_account_name("posteo.net"), "posteo");
        // Nothing usable: a name is still needed for the TOML key.
        assert_eq!(default_account_name("@"), "default");
    }
}
