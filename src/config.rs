//! # Configuration
//!
//! The TOML schema: each account holds named [`SourceConfig`]s over one
//! pimdir store, plus that store's settings.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt,
    path::PathBuf,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Days, Months, NaiveDate, NaiveTime, SecondsFormat, Utc};
use io_pimdir::collection::PimdirScope;
use io_sasl::{
    login::SaslLoginCreds, mechanism::Sasl, rfc4505::anonymous::SaslAnonymousCreds,
    rfc4616::plain::SaslPlainCreds, rfc5801::SaslGs2ChannelBinding, rfc5802::SaslScramCreds,
    rfc7628::oauthbearer::SaslOauthbearerCreds, xoauth2::SaslXoauth2Creds,
};
use log::warn;
use pimalaya_cli::error::CodedError;
use pimalaya_config::{
    command::CommandConfig,
    secret::{Secret, SecretResolver},
    toml::{TomlConfig, opt_shell_expanded_path, shell_expanded_string},
};
use pimalaya_stream::tls::{Rustls, RustlsCrypto, Tls, TlsProvider, TlsServer};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::ErrorCode;

/// The documented sample configuration, which a missing configuration and
/// one holding no account point at.
const SAMPLE_URL: &str = "https://github.com/pimalaya/neverest/blob/master/config.sample.toml";

/// Every backend table an account may write, in the order `account list`
/// names them, `smtp` the send channel's.
pub const BACKENDS: [&str; 11] = [
    "imap",
    "carddav",
    "caldav",
    "jmap",
    "gmail",
    "msgraph",
    "msgraph-contacts",
    "msgraph-calendar",
    "gpeople",
    "gcal",
    "smtp",
];

/// Whether this build serves the backend table `name`, its cargo feature
/// being on. A JMAP source parses and no build opens it.
pub fn is_built(name: &str) -> bool {
    let built = [
        ("imap", cfg!(feature = "imap")),
        ("carddav", cfg!(feature = "dav")),
        ("caldav", cfg!(feature = "dav")),
        ("gmail", cfg!(feature = "gmail")),
        ("msgraph", cfg!(feature = "msgraph")),
        ("msgraph-contacts", cfg!(feature = "msgraph")),
        ("msgraph-calendar", cfg!(feature = "msgraph")),
        ("gpeople", cfg!(feature = "gpeople")),
        ("gcal", cfg!(feature = "gcal")),
        ("smtp", cfg!(feature = "smtp")),
    ];
    built.contains(&(name, true))
}

/// `skip_serializing_if` predicate omitting a defaulted field, so a serialized
/// configuration carries only what differs from the defaults.
fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

/// [`is_default`] for a flag defaulting to true.
fn is_true(value: &bool) -> bool {
    *value
}

/// [`is_default`] for the HTTP ALPN list, whose default is not empty.
fn is_default_http_alpn(alpn: &[String]) -> bool {
    alpn == default_http_alpn().as_slice()
}

/// Splices the per-source shared fields (`collection`, `flag`, `item`,
/// `pool_size`) onto every protocol-specific config struct.
///
/// `collection` and `item` keep a serde alias on their old `mailbox` and
/// `message` spellings, so an existing mail configuration keeps loading.
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
            #[serde(default, alias = "mailbox", skip_serializing_if = "is_default")]
            pub collection: CollectionSourceConfig,
            /// Whether flag updates may be pushed to this source.
            #[serde(default, skip_serializing_if = "is_default")]
            pub flag: FlagSourcePermissions,
            /// Which item mutations may be pushed to this source.
            #[serde(default, alias = "message", skip_serializing_if = "is_default")]
            pub item: ItemSourcePermissions,
            /// Connection pool size override; the default is per backend.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub pool_size: Option<usize>,
            /// The proxy this source's connections go through, falling back
            /// to the account's, then to the `all_proxy` and `https_proxy`
            /// environment variables.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub proxy: Option<ProxyConfig>,
        }
    };
}

/// Generates a [`SourceConfig`] accessor forwarding to the shared field on
/// the source's backend variant, by value.
macro_rules! source_accessor {
    ($name:ident, $ty:ty) => {
        pub fn $name(&self) -> $ty {
            match &self.backend {
                SourceBackendConfig::Imap(c) => c.$name,
                SourceBackendConfig::Carddav(c) => c.$name,
                SourceBackendConfig::Caldav(c) => c.$name,
                SourceBackendConfig::Jmap(c) => c.$name,
                SourceBackendConfig::Gmail(c) => c.$name,
                SourceBackendConfig::Msgraph(c) => c.$name,
                SourceBackendConfig::MsgraphContacts(c) => c.$name,
                SourceBackendConfig::MsgraphCalendar(c) => c.$name,
                SourceBackendConfig::Gpeople(c) => c.$name,
                SourceBackendConfig::Gcal(c) => c.$name,
            }
        }
    };
}

/// [`source_accessor`] for a field too large to copy out.
macro_rules! source_ref_accessor {
    ($name:ident, $ty:ty) => {
        pub fn $name(&self) -> &$ty {
            match &self.backend {
                SourceBackendConfig::Imap(c) => &c.$name,
                SourceBackendConfig::Carddav(c) => &c.$name,
                SourceBackendConfig::Caldav(c) => &c.$name,
                SourceBackendConfig::Jmap(c) => &c.$name,
                SourceBackendConfig::Gmail(c) => &c.$name,
                SourceBackendConfig::Msgraph(c) => &c.$name,
                SourceBackendConfig::MsgraphContacts(c) => &c.$name,
                SourceBackendConfig::MsgraphCalendar(c) => &c.$name,
                SourceBackendConfig::Gpeople(c) => &c.$name,
                SourceBackendConfig::Gcal(c) => &c.$name,
            }
        }
    };
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
        self.accounts
            .iter()
            .map(|(name, account)| (name.as_str(), account.default))
    }

    fn take_named_account(&mut self, name: &str) -> Option<(String, Self::Account)> {
        self.accounts.remove_entry(name)
    }
}

impl Config {
    /// Loads `Config` from `config_paths`, failing when none exists with
    /// pimalaya-config's [`ConfigMissingError`], naming the path looked for
    /// and the sample to start from, which `main` meets a person with as the
    /// welcome.
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
    /// the rest of a store-wide id (pimdir STORAGE §9.2). `default` is
    /// read and ignored, with one warning.
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

        let defaults: Vec<&String> = configured
            .iter()
            .copied()
            .filter(|name| self.accounts[*name].default)
            .collect();
        if !defaults.is_empty() {
            warn!(
                "the `default` key is ignored (set on {}): a command runs over every account \
                 unless `-a` names some",
                join(&defaults),
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
/// An account is one hub of a store it may share with others, its
/// collections grouped under its name. A source's name makes its pimdir
/// source id, `<account>/<name>`, so renaming one orphans its bindings, and
/// a backend written under the account is sugar for a source named after
/// it.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct AccountConfig {
    /// Read and ignored, with a warning: a command runs over every
    /// account unless `-a` names some, so none is the default. Kept so a
    /// configuration an earlier neverest wrote still loads.
    #[serde(default, skip_serializing_if = "is_default")]
    pub default: bool,
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
    /// Direct-backend sugar: a Graph source named after its protocol.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msgraph: Option<MsgraphConfig>,
    /// Direct-backend sugar: a Graph contacts source named after its
    /// protocol.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msgraph_contacts: Option<MsgraphConfig>,
    /// Direct-backend sugar: a Graph calendar source named after its
    /// protocol.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msgraph_calendar: Option<MsgraphConfig>,
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
    /// How a run announces a content conflict it could not merge away.
    #[serde(default)]
    pub conflict: ConflictConfig,
    // TODO: item-level sync filters (date range, sender, subject).
    /// Item-level sync options.
    #[serde(default, alias = "message")]
    pub item: ItemSyncConfig,
    /// Max connections per source for concurrent body fetches, 4 by default.
    ///
    /// Keep it under the provider's per-account connection limit. `sync
    /// --connections N` overrides it for one run.
    #[serde(default)]
    pub connections: Option<usize>,
    /// Removed keys, refused by name as the file is parsed.
    #[allow(dead_code)]
    #[serde(default, skip_serializing)]
    left: Option<LeftRemoved>,
    #[allow(dead_code)]
    #[serde(default, skip_serializing)]
    right: Option<RightRemoved>,
    #[allow(dead_code)]
    #[serde(default, skip_serializing, alias = "mailbox")]
    collection: Option<AccountCollectionRemoved>,
    #[allow(dead_code)]
    #[serde(default, skip_serializing)]
    folder: Option<FolderRemoved>,
    #[allow(dead_code)]
    #[serde(default, skip_serializing)]
    envelope: Option<EnvelopeRemoved>,
}

impl AccountConfig {
    /// Every configured source keyed by its id, the sugar folded into the
    /// explicit `sources` table.
    ///
    /// The sugar's source id is its protocol name, the same id the expanded
    /// form writes, so expanding an account by hand is a store no-op.
    pub fn sources(&self) -> Result<HashMap<String, SourceConfig>> {
        let mut sources = self.sources.clone();

        let sugar = [
            self.imap.clone().map(SourceBackendConfig::Imap),
            self.carddav.clone().map(SourceBackendConfig::Carddav),
            self.caldav.clone().map(SourceBackendConfig::Caldav),
            self.jmap.clone().map(SourceBackendConfig::Jmap),
            self.gmail.clone().map(SourceBackendConfig::Gmail),
            self.msgraph.clone().map(SourceBackendConfig::Msgraph),
            self.msgraph_contacts
                .clone()
                .map(SourceBackendConfig::MsgraphContacts),
            self.msgraph_calendar
                .clone()
                .map(SourceBackendConfig::MsgraphCalendar),
            self.gpeople.clone().map(SourceBackendConfig::Gpeople),
            self.gcal.clone().map(SourceBackendConfig::Gcal),
        ];

        for backend in sugar.into_iter().flatten() {
            let name = backend.protocol().to_string();

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

    /// The backend tables the account writes, its sources' and its
    /// targets', sugar and named alike, each once in [`BACKENDS`] order,
    /// `smtp` for a send channel. Read off the tables alone, so an account
    /// a command would refuse lists too.
    pub fn backends(&self) -> Vec<&'static str> {
        let sugar = [
            self.imap.as_ref().map(|_| "imap"),
            self.carddav.as_ref().map(|_| "carddav"),
            self.caldav.as_ref().map(|_| "caldav"),
            self.jmap.as_ref().map(|_| "jmap"),
            self.gmail.as_ref().map(|_| "gmail"),
            self.msgraph.as_ref().map(|_| "msgraph"),
            self.msgraph_contacts.as_ref().map(|_| "msgraph-contacts"),
            self.msgraph_calendar.as_ref().map(|_| "msgraph-calendar"),
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

    /// This account as the store sees it: every endpoint keyed by its
    /// pimdir source id, `<account>/<name>`, the sugar folded into
    /// `sources`.
    ///
    /// A store shared by several accounts holds their source ids and
    /// collection ids side by side, each unique store-wide (pimdir STORAGE
    /// §4.3, §9.2), so two accounts each holding the `imap` sugar must not
    /// both bind `imap`. Every collection id derives from a source id,
    /// so namespacing the source namespaces the collection too.
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
            msgraph_contacts: None,
            msgraph_calendar: None,
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
    /// refused before a connection rather than halfway through a sync.
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

        // NOTE: called for its refusals: `check` and the sync read the same
        // mode.
        self.mode()?;

        if let Some(since) = &self.item.filter.since {
            resolve_since(since, Utc::now()).context("Invalid `item.filter.since`")?;
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

    /// The scope a run syncs mail under (pimdir SYNC §5): from `since`, the
    /// run's `--since`, when given, else from `item.filter.since`; unbounded
    /// without either.
    ///
    /// A scope bounds mail by its `Date` and nothing else, so one is refused
    /// by the key it came from when an endpoint the run syncs (a source of
    /// `only`, every source when empty, or a target) holds contacts or
    /// calendars: CardDAV, CalDAV, Google People and Calendar, Graph
    /// contacts and calendars.
    pub fn scope(
        &self,
        since: Option<&str>,
        only: &[String],
        now: DateTime<Utc>,
    ) -> Result<PimdirScope> {
        let (raw, key) = match since {
            Some(raw) => (raw, "--since"),
            None => match &self.item.filter.since {
                Some(raw) => (raw.as_str(), "item.filter.since"),
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

// The keys this version removed, each refused by name in the family
// sentence as the file is parsed rather than ignored: honouring neither the
// old meaning nor the new one is how a configuration ends up doing the
// opposite of what it says.
pimalaya_config::removed_key! {
    /// `left`, now `sources`, `targets` and `one-way`.
    pub LeftRemoved: Table {
        name: "left",
        why: "an account naming its endpoints and the direction between them",
        instead: "write the authoritative one under `sources` and the other under \
                  `targets`, adding `one-way = true` to copy rather than merge \
                  (without it they sync both ways, as the pair did)",
    }

    /// `right`, now `sources`, `targets` and `one-way`.
    pub RightRemoved: Table {
        name: "right",
        why: "an account naming its endpoints and the direction between them",
        instead: "write the authoritative one under `sources` and the other under \
                  `targets`, adding `one-way = true` to copy rather than merge \
                  (without it they sync both ways, as the pair did)",
    }

    /// The account-level `collection` (or `mailbox`) table, now per source.
    pub AccountCollectionRemoved: Table {
        name: "collection",
        why: "a filter belonging to the source it filters, an account holding \
              sources of several kinds",
        instead: "write `sources.<name>.collection.filter`, or \
                  `<protocol>.collection.filter` under the account",
    }

    /// The v0.1 `folder` table.
    pub FolderRemoved: Table {
        name: "folder",
        why: "a filter belonging to the source it filters",
        instead: "write its filter as `<protocol>.collection.filter`, its aliases \
                  having no replacement",
    }

    /// The v0.1 `envelope` table, now `item.filter.since`.
    pub EnvelopeRemoved: Table {
        name: "envelope",
        why: "a sync bounding its mail by date alone",
        instead: "write `item.filter.since`",
    }

    /// `collection.namespace`, now the account's arity and `one-way`.
    pub NamespaceRemoved: Key {
        name: "collection.namespace",
        why: "the sources meeting by whether they sit under `sources` or \
              `targets`, and the direction being `one-way`",
        instead: "delete it",
    }

    /// `store.retention`, now the account's `retain`.
    ///
    /// A three-point scale described a store holding only the bodies that
    /// happened to cross, which nothing asked for; mapping it onto `retain`
    /// would guess.
    pub StoreRetentionRemoved: Key {
        name: "store.retention",
        why: "whether the store keeps bodies being the account's `retain`, true \
              when the store is the destination and false when targets are named",
        instead: "delete it",
    }

    /// `store.hydration`, now the account's `retain`.
    pub StoreHydrationRemoved: Key {
        name: "store.hydration",
        why: "whether the store keeps bodies being the account's `retain`, true \
              when the store is the destination and false when targets are named",
        instead: "delete it",
    }

    /// The flat `smtp.login`, now a `sasl` mechanism.
    pub SmtpLoginRemoved: Key {
        name: "smtp.login",
        why: "the send channel naming one SASL mechanism, as `imap` does",
        instead: "write `smtp.sasl.plain.username` and `smtp.sasl.plain.password`",
    }

    /// The flat `smtp.password`, now a `sasl` mechanism.
    pub SmtpPasswordRemoved: Key {
        name: "smtp.password",
        why: "the send channel naming one SASL mechanism, as `imap` does",
        instead: "write `smtp.sasl.plain.username` and `smtp.sasl.plain.password`",
    }
}

/// The only part of conflict handling anybody configures.
///
/// Whether a run merges is not a setting: the three-way merge is a pure
/// function over bodies the store holds, and because nobody can swap it out
/// it resolves only what nobody disagreed about (see [`crate::kind::merge`]).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ConflictConfig {
    /// The merger `conflict resolve --interactive` runs.
    ///
    /// Unset by default; a sync never runs it. The four paths are appended
    /// git-mergetool style, unless the command names {base}, {local}, {remote}
    /// or {output}: `tcard merge {base} {local} {remote} --output {output}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merger: Option<CommandConfig>,
}

/// The local pimdir store an account syncs through, the cache a frontend
/// reads. Implicit, and shared by every account naming no root; this table
/// only customises it.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct StoreConfig {
    /// The store directory, holding `pimdir.db` and `objects/`.
    ///
    /// Defaults to `neverest/` under the platform's state location (XDG on
    /// Linux and the BSDs, Application Support on macOS, `%LOCALAPPDATA%` on
    /// Windows), one store for every account naming no root. Accounts
    /// naming one root share that store.
    #[serde(default, deserialize_with = "opt_shell_expanded_path")]
    pub root: Option<PathBuf>,
    /// How long a retained item survives: `store.purge-after = "90d"`, or an
    /// integer of seconds.
    ///
    /// A pimdir store never truly deletes: the row is retained, hidden but
    /// keeping its body, and neverest is the sweeper. Unset means never
    /// purge and `0` purges at once; there is deliberately no boolean. A
    /// day is 86400 seconds and a week 7 days, months and years being
    /// refused for having no fixed length.
    #[serde(
        default,
        with = "pimalaya_config::duration::option",
        skip_serializing_if = "Option::is_none"
    )]
    pub purge_after: Option<Duration>,
    /// How long the search index may be refreshed after a sync:
    /// `store.index-timeout = "60s"`, the default, or an integer of seconds.
    ///
    /// A large first build spreads over several runs, each going on from
    /// where the last stopped; `neverest index` builds it whole. A drain
    /// refreshes for 2 seconds at most, `0` turns the refresh off, and a
    /// build without the `search` feature ignores it.
    #[serde(
        default,
        with = "pimalaya_config::duration::option",
        skip_serializing_if = "Option::is_none"
    )]
    pub index_timeout: Option<Duration>,
    /// Removed keys, refused by name as the file is parsed.
    #[allow(dead_code)]
    #[serde(default, skip_serializing)]
    retention: Option<StoreRetentionRemoved>,
    #[allow(dead_code)]
    #[serde(default, skip_serializing)]
    hydration: Option<StoreHydrationRemoved>,
}

impl StoreConfig {
    /// The RFC 3339 cutoff at `now`: an older retained item is reclaimed.
    ///
    /// `None` when `purge-after` is unset, or so large no instant precedes
    /// it, which means the same. The format matches what the store stamps
    /// `retained_at` with, so the comparison is plain lexicographic.
    pub fn purge_cutoff(&self, now: DateTime<Utc>) -> Option<String> {
        let after = chrono::Duration::from_std(self.purge_after?).ok()?;
        let cutoff = now.checked_sub_signed(after)?;
        Some(cutoff.to_rfc3339_opts(SecondsFormat::Millis, true))
    }

    /// The longest a search index refresh runs after a sync, 60 seconds
    /// unless `index-timeout` says otherwise; zero turns it off.
    #[cfg(feature = "search")]
    pub fn index_timeout(&self) -> Duration {
        self.index_timeout.unwrap_or(Duration::from_secs(60))
    }
}

/// What an account does: which endpoints, which direction, whether bodies stay.
///
/// Declared, never derived: the mode is the arity of `sources` and
/// `targets` plus the two flags, so no behaviour depends on a coincidence
/// between two sources.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AccountMode {
    /// The source names, sorted, so a report reads the same twice.
    pub sources: Vec<String>,
    /// The target names, sorted; empty means the store is the destination.
    pub targets: Vec<String>,
    /// Whether the `sources` side is authoritative.
    pub one_way: bool,
    /// Whether the store holds bodies, resolved from the destination when
    /// the configuration left it unset.
    pub retain: bool,
}

impl AccountMode {
    /// Whether the local store is the destination rather than a remote.
    pub fn is_local(&self) -> bool {
        self.targets.is_empty()
    }

    /// Whether a crossing may be streamed rather than staged in the store.
    ///
    /// An internal choice, not a mode: what the user declared is `retain`,
    /// which both answers honour. It needs both endpoints on a protocol that
    /// takes a body straight from the other, so IMAP to IMAP today.
    pub fn streams(&self, sources: &HashMap<String, SourceConfig>) -> bool {
        !self.retain
            && !self.is_local()
            && self
                .sources
                .iter()
                .chain(&self.targets)
                .all(|name| sources.get(name).is_some_and(SourceConfig::is_streamable))
    }
}

impl fmt::Display for AccountMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sources = self.sources.join(", ");

        if self.is_local() {
            let verb = if self.one_way {
                "overwrite the local store, discarding local edits"
            } else {
                "sync both ways with the local store"
            };

            return write!(f, "{sources} {verb}");
        }

        let targets = self.targets.join(", ");
        let body = if self.retain {
            ", keeping a local copy"
        } else {
            ", keeping no local copy"
        };

        if self.one_way {
            write!(f, "{sources} overwrites {targets}{body}")
        } else {
            write!(f, "{sources} and {targets} sync both ways{body}")
        }
    }
}

/// One source: the remote it talks to, plus the channel its sends leave by.
///
/// The channel belongs to the source and not the account, since sending
/// natively (Graph's `sendMail`) or needing a companion SMTP server is a
/// property of the provider. At most one source per account declares one.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", try_from = "SourceTable")]
pub struct SourceConfig {
    /// The one remote this source names.
    #[serde(flatten)]
    pub backend: SourceBackendConfig,
    /// The SMTP server this source's queued submit intents flush through.
    ///
    /// Only meaningful on a backend that cannot send by itself (IMAP), a
    /// Graph source using `sendMail` instead. Absent, and with no source
    /// sending natively, submit intents stay pending.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smtp: Option<SmtpConfig>,
}

/// A source as written, read without `flatten`, which unknown keys slip by.
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct SourceTable {
    imap: Option<ImapConfig>,
    carddav: Option<CarddavConfig>,
    caldav: Option<CaldavConfig>,
    jmap: Option<JmapConfig>,
    gmail: Option<GmailConfig>,
    msgraph: Option<MsgraphConfig>,
    msgraph_contacts: Option<MsgraphConfig>,
    msgraph_calendar: Option<MsgraphConfig>,
    gpeople: Option<GoogleConfig>,
    gcal: Option<GoogleConfig>,
    smtp: Option<SmtpConfig>,
}

impl TryFrom<SourceTable> for SourceConfig {
    type Error = String;

    fn try_from(table: SourceTable) -> Result<Self, Self::Error> {
        let backends = [
            table.imap.map(SourceBackendConfig::Imap),
            table.carddav.map(SourceBackendConfig::Carddav),
            table.caldav.map(SourceBackendConfig::Caldav),
            table.jmap.map(SourceBackendConfig::Jmap),
            table.gmail.map(SourceBackendConfig::Gmail),
            table.msgraph.map(SourceBackendConfig::Msgraph),
            table
                .msgraph_contacts
                .map(SourceBackendConfig::MsgraphContacts),
            table
                .msgraph_calendar
                .map(SourceBackendConfig::MsgraphCalendar),
            table.gpeople.map(SourceBackendConfig::Gpeople),
            table.gcal.map(SourceBackendConfig::Gcal),
        ];
        let mut backends = backends.into_iter().flatten();

        let Some(backend) = backends.next() else {
            return Err(String::from(
                "A source declares no backend: write one table of `imap`, `carddav`, `caldav`, \
                 `jmap`, `gmail`, `msgraph`, `msgraph-contacts`, `msgraph-calendar`, `gpeople` \
                 or `gcal` under it",
            ));
        };

        if let Some(other) = backends.next() {
            return Err(format!(
                "A source declares both `{}` and `{}`, and talks to exactly one backend: move \
                 one under a source of its own",
                backend.protocol(),
                other.protocol(),
            ));
        }

        Ok(Self {
            backend,
            smtp: table.smtp,
        })
    }
}

/// The remote backend behind a source; exactly one variant per source.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceBackendConfig {
    Imap(ImapConfig),
    Carddav(CarddavConfig),
    Caldav(CaldavConfig),
    Jmap(JmapConfig),
    Gmail(GmailConfig),
    Msgraph(MsgraphConfig),
    /// Microsoft Graph contacts: the same session as `msgraph`, syncing
    /// contact folders instead of mail folders.
    #[serde(rename = "msgraph-contacts")]
    MsgraphContacts(MsgraphConfig),
    /// Microsoft Graph calendars: the same session as `msgraph`, syncing
    /// calendars instead of mail folders.
    #[serde(rename = "msgraph-calendar")]
    MsgraphCalendar(MsgraphConfig),
    /// Google contacts through the People API.
    Gpeople(GoogleConfig),
    /// Google calendars through the Calendar API.
    Gcal(GoogleConfig),
}

impl SourceBackendConfig {
    /// The proxy the backend table names, `None` when it names none.
    pub fn proxy(&self) -> Option<&ProxyConfig> {
        match self {
            Self::Imap(c) => c.proxy.as_ref(),
            Self::Carddav(c) => c.proxy.as_ref(),
            Self::Caldav(c) => c.proxy.as_ref(),
            Self::Jmap(c) => c.proxy.as_ref(),
            Self::Gmail(c) => c.proxy.as_ref(),
            Self::Msgraph(c) | Self::MsgraphContacts(c) | Self::MsgraphCalendar(c) => {
                c.proxy.as_ref()
            }
            Self::Gpeople(c) | Self::Gcal(c) => c.proxy.as_ref(),
        }
    }

    /// The protocol table this backend is written under, which is also the
    /// id of the source the sugar builds from it.
    pub fn protocol(&self) -> &'static str {
        match self {
            Self::Imap(_) => "imap",
            Self::Carddav(_) => "carddav",
            Self::Caldav(_) => "caldav",
            Self::Jmap(_) => "jmap",
            Self::Gmail(_) => "gmail",
            Self::Msgraph(_) => "msgraph",
            Self::MsgraphContacts(_) => "msgraph-contacts",
            Self::MsgraphCalendar(_) => "msgraph-calendar",
            Self::Gpeople(_) => "gpeople",
            Self::Gcal(_) => "gcal",
        }
    }

    /// The cargo feature that builds this backend.
    pub fn feature(&self) -> &'static str {
        match self {
            Self::Carddav(_) | Self::Caldav(_) => "dav",
            Self::Msgraph(_) | Self::MsgraphContacts(_) | Self::MsgraphCalendar(_) => "msgraph",
            backend => backend.protocol(),
        }
    }
}

#[allow(dead_code)]
impl SourceConfig {
    /// Wraps a backend into a source with no send channel of its own.
    pub fn new(backend: SourceBackendConfig) -> Self {
        Self {
            backend,
            smtp: None,
        }
    }

    source_ref_accessor!(collection, CollectionSourceConfig);
    source_accessor!(flag, FlagSourcePermissions);
    source_accessor!(item, ItemSourcePermissions);
    source_accessor!(pool_size, Option<usize>);

    /// Whether this source is the IMAP backend.
    pub fn is_imap(&self) -> bool {
        matches!(self.backend, SourceBackendConfig::Imap(_))
    }

    /// Whether this source talks HTTP, those sharing a smaller default pool.
    pub fn is_http(&self) -> bool {
        matches!(
            self.backend,
            SourceBackendConfig::Jmap(_)
                | SourceBackendConfig::Gmail(_)
                | SourceBackendConfig::Msgraph(_)
                | SourceBackendConfig::MsgraphContacts(_)
                | SourceBackendConfig::MsgraphCalendar(_)
                | SourceBackendConfig::Gpeople(_)
                | SourceBackendConfig::Gcal(_)
                | SourceBackendConfig::Carddav(_)
                | SourceBackendConfig::Caldav(_)
        )
    }

    /// Whether this source sends by itself: Graph's `sendMail` today, on a
    /// mail source only.
    pub fn sends_natively(&self) -> bool {
        matches!(self.backend, SourceBackendConfig::Msgraph(_))
    }

    /// Whether this source can carry a send channel at all.
    ///
    /// A contacts or calendar source cannot: submission is a mail
    /// capability, so an `smtp` table there is an error, not a dead option.
    pub fn carries_mail(&self) -> bool {
        !matches!(
            self.backend,
            SourceBackendConfig::Carddav(_)
                | SourceBackendConfig::Caldav(_)
                | SourceBackendConfig::MsgraphContacts(_)
                | SourceBackendConfig::MsgraphCalendar(_)
                | SourceBackendConfig::Gpeople(_)
                | SourceBackendConfig::Gcal(_)
        )
    }

    /// Whether a body streams straight from this source to another rather
    /// than being staged on the way.
    ///
    /// Only an IMAP pairing can, so this gates [`AccountMode::streams`],
    /// an optimisation of the declared `retain` and never a mode of its own.
    pub fn is_streamable(&self) -> bool {
        self.is_imap()
    }

    /// Rejects a source whose declared options its backend cannot honour.
    pub fn validate(&self, name: &str) -> Result<()> {
        if self.smtp.is_some() && !self.carries_mail() {
            bail!(
                "The `sources.{name}.smtp` channel is a mail capability and this source syncs \
                 contacts; drop the table, or move it to the source that sends."
            );
        }

        Ok(())
    }

    /// Snapshots the per-source collection/flag/item permissions.
    pub fn permissions(&self) -> SourcePermissions {
        SourcePermissions {
            collection: self.collection().permissions(),
            flag: self.flag(),
            item: self.item(),
        }
    }
}

/// Per-source permission triple gating which sync hunks may materialize.
#[derive(Clone, Copy, Debug)]
pub struct SourcePermissions {
    /// What the source lets a run do to its collection set.
    pub collection: CollectionPermissions,
    /// Whether the source takes flag updates.
    pub flag: FlagSourcePermissions,
    /// What the source lets a run do to its items.
    pub item: ItemSourcePermissions,
}

/// Item-level sync options.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ItemSyncConfig {
    /// Which items a sync lists.
    #[serde(default, skip_serializing_if = "is_default")]
    pub filter: ItemFilter,
}

/// Which items a sync lists (pimdir SYNC §5).
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ItemFilter {
    /// The floor of the scope a mail collection syncs under, on the `Date`
    /// header: a duration back from today (`30d`, `12w`, `6mo`, `1y`), a
    /// date (`2026-01-01`, midnight UTC) or an RFC 3339 instant.
    ///
    /// Mail outside it is neither listed nor deleted: what an earlier,
    /// wider sync stored stays. Mail only, refused on an account syncing
    /// contacts or calendars. `sync --since` overrides it for one run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
}

/// The RFC 3339 instant in UTC a scope floor `raw` names at `now`: a
/// duration back from `now` (`30d`, `12w`, `6mo`, `1y`, or spelled out:
/// `30 days`), taken to the start of its UTC day so a run resumes the
/// round an earlier run of the same day opened; a date (`2026-01-01`) at
/// midnight UTC; or an instant, normalised to UTC.
pub fn resolve_since(raw: &str, now: DateTime<Utc>) -> Result<String> {
    let raw = raw.trim();
    let format = |at: DateTime<Utc>| at.to_rfc3339_opts(SecondsFormat::Secs, true);

    if let Ok(at) = DateTime::parse_from_rfc3339(raw) {
        return Ok(format(at.to_utc()));
    }
    if let Ok(day) = NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
        return Ok(format(day.and_time(NaiveTime::MIN).and_utc()));
    }

    let digits = raw.find(|c: char| !c.is_ascii_digit()).unwrap_or(raw.len());
    let (count, unit) = raw.split_at(digits);
    let count: u32 = count.parse().with_context(|| {
        format!("{raw:?} is neither a duration (`30d`), a date (`2026-01-01`) nor an instant")
    })?;
    let today = now.date_naive();
    let floor = match unit.trim().to_ascii_lowercase().as_str() {
        "d" | "day" | "days" => today.checked_sub_days(Days::new(u64::from(count))),
        "w" | "week" | "weeks" => today.checked_sub_days(Days::new(u64::from(count) * 7)),
        "mo" | "month" | "months" => today.checked_sub_months(Months::new(count)),
        "y" | "year" | "years" => today.checked_sub_months(Months::new(count.saturating_mul(12))),
        unit => bail!(
            "{raw:?} counts in {unit:?}, which is none of d (days), w (weeks), mo (months), y \
             (years)"
        ),
    }
    .with_context(|| format!("{raw:?} reaches before any date"))?;

    Ok(format(floor.and_time(NaiveTime::MIN).and_utc()))
}

/// Collection-name filter: include-list, exclude-list, or keep all.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum CollectionFilter {
    /// Every collection the source lists.
    #[default]
    All,
    /// Only the named collections.
    Include(Vec<String>),
    /// Every collection but the named ones.
    Exclude(Vec<String>),
}

/// A source's collection-level configuration: which collections it syncs,
/// and what it may do to the collection set.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct CollectionSourceConfig {
    /// Whether the sync may create a collection on this source.
    ///
    /// It grants by default, unlike the `item` block, which must be declared
    /// in full: this table also carries `filter`, and demanding a permission
    /// pair from someone writing a filter would be a trap.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub create: bool,
    /// Whether the sync may delete one on this source, off unless asked for.
    ///
    /// A collection delete is inferred from an absence, the only signal IMAP
    /// and DAV leave, and a listing may drop a collection nobody deleted.
    #[serde(default, skip_serializing_if = "is_default")]
    pub delete: bool,
    /// Whether a frontend may rename a collection on this source, through
    /// the queue. Unset, it reads the account's `one-way`: false on a
    /// one-way account, whose sources are copied rather than written to,
    /// true otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rename: Option<bool>,
    /// Removed key, refused by name as the file is parsed.
    ///
    /// Which endpoints meet is now the account's arity, and which way is
    /// [`AccountConfig::one_way`].
    #[serde(default, skip_serializing)]
    namespace: Option<NamespaceRemoved>,
    /// Collection-name filter for this source.
    ///
    /// Per source, because an account may hold several kinds and a mailbox
    /// include-list means nothing to a contacts source. Filters are
    /// therefore asymmetric: a collection may sync on one source only.
    #[serde(default, alias = "filters", skip_serializing_if = "is_default")]
    pub filter: CollectionFilter,
    /// Whether this source lists at most a number of the items a collection
    /// holds, whatever it holds: a folder size limit set on the server.
    ///
    /// Every round of its collections then says so (pimdir SYNC §4, §5):
    /// an item it leaves unlisted is kept, and only a removal the source
    /// states deletes.
    #[serde(default, skip_serializing_if = "is_default")]
    pub capped: bool,
}

impl CollectionSourceConfig {
    /// The copyable permission pair, the only part the sync seam gates on.
    pub fn permissions(&self) -> CollectionPermissions {
        CollectionPermissions {
            create: self.create,
            delete: self.delete,
            rename: self.rename,
        }
    }
}

impl Default for CollectionSourceConfig {
    fn default() -> Self {
        Self {
            create: true,
            delete: false,
            rename: None,
            namespace: None,
            filter: CollectionFilter::default(),
            capped: false,
        }
    }
}

/// Per-source collection permissions gating collection-set mutations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CollectionPermissions {
    /// Whether a run may create a collection here.
    pub create: bool,
    /// Whether a run may delete one.
    pub delete: bool,
    /// Whether a frontend may rename one, `None` taking the account's
    /// answer: allowed unless it is one-way.
    pub rename: Option<bool>,
}

impl CollectionPermissions {
    /// Whether a rename is allowed on an account `one_way` or not.
    pub fn renames(&self, one_way: bool) -> bool {
        self.rename.unwrap_or(!one_way)
    }
}

impl Default for CollectionPermissions {
    fn default() -> Self {
        Self {
            create: true,
            delete: false,
            rename: None,
        }
    }
}

/// Per-source flag permissions, gating flag propagation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct FlagSourcePermissions {
    /// Whether a run may push a flag change here.
    pub update: bool,
}

impl Default for FlagSourcePermissions {
    fn default() -> Self {
        Self { update: true }
    }
}

/// Per-source item permissions gating item mutations.
///
/// `create` and `delete` are required once the block is declared at all;
/// `update` defaults to true, added later so an older configuration keeps
/// parsing. It bites on mutable content alone, mail bodies being immutable.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ItemSourcePermissions {
    /// Whether a run may create an item here.
    pub create: bool,
    /// Whether a run may delete one.
    pub delete: bool,
    /// Whether a run may replace an item's body here.
    #[serde(default = "default_true")]
    pub update: bool,
}

impl Default for ItemSourcePermissions {
    fn default() -> Self {
        Self {
            create: true,
            delete: true,
            update: true,
        }
    }
}

/// `serde` default for a permission that grants by default.
fn default_true() -> bool {
    true
}

source_config! {
    /// An IMAP source (RFC 9051), each mailbox a collection.
    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "kebab-case", deny_unknown_fields)]
    pub struct ImapConfig {
        /// The server, a bare authority or a full URL.
        pub server: String,
        /// How the connection is secured.
        #[serde(default)]
        pub tls: TlsConfig,
        /// Whether a plain connection is upgraded with STARTTLS.
        #[serde(default, skip_serializing_if = "is_default")]
        pub starttls: bool,
        /// ALPN identifiers offered during the TLS handshake.
        ///
        /// Unset takes io-imap's own default (`["imap"]`), which owns it;
        /// `[]` skips ALPN.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub alpn: Option<Vec<String>>,
        /// The one SASL mechanism the session authenticates with.
        pub sasl: Option<SaslConfig>,
        /// Forces the RFC 4959 SASL-IR initial response on or off.
        ///
        /// Unset follows the advertised `SASL-IR` capability, which Coremail
        /// (126.com, 163.com) advertises falsely: set it to `false` there.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub sasl_ir: Option<bool>,
        /// RFC 2971 `ID` quirks, `id.auto = true` opting in to the exchange
        /// mail.qq.com and Fastmail require straight after authentication.
        #[serde(default, skip_serializing_if = "is_default")]
        pub id: ImapIdConfig,
    }
}

/// The `imap.id.*` quirks.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ImapIdConfig {
    /// Chains an `ID` round-trip after the tagged authentication response.
    #[serde(default, skip_serializing_if = "is_default")]
    pub auto: bool,
    /// The parameters the automatic `ID` sends, none sending `ID NIL`.
    ///
    /// `true` sends neverest's canned value for the well-known keys
    /// (`name`, `version`, `vendor`, `support-url`) and `NIL` for the
    /// others, `false` always `NIL`. An absent key is not sent.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, bool>,
}

source_config! {
    /// A CardDAV source (RFC 6352), each address book a collection.
    ///
    /// The server URL is the entry point only: the principal and the
    /// address book home set are discovered from it, unless `home` names the
    /// home set itself. One of the two is required.
    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "kebab-case", deny_unknown_fields)]
    pub struct CarddavConfig {
        /// The DAV entry point.
        ///
        /// A bare authority (`dav.example.org[:port]`, read as
        /// `https://<authority>`) or a full URL, `http://` included for a
        /// server on a trusted network.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub server: Option<String>,
        /// The address book home set, as a full URL: the session lists its
        /// collections there and discovers nothing, `server` then being
        /// ignored.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub home: Option<String>,
        /// How the connection is secured.
        #[serde(default)]
        pub tls: TlsConfig,
        /// ALPN identifiers offered during the TLS handshake, `["http/1.1"]`
        /// by default; `[]` skips ALPN.
        #[serde(
            default = "default_http_alpn",
            skip_serializing_if = "is_default_http_alpn"
        )]
        /// ALPN identifiers offered during the TLS handshake.
        pub alpn: Vec<String>,
        /// How the DAV session authenticates.
        pub auth: DavAuthConfig,
    }
}

source_config! {
    /// A CalDAV source (RFC 4791), each calendar a collection.
    ///
    /// The server URL is the entry point only: the principal and the
    /// calendar home set are discovered from it, unless `home` names the
    /// home set itself. One of the two is required.
    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "kebab-case", deny_unknown_fields)]
    pub struct CaldavConfig {
        /// The DAV entry point.
        ///
        /// A bare authority (`dav.example.org[:port]`, read as
        /// `https://<authority>`) or a full URL, `http://` included for a
        /// server on a trusted network.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub server: Option<String>,
        /// The calendar home set, as a full URL: the session lists its
        /// collections there and discovers nothing, `server` then being
        /// ignored.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub home: Option<String>,
        /// How the connection is secured.
        #[serde(default)]
        pub tls: TlsConfig,
        /// ALPN identifiers offered during the TLS handshake, `["http/1.1"]`
        /// by default; `[]` skips ALPN.
        #[serde(
            default = "default_http_alpn",
            skip_serializing_if = "is_default_http_alpn"
        )]
        /// ALPN identifiers offered during the TLS handshake.
        pub alpn: Vec<String>,
        /// How the DAV session authenticates.
        pub auth: DavAuthConfig,
    }
}

/// DAV authentication: none, HTTP Basic, or a bearer token for a provider
/// fronting DAV with OAuth 2.0.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum DavAuthConfig {
    /// No credentials, for a server that asks for none: `auth = "none"`.
    None,
    /// A username and a password, HTTP Basic.
    Basic {
        #[serde(deserialize_with = "shell_expanded_string")]
        username: String,
        password: Secret,
    },
    /// A bearer token the client formats itself.
    Bearer { token: Secret },
}

#[cfg(feature = "dav")]
impl DavAuthConfig {
    /// Resolves the configured secret and converts to io-webdav's auth.
    ///
    /// The resolver keeps an account's CardDAV and CalDAV sides, which
    /// usually name one password entry, from unlocking it twice. It runs
    /// where the account is built ([`crate::account`]), never per connection.
    pub fn try_into_dav_auth(
        self,
        resolver: &mut SecretResolver,
    ) -> Result<io_webdav::rfc4918::WebdavAuth> {
        use io_http::{rfc6750::bearer::HttpAuthBearer, rfc7617::basic::HttpAuthBasic};
        use io_webdav::rfc4918::WebdavAuth;
        use secrecy::ExposeSecret;

        Ok(match self {
            Self::None => WebdavAuth::None,
            Self::Basic { username, password } => WebdavAuth::Basic(HttpAuthBasic::new(
                username,
                resolver.resolve(password)?.expose_secret(),
            )),
            Self::Bearer { token } => WebdavAuth::Bearer(HttpAuthBearer::new(
                resolver.resolve(token)?.expose_secret(),
            )),
        })
    }
}

/// The proxy a connection goes through.
///
/// `url` is a `socks5://`, `socks5h://` or `http://` proxy URL. Its user
/// info authenticates too, but `username` and `password` keep the secret
/// out of the URL.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ProxyConfig {
    /// The proxy URL.
    pub url: String,
    /// The proxy username, required by `password`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// The proxy password.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<Secret>,
}

impl ProxyConfig {
    /// Resolves the first configuration of `configs`, the one closest to
    /// the connection first, none reading the environment at connect time.
    #[cfg_attr(
        not(any(
            feature = "imap",
            feature = "smtp",
            feature = "dav",
            feature = "msgraph",
            feature = "gpeople",
            feature = "gcal",
            feature = "gmail"
        )),
        allow(dead_code)
    )]
    pub fn resolve(
        configs: &[Option<&Self>],
        resolver: &mut SecretResolver,
    ) -> Result<pimalaya_stream::proxy::Proxy> {
        use pimalaya_stream::proxy::{Proxy, ProxyAuth};
        use secrecy::SecretString;

        let Some(config) = configs.iter().flatten().next() else {
            return Ok(Proxy::System);
        };

        let mut proxy = Proxy::from_url(&config.url)?;

        let auth = match (&config.username, &config.password) {
            (None, None) => return Ok(proxy),
            (None, Some(_)) => bail!("Proxy password requires a username"),
            (Some(user), pass) => ProxyAuth {
                user: user.clone(),
                pass: match pass {
                    Some(pass) => resolver.resolve(pass.clone())?,
                    None => SecretString::default(),
                },
            },
        };

        match &mut proxy {
            Proxy::Socks5 { auth: slot, .. } | Proxy::Http { auth: slot, .. } => {
                *slot = Some(auth);
            }
            Proxy::None | Proxy::System => {}
        }

        Ok(proxy)
    }
}

source_config! {
    /// A JMAP source (RFC 8620), each mailbox a collection.
    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "kebab-case", deny_unknown_fields)]
    pub struct JmapConfig {
        /// The server, a bare authority or a full URL.
        pub server: String,
        /// How the connection is secured.
        #[serde(default)]
        pub tls: TlsConfig,
        /// ALPN identifiers offered during the TLS handshake, `["http/1.1"]`
        /// by default; `[]` skips ALPN.
        #[serde(
            default = "default_http_alpn",
            skip_serializing_if = "is_default_http_alpn"
        )]
        /// ALPN identifiers offered during the TLS handshake.
        pub alpn: Vec<String>,
        /// How the JMAP session authenticates.
        pub auth: JmapAuthConfig,
        /// The JMAP identity a submission is sent under.
        pub identity_id: Option<String>,
        /// The mailbox a submitted draft is written to.
        pub drafts_mailbox_id: Option<String>,
    }
}

/// How a JMAP session authenticates.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum JmapAuthConfig {
    /// A verbatim `Authorization` header value.
    Header(Secret),
    /// A bearer token the client formats itself.
    Bearer { token: Secret },
    /// A username and a password, HTTP Basic.
    Basic {
        #[serde(deserialize_with = "shell_expanded_string")]
        username: String,
        password: Secret,
    },
}

source_config! {
    /// A Gmail REST API source, its labels exposed as mailboxes.
    ///
    /// The API host is fixed, so only the mailbox owner, TLS and the
    /// OAuth 2.0 credential are configurable.
    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "kebab-case", deny_unknown_fields)]
    pub struct GmailConfig {
        /// Gmail user id, `me` by default: the authenticated user.
        #[serde(default = "default_gmail_user_id")]
        pub user_id: String,
        /// How the connection is secured.
        #[serde(default)]
        pub tls: TlsConfig,
        /// ALPN identifiers offered during the TLS handshake, `["http/1.1"]`
        /// by default; `[]` skips ALPN.
        #[serde(
            default = "default_http_alpn",
            skip_serializing_if = "is_default_http_alpn"
        )]
        /// ALPN identifiers offered during the TLS handshake.
        pub alpn: Vec<String>,
        /// How the Gmail session authenticates.
        pub auth: GmailAuthConfig,
        /// The most message reads one HTTP batch carries, 10 by default,
        /// from 1 to 100: a batch with a read Gmail refuses for concurrency
        /// or rate, fails transiently or leaves unanswered halves the next,
        /// down to one read, and each full batch answered whole doubles it
        /// back up to this ceiling.
        ///
        /// Gmail runs the reads of a batch side by side and refuses past
        /// its per-user concurrency limit: measured on 2026-10-10, batches
        /// of 10 sent one after another drew no refusal in 2,250 reads, 25
        /// drew 1.2 % and 50 drew 19 %.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub batch_size: Option<usize>,
        /// How many batches the source's connections may have in flight at
        /// once, 1 by default.
        ///
        /// Two batches of 50 side by side drew 17 to 33 refusals of 50
        /// each: more than one in flight trades latency for refusals, each
        /// waited out, and needs as many connections.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub batches_in_flight: Option<usize>,
        /// The quota units the source spends over any sixty seconds, 6,000
        /// by default, a read counting the 5 units Gmail bills it.
        ///
        /// 6,000 units a minute a user is the quota Google gives a project
        /// today. A project granted more may raise it; a refusal past the
        /// quota is waited out all the same.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub units_per_minute: Option<u32>,
    }
}

/// Gmail authentication: OAuth 2.0 bearer tokens only.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct GmailAuthConfig {
    /// OAuth 2.0 bearer token, the `Bearer ` prefix added by the client.
    ///
    /// Refreshing it is the caller's responsibility.
    pub token: Secret,
}

source_config! {
    /// A Microsoft Graph source, its mail folders exposed as mailboxes.
    ///
    /// The API host is fixed, so only the mailbox owner, TLS and the
    /// OAuth 2.0 credential are configurable.
    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "kebab-case", deny_unknown_fields)]
    pub struct MsgraphConfig {
        /// Graph user id, `me` by default: the authenticated user.
        #[serde(default = "default_msgraph_user_id")]
        pub user_id: String,
        /// How the connection is secured.
        #[serde(default)]
        pub tls: TlsConfig,
        /// ALPN identifiers offered during the TLS handshake, `["http/1.1"]`
        /// by default; `[]` skips ALPN.
        #[serde(
            default = "default_http_alpn",
            skip_serializing_if = "is_default_http_alpn"
        )]
        /// ALPN identifiers offered during the TLS handshake.
        pub alpn: Vec<String>,
        /// How the Graph session authenticates.
        pub auth: MsgraphAuthConfig,
    }
}

/// Microsoft Graph authentication: OAuth 2.0 bearer tokens only, neverest
/// never running an OAuth flow itself.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MsgraphAuthConfig {
    /// OAuth 2.0 bearer token, the `Bearer ` prefix added by the client.
    ///
    /// Acquiring and refreshing it is the caller's job: point
    /// `token.command` at any command printing a valid token, ortie say.
    pub token: Secret,
}

source_config! {
    /// A Google source, People contacts or Calendar events: the API host is
    /// fixed, so only TLS and the OAuth 2.0 credential are configurable.
    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "kebab-case", deny_unknown_fields)]
    pub struct GoogleConfig {
        /// How the connection is secured.
        #[serde(default)]
        pub tls: TlsConfig,
        /// ALPN identifiers offered during the TLS handshake, `["http/1.1"]`
        /// by default; `[]` skips ALPN.
        #[serde(
            default = "default_http_alpn",
            skip_serializing_if = "is_default_http_alpn"
        )]
        pub alpn: Vec<String>,
        /// How the session authenticates.
        pub auth: GoogleAuthConfig,
    }
}

/// Google authentication: OAuth 2.0 bearer tokens only, neverest never
/// running an OAuth flow itself.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct GoogleAuthConfig {
    /// OAuth 2.0 bearer token, the `Bearer ` prefix added by the client.
    ///
    /// It needs the `contacts` scope for People and the `calendar` scope
    /// for Calendar; point `token.command` at a broker such as ortie.
    pub token: Secret,
}

/// The SMTP server a source's queued sends flush through.
///
/// The shape mirrors [`ImapConfig`] field for field, submission being the
/// other half of the same mail account: a bare authority or a URL, the same
/// TLS block, and a `sasl` table naming one mechanism.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SmtpConfig {
    /// The submission server.
    ///
    /// A bare authority (`smtp.example.org[:port]`, read as
    /// `smtps://<authority>`), a cleartext `smtp://` URL, usually with
    /// `starttls`, or an `smtps://` URL for implicit TLS.
    pub server: String,
    /// How the connection is secured.
    #[serde(default)]
    pub tls: TlsConfig,
    /// Upgrades a plain `smtp://` connection via STARTTLS.
    #[serde(default, skip_serializing_if = "is_default")]
    pub starttls: bool,
    /// ALPN identifiers offered during the TLS handshake.
    ///
    /// Unset takes io-smtp's own default (`["smtp"]`, the token RFC 7595
    /// registers), which owns it; `[]` skips ALPN.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alpn: Option<Vec<String>>,
    /// The mechanism the session authenticates with, one [`SaslConfig`].
    ///
    /// Omit it for an unauthenticated relay, which stops after `EHLO` and
    /// sends no `AUTH` at all.
    pub sasl: Option<SaslConfig>,
    /// The proxy the channel goes through, falling back to the account's,
    /// then to the `all_proxy` and `https_proxy` environment variables.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy: Option<ProxyConfig>,
    /// Removed keys, refused by name as the file is parsed.
    #[allow(dead_code)]
    #[serde(default, skip_serializing)]
    login: Option<SmtpLoginRemoved>,
    #[allow(dead_code)]
    #[serde(default, skip_serializing)]
    password: Option<SmtpPasswordRemoved>,
}

/// Resolves a `server` into a URL, `scheme` filling in for a value with none.
///
/// The presence of `://` tells them apart, and it has to: a bare authority
/// is not a relative URL, `url` reading `dav.example.org:8443` as a scheme
/// with a path, which parses cleanly and carries no host.
#[cfg_attr(
    not(any(feature = "imap", feature = "smtp", feature = "dav")),
    allow(dead_code)
)]
pub fn server_url(server: &str, scheme: &str) -> Result<Url> {
    let url = if server.contains("://") {
        Url::parse(server)
    } else {
        Url::parse(&format!("{scheme}://{server}"))
    };

    url.with_context(|| format!("Cannot parse {server} as a server URL"))
}

fn default_gmail_user_id() -> String {
    String::from("me")
}

fn default_msgraph_user_id() -> String {
    String::from("me")
}

/// Default ALPN list for the HTTP backends: their APIs ride on HTTP/1.1.
fn default_http_alpn() -> Vec<String> {
    vec![String::from("http/1.1")]
}

/// How a connection is secured, shared by every backend.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct TlsConfig {
    /// Which TLS provider to use, the build's own default when unset.
    pub provider: Option<TlsProviderConfig>,
    /// Options the rustls provider reads.
    #[serde(default)]
    pub rustls: RustlsConfig,
    /// Path to an extra CA certificate to trust, shell-expanded.
    #[serde(default, deserialize_with = "opt_shell_expanded_path")]
    pub cert: Option<PathBuf>,
}

/// Which TLS implementation carries the connection.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum TlsProviderConfig {
    /// The rustls provider.
    Rustls,
    /// The platform's own TLS stack.
    NativeTls,
}

/// Options the rustls provider reads.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct RustlsConfig {
    /// Which crypto backend rustls uses, its default when unset.
    pub crypto: Option<RustlsCryptoConfig>,
}

/// Which crypto backend rustls uses.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum RustlsCryptoConfig {
    /// The aws-lc-rs backend.
    Aws,
    /// The ring backend.
    Ring,
}

#[cfg_attr(
    not(any(feature = "imap", feature = "msgraph", feature = "smtp")),
    allow(dead_code)
)]
impl TlsConfig {
    /// Builds the runtime [`Tls`] handle the connect helpers expect.
    ///
    /// `alpn` is the protocol-level list, empty to skip ALPN. The schema
    /// never exposes `tls.rustls.alpn`: the per-protocol `*.alpn` field is
    /// folded in here.
    pub fn into_tls(self, alpn: Vec<String>) -> Tls {
        Tls {
            provider: self.provider.map(|p| match p {
                TlsProviderConfig::Rustls => TlsProvider::Rustls,
                TlsProviderConfig::NativeTls => TlsProvider::NativeTls,
            }),
            rustls: Rustls {
                crypto: self.rustls.crypto.map(|c| match c {
                    RustlsCryptoConfig::Aws => RustlsCrypto::Aws,
                    RustlsCryptoConfig::Ring => RustlsCrypto::Ring,
                }),
                alpn,
            },
            server: self.cert.map(|cert| TlsServer { cert }),
            client: None,
        }
    }
}

/// The one SASL mechanism a session authenticates with.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum SaslConfig {
    /// Authenticates nobody, for a relay taking anonymous mail.
    Anonymous(SaslAnonymousConfig),
    /// The legacy LOGIN exchange.
    Login(SaslLoginConfig),
    /// A username and a password in one message.
    Plain(SaslPlainConfig),
    /// A bearer token in the GS2 header.
    Oauthbearer(SaslOauthbearerConfig),
    /// A bearer token in the vendors' own spelling.
    Xoauth2(SaslXoauth2Config),
    /// A salted challenge, so the password never crosses.
    #[serde(rename = "scram-sha-256")]
    ScramSha256(SaslScramSha256Config),
}

/// The ANONYMOUS mechanism (RFC 4505), which authenticates nobody.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SaslAnonymousConfig {
    /// The trace message the mechanism carries, if any.
    pub message: Option<String>,
}

/// The legacy LOGIN mechanism, a username and a password.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SaslLoginConfig {
    /// The identity the mechanism authenticates as.
    #[serde(deserialize_with = "shell_expanded_string")]
    pub username: String,
    /// The password, resolved once per run.
    pub password: Secret,
}

/// The PLAIN mechanism (RFC 4616).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SaslPlainConfig {
    /// The authorization identity, where it differs from the
    /// authentication one.
    pub authzid: Option<String>,
    /// The authentication identity.
    #[serde(deserialize_with = "shell_expanded_string")]
    #[serde(alias = "username")]
    pub authcid: String,
    /// The password, resolved once per run.
    #[serde(alias = "password")]
    pub passwd: Secret,
}

/// The OAUTHBEARER mechanism (RFC 7628).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SaslOauthbearerConfig {
    /// The identity the mechanism authenticates as.
    #[serde(deserialize_with = "shell_expanded_string")]
    pub username: String,
    /// The access token, resolved once per run.
    pub token: Secret,
}

/// The XOAUTH2 mechanism, Google and Microsoft's predecessor to OAUTHBEARER.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SaslXoauth2Config {
    /// The identity the mechanism authenticates as.
    #[serde(deserialize_with = "shell_expanded_string")]
    pub username: String,
    /// The access token, resolved once per run.
    pub token: Secret,
}

/// The SCRAM-SHA-256 mechanism (RFC 7677), which sends no password.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SaslScramSha256Config {
    /// The identity the mechanism authenticates as.
    #[serde(deserialize_with = "shell_expanded_string")]
    pub username: String,
    /// The password, resolved once per run.
    pub password: Secret,
}

#[cfg_attr(not(any(feature = "imap", feature = "smtp")), allow(dead_code))]
impl SaslConfig {
    /// Resolves the SASL config into a runtime [`Sasl`].
    ///
    /// `host` and `port` come from the live server URL and only OAUTHBEARER
    /// uses them, in the GS2 header. The resolver keeps an account's IMAP
    /// and SMTP tables, which usually name one entry, from unlocking twice.
    pub fn try_into_sasl(
        self,
        host: impl ToString,
        port: u16,
        resolver: &mut SecretResolver,
    ) -> Result<Sasl> {
        Ok(match self {
            SaslConfig::Anonymous(c) => Sasl::Anonymous(SaslAnonymousCreds { message: c.message }),
            SaslConfig::Login(c) => Sasl::Login(SaslLoginCreds {
                username: c.username,
                password: resolver.resolve(c.password)?,
            }),
            SaslConfig::Plain(c) => Sasl::Plain(SaslPlainCreds {
                authzid: c.authzid,
                authcid: c.authcid,
                passwd: resolver.resolve(c.passwd)?,
            }),
            SaslConfig::Oauthbearer(c) => Sasl::Oauthbearer(SaslOauthbearerCreds {
                username: c.username,
                host: host.to_string(),
                port,
                token: resolver.resolve(c.token)?,
            }),
            SaslConfig::Xoauth2(c) => Sasl::Xoauth2(SaslXoauth2Creds {
                username: c.username,
                token: resolver.resolve(c.token)?,
            }),
            SaslConfig::ScramSha256(c) => Sasl::ScramSha256(SaslScramCreds {
                username: c.username,
                password: resolver.resolve(c.password)?,
                nonce: Vec::new(),
                channel_binding: SaslGs2ChannelBinding::Unsupported,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    /// Two accounts, `home` declared after `work`, one of them still
    /// claiming the `default` an earlier neverest read.
    fn two_accounts() -> Config {
        toml::from_str(
            r#"
            [accounts.work]
            default = true
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

    /// With no `-a`, a command runs over every account in name order, the
    /// `default` key making none of them special; `-a` filters them.
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

    #[test]
    fn msgraph_auth_is_bearer_token_only() {
        let config: MsgraphConfig = toml::from_str(
            r#"
            auth.token.raw = "tok"
            "#,
        )
        .unwrap();
        assert_eq!(config.user_id, "me");

        let config: MsgraphConfig = toml::from_str(
            r#"
            user-id = "user@example.org"
            auth.token.command = ["ortie", "-a", "msgraph", "token", "show", "--auto-refresh"]
            "#,
        )
        .unwrap();
        assert_eq!(config.user_id, "user@example.org");

        let err = toml::from_str::<MsgraphConfig>(
            r#"
            [auth.device-code]
            client-id = "id"
            "#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("device-code"));
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

    /// Streaming is an optimisation of `retain = false`, never a mode: it
    /// needs both endpoints on a protocol that can take a body from the other.
    #[test]
    fn only_an_imap_pairing_streams_a_crossing() {
        let imap: AccountConfig = toml::from_str(
            r#"
            one-way = true
            sources.a.imap.server = "imaps://a.example.org:993"
            targets.b.imap.server = "imaps://b.example.org:993"
            "#,
        )
        .unwrap();
        assert!(imap.mode().unwrap().streams(&imap.endpoints().unwrap()));

        let dav: AccountConfig = toml::from_str(
            r#"
            one-way = true
            sources.a.carddav.server = "https://a.example.org/"
            sources.a.carddav.auth.bearer.token.raw = "tok"
            targets.b.carddav.server = "https://b.example.org/"
            targets.b.carddav.auth.bearer.token.raw = "tok"
            "#,
        )
        .unwrap();
        assert!(
            !dav.mode().unwrap().streams(&dav.endpoints().unwrap()),
            "a DAV crossing is staged and released, which `retain` cannot tell apart",
        );
    }

    #[test]
    fn a_removed_key_is_refused_by_name_with_its_replacement() {
        let err = toml::from_str::<AccountConfig>(
            r#"
            left.imap.server = "imaps://imap.example.org:993"
            right.imap.server = "imaps://imap.other.org:993"
            "#,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("`targets`"), "got {err}");
        assert!(err.contains("one-way"), "got {err}");

        let err = toml::from_str::<AccountConfig>(
            r#"
            imap.server = "imaps://imap.example.org:993"
            collection.filter.include = ["INBOX"]
            "#,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("collection.filter"), "got {err}");

        let err = toml::from_str::<AccountConfig>(
            r#"
            imap.server = "imaps://imap.example.org:993"
            store.retention = "retain"
            "#,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("the account's `retain`"), "got {err}");

        assert!(
            toml::from_str::<AccountConfig>(
                r#"
            imap.server = "imaps://imap.example.org:993"
            store.hydration = "full"
            "#,
            )
            .is_err()
        );
    }

    #[test]
    fn an_account_with_no_source_is_refused() {
        let account: AccountConfig = toml::from_str("default = true").unwrap();
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

    /// The send channel spells its credentials as the sync side does, and
    /// omits the table for a relay that takes no `AUTH`.
    #[test]
    fn the_send_channel_names_a_sasl_mechanism() {
        let account: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://imap.example.org:993"
            smtp.server = "smtp.example.org"
            smtp.sasl.xoauth2.username = "user@example.org"
            smtp.sasl.xoauth2.token.command = ["ortie", "token", "read", "example"]
            "#,
        )
        .unwrap();

        let smtp = account.smtp.as_ref().expect("a declared channel");
        assert_eq!(smtp.server, "smtp.example.org");
        assert!(matches!(smtp.sasl, Some(SaslConfig::Xoauth2(_))));

        let relay: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://imap.example.org:993"
            smtp.server = "smtp://127.0.0.1:2525"
            "#,
        )
        .unwrap();
        assert!(relay.smtp.expect("a declared channel").sasl.is_none());
    }

    /// Ignoring the retired flat spelling would open an unauthenticated
    /// session against a server that requires one, so it is refused.
    #[test]
    fn the_flat_login_and_password_spelling_is_refused() {
        let err = toml::from_str::<AccountConfig>(
            r#"
            imap.server = "imaps://imap.example.org:993"
            smtp.server = "smtps://smtp.example.org:465"
            smtp.login = "user@example.org"
            smtp.password.raw = "pw"
            "#,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("`smtp.sasl.plain.username`"), "got {err}");
    }

    /// A bare authority with a port used to reach a backend as a hostless
    /// URL, `url` reading `dav.example.org:8443` as a scheme and a path.
    #[test]
    fn a_server_resolves_from_an_authority_with_or_without_a_port() {
        for (server, scheme, host, port) in [
            (
                "dav.example.org:8443",
                "https",
                "dav.example.org",
                Some(8443),
            ),
            ("dav.example.org", "https", "dav.example.org", None),
            (
                "imap.example.org:143",
                "imaps",
                "imap.example.org",
                Some(143),
            ),
            ("smtp.example.org", "smtps", "smtp.example.org", None),
        ] {
            let url = server_url(server, scheme).unwrap();
            assert_eq!(url.scheme(), scheme, "{server}");
            assert_eq!(url.host_str(), Some(host), "{server}");
            assert_eq!(url.port(), port, "{server}");
        }
    }

    /// A value carrying a scheme is used verbatim, so an explicit
    /// cleartext or non-default port survives the resolution.
    #[test]
    fn a_server_carrying_a_scheme_is_left_alone() {
        let url = server_url("http://127.0.0.1:5232/dav/", "https").unwrap();
        assert_eq!(url.scheme(), "http");
        assert_eq!(url.port(), Some(5232));
        assert_eq!(url.path(), "/dav/");

        let url = server_url("imap://example.org:143", "imaps").unwrap();
        assert_eq!(url.scheme(), "imap");
    }

    #[test]
    fn the_pre_generic_pim_sync_spellings_still_load() {
        let account: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://imap.example.org:993"
            imap.mailbox.create = false
            imap.mailbox.delete = false
            imap.message.create = true
            imap.message.delete = false
            "#,
        )
        .unwrap();

        let sources = account.sources().unwrap();
        let perms = sources["imap"].permissions();
        assert!(!perms.collection.create);
        assert!(!perms.collection.delete);
        assert!(perms.item.create);
        assert!(!perms.item.delete);
        assert!(perms.flag.update);
        assert!(perms.item.update);

        let account: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://imap.example.org:993"
            imap.collection.create = false
            imap.collection.delete = false
            imap.collection.filter.include = ["INBOX"]
            imap.item.create = true
            imap.item.delete = false
            "#,
        )
        .unwrap();
        let sources = account.sources().unwrap();
        assert_eq!(
            sources["imap"].collection().filter,
            CollectionFilter::Include(vec![String::from("INBOX")])
        );
        let perms = sources["imap"].permissions();
        assert!(!perms.collection.create);
        assert!(!perms.collection.delete);
        assert!(perms.item.create);
        assert!(!perms.item.delete);
    }

    /// A collection is deleted on a server only where the user asked for it:
    /// absence is the only signal IMAP and DAV leave, and a listing may drop
    /// a collection nobody deleted (pimdir SYNC §5).
    #[test]
    fn a_collection_delete_is_granted_only_when_asked_for() {
        for document in [
            "imap.server = \"imaps://imap.example.org:993\"",
            "imap.server = \"imaps://imap.example.org:993\"\n\
             imap.collection.filter.include = [\"INBOX\"]",
        ] {
            let account: AccountConfig = toml::from_str(document).unwrap();
            let perms = account.sources().unwrap()["imap"].permissions();
            assert!(perms.collection.create, "{document}");
            assert!(!perms.collection.delete, "{document}");
        }

        let account: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://imap.example.org:993"
            imap.collection.delete = true
            "#,
        )
        .unwrap();
        assert!(
            account.sources().unwrap()["imap"]
                .permissions()
                .collection
                .delete
        );
    }

    #[test]
    fn item_update_is_denied_only_when_asked_for() {
        let account: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://imap.example.org:993"
            imap.item.create = true
            imap.item.delete = true
            imap.item.update = false
            "#,
        )
        .unwrap();
        let sources = account.sources().unwrap();
        let perms = sources["imap"].permissions();
        assert!(perms.item.create);
        assert!(perms.item.delete);
        assert!(!perms.item.update);

        let account: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://imap.example.org:993"
            imap.item.create = true
            imap.item.delete = true
            "#,
        )
        .unwrap();
        let sources = account.sources().unwrap();
        assert!(sources["imap"].permissions().item.update);
    }

    /// Every path key expands once, at deserialize, so no call site can read
    /// a literal `./~/…` directory. A document re-serialized after that
    /// carries the expansion and reloads to the same value.
    #[test]
    fn a_tilde_path_is_expanded_at_deserialize() {
        let home = dirs::home_dir().unwrap();

        let store: StoreConfig = toml::from_str(r#"root = "~/store""#).unwrap();
        assert_eq!(store.root, Some(home.join("store")));

        let tls: TlsConfig = toml::from_str(r#"cert = "~/ca.pem""#).unwrap();
        assert_eq!(tls.cert, Some(home.join("ca.pem")));

        let reloaded: TlsConfig = toml::from_str(&toml::to_string(&tls).unwrap()).unwrap();
        assert_eq!(reloaded.cert, tls.cert);

        // NOTE: an absent key never reaches the deserializer, so it stays
        // absent rather than expanding an empty path.
        let tls: TlsConfig = toml::from_str("").unwrap();
        assert_eq!(tls.cert, None);
    }

    /// A path key expands environment variables too, as every other
    /// Pimalaya path does through pimalaya-config, where the tilde alone
    /// used to.
    #[test]
    fn a_variable_in_a_path_is_expanded_at_deserialize() {
        let home = PathBuf::from(std::env::var("HOME").unwrap());

        let store: StoreConfig = toml::from_str(r#"root = "$HOME/store""#).unwrap();
        assert_eq!(store.root, Some(home.join("store")));

        let tls: TlsConfig = toml::from_str(r#"cert = "${HOME}/ca.pem""#).unwrap();
        assert_eq!(tls.cert, Some(home.join("ca.pem")));
    }

    #[test]
    fn the_documented_sample_still_loads() {
        let raw = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/config.sample.toml"))
            .expect("read the sample");
        let config: Config = toml::from_str(&raw).expect("the sample must parse");

        let account = config.accounts.get("example").expect("the sample account");
        account.validate().expect("the sample must validate");
        assert!(account.sources().unwrap()["imap"].is_imap());
    }

    #[test]
    fn the_purge_delay_is_a_human_duration_and_drives_the_cutoff() {
        let now: DateTime<Utc> = "2026-08-07T12:00:00Z".parse().unwrap();

        let account: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://imap.example.org:993"
            "#,
        )
        .unwrap();
        assert!(account.store.purge_after.is_none());
        assert!(account.store.purge_cutoff(now).is_none());

        let account: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://imap.example.org:993"
            store.purge-after = "90d"
            "#,
        )
        .unwrap();
        assert_eq!(
            account.store.purge_cutoff(now).as_deref(),
            Some("2026-05-09T12:00:00.000Z")
        );

        let account: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://imap.example.org:993"
            store.purge-after = "0"
            "#,
        )
        .unwrap();
        assert_eq!(
            account.store.purge_cutoff(now).as_deref(),
            Some("2026-08-07T12:00:00.000Z")
        );

        let err = toml::from_str::<AccountConfig>(
            r#"
            imap.server = "imaps://imap.example.org:993"
            store.purge-after = "90 days"
            "#,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("90 days"), "{err}");
    }

    /// The keys the sibling CLIs read and neverest did not load: a DAV home
    /// set with no server and no credential, the IMAP `SASL-IR` and `ID`
    /// quirks, and a proxy at the account, under a backend and its channel.
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

    /// A proxy table is spelled as the siblings spell it: a typo is refused.
    #[test]
    fn a_proxy_table_refuses_an_unknown_key() {
        let err = toml::from_str::<AccountConfig>("proxy.uri = \"http://p:1\"").unwrap_err();
        assert!(err.to_string().contains("uri"), "{err}");
    }

    /// A duration is pimalaya-config's: an integer of seconds or a number
    /// and its unit, written back in the largest unit dividing it.
    #[test]
    fn a_duration_reads_seconds_or_a_unit_and_round_trips() {
        let cases = [
            ("0", 0, r#""0""#),
            (r#""0""#, 0, r#""0""#),
            ("45", 45, r#""45s""#),
            (r#""45s""#, 45, r#""45s""#),
            (r#""30m""#, 1800, r#""30m""#),
            (r#""12h""#, 43200, r#""12h""#),
            ("7776000", 7776000, r#""90d""#),
            (r#""90d""#, 7776000, r#""90d""#),
            (r#""2w""#, 1209600, r#""2w""#),
            ("90061", 90061, r#""90061s""#),
        ];

        for (raw, secs, written) in cases {
            let store: StoreConfig = toml::from_str(&format!("purge-after = {raw}")).unwrap();
            assert_eq!(store.purge_after, Some(Duration::from_secs(secs)), "{raw}");
            let document = toml::to_string(&store).unwrap();
            assert_eq!(document.trim(), format!("purge-after = {written}"));

            let store: StoreConfig = toml::from_str(&format!("index-timeout = {raw}")).unwrap();
            assert_eq!(
                store.index_timeout,
                Some(Duration::from_secs(secs)),
                "{raw}"
            );
        }

        for raw in [r#""""#, r#""90""#, r#""90y""#, r#""d""#, "-1"] {
            let toml = format!("purge-after = {raw}");
            assert!(toml::from_str::<StoreConfig>(&toml).is_err(), "{raw}");
        }
    }

    #[test]
    fn a_source_pairs_one_backend_with_its_send_channel() {
        let account: AccountConfig = toml::from_str(
            r#"
            msgraph.auth.token.raw = "tok"
            "#,
        )
        .unwrap();
        let sources = account.sources().unwrap();
        assert!(sources["msgraph"].sends_natively());
        assert!(sources["msgraph"].smtp.is_none());

        let err = toml::from_str::<AccountConfig>(
            r#"
            sources.a.imapp.server = "imaps://imap.example.org:993"
            "#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("unknown field `imapp`"), "{err}");

        let account: AccountConfig = toml::from_str(
            r#"
            sources.a.imap.server = "imaps://imap.example.org:993"
            sources.a.smtp.server = "smtps://smtp.example.org:465"
            "#,
        )
        .unwrap();
        let source = &account.sources().unwrap()["a"];
        assert!(source.is_imap());
        assert!(source.smtp.is_some());
    }

    /// The DAV sources are the non-mail ones, so they are where the account
    /// shape stops being mail-shaped.
    #[cfg(feature = "dav")]
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

    #[test]
    fn a_graph_contacts_source_is_neither_mail_nor_a_send_channel() {
        let account: AccountConfig = toml::from_str(
            r#"
            msgraph.auth.token.raw = "tok"
            msgraph-contacts.auth.token.raw = "tok"
            msgraph-calendar.auth.token.raw = "tok"
            sources.work.msgraph-contacts.auth.token.raw = "tok"
            "#,
        )
        .unwrap();

        let sources = account.sources().unwrap();
        assert!(sources["msgraph"].sends_natively());
        for name in ["msgraph-contacts", "msgraph-calendar", "work"] {
            assert!(!sources[name].carries_mail(), "{name} does not submit");
            assert!(!sources[name].sends_natively(), "{name} does not send");
        }
    }

    #[test]
    fn a_dav_source_carries_no_send_channel() {
        let account: AccountConfig = toml::from_str(
            r#"
            carddav.server = "https://dav.example.org/"
            carddav.auth.basic.username = "user"
            carddav.auth.basic.password.raw = "pw"
            caldav.server = "https://dav.example.org/"
            caldav.auth.basic.username = "user"
            caldav.auth.basic.password.raw = "pw"
            "#,
        )
        .unwrap();

        let sources = account.sources().unwrap();
        for name in ["carddav", "caldav"] {
            assert!(!sources[name].carries_mail(), "{name} does not submit");
            assert!(!sources[name].sends_natively());
        }
        account.validate().unwrap();

        let account: AccountConfig = toml::from_str(
            r#"
            sources.dav.carddav.server = "https://dav.example.org/"
            sources.dav.carddav.auth.bearer.token.raw = "tok"
            sources.dav.smtp.server = "smtps://smtp.example.org:465"
            "#,
        )
        .unwrap();

        let err = account.validate().unwrap_err().to_string();
        assert!(err.contains("`sources.dav.smtp`"), "got {err}");
    }

    /// The removed key is refused by name on whichever endpoint carries it,
    /// rather than being ignored and leaving the account doing something else.
    #[test]
    fn a_declared_namespace_is_refused_by_name() {
        let err = toml::from_str::<AccountConfig>(
            r#"
            sources.a.imap.server = "imaps://a.example.org:993"
            sources.a.imap.collection.namespace = "mail"
            "#,
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("The `collection.namespace` key is removed"),
            "got {err}"
        );
        assert!(err.contains("one-way"), "got {err}");
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

    /// A source is one backend: a second one is refused, never dropped.
    #[test]
    fn a_source_declaring_two_backends_is_refused() {
        let err = toml::from_str::<AccountConfig>(
            r#"
            sources.a.imap.server = "imaps://imap.example.org:993"
            sources.a.msgraph.auth.token.raw = "tok"
            "#,
        )
        .map(|_| String::from("loaded"))
        .unwrap_or_else(|err| err.to_string());
        assert!(err.contains("`imap` and `msgraph`"), "{err}");

        let err = toml::from_str::<AccountConfig>("sources.a.smtp.server = \"x\"")
            .map(|_| String::from("loaded"))
            .unwrap_or_else(|err| err.to_string());
        assert!(err.contains("declares no backend"), "{err}");
    }

    /// A key an earlier neverest read is refused by name as the file is
    /// parsed, pointing at the migration notes rather than reading as a
    /// typo.
    #[test]
    fn a_removed_key_points_at_the_migration_notes() {
        let mut missed = Vec::new();

        for (document, named) in [
            (
                "left.imap.server = \"x\"\nright.imap.server = \"y\"",
                "The `left` table is removed, ",
            ),
            (
                "imap.server = \"x\"\nfolder.filter = \"all\"",
                "The `folder` table is removed, ",
            ),
            (
                "imap.server = \"x\"\nenvelope.filter.before = \"2024-01-01\"",
                "The `envelope` table is removed, ",
            ),
            (
                "imap.server = \"x\"\ncollection.filter.include = [\"INBOX\"]",
                "The `collection` table is removed, ",
            ),
            (
                "imap.server = \"x\"\nimap.collection.namespace = \"mail\"",
                "The `collection.namespace` key is removed, ",
            ),
            (
                "imap.server = \"x\"\nstore.retention = \"retain\"",
                "The `store.retention` key is removed, ",
            ),
            (
                "imap.server = \"x\"\nstore.hydration = \"full\"",
                "The `store.hydration` key is removed, ",
            ),
            (
                "imap.server = \"x\"\nsmtp.server = \"y\"\nsmtp.login = \"u\"\n\
                 smtp.password.raw = \"pw\"",
                "The `smtp.login` key is removed, ",
            ),
            (
                "sources.a.imap.server = \"x\"\nsources.a.smtp.server = \"y\"\n\
                 sources.a.smtp.password.raw = \"pw\"",
                "The `smtp.password` key is removed, ",
            ),
        ] {
            // NOTE: refused as the file is parsed, so an account no command
            // opens is refused too.
            let err = match toml::from_str::<AccountConfig>(document) {
                Ok(_) => String::from("parsed"),
                Err(err) => err.to_string(),
            };
            if !err.contains(named) || !err.contains(", see MIGRATION.md") {
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

        assert!(keys > 100, "{keys} keys read from the sample");
    }

    fn at(instant: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(instant).unwrap().to_utc()
    }

    /// A duration counts back from today to the start of its UTC day, so
    /// two runs of one day list one scope; a date is its midnight UTC, and
    /// an instant is normalised to UTC.
    #[test]
    fn a_scope_floor_reads_as_a_duration_a_date_or_an_instant() {
        let now = at("2026-10-07T15:42:10+02:00");

        assert_eq!(resolve_since("30d", now).unwrap(), "2026-09-07T00:00:00Z");
        assert_eq!(
            resolve_since(" 30 days ", now).unwrap(),
            "2026-09-07T00:00:00Z"
        );
        assert_eq!(resolve_since("2w", now).unwrap(), "2026-09-23T00:00:00Z");
        assert_eq!(resolve_since("6mo", now).unwrap(), "2026-04-07T00:00:00Z");
        assert_eq!(resolve_since("1y", now).unwrap(), "2025-10-07T00:00:00Z");
        assert_eq!(resolve_since("0d", now).unwrap(), "2026-10-07T00:00:00Z");
        assert_eq!(
            resolve_since("2026-01-01", now).unwrap(),
            "2026-01-01T00:00:00Z"
        );
        assert_eq!(
            resolve_since("2026-01-01T10:00:00+02:00", now).unwrap(),
            "2026-01-01T08:00:00Z"
        );

        assert!(resolve_since("30m", now).is_err(), "minutes are not a unit");
        assert!(resolve_since("soon", now).is_err());
        assert!(resolve_since("", now).is_err());
    }

    /// `item.filter.since` scopes mail, `--since` overrides it, and either is
    /// refused by its own name on an account syncing contacts.
    #[test]
    fn a_scope_is_read_from_the_flag_or_the_key_and_refused_beside_contacts() {
        let now = at("2026-10-07T12:00:00Z");
        let mail: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://imap.example.org:993"
            item.filter.since = "30d"
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
            item.filter.since = "30d"
            "#,
        )
        .unwrap();
        let err = mixed.scope(None, &[], now).unwrap_err().to_string();
        assert!(err.contains("`item.filter.since`"), "got {err}");
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
            item.filter.since = "a while"
            "#,
        )
        .unwrap();
        let err = format!("{:#}", bad.validate().unwrap_err());
        assert!(err.contains("item.filter.since"), "got {err}");
    }
}
