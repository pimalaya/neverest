//! # Check command
//!
//! Reports what the store keeps, then opens every source and lists its
//! collections, and opens the send channel a source declares, surfacing
//! credential, network or config errors before a real sync.

use std::{fmt, path::PathBuf};

use anyhow::{Result, bail};
use clap::Parser;
use log::info;
use pimalaya_cli::{printer::Printer, spinner::Spinner};
use pimalaya_config::toml::TomlConfig;
use schemars::JsonSchema;
use serde::Serialize;

use crate::{account::Account, client, config::Config, offline::capability};

/// Probes every configured source before a real sync.
///
/// Reports the account's namespaces, then opens each source and lists its
/// collections, and opens and authenticates the SMTP channel a source
/// declares, surfacing credential, network or config errors before a real
/// sync or a first send.
#[derive(Debug, Parser)]
pub struct CheckCommand {}

impl CheckCommand {
    /// Opens every endpoint the account declares and reports what answered.
    pub fn execute(
        self,
        printer: &mut impl Printer,
        config_paths: &[PathBuf],
        account_name: Option<&str>,
    ) -> Result<()> {
        let mut config = Config::load_or_wizard(printer, config_paths)?;

        let Some((name, account_config)) = config.take_account(account_name)? else {
            bail!("Cannot find account");
        };

        account_config.validate()?;

        info!("checking account {name}");

        let account_mode = account_config.mode()?;
        let mode = account_mode.to_string();

        // NOTE: every credential at once, so a check costs one unlock per
        // password command rather than one per endpoint.
        let account = Account::resolve(&account_config)?;

        let mut sources = Vec::new();

        for (endpoint, endpoint_config) in account_config.endpoints()? {
            // NOTE: a target is written to, never declared, as in a sync.
            let capabilities = match account_mode.sources.contains(&endpoint) {
                true => capability::supports(&endpoint_config, account_mode.one_way)
                    .into_iter()
                    .map(CheckedCapability::from)
                    .collect(),
                false => Vec::new(),
            };
            sources.push(check_source(&endpoint, &account, capabilities)?);
        }

        printer.out(CheckOutput {
            account: name,
            mode,
            sources,
        })
    }
}

/// Opens the source and lists its collections with the role each states on
/// the server, then opens its SMTP channel, if it declares one, up to
/// authentication.
fn check_source(
    label: &str,
    account: &Account,
    capabilities: Vec<CheckedCapability>,
) -> Result<SourceCheck> {
    let s = Spinner::start(format!("Checking source {label}…"));
    let source = account.get(label)?;
    let mut client = client::open(&source)?;
    let mut roles = client.collection_roles()?;
    let default = client.default_collection()?;
    let collections: Vec<CheckedCollection> = client
        .list_collections(false)?
        .into_iter()
        .map(|collection| CheckedCollection {
            role: roles.remove(&collection.id),
            default: default.as_ref() == Some(&collection.id),
            id: collection.id,
            name: collection.name,
        })
        .collect();
    s.success(format!(
        "Checked source {label} ({} collections)",
        collections.len()
    ));

    // NOTE: nothing is sent: the session is opened, upgraded and
    // authenticated, then quit.
    #[cfg(feature = "smtp")]
    let smtp = match &source.smtp {
        Some(smtp) => {
            let s = Spinner::start(format!("Checking the SMTP channel of {label}…"));
            let mut session = crate::offline::submit::connect_smtp(smtp)
                .map_err(|err| err.context(format!("Check the SMTP channel of {label}")))?;
            let _ = io_smtp::client::SmtpClient::quit(&mut session);
            s.success(format!("Checked the SMTP channel of {label}"));
            true
        }
        None => false,
    };
    #[cfg(not(feature = "smtp"))]
    let smtp = false;

    Ok(SourceCheck {
        source: label.to_owned(),
        collections,
        capabilities,
        smtp,
    })
}

/// What `neverest check` reports: the account's declared mode, and every
/// source that answered.
///
/// A source that did not answer is not in here: the command stops on the
/// first failure, so reaching this output means every endpoint opened.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CheckOutput {
    /// The account that was checked.
    pub account: String,
    /// What the account does, as its mode reads.
    pub mode: String,
    /// One entry per endpoint the account opens, source and target alike.
    pub sources: Vec<SourceCheck>,
}

impl fmt::Display for CheckOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{mode}", mode = self.mode)?;
        writeln!(f)?;

        for source in &self.sources {
            writeln!(f, " - {source}")?;
        }

        writeln!(f)?;
        writeln!(f, "Account {account} looks healthy", account = self.account)
    }
}

/// One endpoint that answered, and how much it holds.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SourceCheck {
    /// The endpoint's pimdir source id.
    pub source: String,
    /// Every collection it listed, with the role the server states.
    pub collections: Vec<CheckedCollection>,
    /// What the source can do, as a sync declares it in the store (pimdir
    /// STORAGE §15.6), one row per capability: an intent performed on the
    /// collections the source holds reads as it does on them. Empty for a
    /// target and for an undeclared (JMAP) source.
    pub capabilities: Vec<CheckedCapability>,
    /// Whether its SMTP channel was opened and authenticated, `false` when
    /// it declares none.
    pub smtp: bool,
}

impl fmt::Display for SourceCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            source,
            collections,
            smtp,
            ..
        } = self;
        write!(f, "{source} ({} collection(s))", collections.len())?;
        if *smtp {
            write!(f, ", SMTP channel authenticated")?;
        }
        Ok(())
    }
}

/// One collection a source listed, and what the server says it is for.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CheckedCollection {
    /// The collection's id on its source: the mailbox name on IMAP, the
    /// folder path on Graph, the label name on Gmail, the href on DAV.
    pub id: String,
    /// Its display name.
    pub name: String,
    /// The role the server states for it (`inbox`, `sent`, `drafts`,
    /// `trash`, `junk`, `all`, `archive`), never guessed from its name:
    /// IMAP SPECIAL-USE and `INBOX`, Graph's well-known folders, Gmail's
    /// system labels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Whether it is where a new item goes when none is named, as the
    /// server states it: Google's primary calendar, Graph's default
    /// calendar and default Contacts folder, the one People address book,
    /// the CalDAV calendar the scheduling inbox names (RFC 6638
    /// `schedule-default-calendar-URL`). Never set on mail nor CardDAV, at
    /// most once per source, and left out when false.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub default: bool,
}

/// One capability a source declares.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CheckedCapability {
    /// Its pimdir name, e.g. `mail.message.add`, `calendar.reply`.
    pub name: String,
    /// `full`, `partial` or `none`.
    pub support: String,
    /// Why it is partial or refused.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl From<capability::SupportRow> for CheckedCapability {
    fn from((name, support, detail): capability::SupportRow) -> Self {
        Self {
            name: name.to_owned(),
            support: support.as_str().to_owned(),
            detail: detail.map(str::to_owned),
        }
    }
}
