//! Account configuration: each account holds named [`SourceConfig`]s over one
//! pimdir store, plus that store's settings.

use std::{
    collections::HashMap, fmt, fs, path::Path, path::PathBuf, process::exit, time::Duration,
};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, SecondsFormat, Utc};
use io_sasl::{
    login::SaslLoginCreds, mechanism::Sasl, rfc4505::anonymous::SaslAnonymousCreds,
    rfc4616::plain::SaslPlainCreds, rfc5801::SaslGs2ChannelBinding, rfc5802::SaslScramCreds,
    rfc7628::oauthbearer::SaslOauthbearerCreds, xoauth2::SaslXoauth2Creds,
};
use pimalaya_cli::{printer::Printer, prompt};
use pimalaya_config::{
    secret::Secret,
    toml as config_toml,
    toml::{TomlConfig, shell_expanded_string},
};
use pimalaya_stream::tls::{Rustls, RustlsCrypto, Tls, TlsProvider};
use serde::{Deserialize, Serialize};

use crate::wizard;

/// `skip_serializing_if` predicate skipping a field equal to its type's
/// default, so a generated config omits defaulted values (the only
/// serializer is the wizard, see [`crate::wizard`]).
fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

/// [`is_default`] for the HTTP-backend ALPN list, which defaults to a
/// non-empty value.
fn is_default_http_alpn(alpn: &[String]) -> bool {
    alpn == default_http_alpn().as_slice()
}

/// Splices the per-source shared fields (`collection`, `flag`, `item`,
/// `pool_size`) onto every protocol-specific config struct.
///
/// `collection` and `item` keep a serde alias on their pre-`generic-pim-sync`
/// spellings (`mailbox` / `message`), so an existing mail configuration keeps
/// loading unchanged.
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
            /// Per-source connection pool size override; defaults are
            /// picked per backend.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub pool_size: Option<usize>,
        }
    };
}

/// Generates a [`SourceConfig`] accessor that forwards to the matching
/// shared field on the source's backend variant, by value.
macro_rules! source_accessor {
    ($name:ident, $ty:ty) => {
        pub fn $name(&self) -> $ty {
            match &self.backend {
                SourceBackendConfig::Imap(c) => c.$name,
                SourceBackendConfig::Carddav(c) => c.$name,
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
    /// Loads `Config` from `config_paths`, or proposes the wizard when
    /// no file exists. Declining the proposal exits: a command has
    /// nothing to run against without an account.
    pub fn load_or_wizard(printer: &mut impl Printer, config_paths: &[PathBuf]) -> Result<Config> {
        if let Some(config) = Config::from_paths_or_default(config_paths)? {
            return Ok(config);
        }

        let target = Config::target_path(config_paths)?;
        let prompt = format!(
            "No configuration found, create one at {}?",
            target.display(),
        );

        if !prompt::bool(&prompt, true)? {
            exit(0);
        }

        wizard::discover::run(printer, &target)
    }

    /// Serializes `self` to TOML at `path`, creating missing parent
    /// directories. The document renders like himalaya's: one
    /// `[accounts.<name>]` header per account, every field below it a
    /// dotted key, and no table header for anything else.
    pub fn write(&self, path: &Path) -> Result<()> {
        let toml = config_toml::to_string(self).context("Serialize TOML config error")?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Create TOML config parent `{}` error", parent.display())
            })?;
        }

        fs::write(path, toml)
            .with_context(|| format!("Write TOML config `{}` error", path.display()))?;

        Ok(())
    }
}

/// Per-account configuration: one or more named sources over one pimdir store.
///
/// An account is the hub: one store, one database, one blob directory, and
/// whatever sources feed it. A source's name is its pimdir source id, so it
/// names every binding that source owns in the store and renaming one orphans
/// them all.
///
/// A backend written directly under the account (`imap`, `carddav`, …) is sugar
/// for a source named after its protocol, which is the whole configuration for
/// the common single-provider account. The `sources` table is what a mirror or
/// a fan-in reaches for, since those need two sources of one protocol.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct AccountConfig {
    #[serde(default, skip_serializing_if = "is_default")]
    pub default: bool,

    /// Named sources, the map key being the pimdir source id.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub sources: HashMap<String, SourceConfig>,

    /// Direct-backend sugar: each is a source named after its protocol.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imap: Option<ImapConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carddav: Option<CarddavConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jmap: Option<JmapConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gmail: Option<GmailConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msgraph: Option<MsgraphConfig>,

    /// The send channel of the sugar source that carries mail, the flat
    /// spelling of `sources.<name>.smtp`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smtp: Option<SmtpConfig>,

    /// The local pimdir store this account syncs through. Optional: the store is
    /// implicit (per-account state dir) and customised only here, never as a
    /// source.
    #[serde(default)]
    pub store: StoreConfig,

    // TODO: item-level sync filters (date range, sender, subject).
    #[serde(default, alias = "message")]
    pub item: ItemSyncConfig,

    /// Max connections per source for concurrent `Full` body fetches (default
    /// 4). Keep it under your provider's per-account connection limit. A `sync
    /// --connections N` flag overrides it.
    #[serde(default)]
    pub connections: Option<usize>,

    /// Removed keys, kept only so a configuration carrying one is refused by
    /// name rather than as an unknown field. See [`AccountConfig::validate`].
    #[serde(default, skip_serializing)]
    left: Option<RemovedKey>,
    #[serde(default, skip_serializing)]
    right: Option<RemovedKey>,
    #[serde(default, skip_serializing, alias = "mailbox")]
    collection: Option<RemovedKey>,
}

impl AccountConfig {
    /// A single-source account, which is the only shape the wizard writes: one
    /// provider, one protocol, and a store that keeps every body because
    /// nothing crosses. Everything beyond that is configured by hand.
    pub fn with_source(default: bool, source: SourceConfig) -> Self {
        let mut account = Self {
            default,
            ..Self::default()
        };
        account.set_direct_source(source);
        account
    }

    /// Writes `source` as the direct-backend sugar, replacing whatever backend
    /// of that protocol the account carried and lifting its send channel to the
    /// account-level `smtp` table the sugar spells it with.
    pub fn set_direct_source(&mut self, source: SourceConfig) {
        let SourceConfig { backend, smtp } = source;

        self.smtp = smtp;

        match backend {
            SourceBackendConfig::Imap(config) => self.imap = Some(config),
            SourceBackendConfig::Carddav(config) => self.carddav = Some(config),
            SourceBackendConfig::Jmap(config) => self.jmap = Some(config),
            SourceBackendConfig::Gmail(config) => self.gmail = Some(config),
            SourceBackendConfig::Msgraph(config) => self.msgraph = Some(config),
        }
    }

    /// The direct-backend sources in protocol order, which is what the wizard
    /// owns and what `configure` re-runs over. A source from the explicit
    /// `sources` table is hand-written and never appears here.
    pub fn direct_sources(&self) -> Vec<SourceConfig> {
        [
            self.imap.clone().map(SourceBackendConfig::Imap),
            self.carddav.clone().map(SourceBackendConfig::Carddav),
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

    /// Every configured source keyed by its id, the direct-backend sugar folded
    /// into the explicit `sources` table.
    ///
    /// The sugar's source id is its protocol name, which is the id the expanded
    /// form writes too, so expanding an account by hand is a no-op on the store.
    pub fn sources(&self) -> Result<HashMap<String, SourceConfig>> {
        let mut sources = self.sources.clone();

        let sugar = [
            self.imap.clone().map(SourceBackendConfig::Imap),
            self.carddav.clone().map(SourceBackendConfig::Carddav),
            self.jmap.clone().map(SourceBackendConfig::Jmap),
            self.gmail.clone().map(SourceBackendConfig::Gmail),
            self.msgraph.clone().map(SourceBackendConfig::Msgraph),
        ];

        for backend in sugar.into_iter().flatten() {
            let name = backend.protocol().to_string();

            if sources.contains_key(&name) {
                bail!(
                    "Source `{name}` is declared both directly under the account and in the \
                     `sources` table; the direct form is sugar for `sources.{name}`, so keep one."
                );
            }

            sources.insert(name, SourceConfig::new(backend));
        }

        self.attach_send_channel(&mut sources)?;

        Ok(sources)
    }

    /// Hands the account-level `smtp` table to the one sugar source that could
    /// use it. A source in the explicit table carries its own.
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

    /// Whether a source of that name came from the direct-backend sugar rather
    /// than from the explicit table.
    fn is_sugar(&self, name: &str) -> bool {
        !self.sources.contains_key(name)
    }

    /// The account's sources grouped into hub namespaces, ordered by kind then
    /// namespace so a report reads the same twice.
    ///
    /// This is the one place [`StoredBodies`] is decided. Both `check` and the
    /// sync go through it, so what a run reports and what it does cannot drift
    /// apart.
    pub fn groups(&self) -> Result<Vec<SourceGroup>> {
        let sources = self.sources()?;
        let mut grouped: HashMap<(&'static str, String), Vec<String>> = HashMap::new();

        for (name, source) in &sources {
            let key = (
                source.backend.media_type(),
                source.namespace(name).to_string(),
            );
            grouped.entry(key).or_default().push(name.clone());
        }

        let mut groups: Vec<_> = grouped
            .into_iter()
            .map(|((media_type, namespace), mut names)| {
                names.sort();

                // A group streams only when every source in it can, which today
                // means an IMAP to IMAP pairing and nothing else.
                let streamable = names.iter().all(|name| sources[name].is_streamable());

                SourceGroup {
                    media_type,
                    namespace,
                    bodies: StoredBodies::derive(names.len(), streamable),
                    sources: names,
                }
            })
            .collect();

        groups.sort_by(|a, b| (a.media_type, &a.namespace).cmp(&(b.media_type, &b.namespace)));

        // A hub collection id is `<namespace>/<name>` with the kind on the
        // collection row, so two kinds under one namespace would key onto the
        // same ids: a mailbox and an address book both named `Default` would
        // become one collection. Mirroring across kinds means nothing anyway.
        for pair in groups.windows(2) {
            let [previous, group] = pair else { continue };

            if previous.namespace == group.namespace {
                bail!(
                    "Namespace `{}` is claimed by two kinds, `{}` and `{}`, whose collections \
                     would collide. A namespace names the sources that mirror each other, which \
                     only one kind can do; give one of them a namespace of its own.",
                    group.namespace,
                    previous.media_type,
                    group.media_type,
                );
            }
        }

        Ok(groups)
    }

    /// Rejects an account a command cannot run: a removed key, no source at all,
    /// a source declaring options its backend cannot honour, or two sources
    /// racing for the send channel.
    ///
    /// Run by every command that opens an account, so a bad configuration is
    /// refused before any connection is made rather than halfway through a sync.
    pub fn validate(&self) -> Result<()> {
        self.reject_removed_keys()?;

        let sources = self.sources()?;

        if sources.is_empty() {
            bail!(
                "This account declares no source. Write a backend directly under it \
                 (`imap.server = \"…\"`), or name one in its `sources` table."
            );
        }

        for (name, source) in &sources {
            source.validate(name)?;
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

    /// Refuses a key this version removed, naming what replaces it. A removed
    /// key is refused rather than ignored: honouring neither the old meaning nor
    /// the new one silently is how a configuration ends up doing the opposite of
    /// what it says.
    fn reject_removed_keys(&self) -> Result<()> {
        if self.left.is_some() || self.right.is_some() {
            bail!(
                "`left` and `right` are gone: an account holds named sources. Write them as \
                 `sources.left` and `sources.right`, and give both the same \
                 `collection.namespace` to keep them mirroring each other (without it they sync \
                 side by side into the store and never push to one another)."
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

        self.store.reject_removed_keys()
    }
}

/// Presence marker for a configuration key this version removed.
///
/// Deserializing accepts whatever the key held and discards it, so the account
/// can refuse it by name with its replacement instead of failing as an unknown
/// field with no explanation.
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

/// The local pimdir store an account syncs through — the retained cache the app
/// reads. Implicit per account; this only customises it.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct StoreConfig {
    /// Override the store directory (holds `pimdir.db` + `objects/`); defaults
    /// to the per-account XDG state directory.
    #[serde(default, deserialize_with = "shell_expanded_path_opt")]
    pub root: Option<PathBuf>,

    /// How long the store keeps a **retained** (soft-deleted) item before a
    /// sync run reclaims it: `store.purge-after = "90d"`.
    ///
    /// A pimdir store never truly deletes an item; when its last source
    /// binding vanishes the row is retained, hidden from the sync seam and
    /// from normal listings, keeping its body. Purge is explicit and
    /// time-based, and neverest is the sweeper.
    ///
    /// **Unset means never purge** (retained items pile up until an operator
    /// reclaims them). `"0"` purges immediately, reproducing a terminal
    /// delete. There is deliberately no boolean: the delay subsumes the on /
    /// off switch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purge_after: Option<HumanDuration>,

    /// Removed keys, kept only so a configuration carrying one is refused by
    /// name. See [`StoreConfig::reject_removed_keys`].
    #[serde(default, skip_serializing)]
    retention: Option<RemovedKey>,
    #[serde(default, skip_serializing)]
    hydration: Option<RemovedKey>,
}

impl StoreConfig {
    /// Refuses `retention` and `hydration`, which no longer exist: what the
    /// store keeps is derived per kind from how many sources share a namespace
    /// (see [`StoredBodies::derive`]) and reported on every run.
    ///
    /// Accepting and ignoring `retention = "retain"` on a pairing that derives
    /// [`StoredBodies::None`] would hand back the opposite of what was written,
    /// so the key is refused rather than tolerated.
    fn reject_removed_keys(&self) -> Result<()> {
        if self.retention.is_some() || self.hydration.is_some() {
            bail!(
                "`store.retention` and `store.hydration` are gone: what the store keeps is \
                 derived per kind and reported on every run. One source keeps every body, two \
                 sources sharing a `collection.namespace` on a streamable pairing keep none, \
                 anything else keeps what crossed. Run `neverest check` to see the value in force."
            );
        }

        Ok(())
    }
    /// The RFC 3339 purge cutoff of a run starting at `now`: a retained item
    /// whose `retained_at` is strictly older is reclaimed. `None` when
    /// `purge-after` is unset (never purge) or so large that no instant can
    /// precede it, which means the same thing.
    ///
    /// The format matches the one the store stamps `retained_at` with
    /// (`…THH:MM:SS.sssZ`), so the comparison the store runs is a plain
    /// lexicographic one on equally shaped instants.
    pub fn purge_cutoff(&self, now: DateTime<Utc>) -> Option<String> {
        let after = chrono::Duration::from_std(self.purge_after?.0).ok()?;
        let cutoff = now.checked_sub_signed(after)?;
        Some(cutoff.to_rfc3339_opts(SecondsFormat::Millis, true))
    }
}

/// A human-written duration, the shape a scheduling knob takes in the TOML
/// document: one non-negative integer and one unit suffix (`"90d"`, `"12h"`,
/// `"30m"`, `"45s"`, `"2w"`), or a bare `"0"` (every unit agrees on zero).
///
/// A day is 86400 seconds and a week is 7 days: this is a retention delay, not
/// calendar arithmetic, so no time zone or DST rule enters into it. Months and
/// years are refused for the same reason (they have no fixed length).
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
            .map_err(|_| format!("`{raw}` is not a `<number><unit>` duration (e.g. `90d`)"))?;

        let seconds = match unit {
            "s" => 1,
            "m" => 60,
            "h" => 3600,
            "d" => 86400,
            "w" => 7 * 86400,
            "" if count == 0 => 1,
            "" => {
                return Err(format!(
                    "duration `{raw}` misses its unit (`s`, `m`, `h`, `d` or `w`)"
                ));
            }
            other => {
                return Err(format!(
                    "unknown duration unit `{other}` in `{raw}` (expected `s`, `m`, `h`, `d` or `w`)"
                ));
            }
        };

        let total = count
            .checked_mul(seconds)
            .ok_or_else(|| format!("duration `{raw}` overflows"))?;
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

/// Which item bodies the store keeps for one kind.
///
/// Not configured: derived by [`StoredBodies::derive`] from how many sources
/// share a collection namespace and whether their pairing can stream, then
/// reported on every run. The three points replace the old `store.retention`
/// and `store.hydration` pair, whose fourth combination (relay every body) meant
/// nothing.
#[derive(Clone, Copy, Debug, Serialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum StoredBodies {
    /// No body at rest: each crossing is streamed from its holding source to
    /// the target and the store keeps only the spine. The pass-through mirror,
    /// which trades away dedup, cheap retry and resumability.
    None,
    /// Only the bodies that had to cross, kept once they have.
    Crossing,
    /// Every body, the store being the offline replica an app reads.
    All,
}

impl StoredBodies {
    /// What the store keeps for a namespace holding `sources` sources, given
    /// whether that pairing can stream a body server to server.
    ///
    /// One source keeps everything, because nothing crosses and anything less
    /// makes the store an index rather than a replica. Exactly two on a
    /// streamable pairing keep nothing, which is the pass-through migrate. Three
    /// or more, and every pairing that cannot stream, keep what crossed.
    pub fn derive(sources: usize, streamable: bool) -> Self {
        match sources {
            0 | 1 => Self::All,
            2 if streamable => Self::None,
            _ => Self::Crossing,
        }
    }

    /// Whether a body crossing to another source is streamed rather than stored.
    pub fn relays(self) -> bool {
        matches!(self, Self::None)
    }

    /// Whether every item is hydrated, rather than only those about to cross.
    pub fn hydrates_everything(self) -> bool {
        matches!(self, Self::All)
    }
}

impl fmt::Display for StoredBodies {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("none"),
            Self::Crossing => f.write_str("crossing"),
            Self::All => f.write_str("all"),
        }
    }
}

/// `serde` helper: shell-expand an optional path.
fn shell_expanded_path_opt<'de, D>(deserializer: D) -> Result<Option<PathBuf>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: Option<String> = Option::deserialize(deserializer)?;
    Ok(raw.map(|s| PathBuf::from(shellexpand::tilde(&s).into_owned())))
}

/// One source of the account's hub: the remote it talks to, plus the
/// send channel its queued submit intents leave through when that remote
/// cannot submit by itself.
///
/// The channel belongs to the source, not to the account: a backend either
/// sends natively (Microsoft Graph through `sendMail`, and JMAP once its
/// submission lands) or needs a companion SMTP server, which is a
/// property of that provider. At most one source per account may declare one.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct SourceConfig {
    #[serde(flatten)]
    pub backend: SourceBackendConfig,

    /// The SMTP submission server this source's queued submit intents are
    /// flushed through. Only meaningful on a backend that cannot send by
    /// itself (IMAP): a Graph source sends through the Graph `sendMail`
    /// action instead. Absent (and no source that sends natively), queued
    /// submit intents stay pending.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smtp: Option<SmtpConfig>,
}

/// The remote backend behind a source; exactly one variant per source.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub enum SourceBackendConfig {
    Imap(ImapConfig),
    Carddav(CarddavConfig),
    Jmap(JmapConfig),
    Gmail(GmailConfig),
    Msgraph(MsgraphConfig),
}

impl SourceBackendConfig {
    /// The protocol table this backend is written under, which is also the id
    /// of the source the direct-backend sugar builds from it.
    pub fn protocol(&self) -> &'static str {
        match self {
            Self::Imap(_) => "imap",
            Self::Carddav(_) => "carddav",
            Self::Jmap(_) => "jmap",
            Self::Gmail(_) => "gmail",
            Self::Msgraph(_) => "msgraph",
        }
    }

    /// The media type this backend syncs, known from the protocol alone.
    ///
    /// The open client answers the same question (`Client::media_type`), and
    /// has to, since that is what the collection kind is written from. Knowing
    /// it here too is what lets a namespace be grouped, and its
    /// [`StoredBodies`] derivation reported, before a single connection is
    /// opened: `check` answers while a remote is down, and a first `sync`
    /// answers before it fetches anything.
    pub fn media_type(&self) -> &'static str {
        match self {
            Self::Imap(_) | Self::Jmap(_) | Self::Gmail(_) | Self::Msgraph(_) => "message/rfc822",
            Self::Carddav(_) => "text/vcard",
        }
    }
}

/// One hub namespace: every source of one kind sharing one collection
/// namespace, and what the store consequently keeps for them.
///
/// Sources meet here and nowhere else. Two of them in one group bind the same
/// hub collections, which is what makes them mirror; two in different groups
/// sit side by side in the store and never push to one another.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceGroup {
    /// The kind every source in the group syncs.
    pub media_type: &'static str,
    /// The shared `collection.namespace`.
    pub namespace: String,
    /// The group's source names, sorted, so a report reads the same twice.
    pub sources: Vec<String>,
    /// What the store keeps for this group, derived and never configured.
    pub bodies: StoredBodies,
}

impl fmt::Display for SourceGroup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} / {} ({}): bodies {}",
            self.media_type,
            self.namespace,
            self.sources.join(", "),
            self.bodies,
        )
    }
}

// NOTE: `pool_size`, `is_imap` and `is_http` describe the config surface and
// may be unused until pools return; `new` is called from the wizard paths their
// backend feature gates.
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

    /// The hub collection namespace this source binds into: the configured
    /// value, else its own name, which keeps sources isolated unless two are
    /// deliberately pointed at the same one.
    pub fn namespace<'a>(&'a self, name: &'a str) -> &'a str {
        self.collection().namespace.as_deref().unwrap_or(name)
    }

    pub fn is_imap(&self) -> bool {
        matches!(self.backend, SourceBackendConfig::Imap(_))
    }

    /// Whether this source talks a remote HTTP backend (JMAP, Gmail or
    /// Microsoft Graph); these share the smaller default pool size.
    pub fn is_http(&self) -> bool {
        matches!(
            self.backend,
            SourceBackendConfig::Jmap(_)
                | SourceBackendConfig::Gmail(_)
                | SourceBackendConfig::Msgraph(_)
                | SourceBackendConfig::Carddav(_)
        )
    }

    /// Whether this source sends by itself, without a companion SMTP
    /// channel: the Graph `sendMail` action today.
    pub fn sends_natively(&self) -> bool {
        matches!(self.backend, SourceBackendConfig::Msgraph(_))
    }

    /// Whether this source can carry a send channel at all. A contacts or
    /// calendar source cannot: submission is a mail capability, so an `smtp`
    /// table there is a configuration error rather than a dead option.
    pub fn carries_mail(&self) -> bool {
        !matches!(self.backend, SourceBackendConfig::Carddav(_))
    }

    /// Whether a body can be streamed straight from this source to another
    /// rather than stored on the way. Only the IMAP to IMAP pairing can, so
    /// this gates the [`StoredBodies::None`] derivation.
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

/// A source's collection-level configuration: which hub collections it binds
/// into, which of them it syncs, and what it may do to the collection set.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct CollectionSourceConfig {
    /// Whether the sync may create a collection on this source, and delete one.
    ///
    /// Both default to granting, unlike the `item` block, which requires its
    /// pair to be declared in full. The asymmetry is deliberate: this table now
    /// also carries `namespace` and `filter`, so it is declared for reasons that
    /// have nothing to do with permissions, and demanding a permission pair
    /// from someone writing a namespace would be a trap.
    #[serde(default = "default_true")]
    pub create: bool,
    #[serde(default = "default_true")]
    pub delete: bool,

    /// The hub collection namespace, defaulting to the source's own name.
    ///
    /// A hub collection is keyed by `(kind, namespace, name)`, so two sources
    /// of one kind meet only where they declare the same namespace. Meeting is
    /// what propagation *is*: an item sitting in a collection a source
    /// participates in, with no binding for that source, is pushed to it. Two
    /// sources left on their own names therefore sync side by side into one
    /// store and never write to one another, which is the merged read view a
    /// frontend unions at display time.
    ///
    /// Sharing a value is the explicit act of saying "these two are the same
    /// thing", and it is what turns a pair of sources into a mirror.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,

    /// Collection-name filter for this source.
    ///
    /// Per source rather than per account, because an account may hold sources
    /// of several kinds and a mailbox include-list means nothing to a contacts
    /// source. Filters are consequently asymmetric: a collection may be synced
    /// on one source and skipped on another.
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
/// `create` and `delete` are required when the block is declared at all
/// (declare it in full or omit it); `update` instead **defaults to true**,
/// because it was added after the fact: an existing configuration declaring
/// only `create` and `delete` must keep parsing rather than failing on a
/// missing field it could not have known about.
///
/// `update` only bites on a mutable-content backend. Mail bodies are
/// immutable, so an in-place update never arises there and the gate is inert.
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
        /// ALPN protocol identifiers offered during the TLS handshake.
        /// Unset takes io-imap's own default (`["imap"]`), which owns
        /// it; set it to `[]` to skip ALPN.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub alpn: Option<Vec<String>>,
        pub sasl: Option<SaslConfig>,
    }
}

source_config! {
    /// CardDAV side (RFC 6352). The server URL is the entry point only:
    /// the principal and the address book home set are discovered from it,
    /// and each address book becomes a collection.
    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "kebab-case", deny_unknown_fields)]
    pub struct CarddavConfig {
        /// The DAV entry point, a full URL
        /// (`https://dav.example.org/`, `https://dav.example.org/dav/`).
        pub server: String,
        #[serde(default)]
        pub tls: TlsConfig,
        /// ALPN protocol identifiers offered during the TLS handshake.
        /// Defaults to `["http/1.1"]`; set to `[]` to skip ALPN.
        #[serde(
            default = "default_http_alpn",
            skip_serializing_if = "is_default_http_alpn"
        )]
        pub alpn: Vec<String>,
        pub auth: DavAuthConfig,
    }
}

/// CardDAV authentication: HTTP Basic (the common case) or a bearer token
/// for a provider fronting DAV with OAuth 2.0.
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

#[cfg(feature = "carddav")]
impl DavAuthConfig {
    /// Resolves the configured secret and converts to io-webdav's auth. The
    /// secret is read here, once per opened connection, so a broken command
    /// fails at connect time rather than mid-enumeration.
    pub fn try_into_dav_auth(self) -> Result<io_webdav::rfc4918::WebdavAuth> {
        use io_http::{rfc6750::bearer::HttpAuthBearer, rfc7617::basic::HttpAuthBasic};
        use io_webdav::rfc4918::WebdavAuth;
        use secrecy::ExposeSecret;

        Ok(match self {
            Self::Basic { username, password } => WebdavAuth::Basic(HttpAuthBasic::new(
                username,
                password.get()?.expose_secret(),
            )),
            Self::Bearer { token } => {
                WebdavAuth::Bearer(HttpAuthBearer::new(token.get()?.expose_secret()))
            }
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
        /// ALPN protocol identifiers offered during the TLS handshake.
        /// Defaults to `["http/1.1"]`; set to `[]` to skip ALPN.
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
    /// Gmail REST API side (`https://gmail.googleapis.com`). Labels are
    /// exposed as mailboxes; the API host is fixed, so only the mailbox
    /// owner, TLS and the OAuth 2.0 credential are configurable.
    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "kebab-case", deny_unknown_fields)]
    pub struct GmailConfig {
        /// Gmail user id (the mailbox owner). Defaults to `me`, the
        /// authenticated user.
        #[serde(default = "default_gmail_user_id")]
        pub user_id: String,
        #[serde(default)]
        pub tls: TlsConfig,
        /// ALPN protocol identifiers offered during the TLS handshake.
        /// Defaults to `["http/1.1"]`; set to `[]` to skip ALPN.
        #[serde(
            default = "default_http_alpn",
            skip_serializing_if = "is_default_http_alpn"
        )]
        pub alpn: Vec<String>,
        pub auth: GmailAuthConfig,
    }
}

/// Gmail authentication; only OAuth 2.0 bearer tokens are accepted.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct GmailAuthConfig {
    /// OAuth 2.0 bearer access token; the client adds the `Bearer `
    /// prefix itself. Refresh is the caller's responsibility.
    pub token: Secret,
}

source_config! {
    /// Microsoft Graph API side (`https://graph.microsoft.com`). Mail
    /// folders are exposed as mailboxes; the API host is fixed, so only
    /// the mailbox owner, TLS and the OAuth 2.0 credential are
    /// configurable.
    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "kebab-case", deny_unknown_fields)]
    pub struct MsgraphConfig {
        /// Graph user id (the mailbox owner). Defaults to `me`, the
        /// authenticated user.
        #[serde(default = "default_msgraph_user_id")]
        pub user_id: String,
        #[serde(default)]
        pub tls: TlsConfig,
        /// ALPN protocol identifiers offered during the TLS handshake.
        /// Defaults to `["http/1.1"]`; set to `[]` to skip ALPN.
        #[serde(
            default = "default_http_alpn",
            skip_serializing_if = "is_default_http_alpn"
        )]
        pub alpn: Vec<String>,
        pub auth: MsgraphAuthConfig,
    }
}

/// Microsoft Graph authentication; only OAuth 2.0 bearer tokens are
/// accepted, neverest never runs an OAuth flow itself.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MsgraphAuthConfig {
    /// OAuth 2.0 bearer access token; the client adds the `Bearer `
    /// prefix itself. Acquiring and refreshing the token is the
    /// caller's responsibility: point `token.command` at any command
    /// printing a valid token, typically ortie.
    pub token: Secret,
}

/// The SMTP submission server a side's queued sends are flushed
/// through (`<side>.smtp`).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SmtpConfig {
    /// Submission URL: `smtps://host:465` (implicit TLS) or
    /// `smtp://host:587` (plain, usually with `starttls`).
    pub server: String,
    /// Upgrades a plain `smtp://` connection via STARTTLS.
    #[serde(default, skip_serializing_if = "is_default")]
    pub starttls: bool,
    #[serde(default)]
    pub tls: TlsConfig,
    /// ALPN protocol identifiers offered during the TLS handshake.
    /// Unset takes io-smtp's own default (`["smtp"]`, the token RFC
    /// 7595 registers), which owns it; set it to `[]` to skip ALPN.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alpn: Option<Vec<String>>,
    /// The SMTP LOGIN username; omit both `login` and `password` for an
    /// unauthenticated relay.
    #[serde(default, deserialize_with = "shell_expanded_string_opt")]
    pub login: Option<String>,
    /// The SMTP password source.
    #[serde(default)]
    pub password: Option<Secret>,
}

/// `serde` helper: shell-expand an optional string.
fn shell_expanded_string_opt<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: Option<String> = Option::deserialize(deserializer)?;
    Ok(raw.map(|s| shellexpand::tilde(&s).into_owned()))
}

fn default_gmail_user_id() -> String {
    String::from("me")
}

fn default_msgraph_user_id() -> String {
    String::from("me")
}

/// Default ALPN list for the HTTP-based backends (Gmail, Microsoft
/// Graph): the REST APIs ride on HTTP/1.1.
fn default_http_alpn() -> Vec<String> {
    vec![String::from("http/1.1")]
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct TlsConfig {
    pub provider: Option<TlsProviderConfig>,
    #[serde(default)]
    pub rustls: RustlsConfig,
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
    /// `alpn` is the protocol-level ALPN list (e.g. `["imap"]`,
    /// `["http/1.1"]`); pass an empty vec to skip ALPN. The TOML
    /// schema never exposes `tls.rustls.alpn` directly: the per-
    /// protocol `*.alpn` field is folded in here.
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

#[cfg_attr(not(feature = "imap"), allow(dead_code))]
impl SaslConfig {
    /// Resolves the SASL config into a runtime [`Sasl`]. `host` and `port` come
    /// from the live server URL, and only OAUTHBEARER uses them, echoing them
    /// in the GS2 header.
    pub fn try_into_sasl(self, host: impl ToString, port: u16) -> Result<Sasl> {
        Ok(match self {
            SaslConfig::Anonymous(c) => Sasl::Anonymous(SaslAnonymousCreds { message: c.message }),
            SaslConfig::Login(c) => Sasl::Login(SaslLoginCreds {
                username: c.username,
                password: c.password.get()?,
            }),
            SaslConfig::Plain(c) => Sasl::Plain(SaslPlainCreds {
                authzid: c.authzid,
                authcid: c.authcid,
                passwd: c.passwd.get()?,
            }),
            SaslConfig::Oauthbearer(c) => Sasl::Oauthbearer(SaslOauthbearerCreds {
                username: c.username,
                host: host.to_string(),
                port,
                token: c.token.get()?,
            }),
            SaslConfig::Xoauth2(c) => Sasl::Xoauth2(SaslXoauth2Creds {
                username: c.username,
                token: c.token.get()?,
            }),
            // NOTE: the nonce is left empty, io-imap filling it with one it
            // draws when opening the session, an I/O-free coroutine being
            // unable to generate randomness.
            SaslConfig::ScramSha256(c) => Sasl::ScramSha256(SaslScramCreds {
                username: c.username,
                password: c.password.get()?,
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

    /// The sugar's source id is its protocol name, which is the id the expanded
    /// form writes: the store cannot tell the two spellings apart, so expanding
    /// an account by hand never orphans a binding.
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
        assert_eq!(sugar["imap"].namespace("imap"), "imap");
        assert_eq!(explicit["imap"].namespace("imap"), "imap");
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
            sources.fastmail.imap.collection.namespace = "mail"
            sources.gmail.imap.server = "imaps://imap.gmail.com:993"
            sources.gmail.imap.collection.namespace = "mail"
            sources.dav.carddav.server = "https://carddav.fastmail.com/"
            sources.dav.carddav.auth.basic.username = "user"
            sources.dav.carddav.auth.basic.password.raw = "pw"
            "#,
        )
        .unwrap();

        account.validate().unwrap();
        let sources = account.sources().unwrap();
        assert_eq!(sources.len(), 3);
        assert_eq!(sources["fastmail"].namespace("fastmail"), "mail");
        assert_eq!(sources["gmail"].namespace("gmail"), "mail");
        assert_eq!(sources["dav"].namespace("dav"), "dav");
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

    /// A source left on its own name is isolated; two pointed at one namespace
    /// meet, and meeting is what propagation is.
    #[test]
    fn a_namespace_defaults_to_the_source_name() {
        let account: AccountConfig = toml::from_str(
            r#"
            sources.fastmail.imap.server = "imaps://imap.fastmail.com:993"
            sources.gmail.imap.server = "imaps://imap.gmail.com:993"
            "#,
        )
        .unwrap();

        let sources = account.sources().unwrap();
        assert_eq!(sources["fastmail"].namespace("fastmail"), "fastmail");
        assert_eq!(sources["gmail"].namespace("gmail"), "gmail");
        assert_ne!(
            sources["fastmail"].namespace("fastmail"),
            sources["gmail"].namespace("gmail"),
            "isolated by default, so neither pushes to the other",
        );
    }

    #[test]
    fn what_the_store_keeps_is_derived_from_the_namespace() {
        assert_eq!(StoredBodies::derive(1, true), StoredBodies::All);
        assert_eq!(StoredBodies::derive(1, false), StoredBodies::All);
        assert_eq!(StoredBodies::derive(2, true), StoredBodies::None);
        assert_eq!(StoredBodies::derive(2, false), StoredBodies::Crossing);
        assert_eq!(StoredBodies::derive(3, true), StoredBodies::Crossing);

        assert!(StoredBodies::None.relays());
        assert!(!StoredBodies::Crossing.relays());
        assert!(StoredBodies::All.hydrates_everything());
        assert!(!StoredBodies::Crossing.hydrates_everything());
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
        assert!(err.contains("sources.left"), "got {err}");
        assert!(err.contains("collection.namespace"), "got {err}");

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
        assert!(err.contains("derived per kind"), "got {err}");

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

        // NOTE: with two backends on one source the first wins, which is all
        // the flattened enum can express: a source talks one protocol.
        let account: AccountConfig = toml::from_str(
            r#"
            sources.a.imap.server = "imaps://imap.example.org:993"
            sources.a.msgraph.auth.token.raw = "tok"
            "#,
        )
        .unwrap();
        assert!(account.sources().unwrap()["a"].is_imap());
    }

    /// A CardDAV source is the first non-mail one, so it is where the account
    /// shape stops being mail-shaped.
    #[cfg(feature = "carddav")]
    #[test]
    fn a_carddav_source_carries_no_send_channel() {
        let account: AccountConfig = toml::from_str(
            r#"
            carddav.server = "https://dav.example.org/"
            carddav.auth.basic.username = "user"
            carddav.auth.basic.password.raw = "pw"
            "#,
        )
        .unwrap();

        let sources = account.sources().unwrap();
        assert!(!sources["carddav"].carries_mail(), "contacts do not submit");
        assert!(!sources["carddav"].sends_natively());
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

    /// A CardDAV book and a mailbox may both be named `Default`; only the kind
    /// in the hub collection key keeps them apart.
    #[cfg(feature = "carddav")]
    #[test]
    fn a_streamable_pairing_is_imap_to_imap_only() {
        let account: AccountConfig = toml::from_str(
            r#"
            sources.a.carddav.server = "https://a.example.org/"
            sources.a.carddav.auth.bearer.token.raw = "tok"
            sources.a.carddav.collection.namespace = "cards"
            sources.b.carddav.server = "https://b.example.org/"
            sources.b.carddav.auth.bearer.token.raw = "tok"
            sources.b.carddav.collection.namespace = "cards"
            "#,
        )
        .unwrap();

        let sources = account.sources().unwrap();
        assert!(!sources["a"].is_streamable());
        assert_eq!(
            StoredBodies::derive(2, sources["a"].is_streamable()),
            StoredBodies::Crossing,
            "a DAV pairing cannot stream, so it keeps what crossed",
        );
    }
}
