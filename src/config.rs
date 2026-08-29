//! # Configuration
//!
//! The TOML schema: each account holds named [`SourceConfig`]s over one
//! pimdir store, plus that store's settings.

use std::{
    collections::HashMap,
    fmt, fs,
    io::{IsTerminal, stdin},
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, SecondsFormat, Utc};
use io_sasl::{
    login::SaslLoginCreds, mechanism::Sasl, rfc4505::anonymous::SaslAnonymousCreds,
    rfc4616::plain::SaslPlainCreds, rfc5801::SaslGs2ChannelBinding, rfc5802::SaslScramCreds,
    rfc7628::oauthbearer::SaslOauthbearerCreds, xoauth2::SaslXoauth2Creds,
};
use pimalaya_cli::printer::Printer;
use pimalaya_config::{
    command::CommandConfig,
    secret::{Secret, SecretResolver},
    toml as config_toml,
    toml::{TomlConfig, shell_expanded_string},
};
use pimalaya_stream::tls::{Rustls, RustlsCrypto, Tls, TlsProvider};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::wizard::discover::{CONFIG_SAMPLE_URL, offer_configuration};

/// `skip_serializing_if` predicate omitting a defaulted field, so what the
/// wizard writes carries only what the user chose.
fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
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
            #[serde(default, alias = "mailbox", skip_serializing_if = "is_default")]
            pub collection: CollectionSourceConfig,
            #[serde(default, skip_serializing_if = "is_default")]
            pub flag: FlagSourcePermissions,
            #[serde(default, alias = "message", skip_serializing_if = "is_default")]
            pub item: ItemSourcePermissions,
            /// Connection pool size override; the default is per backend.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub pool_size: Option<usize>,
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
            }
        }
    };
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    pub accounts: HashMap<String, AccountConfig>,
}

impl TomlConfig for Config {
    type Account = AccountConfig;

    fn project_name() -> &'static str {
        env!("CARGO_PKG_NAME")
    }

    fn take_named_account(&mut self, name: &str) -> Option<(String, Self::Account)> {
        self.accounts.remove_entry(name)
    }

    fn take_default_account(&mut self) -> Option<(String, Self::Account)> {
        let name = self
            .accounts
            .iter()
            .find_map(|(name, account)| account.default.then(|| name.clone()))?;

        self.take_named_account(&name)
    }
}

impl Config {
    /// Loads `Config` from `config_paths`, or offers the wizard when no
    /// file exists.
    ///
    /// A missing configuration is met with the wizard rather than an error:
    /// the command carries on either way, accepting giving it a chance to
    /// work and declining leaving it to fail on what it has not got.
    pub fn load_or_wizard(printer: &mut impl Printer, config_paths: &[PathBuf]) -> Result<Config> {
        if let Some(config) = Config::from_paths_or_default(config_paths)? {
            return Ok(config);
        }

        let target = Config::target_path(config_paths)?;

        if !printer.is_json() && stdin().is_terminal() {
            offer_configuration(printer, &target)?;
        }

        match Config::from_paths_or_default(config_paths)? {
            Some(config) => Ok(config),
            None => bail!(
                "No configuration found at {}, run `neverest` to generate one or write it by hand: {CONFIG_SAMPLE_URL}",
                target.display(),
            ),
        }
    }

    /// Serializes `self` to TOML at `path`, creating missing parents.
    ///
    /// The document renders like himalaya's: one `[accounts.<name>]` header
    /// per account, every field below it a dotted key.
    pub fn write(&self, path: &Path) -> Result<()> {
        let toml = config_toml::to_string(self).context("Serialize TOML config error")?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Create TOML config parent {} error", parent.display()))?;
        }

        fs::write(path, toml)
            .with_context(|| format!("Write TOML config {} error", path.display()))?;

        Ok(())
    }
}

/// Per-account configuration: named sources over one pimdir store.
///
/// An account is the hub: one store, one blob directory. A source's name
/// is its pimdir source id, so renaming one orphans its bindings, and a
/// backend written under the account is sugar for a source named after it.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct AccountConfig {
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

    /// Makes the `sources` side authoritative, the other side's change being
    /// discarded rather than merged, so no conflict is recorded.
    ///
    /// The other side is still enumerated every run, or every item would be
    /// re-pushed; its state decides what is left to do and never who wins.
    /// Changes are overwritten, not merged.
    #[serde(default, skip_serializing_if = "is_default")]
    pub one_way: bool,

    /// Whether the store holds bodies and is readable by a frontend, rather
    /// than being only the ledger of spines and checkpoints.
    ///
    /// Unset takes the destination's answer: true with no targets, the store
    /// being what the account syncs into; false with targets, which asked to
    /// copy rather than to fill a disk. Set it to keep a copy of a migration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retain: Option<bool>,

    /// Direct-backend sugar: each is a source named after its protocol.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imap: Option<ImapConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carddav: Option<CarddavConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caldav: Option<CaldavConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jmap: Option<JmapConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gmail: Option<GmailConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msgraph: Option<MsgraphConfig>,

    /// The send channel of the sugar source carrying mail, the flat
    /// spelling of `sources.<name>.smtp`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smtp: Option<SmtpConfig>,

    /// The local pimdir store this account syncs through.
    ///
    /// Optional: the store is implicit, a per-account state directory, and
    /// is customised only here, never declared as a source.
    #[serde(default)]
    pub store: StoreConfig,

    /// How a run announces a content conflict it could not merge away.
    #[serde(default)]
    pub conflict: ConflictConfig,

    // TODO: item-level sync filters (date range, sender, subject).
    #[serde(default, alias = "message")]
    pub item: ItemSyncConfig,

    /// Max connections per source for concurrent body fetches, 4 by default.
    ///
    /// Keep it under the provider's per-account connection limit. `sync
    /// --connections N` overrides it for one run.
    #[serde(default)]
    pub connections: Option<usize>,

    /// Removed keys, kept so a configuration carrying one is refused by name
    /// rather than as an unknown field. See [`AccountConfig::validate`].
    #[serde(default, skip_serializing)]
    left: Option<RemovedKey>,
    #[serde(default, skip_serializing)]
    right: Option<RemovedKey>,
    #[serde(default, skip_serializing, alias = "mailbox")]
    collection: Option<RemovedKey>,
}

impl AccountConfig {
    /// A single-source account, the only shape the wizard writes: one
    /// provider, one protocol, a store keeping every body.
    pub fn with_source(default: bool, source: SourceConfig) -> Self {
        let mut account = Self {
            default,
            ..Self::default()
        };
        account.set_direct_source(source);
        account
    }

    /// Writes `source` as the direct-backend sugar, replacing the backend of
    /// that protocol and lifting its send channel to the account `smtp`.
    pub fn set_direct_source(&mut self, source: SourceConfig) {
        let SourceConfig { backend, smtp } = source;

        self.smtp = smtp;

        match backend {
            SourceBackendConfig::Imap(config) => self.imap = Some(config),
            SourceBackendConfig::Carddav(config) => self.carddav = Some(config),
            SourceBackendConfig::Caldav(config) => self.caldav = Some(config),
            SourceBackendConfig::Jmap(config) => self.jmap = Some(config),
            SourceBackendConfig::Gmail(config) => self.gmail = Some(config),
            SourceBackendConfig::Msgraph(config) => self.msgraph = Some(config),
        }
    }

    /// The direct-backend sources in protocol order, what the wizard owns
    /// and `configure` re-runs over.
    ///
    /// A source from the explicit `sources` table is hand-written and never
    /// appears here.
    pub fn direct_sources(&self) -> Vec<SourceConfig> {
        [
            self.imap.clone().map(SourceBackendConfig::Imap),
            self.carddav.clone().map(SourceBackendConfig::Carddav),
            self.caldav.clone().map(SourceBackendConfig::Caldav),
            self.jmap.clone().map(SourceBackendConfig::Jmap),
            self.gmail.clone().map(SourceBackendConfig::Gmail),
            self.msgraph.clone().map(SourceBackendConfig::Msgraph),
        ]
        .into_iter()
        .flatten()
        .map(|backend| SourceConfig {
            backend,
            smtp: self.smtp.clone(),
        })
        .collect()
    }

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

    /// The account's mode: which endpoints, which direction, whether the
    /// store keeps bodies.
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

    /// Rejects an account a command cannot run: a removed key, no source, a
    /// source declaring what its backend cannot honour, two senders.
    ///
    /// Run by every command that opens an account, so a bad configuration is
    /// refused before a connection rather than halfway through a sync.
    pub fn validate(&self) -> Result<()> {
        self.reject_removed_keys()?;

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

        // Called for its refusals: `check` and the sync read the same mode.
        self.mode()?;

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

    /// Refuses a key this version removed, naming what replaces it.
    ///
    /// Refused rather than ignored: silently honouring neither the old
    /// meaning nor the new one is how a configuration ends up doing the
    /// opposite of what it says.
    fn reject_removed_keys(&self) -> Result<()> {
        if self.left.is_some() || self.right.is_some() {
            bail!(
                "`left` and `right` are gone: an account names its endpoints and the direction \
                 between them. Write the authoritative one under `sources` and the other under \
                 `targets`, and add `one-way = true` to copy rather than merge; leaving it off \
                 keeps them syncing both ways, which is what the pair used to do."
            );
        }

        if self.collection.is_some() {
            bail!(
                "The account-level `collection` table is gone: a filter belongs to the source it \
                 filters, since an account may hold sources of several kinds. Write it as \
                 `sources.<name>.collection.filter`, or `<protocol>.collection.filter` under the \
                 account."
            );
        }

        let namespaced: Vec<_> = self
            .sources
            .iter()
            .chain(&self.targets)
            .filter(|(_, source)| source.declares_namespace())
            .map(|(name, _)| name.as_str())
            .collect();

        if !namespaced.is_empty() {
            bail!(
                "`collection.namespace` is gone, on {}: it said which sources met, which is now \
                 whether they sit under `sources` or `targets`, and it never said which way, \
                 which is now `one-way`. Drop it.",
                namespaced.join(", "),
            );
        }

        self.store.reject_removed_keys()
    }
}

/// Presence marker for a configuration key this version removed.
///
/// Deserializing accepts whatever the key held and discards it, so the
/// account refuses it by name with its replacement rather than as an
/// unknown field with no explanation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RemovedKey;

impl<'de> Deserialize<'de> for RemovedKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        serde::de::IgnoredAny::deserialize(deserializer)?;
        Ok(Self)
    }
}

/// How an account announces a content conflict, the only part of conflict
/// handling anybody configures.
///
/// Whether a run merges is not a setting: the three-way merge is a pure
/// function over bodies the store holds, and because nobody can swap it out
/// it resolves only what nobody disagreed about (see [`crate::kind::merge`]).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ConflictConfig {
    /// The merger `conflict resolve --interactive` runs, `"tcal merge"`.
    ///
    /// Unset by default; a sync never runs it. Paths are appended
    /// git-mergetool style (base, sides, output) unless the command names
    /// {base}, {local}, {remote} or {output}; only a written output counts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merger: Option<CommandConfig>,
}

/// The local pimdir store an account syncs through, the cache a frontend
/// reads. Implicit per account; this table only customises it.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct StoreConfig {
    /// The store directory, holding `pimdir.db` and `objects/`.
    ///
    /// Defaults to the per-account XDG state directory.
    #[serde(default, deserialize_with = "shell_expanded_path_opt")]
    pub root: Option<PathBuf>,

    /// How long a retained (soft-deleted) item survives before a sync run
    /// reclaims it: `store.purge-after = "90d"`.
    ///
    /// A pimdir store never truly deletes: the row is retained, hidden but
    /// keeping its body, and neverest is the sweeper. Unset means never
    /// purge and `"0"` purges at once; there is deliberately no boolean.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purge_after: Option<HumanDuration>,

    /// Removed keys, kept so a configuration carrying one is refused by
    /// name. See [`StoreConfig::reject_removed_keys`].
    #[serde(default, skip_serializing)]
    retention: Option<RemovedKey>,
    #[serde(default, skip_serializing)]
    hydration: Option<RemovedKey>,
}

impl StoreConfig {
    /// Refuses `retention` and `hydration`, whose one answer is now the
    /// account's `retain`.
    ///
    /// A three-point scale described a store holding only the bodies that
    /// happened to cross, which nothing asked for; mapping either key onto
    /// `retain` would guess, so both are refused by name.
    fn reject_removed_keys(&self) -> Result<()> {
        if self.retention.is_some() || self.hydration.is_some() {
            bail!(
                "`store.retention` and `store.hydration` are gone: whether the store keeps \
                 bodies is the account's `retain`, which is true when the store is the \
                 destination and false when targets are named."
            );
        }

        Ok(())
    }

    /// The RFC 3339 purge cutoff of a run starting at `now`: a retained item
    /// strictly older than it is reclaimed.
    ///
    /// `None` when `purge-after` is unset, or so large no instant precedes
    /// it, which means the same. The format matches what the store stamps
    /// `retained_at` with, so the comparison is plain lexicographic.
    pub fn purge_cutoff(&self, now: DateTime<Utc>) -> Option<String> {
        let after = chrono::Duration::from_std(self.purge_after?.0).ok()?;
        let cutoff = now.checked_sub_signed(after)?;
        Some(cutoff.to_rfc3339_opts(SecondsFormat::Millis, true))
    }
}

/// A human-written duration: one non-negative integer and one unit suffix
/// (`"90d"`, `"12h"`, `"30m"`, `"45s"`, `"2w"`), or a bare `"0"`.
///
/// A day is 86400 seconds and a week 7 days: a retention delay is not
/// calendar arithmetic, so no time zone or DST rule enters into it, and
/// months and years are refused for having no fixed length.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HumanDuration(pub Duration);

impl HumanDuration {
    /// Parses `"<integer><unit>"`, or a bare `"0"`.
    fn parse(raw: &str) -> Result<Self, String> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(String::from("empty duration"));
        }

        let digits = raw.trim_end_matches(|c: char| c.is_ascii_alphabetic());
        let unit = &raw[digits.len()..];
        let count: u64 = digits
            .parse()
            .map_err(|_| format!("{raw} is not a `<number><unit>` duration (e.g. `90d`)"))?;

        let seconds = match unit {
            "s" => 1,
            "m" => 60,
            "h" => 3600,
            "d" => 86400,
            "w" => 7 * 86400,
            "" if count == 0 => 1,
            "" => {
                return Err(format!(
                    "duration {raw} misses its unit (`s`, `m`, `h`, `d` or `w`)"
                ));
            }
            other => {
                return Err(format!(
                    "unknown duration unit {other} in {raw} (expected `s`, `m`, `h`, `d` or `w`)"
                ));
            }
        };

        let total = count
            .checked_mul(seconds)
            .ok_or_else(|| format!("duration {raw} overflows"))?;
        Ok(Self(Duration::from_secs(total)))
    }
}

impl fmt::Display for HumanDuration {
    /// Renders back the largest unit dividing the duration evenly, so a
    /// round-trip through the config document is stable.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let secs = self.0.as_secs();
        if secs == 0 {
            return f.write_str("0");
        }
        for (unit, size) in [("w", 7 * 86400), ("d", 86400), ("h", 3600), ("m", 60)] {
            if secs.is_multiple_of(size) {
                return write!(f, "{}{unit}", secs / size);
            }
        }
        write!(f, "{secs}s")
    }
}

impl<'de> Deserialize<'de> for HumanDuration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        HumanDuration::parse(&raw).map_err(serde::de::Error::custom)
    }
}

impl Serialize for HumanDuration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

/// What an account does: which endpoints, which direction, whether the
/// store keeps bodies.
///
/// Declared, never derived: the mode is the arity of `sources` and
/// `targets` plus the two flags, so no behaviour depends on a coincidence
/// between two sources.
#[derive(Clone, Debug, Eq, PartialEq)]
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

    /// Whether a crossing between two remotes may be streamed rather than
    /// staged in the store.
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

/// `serde` helper: shell-expand an optional path.
///
/// TODO: replace with `pimalaya_config::toml::shell_expanded_path_opt`, the
/// optional twin of the `shell_expanded_path` and `shell_expanded_string`
/// the crate already ships. ortie hand-rolls the same function, so the two
/// copies exist only because the shared one does not.
fn shell_expanded_path_opt<'de, D>(deserializer: D) -> Result<Option<PathBuf>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: Option<String> = Option::deserialize(deserializer)?;
    Ok(raw.map(|s| PathBuf::from(shellexpand::tilde(&s).into_owned())))
}

/// One source of the account's hub: the remote it talks to, plus the send
/// channel its queued submit intents leave through.
///
/// The channel belongs to the source and not the account, since sending
/// natively (Graph's `sendMail`) or needing a companion SMTP server is a
/// property of the provider. At most one source per account declares one.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct SourceConfig {
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

/// The remote backend behind a source; exactly one variant per source.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub enum SourceBackendConfig {
    Imap(ImapConfig),
    Carddav(CarddavConfig),
    Caldav(CaldavConfig),
    Jmap(JmapConfig),
    Gmail(GmailConfig),
    Msgraph(MsgraphConfig),
}

impl SourceBackendConfig {
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

    /// Whether this source carries the removed `collection.namespace` key.
    fn declares_namespace(&self) -> bool {
        self.collection().namespace.is_some()
    }

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
                | SourceBackendConfig::Carddav(_)
                | SourceBackendConfig::Caldav(_)
        )
    }

    /// Whether this source sends by itself: Graph's `sendMail` today.
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
            SourceBackendConfig::Carddav(_) | SourceBackendConfig::Caldav(_)
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
    pub collection: CollectionPermissions,
    pub flag: FlagSourcePermissions,
    pub item: ItemSourcePermissions,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ItemSyncConfig {}

/// Collection-name filter: include-list, exclude-list, or keep all.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum CollectionFilter {
    #[default]
    All,
    Include(Vec<String>),
    Exclude(Vec<String>),
}

/// A source's collection-level configuration: which collections it syncs,
/// and what it may do to the collection set.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct CollectionSourceConfig {
    /// Whether the sync may create a collection on this source, and delete
    /// one.
    ///
    /// Both grant by default, unlike the `item` block, which must be
    /// declared in full: this table also carries `filter`, and demanding a
    /// permission pair from someone writing a filter would be a trap.
    #[serde(default = "default_true")]
    pub create: bool,
    #[serde(default = "default_true")]
    pub delete: bool,

    /// Removed key, kept so a configuration carrying it is refused by name.
    ///
    /// Which endpoints meet is now the account's arity, and which way is
    /// [`AccountConfig::one_way`].
    #[serde(default, skip_serializing)]
    namespace: Option<RemovedKey>,

    /// Collection-name filter for this source.
    ///
    /// Per source, because an account may hold several kinds and a mailbox
    /// include-list means nothing to a contacts source. Filters are
    /// therefore asymmetric: a collection may sync on one source only.
    #[serde(default, alias = "filters", skip_serializing_if = "is_default")]
    pub filter: CollectionFilter,
}

impl CollectionSourceConfig {
    /// The copyable permission pair, the only part the sync seam gates on.
    pub fn permissions(&self) -> CollectionPermissions {
        CollectionPermissions {
            create: self.create,
            delete: self.delete,
        }
    }
}

impl Default for CollectionSourceConfig {
    fn default() -> Self {
        Self {
            create: true,
            delete: true,
            namespace: None,
            filter: CollectionFilter::default(),
        }
    }
}

/// Per-source collection permissions gating collection-set mutations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CollectionPermissions {
    pub create: bool,
    pub delete: bool,
}

impl Default for CollectionPermissions {
    fn default() -> Self {
        Self {
            create: true,
            delete: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct FlagSourcePermissions {
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
    pub create: bool,
    pub delete: bool,
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
    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "kebab-case", deny_unknown_fields)]
    pub struct ImapConfig {
        pub server: String,
        #[serde(default)]
        pub tls: TlsConfig,
        #[serde(default, skip_serializing_if = "is_default")]
        pub starttls: bool,
        /// ALPN identifiers offered during the TLS handshake.
        ///
        /// Unset takes io-imap's own default (`["imap"]`), which owns it;
        /// `[]` skips ALPN.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub alpn: Option<Vec<String>>,
        pub sasl: Option<SaslConfig>,
    }
}

source_config! {
    /// A CardDAV source (RFC 6352), each address book a collection.
    ///
    /// The server URL is the entry point only: the principal and the
    /// address book home set are discovered from it.
    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "kebab-case", deny_unknown_fields)]
    pub struct CarddavConfig {
        /// The DAV entry point.
        ///
        /// A bare authority (`dav.example.org[:port]`, read as
        /// `https://<authority>`) or a full URL, `http://` included for a
        /// server on a trusted network.
        pub server: String,
        #[serde(default)]
        pub tls: TlsConfig,
        /// ALPN identifiers offered during the TLS handshake, `["http/1.1"]`
        /// by default; `[]` skips ALPN.
        #[serde(
            default = "default_http_alpn",
            skip_serializing_if = "is_default_http_alpn"
        )]
        pub alpn: Vec<String>,
        pub auth: DavAuthConfig,
    }
}

source_config! {
    /// A CalDAV source (RFC 4791), each calendar a collection.
    ///
    /// The server URL is the entry point only: the principal and the
    /// calendar home set are discovered from it.
    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "kebab-case", deny_unknown_fields)]
    pub struct CaldavConfig {
        /// The DAV entry point.
        ///
        /// A bare authority (`dav.example.org[:port]`, read as
        /// `https://<authority>`) or a full URL, `http://` included for a
        /// server on a trusted network.
        pub server: String,
        #[serde(default)]
        pub tls: TlsConfig,
        /// ALPN identifiers offered during the TLS handshake, `["http/1.1"]`
        /// by default; `[]` skips ALPN.
        #[serde(
            default = "default_http_alpn",
            skip_serializing_if = "is_default_http_alpn"
        )]
        pub alpn: Vec<String>,
        pub auth: DavAuthConfig,
    }
}

/// DAV authentication: HTTP Basic, or a bearer token for a provider
/// fronting DAV with OAuth 2.0.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum DavAuthConfig {
    Basic {
        #[serde(deserialize_with = "shell_expanded_string")]
        username: String,
        password: Secret,
    },
    Bearer {
        token: Secret,
    },
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

source_config! {
    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "kebab-case", deny_unknown_fields)]
    pub struct JmapConfig {
        pub server: String,
        #[serde(default)]
        pub tls: TlsConfig,
        /// ALPN identifiers offered during the TLS handshake, `["http/1.1"]`
        /// by default; `[]` skips ALPN.
        #[serde(
            default = "default_http_alpn",
            skip_serializing_if = "is_default_http_alpn"
        )]
        pub alpn: Vec<String>,
        pub auth: JmapAuthConfig,
        pub identity_id: Option<String>,
        pub drafts_mailbox_id: Option<String>,
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum JmapAuthConfig {
    Header(Secret),
    Bearer {
        token: Secret,
    },
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
        #[serde(default)]
        pub tls: TlsConfig,
        /// ALPN identifiers offered during the TLS handshake, `["http/1.1"]`
        /// by default; `[]` skips ALPN.
        #[serde(
            default = "default_http_alpn",
            skip_serializing_if = "is_default_http_alpn"
        )]
        pub alpn: Vec<String>,
        pub auth: GmailAuthConfig,
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
        #[serde(default)]
        pub tls: TlsConfig,
        /// ALPN identifiers offered during the TLS handshake, `["http/1.1"]`
        /// by default; `[]` skips ALPN.
        #[serde(
            default = "default_http_alpn",
            skip_serializing_if = "is_default_http_alpn"
        )]
        pub alpn: Vec<String>,
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
}

/// Resolves a configured `server` into a URL, `scheme` filling in for a
/// value carrying none, so a bare authority is as good as a full URL.
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

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct TlsConfig {
    pub provider: Option<TlsProviderConfig>,
    #[serde(default)]
    pub rustls: RustlsConfig,
    /// Path to an extra CA certificate to trust, shell-expanded.
    #[serde(default, deserialize_with = "shell_expanded_path_opt")]
    pub cert: Option<PathBuf>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum TlsProviderConfig {
    Rustls,
    NativeTls,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct RustlsConfig {
    pub crypto: Option<RustlsCryptoConfig>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum RustlsCryptoConfig {
    Aws,
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
            cert: self.cert,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum SaslConfig {
    Anonymous(SaslAnonymousConfig),
    Login(SaslLoginConfig),
    Plain(SaslPlainConfig),
    Oauthbearer(SaslOauthbearerConfig),
    Xoauth2(SaslXoauth2Config),
    #[serde(rename = "scram-sha-256")]
    ScramSha256(SaslScramSha256Config),
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SaslAnonymousConfig {
    pub message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SaslLoginConfig {
    #[serde(deserialize_with = "shell_expanded_string")]
    pub username: String,
    pub password: Secret,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SaslPlainConfig {
    pub authzid: Option<String>,
    #[serde(deserialize_with = "shell_expanded_string")]
    #[serde(alias = "username")]
    pub authcid: String,
    #[serde(alias = "password")]
    pub passwd: Secret,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SaslOauthbearerConfig {
    #[serde(deserialize_with = "shell_expanded_string")]
    pub username: String,
    pub token: Secret,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SaslXoauth2Config {
    #[serde(deserialize_with = "shell_expanded_string")]
    pub username: String,
    pub token: Secret,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SaslScramSha256Config {
    #[serde(deserialize_with = "shell_expanded_string")]
    pub username: String,
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
    use super::*;

    #[test]
    fn a_generated_config_renders_as_dotted_keys_under_one_header() {
        let account: AccountConfig = toml::from_str(
            r#"
            default = true
            msgraph.user-id = "me"
            msgraph.auth.token.command = ["ortie", "token", "show", "-a", "msgraph"]
            "#,
        )
        .unwrap();

        let config = Config {
            accounts: HashMap::from([(String::from("outlook"), account)]),
        };

        assert_eq!(
            config_toml::to_string(&config).unwrap(),
            r#"[accounts.outlook]
default = true
msgraph.auth.token.command = ["ortie", "token", "show", "-a", "msgraph"]
msgraph.user-id = "me"
"#
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
        let account: AccountConfig = toml::from_str(
            r#"
            left.imap.server = "imaps://imap.example.org:993"
            right.imap.server = "imaps://imap.other.org:993"
            "#,
        )
        .unwrap();
        let err = account.validate().unwrap_err().to_string();
        assert!(err.contains("`targets`"), "got {err}");
        assert!(err.contains("one-way"), "got {err}");

        let account: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://imap.example.org:993"
            collection.filter.include = ["INBOX"]
            "#,
        )
        .unwrap();
        let err = account.validate().unwrap_err().to_string();
        assert!(err.contains("collection.filter"), "got {err}");

        let account: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://imap.example.org:993"
            store.retention = "retain"
            "#,
        )
        .unwrap();
        let err = account.validate().unwrap_err().to_string();
        assert!(err.contains("the account's `retain`"), "got {err}");

        let account: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://imap.example.org:993"
            store.hydration = "full"
            "#,
        )
        .unwrap();
        assert!(account.validate().is_err());
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

        assert!(err.contains("login"), "got {err}");
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
        let home = PathBuf::from(shellexpand::tilde("~").into_owned());

        let store: StoreConfig = toml::from_str(r#"root = "~/store""#).unwrap();
        assert_eq!(store.root, Some(home.join("store")));

        let tls: TlsConfig = toml::from_str(r#"cert = "~/ca.pem""#).unwrap();
        assert_eq!(tls.cert, Some(home.join("ca.pem")));

        let reloaded: TlsConfig = toml::from_str(&toml::to_string(&tls).unwrap()).unwrap();
        assert_eq!(reloaded.cert, tls.cert);

        // An absent key never reaches the deserializer, so it stays absent
        // rather than expanding an empty path.
        let tls: TlsConfig = toml::from_str("").unwrap();
        assert_eq!(tls.cert, None);
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

    #[test]
    fn a_human_duration_round_trips_through_the_document() {
        for (raw, secs) in [
            ("0", 0),
            ("45s", 45),
            ("30m", 1800),
            ("12h", 43200),
            ("90d", 7776000),
            ("2w", 1209600),
        ] {
            let parsed = HumanDuration::parse(raw).expect(raw);
            assert_eq!(parsed.0.as_secs(), secs, "{raw}");
            assert_eq!(parsed.to_string(), raw, "{raw}");
        }

        assert_eq!(HumanDuration(Duration::from_secs(86400)).to_string(), "1d");
        assert_eq!(
            HumanDuration(Duration::from_secs(90061)).to_string(),
            "90061s"
        );

        assert!(HumanDuration::parse("").is_err());
        assert!(HumanDuration::parse("90").is_err());
        assert!(HumanDuration::parse("90y").is_err());
        assert!(HumanDuration::parse("d").is_err());
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
        assert!(
            err.to_string()
                .contains("no variant of enum SourceBackendConfig")
        );

        let account: AccountConfig = toml::from_str(
            r#"
            sources.a.imap.server = "imaps://imap.example.org:993"
            sources.a.msgraph.auth.token.raw = "tok"
            "#,
        )
        .unwrap();
        assert!(account.sources().unwrap()["a"].is_imap());
    }

    /// The DAV sources are the non-mail ones, so they are where the account
    /// shape stops being mail-shaped.
    #[cfg(feature = "dav")]
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
        let account: AccountConfig = toml::from_str(
            r#"
            sources.a.imap.server = "imaps://a.example.org:993"
            sources.a.imap.collection.namespace = "mail"
            "#,
        )
        .unwrap();

        let err = account.validate().unwrap_err().to_string();
        assert!(err.contains("`collection.namespace` is gone"), "got {err}");
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
}
