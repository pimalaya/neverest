//! # Check command
//!
//! Reports what the store keeps, then opens every source and lists its
//! collections, and opens the send channel a source declares, surfacing
//! credential, network or config errors before a real sync.

use std::{
    collections::{BTreeMap, HashMap},
    fmt,
    path::PathBuf,
};

use anyhow::{Result, bail};
use clap::Parser;
use log::info;
use pimalaya_cli::{
    error::{PartialOutcome, code_of},
    printer::Printer,
    spinner::Spinner,
};
use pimalaya_config::toml::TomlConfig;
use schemars::JsonSchema;
use serde::Serialize;

use crate::{
    account::{Account, SourceAccount},
    client,
    config::{AccountMode, Config, SourceConfig},
    error::ErrorCode,
    item::collection::Collection,
    offline::capability,
};

/// Probes every configured source before a real sync.
///
/// Reports the account's mode, then opens each endpoint and lists its
/// collections, and opens and authenticates the SMTP channel an endpoint
/// declares, surfacing credential, network or config errors before a real
/// sync or a first send.
///
/// A doctor, not a data source: the collections, roles and capabilities it
/// prints are diagnostics for a person. A program reads whether each
/// endpoint answered; what an account holds it reads from the store, which
/// `sync --declare-only` fills without fetching an item.
///
/// Every endpoint is checked, whatever the others answer, and the report
/// names each one's outcome. An endpoint or a channel that did not answer
/// exits 2, the report being the output rather than a failure object.
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

        let mode = account_config.mode()?;

        // NOTE: every credential at once, so a check costs one unlock per
        // password command rather than one per endpoint. One that fails
        // fails its endpoint's row alone.
        let account = Account::resolve(&account_config)?;

        let endpoints = account_config.endpoints()?;
        let mut names: Vec<&String> = endpoints.keys().collect();
        names.sort();

        let sources: Vec<SourceCheck> = names
            .into_iter()
            .map(|endpoint| {
                let config = &endpoints[endpoint];
                let capabilities = declared(endpoint, config, &mode, &endpoints);
                check_source(endpoint, config, &account, capabilities)
            })
            .collect();

        let report = CheckOutput {
            account: name,
            mode,
            sources,
        };
        let failed = report.failed();
        printer.out(report)?;

        // NOTE: the report names every endpoint's outcome, so a failed one
        // ends the command with the partial-outcome status rather than a
        // failure object.
        if failed {
            return Err(PartialOutcome::default().into());
        }

        Ok(())
    }
}

/// What a sync declares `endpoint` can do, from the configuration alone:
/// every Annex B capability of its kind and the source-wide
/// `calendar.component.vtodo` row, its provider's or a target's (pimdir
/// STORAGE §15.6). A target is written to, never declared, as in a sync.
fn declared(
    endpoint: &str,
    config: &SourceConfig,
    mode: &AccountMode,
    endpoints: &HashMap<String, SourceConfig>,
) -> Vec<CheckedCapability> {
    if !mode.sources.iter().any(|source| source == endpoint) {
        return Vec::new();
    }
    let targets: Vec<(&str, &SourceConfig)> = mode
        .targets
        .iter()
        .filter_map(|name| Some((name.as_str(), endpoints.get(name)?)))
        .collect();
    let mut capabilities: Vec<CheckedCapability> =
        capability::supports(config, mode.one_way, !mode.targets.is_empty())
            .into_iter()
            .map(CheckedCapability::from)
            .collect();
    if let Some(wide) = capability::vtodo_wide(config, &targets) {
        capabilities.push(CheckedCapability {
            name: wide.name,
            support: wide.support.as_str().to_owned(),
            detail: wide.detail,
        });
    }
    capabilities
}

/// Opens the endpoint and lists its collections with the role each states
/// on the server, then opens its SMTP channel, if it declares one, up to
/// authentication. A failure lands in the row, never ends the check.
fn check_source(
    label: &str,
    config: &SourceConfig,
    account: &Account,
    capabilities: Vec<CheckedCapability>,
) -> SourceCheck {
    let s = Spinner::start(format!("Checking source {label}…"));
    let (listed, source) = match account.get(label) {
        Ok(source) => (list(&source), Some(source)),
        Err(err) => (Err(err), None),
    };

    match &listed {
        Ok(collections) => s.success(format!(
            "Checked source {label} ({} collections)",
            collections.len()
        )),
        Err(err) => s.success(format!("Source {label} failed ({err:#})")),
    }

    // NOTE: nothing is sent: the session is opened, upgraded and
    // authenticated, then quit.
    #[cfg(feature = "smtp")]
    let smtp = source
        .as_ref()
        .and_then(|source| source.smtp.as_ref())
        .map(|smtp| {
            let s = Spinner::start(format!("Checking the SMTP channel of {label}…"));
            let opened = crate::offline::submit::connect_smtp(smtp)
                .map(|mut session| {
                    let _ = io_smtp::client::SmtpClient::quit(&mut session);
                })
                .map_err(|err| err.context(format!("Check the SMTP channel of {label}")));
            match &opened {
                Ok(()) => s.success(format!("Checked the SMTP channel of {label}")),
                Err(err) => s.success(format!("SMTP channel of {label} failed ({err:#})")),
            }
            ChannelCheck::from("smtp", opened)
        });
    #[cfg(not(feature = "smtp"))]
    let smtp = {
        let _ = source;
        None
    };

    let (collections, outcome) = match listed {
        Ok(collections) => (collections, Ok(())),
        Err(err) => (Vec::new(), Err(err)),
    };
    let ChannelCheck {
        backend,
        ok,
        code,
        error,
    } = ChannelCheck::from(config.backend.protocol(), outcome);

    SourceCheck {
        source: label.to_owned(),
        backend,
        ok,
        code,
        error,
        collections,
        capabilities,
        smtp,
    }
}

/// Opens one connection to `source` and lists its collections, with the
/// role and the default mark the server states.
fn list(source: &SourceAccount) -> Result<Vec<CheckedCollection>> {
    let mut client = client::open(source)?;
    let mut roles = client.collection_roles()?;
    let default = client.default_collection()?;
    let collections = client
        .list_collections(false)?
        .into_iter()
        .map(|collection| checked(collection, &mut roles, default.as_ref()))
        .collect();
    Ok(collections)
}

/// One listed collection as a check reports it, with the role and the
/// default mark the server states, taken out of `roles`.
fn checked(
    collection: Collection,
    roles: &mut BTreeMap<String, String>,
    default: Option<&String>,
) -> CheckedCollection {
    CheckedCollection {
        role: roles.remove(&collection.id),
        default: default == Some(&collection.id),
        takes_tasks: collection.takes_tasks,
        id: collection.id,
        name: collection.name,
    }
}

/// What `neverest check` reports: the account's declared mode, and the
/// outcome of every endpoint, whether it answered or not.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CheckOutput {
    /// The account that was checked.
    pub account: String,
    /// What the account does: its endpoints, direction and whether the
    /// store keeps bodies.
    pub mode: AccountMode,
    /// One row per endpoint the account opens, source and target alike,
    /// sorted by name.
    pub sources: Vec<SourceCheck>,
}

impl CheckOutput {
    /// Whether an endpoint or its send channel did not answer.
    pub fn failed(&self) -> bool {
        self.sources
            .iter()
            .any(|source| !source.ok || source.smtp.as_ref().is_some_and(|smtp| !smtp.ok))
    }
}

impl fmt::Display for CheckOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{mode}", mode = self.mode)?;
        writeln!(f)?;

        for source in &self.sources {
            writeln!(f, " - {source}")?;
        }

        writeln!(f)?;

        let account = &self.account;
        match self.failed() {
            true => writeln!(f, "Account {account} has endpoints that did not answer"),
            false => writeln!(f, "Account {account} looks healthy"),
        }
    }
}

/// One endpoint, whether it answered, and how much it holds.
///
/// Its first four keys are the backend check row every pimalaya binary
/// prints.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SourceCheck {
    /// The endpoint's pimdir source id.
    pub source: String,
    /// Its protocol, the table it is written under: `imap`, `caldav`,
    /// `msgraph-calendar`…
    pub backend: &'static str,
    /// Whether it opened and listed its collections.
    pub ok: bool,
    /// The stable code of the failure, `null` when it answered or the
    /// failure carries none.
    pub code: Option<ErrorCode>,
    /// Why it did not answer, `null` when it did.
    pub error: Option<String>,
    /// Every collection it listed, with the role the server states; empty
    /// when it did not answer.
    pub collections: Vec<CheckedCollection>,
    /// What the source can do, as a sync declares it in the store (pimdir
    /// STORAGE §15.6), one row per capability: an intent performed on the
    /// collections the source holds reads as it does on them. Read from
    /// the configuration, so present whether it answered or not. Empty for
    /// a target and for an undeclared (JMAP) source.
    pub capabilities: Vec<CheckedCapability>,
    /// Whether its SMTP channel opened and authenticated, `null` when it
    /// declares none, when its credentials did not resolve, or in a build
    /// without SMTP.
    pub smtp: Option<ChannelCheck>,
}

impl fmt::Display for SourceCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let source = &self.source;
        match &self.error {
            None => write!(f, "{source} ({} collection(s))", self.collections.len())?,
            Some(err) => write!(f, "{source}: FAIL ({err})")?,
        }
        match &self.smtp {
            Some(ChannelCheck { error: None, .. }) => write!(f, ", SMTP channel authenticated"),
            Some(ChannelCheck {
                error: Some(err), ..
            }) => write!(f, ", SMTP channel: FAIL ({err})"),
            None => Ok(()),
        }
    }
}

/// One channel an endpoint opens beside its own, and whether it answered:
/// the backend check row every pimalaya binary prints.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChannelCheck {
    /// The backend that was checked.
    pub backend: &'static str,
    /// Whether it answered.
    pub ok: bool,
    /// The stable code of the failure, `null` when it answered or the
    /// failure carries none.
    pub code: Option<ErrorCode>,
    /// Why it did not answer, `null` when it did.
    pub error: Option<String>,
}

impl ChannelCheck {
    /// Folds an attempt at `backend` into its outcome.
    fn from(backend: &'static str, result: Result<()>) -> Self {
        match result {
            Ok(()) => Self {
                backend,
                ok: true,
                code: None,
                error: None,
            },
            Err(err) => Self {
                backend,
                ok: false,
                code: code_of(&err),
                error: Some(format!("{err:#}")),
            },
        }
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
    pub role: Option<String>,
    /// Whether it is where a new item goes when none is named, as the
    /// server states it: Google's primary calendar, Graph's default
    /// calendar and default Contacts folder, the one People address book,
    /// the CalDAV calendar the scheduling inbox names (RFC 6638
    /// `schedule-default-calendar-URL`). Never set on mail nor CardDAV, at
    /// most once per source.
    pub default: bool,
    /// Whether the calendar takes tasks, as a CalDAV server states it by
    /// its `supported-calendar-component-set` (RFC 4791 §5.2.3): `false`
    /// when the set leaves VTODO out, which a sync declares as
    /// `calendar.component.vtodo` `none` on the source's calendar of that id, a
    /// target's included.
    /// `null` where no set is stated, which takes any component, and on
    /// every other backend: Google and Graph, whose calendars take no task,
    /// say so by a source-wide capability.
    pub takes_tasks: Option<bool>,
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

#[cfg(test)]
mod tests {
    use crate::config::AccountConfig;

    use super::*;

    /// What a check reports each endpoint of `toml`'s account declares, by
    /// endpoint.
    fn declared_by(toml: &str) -> BTreeMap<String, Vec<CheckedCapability>> {
        let account: AccountConfig = toml::from_str(toml).unwrap();
        let mode = account.mode().unwrap();
        let endpoints = account.endpoints().unwrap();
        endpoints
            .iter()
            .map(|(name, config)| (name.clone(), declared(name, config, &mode, &endpoints)))
            .collect()
    }

    fn vtodo(capabilities: &[CheckedCapability]) -> Option<&CheckedCapability> {
        capabilities
            .iter()
            .find(|row| row.name == io_pimdir::capability::CALENDAR_COMPONENT_VTODO)
    }

    /// The source-wide `calendar.component.vtodo` row a sync declares is
    /// reported with the Annex B ones: a Google or Graph calendar source, and
    /// a source copied to a Google target, take no task; a target lists
    /// nothing.
    #[test]
    fn a_check_reports_the_calendars_that_take_no_tasks() {
        let declared = declared_by("gcal.auth.token.raw = \"token\"");
        let row = vtodo(&declared["gcal"]).expect("a Google source takes no task");
        assert_eq!(row.support, "none");
        assert!(row.detail.as_deref().unwrap().contains("Google Tasks"));

        let declared = declared_by("msgraph-calendar.auth.token.raw = \"token\"");
        let row = vtodo(&declared["msgraph-calendar"]).expect("a Graph source takes no task");
        assert!(row.detail.as_deref().unwrap().contains("Microsoft To Do"));

        let caldav = "caldav.server = \"https://dav.example.org/\"\n\
                      caldav.auth.basic.username = \"user\"\n\
                      caldav.auth.basic.password.raw = \"pw\"\n";
        assert!(vtodo(&declared_by(caldav)["caldav"]).is_none());

        let declared = declared_by(&format!(
            "{caldav}one-way = true\ntargets.google.gcal.auth.token.raw = \"token\""
        ));
        let row = vtodo(&declared["caldav"]).expect("a source copied to Google takes no task");
        assert_eq!(row.support, "none");
        assert!(row.detail.as_deref().unwrap().contains("target google"));
        assert!(declared["google"].is_empty());
    }

    /// A CalDAV calendar's component set is reported on the collection, a
    /// listing stating none printing `null`.
    #[test]
    fn a_check_names_what_each_calendar_takes() {
        let listed = |id: &str, takes_tasks| Collection {
            id: id.into(),
            name: id.into(),
            total: None,
            unread: None,
            role: None,
            takes_tasks,
        };
        let json = |collection| {
            let checked = checked(collection, &mut BTreeMap::new(), None);
            serde_json::to_value(checked).unwrap()
        };

        assert_eq!(
            json(listed("events", Some(false)))["takesTasks"],
            serde_json::Value::Bool(false)
        );
        assert_eq!(
            json(listed("work", Some(true)))["takesTasks"],
            serde_json::Value::Bool(true)
        );
        assert_eq!(
            json(listed("any", None))["takesTasks"],
            serde_json::Value::Null
        );
    }
}
