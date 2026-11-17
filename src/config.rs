//! # Configuration
//!
//! The TOML schema: each account holds named [`SourceConfig`]s over one
//! pimdir store, plus that store's settings.
//!
//! The configuration is a plain model every build parses whole, the tables
//! of the backends it leaves out included. The features turn it into
//! runtime values (src/tls.rs, src/proxy.rs, src/sasl.rs, src/server.rs),
//! so a build missing a backend leaves part of the model unread.

#![cfg_attr(
    not(all(
        feature = "imap",
        feature = "smtp",
        feature = "carddav",
        feature = "caldav",
        feature = "msgraph",
        feature = "gpeople",
        feature = "gcal",
        feature = "gmail",
        feature = "search"
    )),
    allow(dead_code)
)]

/// Splices the per-source shared fields (`collection`, `flag`, `item`,
/// `proxy`) onto every protocol-specific config struct.
///
/// The old `mailbox` and `message` spellings of `collection` and `item`
/// are refused by name.
macro_rules! source_config {
    (
        $(#[$struct_meta:meta])*
        pub struct $Name:ident {
            $(
                $(#[$field_meta:meta])*
                pub $field_name:ident: $field_ty:ty,
            )*
        }
    ) => {
        $(#[$struct_meta])*
        pub struct $Name {
            $(
                $(#[$field_meta])*
                pub $field_name: $field_ty,
            )*
            /// Which collections this source syncs, and what may be created
            /// or deleted on it.
            #[serde(default, skip_serializing_if = "crate::config::is_default")]
            pub collection: $crate::config::permissions::CollectionSourceConfig,
            /// Whether flag updates may be pushed to this source.
            #[serde(default, skip_serializing_if = "crate::config::is_default")]
            pub flag: $crate::config::permissions::FlagSourcePermissions,
            /// Which item mutations may be pushed to this source.
            #[serde(default, skip_serializing_if = "crate::config::is_default")]
            pub item: $crate::config::permissions::ItemSourcePermissions,
            /// The proxy this source's connections go through.
            ///
            /// Falls back to the account's, then to the `all_proxy` and
            /// `https_proxy` environment variables.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub proxy: Option<$crate::config::proxy::ProxyConfig>,
        }
    };
}

pub mod calendar;
pub mod contact;
pub mod dav;
pub mod filter;
pub mod gmail;
pub mod google;
pub mod imap;
pub mod jmap;
pub mod mail;
pub mod mode;
pub mod msgraph;
pub mod permissions;
pub mod proxy;
pub mod sasl;
pub mod smtp;
pub mod source;
pub mod store;
pub mod tls;

use std::{
    collections::{BTreeSet, HashMap},
    path::PathBuf,
};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use io_pimdir::collection::PimdirScope;
use pimalaya_cli::error::CodedError;
use pimalaya_config::toml::TomlConfig;
use serde::{Deserialize, Serialize};

use crate::{
    config::{
        calendar::CalendarConfig,
        contact::ContactConfig,
        dav::{CaldavConfig, CarddavConfig},
        filter::resolve_since,
        gmail::GmailConfig,
        google::GoogleConfig,
        imap::ImapConfig,
        jmap::JmapConfig,
        mail::MailConfig,
        mode::AccountMode,
        msgraph::{MsgraphTable, msgraph_sugar},
        proxy::ProxyConfig,
        smtp::SmtpConfig,
        source::{SourceBackendConfig, SourceConfig},
        store::StoreConfig,
    },
    error::ErrorCode,
};

/// The documented sample configuration, which a missing configuration and
/// one holding no account point at.
const SAMPLE_URL: &str = "https://github.com/pimalaya/neverest/blob/master/config.sample.toml";

/// Every backend table an account may write, in the order `account list`
/// names them, `smtp` the send channel's.
pub const BACKENDS: [&str; 9] = [
    "imap", "carddav", "caldav", "jmap", "gmail", "msgraph", "gpeople", "gcal", "smtp",
];

/// Whether this build serves the backend table `name`, its cargo feature
/// being on. A JMAP source parses and no build opens it.
pub fn is_built(name: &str) -> bool {
    let built = [
        ("imap", cfg!(feature = "imap")),
        ("carddav", cfg!(feature = "carddav")),
        ("caldav", cfg!(feature = "caldav")),
        ("gmail", cfg!(feature = "gmail")),
        ("msgraph", cfg!(feature = "msgraph")),
        ("gpeople", cfg!(feature = "gpeople")),
        ("gcal", cfg!(feature = "gcal")),
        ("smtp", cfg!(feature = "smtp")),
    ];
    built.contains(&(name, true))
}

/// The whole configuration document: nothing but named accounts.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Config {
    /// The named accounts the document declares.
    pub accounts: HashMap<String, AccountConfig>,
}

impl TomlConfig for Config {
    type Account = AccountConfig;

    fn project_name() -> &'static str {
        env!("CARGO_PKG_NAME")
    }

    fn sample_url() -> &'static str {
        SAMPLE_URL
    }

    fn accounts(&self) -> impl Iterator<Item = (&str, bool)> {
        self.accounts.keys().map(|name| (name.as_str(), false))
    }

    fn take_named_account(&mut self, name: &str) -> Option<(String, Self::Account)> {
        self.accounts.remove_entry(name)
    }
}

impl Config {
    /// Loads `Config` from `config_paths`.
    ///
    /// Fails when none exists with [`ConfigMissingError`], naming the path
    /// and the sample to start from, which `main` shows as the welcome.
    ///
    /// [`ConfigMissingError`]: pimalaya_config::toml::ConfigMissingError
    pub fn load(config_paths: &[PathBuf]) -> Result<Config> {
        <Config as TomlConfig>::load(config_paths)
    }

    /// The accounts a command runs over, each with its configuration:
    /// those `names` names, else every one, in name order.
    ///
    /// A name the configuration does not declare fails before anything
    /// runs, and so does one holding `/`, which separates the account from
    /// the rest of a store-wide id (pimdir STORAGE §9.2).
    pub fn select(&self, names: &[String]) -> Result<Vec<(String, AccountConfig)>> {
        let mut configured: Vec<&String> = self.accounts.keys().collect();
        configured.sort();

        if configured.is_empty() {
            bail!("The configuration holds no account, start from the sample: {SAMPLE_URL}");
        }

        if let Some(unknown) = names.iter().find(|name| !self.accounts.contains_key(*name)) {
            let message = format!(
                "Account `{unknown}` not found, the configuration holds: {}",
                join(&configured),
            );
            return Err(CodedError::new(ErrorCode::AccountMissing, message).into());
        }

        if let Some(name) = configured.iter().find(|name| name.contains('/')) {
            bail!(
                "Account name {name} holds a `/`, which separates the account from the source \
                 in the store's ids: rename its `[accounts.<name>]` key"
            );
        }

        Ok(configured
            .into_iter()
            .filter(|name| names.is_empty() || names.contains(name))
            .map(|name| (name.clone(), self.accounts[name].clone()))
            .collect())
    }

    /// The one account a command addressing one item, or starting one
    /// account over, runs on: what `names` leaves must be exactly one.
    pub fn select_one(&self, names: &[String], what: &str) -> Result<(String, AccountConfig)> {
        let mut selected = self.select(names)?;

        if selected.len() != 1 {
            let names: Vec<&String> = selected.iter().map(|(name, _)| name).collect();
            bail!(
                "{what} acts on one account, and the run covers {}: name one with `-a`",
                join(&names),
            );
        }

        Ok(selected.remove(0))
    }
}

/// Names joined for a message, comma-separated.
fn join(names: &[&String]) -> String {
    names
        .iter()
        .map(|name| name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Per-account configuration: named sources over one pimdir store.
///
/// An account is one hub of a store it may share. A source's name makes its
/// pimdir source id, `<account>/<name>`, so renaming one orphans its
/// bindings; a backend written under the account is sugar named after it.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct AccountConfig {
    /// Named sources, the map key being the pimdir source id.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub sources: HashMap<String, SourceConfig>,
    /// Named targets, on the same terms as [`sources`](Self::sources).
    ///
    /// Absent means the local store is the destination. Named, not
    /// positional: a list would reassign every binding on a reorder, which
    /// is why `left` and `right` are gone and not worth reintroducing.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub targets: HashMap<String, SourceConfig>,
    /// Makes the `sources` side authoritative, the other's change discarded.
    ///
    /// Nothing is merged and no conflict is recorded. The other side is still
    /// enumerated every run, or every item would be re-pushed; its state
    /// decides what is left to do and never who wins.
    #[serde(default, skip_serializing_if = "is_default")]
    pub one_way: bool,
    /// Whether the store holds bodies, not spines and checkpoints alone.
    ///
    /// Unset takes the destination's answer: true with no targets, the store
    /// being what the account syncs into; false with targets, which asked to
    /// copy rather than to fill a disk. Set it to keep a copy of a migration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retain: Option<bool>,
    /// Direct-backend sugar: an IMAP source named after its protocol.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imap: Option<ImapConfig>,
    /// Direct-backend sugar: a CardDAV source named after its protocol.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carddav: Option<CarddavConfig>,
    /// Direct-backend sugar: a CalDAV source named after its protocol.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caldav: Option<CaldavConfig>,
    /// Direct-backend sugar: a JMAP source named after its protocol.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jmap: Option<JmapConfig>,
    /// Direct-backend sugar: a Gmail source named after its protocol.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gmail: Option<GmailConfig>,
    /// Direct-backend sugar: a Graph source per domain the table names.
    ///
    /// Each is named after the table and its domain (`msgraph-mail`,
    /// `msgraph-contact`, `msgraph-calendar`).
    #[serde(
        default,
        deserialize_with = "msgraph_sugar",
        skip_serializing_if = "Option::is_none"
    )]
    pub msgraph: Option<MsgraphTable>,
    /// Direct-backend sugar: a Google People source named after its
    /// protocol.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpeople: Option<GoogleConfig>,
    /// Direct-backend sugar: a Google Calendar source named after its
    /// protocol.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gcal: Option<GoogleConfig>,
    /// The send channel of the sugar source carrying mail, the flat
    /// spelling of `sources.<name>.smtp`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smtp: Option<SmtpConfig>,
    /// The proxy every endpoint of the account goes through unless its own
    /// table names one, the environment's when neither does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy: Option<ProxyConfig>,
    /// The local pimdir store this account syncs through.
    ///
    /// Optional: the store is implicit, the one every account naming no
    /// root shares, and is customised only here, never declared as a
    /// source.
    #[serde(default)]
    pub store: StoreConfig,
    /// What applies to the account's mail, whichever source holds it.
    #[serde(default)]
    pub mail: MailConfig,
    /// What applies to the account's contacts, whichever source holds them.
    #[serde(default)]
    pub contact: ContactConfig,
    /// What applies to the account's calendars, whichever source holds them.
    #[serde(default)]
    pub calendar: CalendarConfig,
    /// Max connections per source for concurrent body fetches, 4 by default.
    ///
    /// Keep it under the provider's per-account connection limit. `sync
    /// --connections N` overrides it for one run.
    #[serde(default)]
    pub connections: Option<usize>,
}

impl AccountConfig {
    /// Every configured source keyed by its id, the sugar folded into the
    /// explicit `sources` table.
    ///
    /// The sugar's source id is its protocol name, a Graph domain's the
    /// table and the domain, the same id the expanded form writes, so
    /// expanding an account by hand is a store no-op.
    pub fn sources(&self) -> Result<HashMap<String, SourceConfig>> {
        let mut sources = self.sources.clone();

        let sugar = [
            self.imap.clone().map(SourceBackendConfig::Imap),
            self.carddav.clone().map(SourceBackendConfig::Carddav),
            self.caldav.clone().map(SourceBackendConfig::Caldav),
            self.jmap.clone().map(SourceBackendConfig::Jmap),
            self.gmail.clone().map(SourceBackendConfig::Gmail),
            self.gpeople.clone().map(SourceBackendConfig::Gpeople),
            self.gcal.clone().map(SourceBackendConfig::Gcal),
        ];
        let graph = self.msgraph.iter().flat_map(MsgraphTable::domains);

        for backend in sugar.into_iter().flatten().chain(graph) {
            let name = backend.sugar_name().to_string();

            if sources.contains_key(&name) {
                bail!(
                    "Source {name} is declared both directly under the account and in the \
                     `sources` table; the direct form is sugar for `sources.{name}`, so keep one."
                );
            }

            sources.insert(name, SourceConfig::new(backend));
        }

        self.attach_send_channel(&mut sources)?;

        Ok(sources)
    }

    /// Hands the account `smtp` table to the one sugar source that could use
    /// it; a source in the explicit table carries its own.
    fn attach_send_channel(&self, sources: &mut HashMap<String, SourceConfig>) -> Result<()> {
        let Some(smtp) = &self.smtp else {
            return Ok(());
        };

        let mut candidates: Vec<_> = sources
            .iter()
            .filter(|(name, source)| {
                self.is_sugar(name) && source.carries_mail() && !source.sends_natively()
            })
            .map(|(name, _)| name.clone())
            .collect();
        candidates.sort();

        let [name] = candidates.as_slice() else {
            bail!(
                "The account-level `smtp` channel needs exactly one direct mail backend to \
                 complete, and this account has {}; move it under the source that sends, as \
                 `sources.<name>.smtp`.",
                candidates.len()
            );
        };

        sources
            .get_mut(name)
            .expect("candidate name comes from the map")
            .smtp = Some(smtp.clone());

        Ok(())
    }

    /// Whether that name came from the sugar rather than the explicit table.
    fn is_sugar(&self, name: &str) -> bool {
        !self.sources.contains_key(name)
    }

    /// The backend tables the account writes, each once in [`BACKENDS`]
    /// order, `smtp` for a send channel.
    ///
    /// Read off the tables alone, so an account a command would refuse lists
    /// too.
    pub fn backends(&self) -> Vec<&'static str> {
        let sugar = [
            self.imap.as_ref().map(|_| "imap"),
            self.carddav.as_ref().map(|_| "carddav"),
            self.caldav.as_ref().map(|_| "caldav"),
            self.jmap.as_ref().map(|_| "jmap"),
            self.gmail.as_ref().map(|_| "gmail"),
            self.msgraph.as_ref().map(|_| "msgraph"),
            self.gpeople.as_ref().map(|_| "gpeople"),
            self.gcal.as_ref().map(|_| "gcal"),
            self.smtp.as_ref().map(|_| "smtp"),
        ];
        let named = self.sources.values().chain(self.targets.values());
        let named = named.flat_map(|source| {
            let smtp = source.smtp.as_ref().map(|_| "smtp");
            [Some(source.backend.protocol()), smtp]
        });
        let declared: BTreeSet<&str> = sugar.into_iter().chain(named).flatten().collect();

        BACKENDS
            .into_iter()
            .filter(|name| declared.contains(name))
            .collect()
    }

    /// Every endpoint the account opens, keyed by its pimdir source id.
    ///
    /// A target is a source handle of the same store: it enumerates, holds
    /// bindings and is written to. Only direction separates the two, which
    /// is [`AccountMode`] and not the seam that opens them.
    pub fn endpoints(&self) -> Result<HashMap<String, SourceConfig>> {
        let mut endpoints = self.sources()?;
        endpoints.extend(self.targets.clone());
        Ok(endpoints)
    }

    /// This account as the store sees it: every endpoint keyed by its pimdir
    /// source id, `<account>/<name>`, the sugar folded into `sources`.
    ///
    /// Source ids are unique store-wide (pimdir STORAGE §4.3, §9.2), so two
    /// accounts holding the `imap` sugar must not both bind `imap`; a
    /// collection id derives from its source id, so it follows.
    pub fn namespaced(&self, account: &str) -> Result<Self> {
        let id = |(name, source): (String, SourceConfig)| (format!("{account}/{name}"), source);

        Ok(Self {
            sources: self.sources()?.into_iter().map(id).collect(),
            targets: self.targets.clone().into_iter().map(id).collect(),
            imap: None,
            carddav: None,
            caldav: None,
            jmap: None,
            gmail: None,
            msgraph: None,
            gpeople: None,
            gcal: None,
            smtp: None,
            ..self.clone()
        })
    }

    /// The account's mode: which endpoints, which way, whether bodies stay.
    ///
    /// Both `check` and the sync go through it, so what a run reports and
    /// what it does cannot drift apart. Every illegal arity is refused here,
    /// naming the cell reached and the nearest legal one.
    pub fn mode(&self) -> Result<AccountMode> {
        let sources = self.sources()?;
        let mut source_names: Vec<String> = sources.keys().cloned().collect();
        let mut target_names: Vec<String> = self.targets.keys().cloned().collect();
        source_names.sort();
        target_names.sort();

        match (source_names.len(), target_names.len(), self.one_way) {
            (0, _, _) => bail!(
                "This account declares no source. Write a backend directly under it \
                 (`imap.server = \"…\"`), or name one in its `sources` table."
            ),
            (_, 0, _) => {}
            (1, 1, _) => {}
            (1, _, true) => {}
            (1, n, false) => bail!(
                "One source and {n} targets is a one-way copy: add `one-way = true`. Without it \
                 each target would also write back, and propagating between {} endpoints has no \
                 resolution order for neverest to pick.",
                n + 1,
            ),
            (n, _, _) => bail!(
                "{n} sources and {} targets is not a shape neverest syncs. Either drop the \
                 targets, so every source syncs into the local store, or keep one source and \
                 copy it to the targets with `one-way = true`.",
                target_names.len(),
            ),
        }

        if target_names.is_empty() && self.retain == Some(false) {
            bail!(
                "`retain = false` with no target would sync to nowhere: the local store is this \
                 account's destination. Drop the key, or name the targets to copy to."
            );
        }

        let retain = self.retain.unwrap_or(target_names.is_empty());

        Ok(AccountMode {
            sources: source_names,
            targets: target_names,
            one_way: self.one_way,
            retain,
        })
    }

    /// Rejects an account a command cannot run: no source, a source
    /// declaring what its backend cannot honour, two senders.
    ///
    /// Run by every command that opens an account, so a bad configuration is
    /// refused before a connection rather than halfway through a sync, the
    /// mode's refusals included.
    pub fn validate(&self) -> Result<()> {
        let sources = self.sources()?;

        if sources.is_empty() {
            bail!(
                "This account declares no source. Write a backend directly under it \
                 (`imap.server = \"…\"`), or name one in its `sources` table."
            );
        }

        for (name, source) in sources.iter().chain(&self.targets) {
            source.validate(name)?;
        }

        if let Some(name) = sources.keys().find(|name| self.targets.contains_key(*name)) {
            bail!(
                "{name} is both a source and a target. A name is the pimdir source id every \
                 binding it owns is recorded under, so one name cannot be two endpoints; rename \
                 one of them."
            );
        }

        let mut sending_targets: Vec<_> = self
            .targets
            .iter()
            .filter(|(_, target)| target.smtp.is_some())
            .map(|(name, _)| name.clone())
            .collect();
        sending_targets.sort();
        if !sending_targets.is_empty() {
            bail!(
                "Target {} declares an `smtp` channel, which nothing uses: an account sends \
                 through a source. Move the table under the source that sends, or drop it.",
                sending_targets.join(", "),
            );
        }

        self.mode()?;

        if let Some(since) = &self.mail.filter.since {
            resolve_since(since, Utc::now()).context("Invalid `mail.filter.since`")?;
        }

        let mut senders: Vec<_> = sources
            .iter()
            .filter(|(_, source)| source.smtp.is_some())
            .map(|(name, _)| name.clone())
            .collect();
        senders.sort();

        if senders.len() > 1 {
            bail!(
                "Sources {} each declare an `smtp` channel, and an account sends through one; \
                 keep the table on the source that sends and drop the others.",
                senders.join(", "),
            );
        }

        Ok(())
    }

    /// The scope a run syncs mail under (pimdir SYNC §5): from `since` (the
    /// run's `--since`), else `mail.filter.since`, else unbounded.
    ///
    /// A scope bounds mail by its `Date` and nothing else, so one is refused
    /// by the key it came from when an endpoint the run syncs (`only`'s
    /// sources, or all, and the targets) holds contacts or calendars.
    pub fn scope(
        &self,
        since: Option<&str>,
        only: &[String],
        now: DateTime<Utc>,
    ) -> Result<PimdirScope> {
        let (raw, key) = match since {
            Some(raw) => (raw, "--since"),
            None => match &self.mail.filter.since {
                Some(raw) => (raw.as_str(), "mail.filter.since"),
                None => return Ok(PimdirScope::unbounded()),
            },
        };
        let since = resolve_since(raw, now).with_context(|| format!("Invalid `{key}`"))?;

        let mut unscoped: Vec<String> = self
            .sources()?
            .into_iter()
            .filter(|(name, _)| only.is_empty() || only.contains(name))
            .chain(self.targets.clone())
            .filter(|(_, source)| !source.carries_mail())
            .map(|(name, _)| name)
            .collect();
        unscoped.sort();
        if !unscoped.is_empty() {
            bail!(
                "`{key}` bounds mail by its date, and {} sync(s) contacts or calendars, which \
                 take no scope: drop `{key}`, narrow the run to the mail sources with \
                 `--source`, or keep these in an account of their own.",
                unscoped.join(", "),
            );
        }

        Ok(PimdirScope::since(since))
    }
}

/// `skip_serializing_if` predicate omitting a defaulted field, so a serialized
/// configuration carries only what differs from the defaults.
fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

/// [`is_default`] for the HTTP ALPN list, whose default is not empty.
fn is_default_http_alpn(alpn: &[String]) -> bool {
    alpn == default_http_alpn().as_slice()
}

/// Default ALPN list for the HTTP backends: their APIs ride on HTTP/1.1.
fn default_http_alpn() -> Vec<String> {
    vec![String::from("http/1.1")]
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs};

    use crate::config::{dav::DavAuthConfig, tls::TlsConfig, *};

    /// Two accounts, `home` declared after `work`.
    fn two_accounts() -> Config {
        toml::from_str(
            r#"
            [accounts.work]
            imap.server = "imaps://w.example.org:993"

            [accounts.home]
            imap.server = "imaps://h.example.org:993"
            "#,
        )
        .unwrap()
    }

    fn names(selected: &[(String, AccountConfig)]) -> Vec<&str> {
        selected.iter().map(|(name, _)| name.as_str()).collect()
    }

    /// With no `-a`, a command runs over every account in name order; `-a`
    /// filters them.
    #[test]
    fn every_account_runs_unless_named() {
        let config = two_accounts();

        assert_eq!(names(&config.select(&[]).unwrap()), ["home", "work"]);
        let work = config.select(&[String::from("work")]).unwrap();
        assert_eq!(names(&work), ["work"]);
        let both = [String::from("work"), String::from("home")];
        assert_eq!(names(&config.select(&both).unwrap()), ["home", "work"]);
    }

    /// A name the configuration does not declare fails before anything
    /// runs, naming the ones it does.
    #[test]
    fn an_unknown_account_names_the_configured_ones() {
        let err = two_accounts()
            .select(&[String::from("perso")])
            .unwrap_err()
            .to_string();

        assert!(err.contains("perso"), "{err}");
        assert!(err.contains("home, work"), "{err}");
    }

    /// A command addressing one item, or starting one account over, needs
    /// the filter to leave exactly one account.
    #[test]
    fn one_account_is_needed_where_one_is_acted_on() {
        let config = two_accounts();

        let err = config.select_one(&[], "A reset").unwrap_err().to_string();
        assert!(err.contains("A reset acts on one account"), "{err}");
        assert!(err.contains("home, work"), "{err}");

        let (name, _) = config
            .select_one(&[String::from("home")], "A reset")
            .unwrap();
        assert_eq!(name, "home");
    }

    /// The account prefixes every store-wide id, `/` separating it from
    /// the source, so a name holding one is refused.
    #[test]
    fn an_account_name_holding_a_slash_is_refused() {
        let config: Config = toml::from_str(
            r#"
            [accounts."me/work"]
            imap.server = "imaps://w.example.org:993"
            "#,
        )
        .unwrap();

        let err = config.select(&[]).unwrap_err().to_string();
        assert!(err.contains("me/work"), "{err}");
    }

    /// The store sees an account's endpoints by their source ids,
    /// `<account>/<name>`, the sugar and its send channel folded in.
    #[test]
    fn a_namespaced_account_keys_its_endpoints_by_source_id() {
        let account: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://imap.example.org:993"
            smtp.server = "smtps://smtp.example.org:465"
            targets.backup.imap.server = "imaps://backup.example.org:993"
            one-way = true
            "#,
        )
        .unwrap();

        let namespaced = account.namespaced("work").unwrap();

        let mode = namespaced.mode().unwrap();
        assert_eq!(mode.sources, ["work/imap"]);
        assert_eq!(mode.targets, ["work/backup"]);
        assert!(namespaced.imap.is_none());
        let sources = namespaced.sources().unwrap();
        assert!(
            sources["work/imap"].smtp.is_some(),
            "the send channel follows"
        );
    }

    /// Every Graph endpoint by source id, with its domain and whether it
    /// deletes items, sorted.
    fn graph_endpoints(document: &str) -> Vec<(String, &'static str, bool)> {
        let account: AccountConfig = toml::from_str(document).unwrap();
        let mut endpoints: Vec<_> = account
            .namespaced("a")
            .unwrap()
            .endpoints()
            .unwrap()
            .into_iter()
            .map(|(id, source)| {
                let domain = match source.backend {
                    SourceBackendConfig::Msgraph(_) => "mail",
                    SourceBackendConfig::MsgraphContacts(_) => "contact",
                    SourceBackendConfig::MsgraphCalendar(_) => "calendar",
                    _ => "other",
                };
                (id, domain, source.permissions().item.delete)
            })
            .collect();
        endpoints.sort();
        endpoints
    }

    /// The Graph sugar names a source after the table and its domain, and a
    /// named source keeps its own name whatever domain it syncs.
    #[test]
    fn a_graph_source_is_named_after_its_domain() {
        let id = |id: &str, domain, delete| (String::from(id), domain, delete);

        let sugar = graph_endpoints(
            r#"
            msgraph.auth.token.raw = "tok"
            msgraph.mail = true
            msgraph.contact.item.create = true
            msgraph.contact.item.delete = false
            msgraph.calendar = true
            "#,
        );
        assert_eq!(
            sugar,
            [
                id("a/msgraph-calendar", "calendar", true),
                id("a/msgraph-contact", "contact", false),
                id("a/msgraph-mail", "mail", true),
            ]
        );

        let named = graph_endpoints(
            r#"
            sources.outlook.msgraph.auth.token.raw = "tok"
            sources.outlook.msgraph.mail = true
            sources.people.msgraph.auth.token.raw = "tok"
            sources.people.msgraph.contact = true
            sources.agenda.msgraph.auth.token.raw = "tok"
            sources.agenda.msgraph.calendar.item.create = true
            sources.agenda.msgraph.calendar.item.delete = false
            "#,
        );
        assert_eq!(
            named,
            [
                id("a/agenda", "calendar", false),
                id("a/outlook", "mail", true),
                id("a/people", "contact", true),
            ]
        );

        let copied = graph_endpoints(
            r#"
            one-way = true
            sources.people.msgraph.auth.token.raw = "tok"
            sources.people.msgraph.contact = true
            targets.copy.msgraph.auth.token.raw = "other"
            targets.copy.msgraph.contact = true
            "#,
        );
        assert_eq!(
            copied,
            [
                id("a/copy", "contact", true),
                id("a/people", "contact", true),
            ]
        );
    }

    /// The store cannot tell the two spellings apart, so expanding an
    /// account by hand never orphans a binding.
    #[test]
    fn the_direct_backend_sugar_expands_to_the_same_source() {
        let sugar: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://imap.example.org:993"
            imap.item.create = true
            imap.item.delete = false
            "#,
        )
        .unwrap();

        let explicit: AccountConfig = toml::from_str(
            r#"
            sources.imap.imap.server = "imaps://imap.example.org:993"
            sources.imap.imap.item.create = true
            sources.imap.imap.item.delete = false
            "#,
        )
        .unwrap();

        let sugar = sugar.sources().unwrap();
        let explicit = explicit.sources().unwrap();

        assert_eq!(sugar.keys().collect::<Vec<_>>(), vec!["imap"]);
        assert_eq!(explicit.keys().collect::<Vec<_>>(), vec!["imap"]);
        assert!(!sugar["imap"].permissions().item.delete);
        assert!(!explicit["imap"].permissions().item.delete);
    }

    #[test]
    fn a_protocol_declared_both_ways_is_refused() {
        let account: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://imap.example.org:993"
            sources.imap.imap.server = "imaps://other.example.org:993"
            "#,
        )
        .unwrap();

        let err = account.sources().unwrap_err().to_string();
        assert!(err.contains("declared both"), "got {err}");
    }

    #[test]
    fn several_sources_of_one_protocol_live_under_one_account() {
        let account: AccountConfig = toml::from_str(
            r#"
            sources.fastmail.imap.server = "imaps://imap.fastmail.com:993"
            sources.gmail.imap.server = "imaps://imap.gmail.com:993"
            sources.dav.carddav.server = "https://carddav.fastmail.com/"
            sources.dav.carddav.auth.basic.username = "user"
            sources.dav.carddav.auth.basic.password.raw = "pw"
            "#,
        )
        .unwrap();

        account.validate().unwrap();
        let sources = account.sources().unwrap();
        assert_eq!(sources.len(), 3);
    }

    /// Mail and contacts under one account is the case the whole change exists
    /// for: their kinds differ, so nothing forces them to agree.
    #[test]
    fn mail_and_contacts_sit_under_one_account() {
        let account: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://imap.fastmail.com:993"
            carddav.server = "https://carddav.fastmail.com/"
            carddav.auth.basic.username = "user"
            carddav.auth.basic.password.raw = "pw"
            smtp.server = "smtps://smtp.fastmail.com:465"
            "#,
        )
        .unwrap();

        account.validate().unwrap();
        let sources = account.sources().unwrap();

        assert!(sources["imap"].is_imap());
        assert!(sources["imap"].smtp.is_some(), "the channel completes mail");
        assert!(!sources["carddav"].carries_mail());
        assert!(sources["carddav"].smtp.is_none());
    }

    /// The legal cells of the matrix, and what each resolves `retain` to.
    #[test]
    fn the_mode_is_the_arity_and_the_two_flags() {
        let local: AccountConfig = toml::from_str(
            r#"
            sources.fastmail.imap.server = "imaps://imap.fastmail.com:993"
            sources.gmail.imap.server = "imaps://imap.gmail.com:993"
            "#,
        )
        .unwrap();
        let mode = local.mode().unwrap();
        assert!(mode.is_local());
        assert!(!mode.one_way);
        assert!(mode.retain, "the store is the destination");

        let mirror: AccountConfig = toml::from_str(
            r#"
            sources.a.imap.server = "imaps://a.example.org:993"
            targets.b.imap.server = "imaps://b.example.org:993"
            "#,
        )
        .unwrap();
        let mode = mirror.mode().unwrap();
        assert!(!mode.is_local());
        assert!(!mode.one_way, "two-way remote to remote is the default");
        assert!(!mode.retain, "a named target asked to copy, not to store");

        let copy: AccountConfig = toml::from_str(
            r#"
            one-way = true
            retain = true
            sources.a.imap.server = "imaps://a.example.org:993"
            targets.b.imap.server = "imaps://b.example.org:993"
            targets.c.imap.server = "imaps://c.example.org:993"
            "#,
        )
        .unwrap();
        let mode = copy.mode().unwrap();
        assert_eq!(mode.targets, vec!["b", "c"]);
        assert!(mode.one_way);
        assert!(mode.retain, "migrating while keeping a local copy");
    }

    /// Several targets have no resolution order without an authority, so the
    /// cell is refused rather than syncing them pairwise in map order.
    #[test]
    fn many_targets_without_one_way_are_refused() {
        let account: AccountConfig = toml::from_str(
            r#"
            sources.a.imap.server = "imaps://a.example.org:993"
            targets.b.imap.server = "imaps://b.example.org:993"
            targets.c.imap.server = "imaps://c.example.org:993"
            "#,
        )
        .unwrap();

        let err = account.mode().unwrap_err().to_string();
        assert!(err.contains("one-way = true"), "got {err}");
    }

    /// Several sources are the local case, so naming a target alongside them
    /// is a cell with no meaning rather than a fan-in.
    #[test]
    fn many_sources_with_a_target_are_refused() {
        let account: AccountConfig = toml::from_str(
            r#"
            sources.a.imap.server = "imaps://a.example.org:993"
            sources.b.imap.server = "imaps://b.example.org:993"
            targets.c.imap.server = "imaps://c.example.org:993"
            "#,
        )
        .unwrap();

        let err = account.mode().unwrap_err().to_string();
        assert!(err.contains("not a shape neverest syncs"), "got {err}");
    }

    /// With no target the store is the destination, so refusing to keep
    /// bodies would sync to nowhere.
    #[test]
    fn refusing_to_retain_with_no_target_is_refused() {
        let account: AccountConfig = toml::from_str(
            r#"
            retain = false
            imap.server = "imaps://imap.example.org:993"
            "#,
        )
        .unwrap();

        let err = account.mode().unwrap_err().to_string();
        assert!(err.contains("sync to nowhere"), "got {err}");
    }

    #[test]
    fn an_account_with_no_source_is_refused() {
        let account: AccountConfig = toml::from_str("").unwrap();
        let err = account.validate().unwrap_err().to_string();
        assert!(err.contains("no source"), "got {err}");
    }

    #[test]
    fn one_source_at_most_carries_the_send_channel() {
        let account: AccountConfig = toml::from_str(
            r#"
            sources.a.imap.server = "imaps://a.example.org:993"
            sources.a.smtp.server = "smtps://a.example.org:465"
            sources.b.imap.server = "imaps://b.example.org:993"
            sources.b.smtp.server = "smtps://b.example.org:465"
            "#,
        )
        .unwrap();

        let err = account.validate().unwrap_err().to_string();
        assert!(err.contains("a, b"), "got {err}");

        let account: AccountConfig = toml::from_str(
            r#"
            sources.a.imap.server = "imaps://a.example.org:993"
            sources.a.smtp.server = "smtps://a.example.org:465"
            sources.b.imap.server = "imaps://b.example.org:993"
            "#,
        )
        .unwrap();
        account.validate().unwrap();
    }

    /// The flat `smtp` table completes the one direct mail backend. With none
    /// or several it names nothing, and guessing would be worse than refusing.
    #[test]
    fn the_flat_send_channel_needs_one_direct_mail_backend() {
        let account: AccountConfig = toml::from_str(
            r#"
            carddav.server = "https://dav.example.org/"
            carddav.auth.bearer.token.raw = "tok"
            smtp.server = "smtps://smtp.example.org:465"
            "#,
        )
        .unwrap();
        let err = account.validate().unwrap_err().to_string();
        assert!(err.contains("exactly one direct mail backend"), "got {err}");

        let account: AccountConfig = toml::from_str(
            r#"
            sources.a.imap.server = "imaps://a.example.org:993"
            smtp.server = "smtps://a.example.org:465"
            "#,
        )
        .unwrap();
        let err = account.validate().unwrap_err().to_string();
        assert!(err.contains("exactly one direct mail backend"), "got {err}");
    }

    /// A target's `smtp` is refused: an account sends through a source, so
    /// the table loaded, its server was checked, and nothing ever sent.
    #[test]
    fn a_target_declares_no_send_channel() {
        let account: AccountConfig = toml::from_str(
            r#"
            sources.src.imap.server = "imaps://a.example.org:993"
            targets.dst.imap.server = "imaps://b.example.org:993"
            targets.dst.smtp.server = "smtps://b.example.org:465"
            "#,
        )
        .unwrap();
        let err = account.validate().unwrap_err().to_string();
        assert!(
            err.contains("Target dst declares an `smtp` channel"),
            "got {err}"
        );
    }

    /// Every path key expands once, at deserialize, so no call site can read
    /// a literal `./~/…` directory.
    ///
    /// A re-serialized document carries the expansion and reloads the same;
    /// an absent key never reaches the deserializer, so stays absent.
    #[test]
    fn a_tilde_path_is_expanded_at_deserialize() {
        let home = dirs::home_dir().unwrap();

        let store: StoreConfig = toml::from_str(r#"root = "~/store""#).unwrap();
        assert_eq!(store.root, Some(home.join("store")));

        let tls: TlsConfig = toml::from_str(r#"cert = "~/ca.pem""#).unwrap();
        assert_eq!(tls.cert, Some(home.join("ca.pem")));

        let reloaded: TlsConfig = toml::from_str(&toml::to_string(&tls).unwrap()).unwrap();
        assert_eq!(reloaded.cert, tls.cert);

        let tls: TlsConfig = toml::from_str("").unwrap();
        assert_eq!(tls.cert, None);
        assert!(tls.client.is_none());

        let tls: TlsConfig =
            toml::from_str(r#"client = { cert = "~/me.pem", key = "~/me.key.pem" }"#).unwrap();
        let client = tls.client.unwrap();
        assert_eq!(client.cert, home.join("me.pem"));
        assert_eq!(client.key, home.join("me.key.pem"));
        assert!(toml::from_str::<TlsConfig>(r#"client.cert = "~/me.pem""#).is_err());
    }

    /// A path key expands environment variables too, as every other
    /// Pimalaya path does through pimalaya-config.
    #[test]
    fn a_variable_in_a_path_is_expanded_at_deserialize() {
        let home = PathBuf::from(std::env::var("HOME").unwrap());

        let store: StoreConfig = toml::from_str(r#"root = "$HOME/store""#).unwrap();
        assert_eq!(store.root, Some(home.join("store")));

        let tls: TlsConfig = toml::from_str(r#"cert = "${HOME}/ca.pem""#).unwrap();
        assert_eq!(tls.cert, Some(home.join("ca.pem")));
    }

    /// The keys the sibling CLIs read and neverest did not load.
    ///
    /// A DAV block naming no `auth` loads as `auth = "none"`, anonymous DAV
    /// as the family reads it (a Radicale with no auth, a server behind a
    /// proxy that authenticates).
    #[test]
    fn a_dav_block_without_auth_is_anonymous() {
        let account: AccountConfig =
            toml::from_str(r#"carddav.home = "https://dav.example.org/card/user/""#).unwrap();
        account.validate().unwrap();
        let source = account.sources().unwrap().remove("carddav").unwrap();
        let SourceBackendConfig::Carddav(dav) = source.backend else {
            panic!("a CardDAV source");
        };
        assert!(matches!(dav.auth, dav::DavAuthConfig::None));
    }

    /// A DAV home set with no server and no credential, the IMAP `SASL-IR`
    /// and `ID` quirks, and a proxy at the account, a backend and a channel.
    #[test]
    fn the_family_connection_keys_load() {
        let account: AccountConfig = toml::from_str(
            r#"
            proxy.url = "http://proxy.example.org:3128"
            carddav.home = "https://dav.example.org/card/user/"
            carddav.auth = "none"
            sources.cal.caldav.home = "https://dav.example.org/cal/user/"
            sources.cal.caldav.auth.basic.username = "user"
            sources.cal.caldav.auth.basic.password.raw = "pw"
            sources.cal.caldav.proxy.url = "socks5h://127.0.0.1:1080"
            sources.cal.caldav.proxy.username = "user"
            sources.cal.caldav.proxy.password.raw = "pw"
            sources.mail.imap.server = "imap.example.org"
            sources.mail.imap.sasl-ir = false
            sources.mail.imap.id.auto = true
            sources.mail.imap.id.fields.name = true
            sources.mail.imap.id.fields.os = false
            sources.mail.smtp.server = "smtp.example.org"
            sources.mail.smtp.proxy.url = "http://smtp-proxy.example.org:3128"
            "#,
        )
        .unwrap();
        let sources = account.sources().unwrap();

        assert_eq!(account.proxy.unwrap().url, "http://proxy.example.org:3128");
        let SourceBackendConfig::Carddav(card) = &sources["carddav"].backend else {
            panic!("a CardDAV source");
        };
        assert_eq!(card.server, None);
        assert_eq!(
            card.home.as_deref(),
            Some("https://dav.example.org/card/user/")
        );
        assert!(matches!(card.auth, DavAuthConfig::None));
        assert!(card.proxy.is_none());

        let proxy = sources["cal"].backend.proxy().unwrap();
        assert_eq!(proxy.url, "socks5h://127.0.0.1:1080");
        assert_eq!(proxy.username.as_deref(), Some("user"));

        let SourceBackendConfig::Imap(imap) = &sources["mail"].backend else {
            panic!("an IMAP source");
        };
        assert_eq!(imap.sasl_ir, Some(false));
        assert!(imap.id.auto);
        let fields = BTreeMap::from([(String::from("name"), true), (String::from("os"), false)]);
        assert_eq!(imap.id.fields, fields);
        let smtp = sources["mail"].smtp.as_ref().unwrap();
        assert_eq!(
            smtp.proxy.as_ref().unwrap().url,
            "http://smtp-proxy.example.org:3128"
        );
    }

    /// The DAV sources are the non-mail ones, so they are where the account
    /// shape stops being mail-shaped.
    #[cfg(any(feature = "carddav", feature = "caldav"))]
    #[test]
    fn google_sources_are_neither_mail_nor_a_send_channel() {
        let account: AccountConfig = toml::from_str(
            r#"
            gpeople.auth.token.raw = "tok"
            gcal.auth.token.raw = "tok"
            "#,
        )
        .unwrap();

        let sources = account.sources().unwrap();
        for name in ["gpeople", "gcal"] {
            assert!(!sources[name].carries_mail(), "{name} does not submit");
            assert!(!sources[name].sends_natively(), "{name} does not send");
            assert!(sources[name].is_http());
        }
        account.validate().unwrap();
    }

    /// A name is the pimdir source id its bindings are recorded under, so one
    /// name cannot be two endpoints.
    #[test]
    fn a_name_used_twice_is_refused() {
        let account: AccountConfig = toml::from_str(
            r#"
            sources.a.imap.server = "imaps://a.example.org:993"
            targets.a.imap.server = "imaps://b.example.org:993"
            "#,
        )
        .unwrap();

        let err = account.validate().unwrap_err().to_string();
        assert!(err.contains("both a source and a target"), "got {err}");
    }

    /// A key nothing reads is a typo, refused at load wherever it sits.
    #[test]
    fn an_unknown_key_is_refused_at_every_level() {
        let mut missed = Vec::new();

        for (document, key) in [
            (
                "root-typo = 1\n[accounts.a]\nimap.server = \"imap.example.org\"",
                "root-typo",
            ),
            (
                "[accounts.a]\nimap.server = \"imap.example.org\"\nsorces.b.imap.server = \"x\"",
                "sorces",
            ),
            (
                "[accounts.a]\nsources.b.imap.server = \"x\"\nsources.b.source-typo = 1",
                "source-typo",
            ),
            (
                "[accounts.a]\nsources.b.imap.server = \"x\"\ntargets.c.imap.server = \"y\"\n\
                 targets.c.target-typo = 1",
                "target-typo",
            ),
            (
                "[accounts.a]\nsources.b.imap.server = \"x\"\nsources.b.imap.imap-typo = 1",
                "imap-typo",
            ),
            (
                "[accounts.a]\nsources.b.imap.server = \"x\"\nsources.b.smtp.server = \"y\"\n\
                 sources.b.smtp.smtp-typo = 1",
                "smtp-typo",
            ),
            (
                "[accounts.a]\nimap.server = \"x\"\nimap.tls.rustls.rustls-typo = 1",
                "rustls-typo",
            ),
            (
                "[accounts.a]\nimap.server = \"x\"\nimap.collection.collection-typo = 1",
                "collection-typo",
            ),
            (
                "[accounts.a]\nimap.server = \"x\"\nstore.store-typo = 1",
                "store-typo",
            ),
        ] {
            let err = toml::from_str::<Config>(document)
                .map(|_| String::from("loaded"))
                .unwrap_or_else(|err| err.to_string());
            if !err.contains(&format!("unknown field `{key}`")) {
                missed.push(format!("{key}: {err}"));
            }
        }

        assert!(missed.is_empty(), "{missed:#?}");
    }

    /// A key an earlier neverest read, `default` included, is refused as
    /// unknown at whatever level it sits.
    ///
    /// Refused as the file parses, so an account no command opens is too.
    #[test]
    fn a_key_this_version_does_not_read_is_refused() {
        let mut missed = Vec::new();

        for (document, named) in [
            (
                "default = true\nimap.server = \"x\"",
                "unknown field `default`",
            ),
            (
                "left.imap.server = \"x\"\nright.imap.server = \"y\"",
                "unknown field `left`",
            ),
            (
                "imap.server = \"x\"\nfolder.filter = \"all\"",
                "unknown field `folder`",
            ),
            (
                "imap.server = \"x\"\nenvelope.filter.before = \"2024-01-01\"",
                "unknown field `envelope`",
            ),
            (
                "imap.server = \"x\"\ncollection.filter.include = [\"INBOX\"]",
                "unknown field `collection`",
            ),
            (
                "imap.server = \"x\"\nimap.collection.namespace = \"mail\"",
                "unknown field `namespace`",
            ),
            (
                "imap.server = \"x\"\nstore.retention = \"retain\"",
                "unknown field `retention`",
            ),
            (
                "imap.server = \"x\"\nstore.hydration = \"full\"",
                "unknown field `hydration`",
            ),
            (
                "imap.server = \"x\"\nsmtp.server = \"y\"\nsmtp.login = \"u\"\n\
                 smtp.password.raw = \"pw\"",
                "unknown field `login`",
            ),
            (
                "sources.a.imap.server = \"x\"\nsources.a.smtp.server = \"y\"\n\
                 sources.a.smtp.password.raw = \"pw\"",
                "unknown field `password`",
            ),
            (
                "imap.server = \"x\"\nmailbox.filter.include = [\"INBOX\"]",
                "unknown field `mailbox`",
            ),
            (
                "imap.server = \"x\"\nmessage.filter.since = \"30d\"",
                "unknown field `message`",
            ),
            (
                "imap.server = \"x\"\nimap.mailbox.create = false",
                "unknown field `mailbox`",
            ),
            (
                "sources.a.carddav.server = \"x\"\nsources.a.carddav.mailbox.delete = false",
                "unknown field `mailbox`",
            ),
            (
                "imap.server = \"x\"\nimap.message.delete = false",
                "unknown field `message`",
            ),
            (
                "sources.a.imap.server = \"x\"\ntargets.b.imap.server = \"y\"\n\
                 targets.b.imap.message.create = true",
                "unknown field `message`",
            ),
            (
                "imap.server = \"x\"\nimap.collection.filters = \"all\"",
                "unknown field `filters`",
            ),
            (
                "imap.server = \"x\"\nimap.collection.filters.include = [\"INBOX\"]",
                "unknown field `filters`",
            ),
            (
                "msgraph-contacts.auth.token.raw = \"tok\"",
                "unknown field `msgraph-contacts`",
            ),
            (
                "msgraph-calendar.auth.token.raw = \"tok\"",
                "unknown field `msgraph-calendar`",
            ),
            (
                "sources.people.msgraph-contacts.auth.token.raw = \"tok\"",
                "unknown field `msgraph-contacts`",
            ),
            (
                "sources.a.imap.server = \"x\"\ntargets.copy.msgraph-calendar.user-id = \"me\"",
                "unknown field `msgraph-calendar`",
            ),
            (
                "msgraph.auth.token.raw = \"tok\"\nmsgraph.mail = true\n\
                 msgraph.collection.filter.include = [\"Inbox\"]",
                "unknown field `collection`",
            ),
            (
                "msgraph.auth.token.raw = \"tok\"\nmsgraph.mail = true\nmsgraph.item.delete = false\n\
                 msgraph.item.create = true",
                "unknown field `item`",
            ),
            (
                "msgraph.auth.token.raw = \"tok\"\nmsgraph.mail = true\nmsgraph.flag.update = false",
                "unknown field `flag`",
            ),
            (
                "sources.a.msgraph.auth.token.raw = \"tok\"\nsources.a.msgraph.mail = true\n\
                 sources.a.msgraph.pool-size = 2",
                "unknown field `pool-size`",
            ),
            (
                "imap.server = \"imap.example.org\"\nimap.pool-size = 2",
                "unknown field `pool-size`",
            ),
            (
                "msgraph.auth.token.raw = \"tok\"\nmsgraph.mail.mailbox.create = false",
                "unknown field `mailbox`",
            ),
        ] {
            let err = match toml::from_str::<AccountConfig>(document) {
                Ok(_) => String::from("parsed"),
                Err(err) => err.to_string(),
            };
            if !err.contains(named) {
                missed.push(format!("{named}: {err}"));
            }
        }

        assert!(missed.is_empty(), "{missed:#?}");
    }

    /// Every key the sample documents, commented out or not, is one the
    /// configuration reads: none is refused as unknown.
    #[test]
    fn every_key_the_sample_documents_is_known() {
        let raw = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/config.sample.toml"))
            .expect("read the sample");
        let mut keys = 0;

        for line in raw.lines() {
            let code = line.trim_start_matches('#').trim_start();
            let Some((key, _)) = code.split_once(" = ") else {
                continue;
            };
            let is_key = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || "-.".contains(c);
            if key.is_empty() || !key.chars().all(is_key) {
                continue;
            }

            keys += 1;
            let document = format!("[accounts.example]\n{code}");
            if let Err(err) = toml::from_str::<Config>(&document) {
                let err = err.to_string();
                assert!(!err.contains("unknown"), "{code}\n{err}");
            }
        }

        assert!(keys > 50, "{keys} keys read from the sample");
    }

    /// Checks config.sample.toml: one `[accounts.example]` table and no
    /// other, parsed as written.
    ///
    /// Then once per rank, every line uncommented and set to that rank's
    /// value of each field.
    #[test]
    fn the_sample_configuration_parses_with_each_variant() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/config.sample.toml");
        let sample = fs::read_to_string(path).unwrap();
        let headers: Vec<_> = sample.lines().filter(|l| l.starts_with('[')).collect();
        assert_eq!(headers, ["[accounts.example]"]);

        load_sample(&sample, "as written");

        let paragraphs = sample_paragraphs(&sample);

        for rank in 1..=sample_ranks(&paragraphs) {
            let text = sample_variant(&paragraphs, rank);
            load_sample(&text, &format!("variant {rank}"));
        }
    }

    /// One line of config.sample.toml.
    struct SampleLine<'a> {
        /// The line as written.
        line: &'a str,
        /// The code before the comment, one `#` off when commented.
        code: &'a str,
        /// Whether the code is commented.
        commented: bool,
        /// The field the code sets, a secret's `raw` and `command` being
        /// one.
        field: Option<&'a str>,
        /// The other values the comment lists in brackets.
        values: Vec<&'a str>,
    }

    impl<'a> SampleLine<'a> {
        fn parse(line: &'a str) -> Self {
            let (code, comment) = line.split_once('\t').unwrap_or((line, ""));
            let commented = code.starts_with('#');
            let code = code.strip_prefix('#').unwrap_or(code);

            let field = code
                .split_once(" = ")
                .map(|(key, _)| key)
                .filter(|key| {
                    key.chars()
                        .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
                })
                .map(|key| {
                    let secret = key.strip_suffix(".raw").or(key.strip_suffix(".command"));
                    secret.unwrap_or(key)
                });

            let values = comment
                .trim_end()
                .strip_suffix(']')
                .and_then(|comment| comment.rsplit_once('['))
                .map(|(_, values)| values.split(", ").collect())
                .unwrap_or_default();

            Self {
                line,
                code,
                commented,
                field,
                values,
            }
        }

        /// The first two segments of the field, the table a replacing
        /// paragraph shares with the one above it (`imap.sasl`).
        fn table(&self) -> Option<&'a str> {
            let field = self.field?;
            let end = field
                .match_indices('.')
                .nth(1)
                .map_or(field.len(), |(i, _)| i);
            Some(&field[..end])
        }

        /// The values the line offers: its code as written, then each
        /// bracketed value in its place, typed like the written one.
        fn options(&self) -> Vec<String> {
            let (key, value) = self.code.split_once(" = ").unwrap();
            let mut options = vec![self.code.to_owned()];

            for other in &self.values {
                if value.starts_with('"') {
                    options.push(format!("{key} = \"{other}\""));
                } else {
                    options.push(format!("{key} = {other}"));
                }
            }

            options
        }
    }

    fn load_sample(text: &str, name: &str) {
        let config: Config = toml::from_str(text)
            .unwrap_or_else(|err| panic!("config.sample.toml, {name}: {err}\n{text}"));
        let names: Vec<String> = config.accounts.into_keys().collect();
        assert_eq!(names, ["example"], "config.sample.toml, {name}");
    }

    fn sample_paragraphs(sample: &str) -> Vec<Vec<SampleLine<'_>>> {
        let paragraphs = sample.split("\n\n");
        let paragraphs = paragraphs.map(|p| p.lines().map(SampleLine::parse).collect());
        paragraphs.collect()
    }

    /// Whether `below`, all commented, replaces `above`, setting keys under
    /// a table one of its lines sets.
    fn sample_replaces(above: &[SampleLine], below: &[SampleLine]) -> bool {
        let mut code = below.iter().filter(|line| line.field.is_some());
        let Some(first) = code.next() else {
            return false;
        };

        if !first.commented || code.any(|line| !line.commented) {
            return false;
        }

        above
            .iter()
            .filter_map(SampleLine::table)
            .any(|t| Some(t) == first.table())
    }

    /// The values each field takes in turn, the lines setting it in their
    /// order, each followed by its bracketed values.
    fn sample_options<'a>(lines: &[&'a SampleLine<'a>]) -> HashMap<&'a str, Vec<String>> {
        let mut options: HashMap<&str, Vec<String>> = HashMap::new();

        for line in lines {
            if let Some(field) = line.field {
                options.entry(field).or_default().extend(line.options());
            }
        }

        options
    }

    /// How many variants give every value its turn: the most values of
    /// one field, two when a paragraph replaces another.
    fn sample_ranks(paragraphs: &[Vec<SampleLine>]) -> usize {
        let lines: Vec<&SampleLine> = paragraphs.iter().flatten().collect();
        let options = sample_options(&lines);
        let values = options.values().map(Vec::len).max().unwrap_or(0);

        let replaces = paragraphs.windows(2).any(|w| sample_replaces(&w[0], &w[1]));

        if replaces { values.max(2) } else { values }
    }

    /// The sample at `rank`, every line uncommented.
    ///
    /// Each field is set once, to its value of that rank, the last when it
    /// has fewer. A replacing paragraph stays out at rank 1 and takes the
    /// place of the one above it from rank 2.
    fn sample_variant(paragraphs: &[Vec<SampleLine>], rank: usize) -> String {
        let mut dropped = vec![false; paragraphs.len()];

        for i in 1..paragraphs.len() {
            if sample_replaces(&paragraphs[i - 1], &paragraphs[i]) {
                let i = if rank == 1 { i } else { i - 1 };
                dropped[i] = true;
            }
        }

        let kept = paragraphs
            .iter()
            .zip(&dropped)
            .filter(|(_, dropped)| !**dropped);
        let lines: Vec<&SampleLine> = kept.flat_map(|(lines, _)| lines).collect();

        let mut options = sample_options(&lines);
        let mut text = Vec::with_capacity(lines.len());

        for line in lines {
            let Some(field) = line.field else {
                text.push(line.line.to_owned());
                continue;
            };

            if let Some(values) = options.remove(field) {
                text.push(values[rank.min(values.len()) - 1].clone());
            }
        }

        text.join("\n")
    }

    fn at(instant: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(instant).unwrap().to_utc()
    }

    /// `mail.filter.since` scopes mail, `--since` overrides it, and either is
    /// refused by its own name on an account syncing contacts.
    #[test]
    fn a_scope_is_read_from_the_flag_or_the_key_and_refused_beside_contacts() {
        let now = at("2026-10-07T12:00:00Z");
        let mail: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://imap.example.org:993"
            mail.filter.since = "30d"
            "#,
        )
        .unwrap();
        mail.validate().unwrap();

        assert_eq!(
            mail.scope(None, &[], now).unwrap(),
            PimdirScope::since("2026-09-07T00:00:00Z")
        );
        assert_eq!(
            mail.scope(Some("2026-01-01"), &[], now).unwrap(),
            PimdirScope::since("2026-01-01T00:00:00Z")
        );
        assert!(
            AccountConfig::default()
                .scope(None, &[], now)
                .unwrap()
                .is_unbounded()
        );

        let mixed: AccountConfig = toml::from_str(
            r#"
            sources.mail.imap.server = "imaps://imap.example.org:993"
            sources.dav.carddav.server = "https://carddav.example.org/"
            sources.dav.carddav.auth.basic.username = "user"
            sources.dav.carddav.auth.basic.password.raw = "pw"
            mail.filter.since = "30d"
            "#,
        )
        .unwrap();
        let err = mixed.scope(None, &[], now).unwrap_err().to_string();
        assert!(err.contains("`mail.filter.since`"), "got {err}");
        assert!(err.contains("dav"), "got {err}");
        let err = mixed.scope(Some("7d"), &[], now).unwrap_err().to_string();
        assert!(err.contains("`--since`"), "got {err}");
        assert!(
            mixed.scope(None, &[String::from("mail")], now).is_ok(),
            "a run narrowed to the mail source takes it"
        );

        let bad: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://imap.example.org:993"
            mail.filter.since = "a while"
            "#,
        )
        .unwrap();
        let err = format!("{:#}", bad.validate().unwrap_err());
        assert!(err.contains("mail.filter.since"), "got {err}");
    }
}
