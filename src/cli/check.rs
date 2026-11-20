//! # Check command
//!
//! Reports what each account does, then opens every source and lists its
//! collections, and opens the send channel a source declares, surfacing
//! credential, network or config errors before a real sync; then names the
//! accounts a store holds that no configuration names.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt,
    path::PathBuf,
};

use anyhow::{Context, Result};
use clap::Parser;
use io_pimdir::capability::PimdirSupport;
use io_pimdir::client::reader::PimdirReader;
use log::{info, warn};
use pimalaya_cli::{
    account::BackendCheck, capability::CapabilityLevel, error::PartialOutcome, fields::Fields,
    printer::Printer, sanitize,
};
use schemars::JsonSchema;
use serde::Serialize;

use crate::{
    account::{Account, SourceAccount},
    client,
    collection::Collection,
    config::{AccountConfig, Config, mode::AccountMode, source::SourceConfig, tls::TlsConfig},
    error::{self, ErrorCode},
    sync::{capability, driver},
};

/// Probe every configured source before a real sync.
///
/// Reports each account's mode, then opens each endpoint and lists its
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
/// exits 3, the report being the output rather than a failure object.
///
/// It also names the accounts a store holds that no configured account
/// names: renaming an account's key leaves its collections behind, under the
/// old name, until the key is renamed back. Nothing is deleted.
#[derive(Debug, Parser)]
pub struct CheckCommand {}

impl CheckCommand {
    /// Opens every endpoint of the accounts the run covers and reports what
    /// answered, then the store accounts no configuration names.
    pub fn execute(
        self,
        printer: &mut impl Printer,
        config_paths: &[PathBuf],
        accounts: &[String],
    ) -> Result<()> {
        let config = Config::load(config_paths)?;
        let selected = config.select(accounts)?;

        let mut checked = Vec::new();
        for (name, account_config) in &selected {
            account_config
                .validate()
                .with_context(|| format!("Validate account {name}"))?;
        }
        for (name, account_config) in &selected {
            checked.push(check_account(name, account_config)?);
        }

        let report = CheckRunOutput {
            accounts: checked,
            orphans: orphans(&config, &selected),
        };
        let failed = report.accounts.iter().any(CheckOutput::failed);
        printer.out(report)?;

        if failed {
            return Err(PartialOutcome::default().into());
        }

        Ok(())
    }
}

/// Opens every endpoint `account_config` declares and reports what answered.
///
/// Every credential is resolved up front, so a check costs one unlock per
/// password command rather than one per endpoint.
fn check_account(name: &str, account_config: &AccountConfig) -> Result<CheckOutput> {
    info!("checking account {name}");

    let mode = account_config.mode()?;

    let account = Account::resolve(account_config)?;

    let endpoints = account_config.endpoints()?;
    let mut names: Vec<&String> = endpoints.keys().collect();
    names.sort();

    let backends: Vec<SourceCheck> = names
        .into_iter()
        .map(|endpoint| {
            let config = &endpoints[endpoint];
            let capabilities = declared(endpoint, config, &mode, &endpoints);
            check_source(endpoint, config, &account, capabilities)
        })
        .collect();

    Ok(CheckOutput {
        account: name.to_owned(),
        backends,
        mode,
    })
}

/// By store, the accounts holding collections there that no configured
/// account sharing it names: what renaming an account's key leaves behind.
///
/// Read without a lock and never written; a store that does not read is
/// skipped with a warning.
fn orphans(config: &Config, selected: &[(String, AccountConfig)]) -> Vec<OrphanAccount> {
    let stores: BTreeSet<PathBuf> = selected
        .iter()
        .filter_map(|(_, account_config)| driver::store_dir(account_config).ok())
        .collect();

    let mut orphans = Vec::new();
    for store in stores {
        if !store.join("pimdir.db").exists() {
            continue;
        }

        let held = PimdirReader::open(&store).and_then(|reader| reader.list_accounts());
        let held = match held {
            Ok(held) => held,
            Err(err) => {
                warn!("cannot read the accounts {} holds: {err}", store.display());
                continue;
            }
        };

        let configured: BTreeSet<&String> = config
            .accounts
            .iter()
            .filter(|(_, account_config)| {
                driver::store_dir(account_config).ok() == Some(store.clone())
            })
            .map(|(name, _)| name)
            .collect();

        for account in held {
            if !configured.contains(&account) {
                orphans.push(OrphanAccount {
                    store: store.clone(),
                    account,
                });
            }
        }
    }

    orphans
}

/// What a sync declares `endpoint` can do, from the configuration alone.
///
/// Every Annex B capability of its kind and the source-wide
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
            level: level(wide.support),
            detail: wide.detail,
        });
    }
    capabilities
}

/// Opens the endpoint and lists its collections with the role each states,
/// then opens its SMTP channel, if any, up to authentication.
///
/// A failure lands in the row, never ends the check.
fn check_source(
    label: &str,
    config: &SourceConfig,
    account: &Account,
    capabilities: Vec<CheckedCapability>,
) -> SourceCheck {
    let (listed, source) = match account.get(label) {
        Ok(source) => (list(&source), Some(source)),
        Err(err) => (Err(err), None),
    };

    match &listed {
        Ok(collections) => crate::notice::print(format!(
            "Checked source {label} ({})",
            counted(collections.len(), "collection")
        )),
        Err(err) => crate::notice::print(format!("Source {label} failed ({err:#})")),
    }

    #[cfg(feature = "smtp")]
    let smtp = source
        .as_ref()
        .and_then(|source| source.smtp.as_ref())
        .map(|smtp| {
            let opened = crate::sync::submit::connect_smtp(smtp)
                .map(|mut session| {
                    let _ = io_smtp::client::SmtpClient::quit(&mut session);
                })
                .map_err(|err| err.context(format!("Check the SMTP channel of {label}")));
            match &opened {
                Ok(()) => crate::notice::print(format!("Checked the SMTP channel of {label}")),
                Err(err) => {
                    crate::notice::print(format!("SMTP channel of {label} failed ({err:#})"))
                }
            }
            let mut check = BackendCheck::new("smtp", opened.map_err(error::coded));
            check.tls = config
                .smtp
                .as_ref()
                .and_then(|smtp| smtp.secured())
                .map(TlsConfig::checked);
            check
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
    let mut check = BackendCheck::new(config.backend.protocol(), outcome.map_err(error::coded));
    check.tls = config.secured().map(TlsConfig::checked);

    SourceCheck {
        check,
        source: label.to_owned(),
        collections,
        capabilities,
        smtp,
    }
}

/// Opens one connection to `source` and lists its collections, with the
/// role and the default mark the server states: the per-endpoint test.
pub fn list(source: &SourceAccount) -> Result<Vec<CheckedCollection>> {
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

/// What `neverest account check` reports: pimalaya-cli's account check
/// output, one entry per account checked, then the accounts its stores hold
/// that no configuration names.
///
/// Its shared keys come first, `orphans` after them, as every row's own
/// keys after the shared ones.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CheckRunOutput {
    /// One entry per account the run covered, in name order.
    pub accounts: Vec<CheckOutput>,
    /// The accounts a store the run reads holds collections of, and no
    /// configured account sharing that store names.
    pub orphans: Vec<OrphanAccount>,
}

/// One block per account, a blank line apart, then the orphans.
impl fmt::Display for CheckRunOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, account) in self.accounts.iter().enumerate() {
            if index > 0 {
                writeln!(f)?;
            }
            write!(f, "{account}")?;
        }

        if !self.orphans.is_empty() {
            writeln!(f)?;
            for orphan in &self.orphans {
                writeln!(f, "{orphan}")?;
            }
        }

        Ok(())
    }
}

/// An account a store holds collections of that no configured account
/// sharing the store names: what renaming an account's key leaves behind.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OrphanAccount {
    /// The store directory holding it.
    pub store: PathBuf,
    /// The account name its collections are grouped under.
    pub account: String,
}

impl fmt::Display for OrphanAccount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { store, account } = self;
        let store = store.display().to_string();
        write!(
            f,
            "Store {} holds account {}, which no configured account names: rename an \
             account's key back to `{}` to reach its collections",
            sanitize::text(&store),
            sanitize::text(account),
            sanitize::text(account),
        )
    }
}

/// One account checked, as pimalaya-cli's account check prints it: the
/// account, one row per endpoint, then its declared mode.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CheckOutput {
    /// The account that was checked.
    pub account: String,
    /// One row per endpoint the account opens, source and target alike,
    /// sorted by name.
    pub backends: Vec<SourceCheck>,
    /// What the account does: its endpoints, direction and whether the
    /// store keeps bodies.
    pub mode: AccountMode,
}

impl CheckOutput {
    /// Whether an endpoint or its send channel did not answer.
    pub fn failed(&self) -> bool {
        self.backends
            .iter()
            .any(|source| !source.check.ok || source.smtp.as_ref().is_some_and(|smtp| !smtp.ok))
    }
}

/// The account's fields, as pimalaya-cli prints a check: `Account`, its
/// mode, then one field per endpoint named as the account names it.
impl fmt::Display for CheckOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut fields = Fields::default();
        fields.push("Account", sanitize::text(&self.account));
        fields.push("Mode", sanitize::text(&self.mode.to_string()));

        for source in &self.backends {
            fields.push(&source.source, source);
        }

        write!(f, "{fields}")
    }
}

/// One endpoint, whether it answered, and how much it holds.
///
/// pimalaya-cli's backend check row, `backend` the table the endpoint is
/// written under, followed by neverest's own keys.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SourceCheck {
    /// Whether the endpoint opened and listed its collections, and the
    /// TLS it connected with: `backend` its protocol table (`imap`,
    /// `caldav`, `msgraph` for every Graph domain…).
    #[serde(flatten)]
    pub check: BackendCheck<ErrorCode>,
    /// The endpoint, as the account names it.
    pub source: String,
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
    /// without SMTP: pimalaya-cli's backend check row.
    pub smtp: Option<BackendCheck<ErrorCode>>,
}

/// `OK` or `FAIL (<error>)` and the TLS, as pimalaya-cli prints a backend,
/// then the collections and the send channel.
impl fmt::Display for SourceCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", outcome(&self.check))?;

        if self.check.ok {
            write!(f, ", {}", counted(self.collections.len(), "collection"))?;
        }

        if let Some(smtp) = &self.smtp {
            write!(f, ", SMTP {}", outcome(smtp))?;
        }

        Ok(())
    }
}

/// One collection a source listed, and what the server says it is for.
#[derive(Clone, Debug, Serialize, JsonSchema)]
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

/// One capability a source declares, as the capability rows of every
/// pimalaya binary read: its name, how well it is served and what that
/// leaves out. The backend serving it is the row's endpoint.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CheckedCapability {
    /// Its pimdir name, e.g. `mail.message.add`, `calendar.reply`.
    pub name: String,
    /// How well the source serves it: `full`, `partial` or `none`, as the
    /// store records it.
    pub level: CapabilityLevel,
    /// What a partial level leaves out, or why it is refused.
    pub detail: Option<String>,
}

impl From<capability::SupportRow> for CheckedCapability {
    fn from((name, support, detail): capability::SupportRow) -> Self {
        Self {
            name: name.to_owned(),
            level: level(support),
            detail: detail.map(str::to_owned),
        }
    }
}

/// The shared level of a support the store records.
fn level(support: PimdirSupport) -> CapabilityLevel {
    match support {
        PimdirSupport::Full => CapabilityLevel::Full,
        PimdirSupport::Partial => CapabilityLevel::Partial,
        PimdirSupport::None => CapabilityLevel::None,
    }
}

/// A check row as pimalaya-cli prints it: `OK` or `FAIL (<error>)`, then
/// `, TLS` and the parts its table sets when it connected over TLS.
fn outcome(check: &BackendCheck<ErrorCode>) -> String {
    let mut outcome = match &check.error {
        None => String::from("OK"),
        Some(err) => format!("FAIL ({})", sanitize::text(err)),
    };

    if let Some(tls) = &check.tls {
        outcome.push_str(", TLS");

        let tls = tls.to_string();
        if !tls.is_empty() {
            outcome.push(' ');
            outcome.push_str(&tls);
        }
    }

    outcome
}

/// `count` `noun`s, the noun plural but for one.
fn counted(count: usize, noun: &str) -> String {
    match count {
        1 => format!("1 {noun}"),
        n => format!("{n} {noun}s"),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::PoisonError;

    use io_pimdir::client::PimdirStore;

    use crate::{cli::check::*, conflict::merger::SPAWNS};

    /// A store holding the collections of an account no configured account
    /// sharing it names is reported, its collections left where they are.
    #[test]
    fn a_check_names_the_accounts_a_store_holds_and_no_configuration_names() {
        let _owning = SPAWNS.read().unwrap_or_else(PoisonError::into_inner);
        let dir = tempfile::tempdir().unwrap();
        for account in ["old", "new"] {
            PimdirStore::open(dir.path())
                .unwrap()
                .for_account(account)
                .ensure_collection(format!("{account}/imap/INBOX"), "message/rfc822")
                .unwrap();
        }
        let config: Config = toml::from_str(&format!(
            "[accounts.new]\n\
             imap.server = \"imaps://imap.example.org:993\"\n\
             store.root = \"{}\"\n",
            dir.path().display(),
        ))
        .unwrap();

        let selected = config.select(&[]).unwrap();
        let orphans = orphans(&config, &selected);

        assert_eq!(orphans.len(), 1, "{orphans:?}");
        assert_eq!(orphans[0].account, "old");
        assert_eq!(orphans[0].store, dir.path());
        assert!(orphans[0].to_string().contains("rename"));
        let reader = PimdirReader::open(dir.path()).unwrap();
        let old = reader.list_collections_by_account(Some("old")).unwrap();
        assert_eq!(old.len(), 1, "nothing is deleted");
    }

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
    /// reported with the Annex B ones.
    ///
    /// A Google or Graph calendar source, and a source copied to a Google
    /// target, take no task; a target lists nothing.
    #[test]
    fn a_check_reports_the_calendars_that_take_no_tasks() {
        let declared = declared_by("gcal.auth.token.raw = \"token\"");
        let row = vtodo(&declared["gcal"]).expect("a Google source takes no task");
        assert_eq!(row.level, CapabilityLevel::None);
        assert!(row.detail.as_deref().unwrap().contains("Google Tasks"));

        let declared = declared_by("msgraph.auth.token.raw = \"token\"\nmsgraph.calendar = true");
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
        assert_eq!(row.level, CapabilityLevel::None);
        assert!(row.detail.as_deref().unwrap().contains("target google"));
        assert!(declared["google"].is_empty());
    }

    /// A capability row reads as every pimalaya binary's: `level`, the
    /// shared enum, where it was a free `support` string.
    #[test]
    fn a_capability_row_carries_the_shared_level() {
        let declared = declared_by("gcal.auth.token.raw = \"token\"");
        let json = serde_json::to_value(&declared["gcal"]).unwrap();
        let rows = json.as_array().unwrap();

        assert!(
            rows.iter().all(|row| row.get("support").is_none()),
            "{json}"
        );
        let levels = ["full", "partial", "none"];
        let level = |row: &serde_json::Value| levels.contains(&row["level"].as_str().unwrap());
        assert!(rows.iter().all(level), "{json}");
        let vtodo = rows
            .iter()
            .find(|row| row["name"] == io_pimdir::capability::CALENDAR_COMPONENT_VTODO)
            .unwrap();
        assert_eq!(vtodo["level"], "none", "{json}");
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
