//! Account configuration: each account pairs a `left` and a `right`
//! [`SideConfig`] plus collection/item sync settings.

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

/// Splices the per-side shared fields (`collection`, `flag`, `item`,
/// `pool_size`) onto every protocol-specific config struct.
///
/// `collection` and `item` keep a serde alias on their pre-`generic-pim-sync`
/// spellings (`mailbox` / `message`), so an existing mail configuration keeps
/// loading unchanged.
macro_rules! side_config {
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
            pub collection: CollectionSidePermissions,
            #[serde(default, skip_serializing_if = "is_default")]
            pub flag: FlagSidePermissions,
            #[serde(default, alias = "message", skip_serializing_if = "is_default")]
            pub item: ItemSidePermissions,
            /// Per-side connection pool size override; defaults are
            /// picked per backend.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub pool_size: Option<usize>,
        }
    };
}

/// Generates a [`SideConfig`] accessor that forwards to the matching
/// shared field on the side's backend variant.
macro_rules! side_accessor {
    ($name:ident, $ty:ty) => {
        pub fn $name(&self) -> $ty {
            match &self.backend {
                SideBackendConfig::Imap(c) => c.$name,
                SideBackendConfig::Carddav(c) => c.$name,
                SideBackendConfig::Jmap(c) => c.$name,
                SideBackendConfig::Gmail(c) => c.$name,
                SideBackendConfig::Msgraph(c) => c.$name,
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

/// Per-account configuration: two sides plus optional sync filters.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct AccountConfig {
    #[serde(default, skip_serializing_if = "is_default")]
    pub default: bool,

    /// The account's sync sides. At least one SHALL be set; exactly one makes
    /// this a **local sync** (that remote against the retained pimdir store the
    /// app reads), both make it a **remote-to-remote** sync through the store.
    #[serde(default)]
    pub left: Option<SideConfig>,
    #[serde(default)]
    pub right: Option<SideConfig>,

    /// The local pimdir store this account syncs through. Optional: the store is
    /// implicit (per-account state dir) and customised only here, never as a
    /// side.
    #[serde(default)]
    pub store: StoreConfig,

    /// Collection-level sync settings shared by both sides.
    #[serde(default, alias = "mailbox")]
    pub collection: CollectionSyncConfig,

    // TODO: item-level sync filters (date range, sender, subject).
    #[serde(default, alias = "message")]
    pub item: ItemSyncConfig,

    /// Max connections per side for concurrent `Full` body fetches (default 4).
    /// Keep it under your provider's per-account connection limit. A `sync
    /// --connections N` flag overrides it.
    #[serde(default)]
    pub connections: Option<usize>,
}

/// A synced side and its configured backend (used when reporting/iterating over
/// the account's sides).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SideName {
    Left,
    Right,
}

impl std::fmt::Display for SideName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SideName::Left => f.write_str("left"),
            SideName::Right => f.write_str("right"),
        }
    }
}

impl AccountConfig {
    /// Rejects an account whose sides declare options their backend cannot
    /// honour. Run by every command that opens an account, so a bad
    /// configuration is refused before any connection is made rather than
    /// halfway through a sync.
    pub fn validate(&self) -> Result<()> {
        for (name, side) in self.sides() {
            side.validate(name)?;
        }

        Ok(())
    }

    /// The configured sides in order, skipping unset ones. Empty means the
    /// account has no side (a config error the commands reject).
    pub fn sides(&self) -> Vec<(SideName, &SideConfig)> {
        let mut sides = Vec::new();
        if let Some(left) = &self.left {
            sides.push((SideName::Left, left));
        }
        if let Some(right) = &self.right {
            sides.push((SideName::Right, right));
        }
        sides
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

    /// Whether bodies are kept in the store. Defaults by mode: a one-side (local)
    /// sync always retains (the app reads bodies); a two-side sync relays
    /// (streams body between the servers, the store keeping only the spine) when
    /// both sides are streamable, else retains. Set to force one.
    #[serde(default)]
    pub retention: Option<Retention>,

    /// How far a two-side retain sync hydrates bodies. Defaults by mode: a
    /// one-side (local) sync always hydrates everything (the app reads bodies);
    /// a two-side sync hydrates only the bodies about to cross (`crossing`).
    /// `full` makes a two-side sync mirror every body into the store (and
    /// forces retention).
    #[serde(default)]
    pub hydration: Option<Hydration>,

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
}

impl StoreConfig {
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

/// How far a two-side retain sync hydrates item bodies.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Hydration {
    /// Hydrate only the bodies about to cross to the other side (the
    /// two-source default).
    Crossing,
    /// Hydrate every placement to `Full` so the store mirrors every body
    /// (the gateway deployment).
    Full,
}

/// Whether a two-side sync keeps item bodies in the store.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Retention {
    /// Keep every body in the store (browsable, deduped, resumable).
    Retain,
    /// Stream the body server-to-server, keeping only the spine (no body at
    /// rest) — a pure pass-through mirror.
    Relay,
}

/// `serde` helper: shell-expand an optional path.
fn shell_expanded_path_opt<'de, D>(deserializer: D) -> Result<Option<PathBuf>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: Option<String> = Option::deserialize(deserializer)?;
    Ok(raw.map(|s| PathBuf::from(shellexpand::tilde(&s).into_owned())))
}

/// One side of the bidirectional sync: the remote it talks to, plus the
/// send channel its queued submit intents leave through when that remote
/// cannot submit by itself.
///
/// The channel belongs to the side, not to the account: a backend either
/// sends natively (Microsoft Graph through `sendMail`, and JMAP once its
/// submission lands) or needs a companion SMTP server, which is a
/// property of that provider. An account whose two sides both offer one
/// flushes through the first (`left`, then `right`).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct SideConfig {
    #[serde(flatten)]
    pub backend: SideBackendConfig,

    /// The SMTP submission server this side's queued submit intents are
    /// flushed through. Only meaningful on a backend that cannot send by
    /// itself (IMAP): a Graph side sends through the Graph `sendMail`
    /// action instead. Absent (and no side that sends natively), queued
    /// submit intents stay pending.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smtp: Option<SmtpConfig>,
}

/// The remote backend behind a side; exactly one variant per side.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub enum SideBackendConfig {
    Imap(ImapConfig),
    Carddav(CarddavConfig),
    Jmap(JmapConfig),
    Gmail(GmailConfig),
    Msgraph(MsgraphConfig),
}

// NOTE: `pool_size`, `is_imap` and `is_http` describe the config surface and
// may be unused until pools return; `new` is called from the wizard paths their
// backend feature gates.
#[allow(dead_code)]
impl SideConfig {
    /// Wraps a backend into a side with no send channel of its own.
    pub fn new(backend: SideBackendConfig) -> Self {
        Self {
            backend,
            smtp: None,
        }
    }

    side_accessor!(collection, CollectionSidePermissions);
    side_accessor!(flag, FlagSidePermissions);
    side_accessor!(item, ItemSidePermissions);
    side_accessor!(pool_size, Option<usize>);

    pub fn is_imap(&self) -> bool {
        matches!(self.backend, SideBackendConfig::Imap(_))
    }

    /// Whether this side talks a remote HTTP backend (JMAP, Gmail or
    /// Microsoft Graph); these share the smaller default pool size.
    pub fn is_http(&self) -> bool {
        matches!(
            self.backend,
            SideBackendConfig::Jmap(_)
                | SideBackendConfig::Gmail(_)
                | SideBackendConfig::Msgraph(_)
                | SideBackendConfig::Carddav(_)
        )
    }

    /// Whether this side sends by itself, without a companion SMTP
    /// channel: the Graph `sendMail` action today.
    pub fn sends_natively(&self) -> bool {
        matches!(self.backend, SideBackendConfig::Msgraph(_))
    }

    /// Whether this side can carry a send channel at all. A contacts or
    /// calendar side cannot: submission is a mail capability, so an `smtp`
    /// table there is a configuration error rather than a dead option.
    pub fn carries_mail(&self) -> bool {
        !matches!(self.backend, SideBackendConfig::Carddav(_))
    }

    /// Rejects a side whose declared options its backend cannot honour.
    pub fn validate(&self, side: SideName) -> Result<()> {
        if self.smtp.is_some() && !self.carries_mail() {
            bail!(
                "The `{side}.smtp` channel is a mail capability and this side syncs contacts; \
                 drop the table, or move it to the mail account that sends."
            );
        }

        Ok(())
    }

    /// Snapshots the per-side collection/flag/item permissions.
    pub fn permissions(&self) -> SidePermissions {
        SidePermissions {
            collection: self.collection(),
            flag: self.flag(),
            item: self.item(),
        }
    }
}

/// Per-side permission triple gating which sync hunks may materialize.
#[derive(Clone, Copy, Debug)]
pub struct SidePermissions {
    pub collection: CollectionSidePermissions,
    pub flag: FlagSidePermissions,
    pub item: ItemSidePermissions,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct CollectionSyncConfig {
    /// Collection-name filter applied symmetrically to both sides.
    #[serde(default, alias = "filters", skip_serializing_if = "is_default")]
    pub filter: CollectionFilter,
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

/// Per-side collection permissions gating collection-set mutations.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct CollectionSidePermissions {
    pub create: bool,
    pub delete: bool,
}

impl Default for CollectionSidePermissions {
    fn default() -> Self {
        Self {
            create: true,
            delete: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct FlagSidePermissions {
    pub update: bool,
}

impl Default for FlagSidePermissions {
    fn default() -> Self {
        Self { update: true }
    }
}

/// Per-side item permissions gating item mutations.
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
pub struct ItemSidePermissions {
    pub create: bool,
    pub delete: bool,
    #[serde(default = "default_true")]
    pub update: bool,
}

impl Default for ItemSidePermissions {
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

side_config! {
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

side_config! {
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

side_config! {
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

side_config! {
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

side_config! {
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
            left.msgraph.user-id = "me"
            left.msgraph.auth.token.command = ["ortie", "token", "show", "-a", "msgraph"]
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
left.msgraph.auth.token.command = ["ortie", "token", "show", "-a", "msgraph"]
left.msgraph.user-id = "me"
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

    #[test]
    fn smtp_channel_and_hydration_parse() {
        let account: AccountConfig = toml::from_str(
            r#"
            left.imap.server = "imaps://imap.example.org:993"

            [left.smtp]
            server = "smtp://smtp.example.org:587"
            starttls = true
            login = "user@example.org"
            password.raw = "pw"

            [store]
            hydration = "full"
            "#,
        )
        .unwrap();

        let left = account.left.expect("a left side");
        let smtp = left.smtp.expect("an smtp channel");
        assert_eq!(smtp.server, "smtp://smtp.example.org:587");
        assert!(smtp.starttls);
        assert_eq!(smtp.login.as_deref(), Some("user@example.org"));
        assert!(matches!(left.backend, SideBackendConfig::Imap(_)));
        assert_eq!(account.store.hydration, Some(Hydration::Full));

        let account: AccountConfig = toml::from_str(
            r#"
            left.imap.server = "imaps://imap.example.org:993"
            "#,
        )
        .unwrap();
        assert!(account.left.expect("a left side").smtp.is_none());
        assert!(account.store.hydration.is_none());
    }

    #[test]
    fn an_account_level_smtp_table_is_refused() {
        let err = toml::from_str::<AccountConfig>(
            r#"
            left.imap.server = "imaps://imap.example.org:993"
            smtp.server = "smtp://smtp.example.org:587"
            "#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("unknown field `smtp`"));
    }

    #[test]
    fn the_pre_generic_pim_sync_spellings_still_load() {
        let account: AccountConfig = toml::from_str(
            r#"
            mailbox.filter.include = ["INBOX"]
            left.imap.server = "imaps://imap.example.org:993"
            left.imap.mailbox.create = false
            left.imap.mailbox.delete = false
            left.imap.message.create = true
            left.imap.message.delete = false
            "#,
        )
        .unwrap();

        assert_eq!(
            account.collection.filter,
            CollectionFilter::Include(vec![String::from("INBOX")])
        );

        let perms = account.left.expect("a left side").permissions();
        assert!(!perms.collection.create);
        assert!(!perms.collection.delete);
        assert!(perms.item.create);
        assert!(!perms.item.delete);
        assert!(perms.flag.update);
        assert!(perms.item.update);

        let account: AccountConfig = toml::from_str(
            r#"
            collection.filter.include = ["INBOX"]
            left.imap.server = "imaps://imap.example.org:993"
            left.imap.collection.create = false
            left.imap.collection.delete = false
            left.imap.item.create = true
            left.imap.item.delete = false
            "#,
        )
        .unwrap();
        assert_eq!(
            account.collection.filter,
            CollectionFilter::Include(vec![String::from("INBOX")])
        );
        let perms = account.left.expect("a left side").permissions();
        assert!(!perms.collection.create);
        assert!(!perms.collection.delete);
        assert!(perms.item.create);
        assert!(!perms.item.delete);
    }

    #[test]
    fn item_update_is_denied_only_when_asked_for() {
        let account: AccountConfig = toml::from_str(
            r#"
            left.imap.server = "imaps://imap.example.org:993"
            left.imap.item.create = true
            left.imap.item.delete = true
            left.imap.item.update = false
            "#,
        )
        .unwrap();
        let perms = account.left.expect("a left side").permissions();
        assert!(perms.item.create);
        assert!(perms.item.delete);
        assert!(!perms.item.update);

        let account: AccountConfig = toml::from_str(
            r#"
            left.imap.server = "imaps://imap.example.org:993"
            left.imap.item.create = true
            left.imap.item.delete = true
            "#,
        )
        .unwrap();
        assert!(account.left.expect("a left side").permissions().item.update);
    }

    #[test]
    fn the_documented_sample_still_loads() {
        let raw = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/config.sample.toml"))
            .expect("read the sample");
        let config: Config = toml::from_str(&raw).expect("the sample must parse");

        let account = config.accounts.get("example").expect("the sample account");
        assert!(account.left.as_ref().expect("a left side").is_imap());
    }

    #[test]
    fn the_purge_delay_is_a_human_duration_and_drives_the_cutoff() {
        let now: DateTime<Utc> = "2026-08-07T12:00:00Z".parse().unwrap();

        let account: AccountConfig = toml::from_str(
            r#"
            left.imap.server = "imaps://imap.example.org:993"
            "#,
        )
        .unwrap();
        assert!(account.store.purge_after.is_none());
        assert!(account.store.purge_cutoff(now).is_none());

        let account: AccountConfig = toml::from_str(
            r#"
            left.imap.server = "imaps://imap.example.org:993"
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
            left.imap.server = "imaps://imap.example.org:993"
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
            left.imap.server = "imaps://imap.example.org:993"
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
    fn a_side_pairs_one_backend_with_its_send_channel() {
        let account: AccountConfig = toml::from_str(
            r#"
            left.msgraph.auth.token.raw = "tok"
            "#,
        )
        .unwrap();
        let left = account.left.expect("a left side");
        assert!(left.sends_natively());
        assert!(left.smtp.is_none());

        let err = toml::from_str::<AccountConfig>(
            r#"
            left.imapp.server = "imaps://imap.example.org:993"
            "#,
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("no variant of enum SideBackendConfig")
        );

        // NOTE: with two backends on one side the first wins, which is all the
        // flattened enum can express: a side talks one protocol.
        let account: AccountConfig = toml::from_str(
            r#"
            left.imap.server = "imaps://imap.example.org:993"
            left.msgraph.auth.token.raw = "tok"
            "#,
        )
        .unwrap();
        assert!(account.left.expect("a left side").is_imap());
    }

    /// A CardDAV side is the first non-mail one, so it is where the account
    /// shape stops being mail-shaped.
    #[cfg(feature = "carddav")]
    #[test]
    fn a_carddav_side_carries_no_send_channel() {
        let account: AccountConfig = toml::from_str(
            r#"
            left.carddav.server = "https://dav.example.org/"
            left.carddav.auth.basic.username = "user"
            left.carddav.auth.basic.password.raw = "pw"
            "#,
        )
        .unwrap();

        let left = account.left.as_ref().expect("a left side");
        assert!(!left.carries_mail(), "contacts do not submit");
        assert!(!left.sends_natively());
        account.validate().unwrap();

        let account: AccountConfig = toml::from_str(
            r#"
            left.carddav.server = "https://dav.example.org/"
            left.carddav.auth.bearer.token.raw = "tok"
            left.smtp.server = "smtps://smtp.example.org:465"
            "#,
        )
        .unwrap();

        let err = account.validate().unwrap_err().to_string();
        assert!(err.contains("`left.smtp`"), "got {err}");
    }
}
