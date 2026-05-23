//! Global configuration for neverest.
//!
//! Each account declares a `left` and a `right` [`SideConfig`]. Sync
//! orchestration walks both sides through [`io_email::client::EmailClientStd`]
//! and applies the per-side [`MailboxSidePermissions`] /
//! [`FlagSidePermissions`] / [`MessageSidePermissions`].

use std::{collections::HashMap, fs, path::Path, path::PathBuf};

use anyhow::{Context, Result};
use pimalaya_config::{
    secret::Secret,
    toml::{TomlConfig, shell_expanded_path, shell_expanded_string},
};
use pimalaya_stream::{
    sasl::{
        Sasl, SaslAnonymous, SaslLogin, SaslOauthbearer, SaslPlain, SaslScramSha256, SaslXoauth2,
    },
    tls::{Rustls, RustlsCrypto, Tls, TlsProvider},
};
use serde::{Deserialize, Serialize};

use crate::wizard;

/// Wraps a `pub struct $Name { ... }` declaration and splices in the
/// per-side shared fields (`mailbox`, `flag`, `message`, `pool_size`)
/// at the end. Used by every protocol-specific config so each variant
/// of [`SideConfig`] carries the same permission and pool-size knobs
/// without copy-pasting the four fields.
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
            #[serde(default)]
            pub mailbox: MailboxSidePermissions,
            #[serde(default)]
            pub flag: FlagSidePermissions,
            #[serde(default)]
            pub message: MessageSidePermissions,
            /// Optional override for the per-side connection pool
            /// size. When unset, defaults are picked per backend
            /// (IMAP 8, JMAP 4, m2dir 8); see
            /// [`crate::sync::pool::SidePool::open`].
            #[serde(default)]
            pub pool_size: Option<usize>,
        }
    };
}

/// Generates an accessor on [`SideConfig`] that forwards to the
/// matching shared field on the active variant's protocol config. The
/// shared fields (`mailbox`, `flag`, `message`, `pool_size`) are
/// appended to every protocol config by [`side_config!`], so all three
/// arms have the same field by construction.
macro_rules! side_accessor {
    ($name:ident, $ty:ty) => {
        pub fn $name(&self) -> $ty {
            match self {
                Self::Imap(c) => c.$name,
                Self::Jmap(c) => c.$name,
                Self::M2dir(c) => c.$name,
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
    /// Loads `Config` from the merged `config_paths` or, when no file
    /// exists, runs the wizard against the target path. Called by every
    /// command that needs config (sync, doctor, configure).
    pub fn load_or_wizard(config_paths: &[PathBuf]) -> Result<Config> {
        match Config::from_paths_or_default(config_paths)? {
            Some(config) => Ok(config),
            None => wizard::discover::run(&Config::target_path(config_paths)?),
        }
    }

    /// Serializes `self` to TOML and writes it to `path`, creating
    /// any missing parent directories. Used by the wizard to persist
    /// a freshly-built configuration.
    pub fn write(&self, path: &Path) -> Result<()> {
        let toml = toml::to_string_pretty(self).context("Serialize TOML config error")?;

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
    #[serde(default)]
    pub default: bool,

    pub left: SideConfig,
    pub right: SideConfig,

    /// Mailbox-level sync settings shared by both sides (filters +
    /// aliases). Per-side permissions live on each [`SideConfig`].
    #[serde(default)]
    pub mailbox: MailboxSyncConfig,

    /// Reserved for message-level sync filters (date range, sender,
    /// subject). Currently a placeholder so future fields don't break
    /// existing config files.
    #[serde(default)]
    pub message: MessageSyncConfig,
}

/// One side of the bidirectional sync. Exactly one variant is set
/// (serde rejects multiples and `deny_unknown_fields` rejects none),
/// so client construction never has to check "how many backends".
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub enum SideConfig {
    Imap(ImapConfig),
    Jmap(JmapConfig),
    M2dir(M2dirConfig),
}

impl SideConfig {
    side_accessor!(mailbox, MailboxSidePermissions);
    side_accessor!(flag, FlagSidePermissions);
    side_accessor!(message, MessageSidePermissions);
    side_accessor!(pool_size, Option<usize>);

    pub fn is_imap(&self) -> bool {
        matches!(self, Self::Imap(_))
    }

    pub fn is_jmap(&self) -> bool {
        matches!(self, Self::Jmap(_))
    }

    /// Bundles the three permission accessors so the sync engine can
    /// snapshot the gating policy once per side instead of re-walking
    /// the config inside its inner loops.
    pub fn permissions(&self) -> SidePermissions {
        SidePermissions {
            mailbox: self.mailbox(),
            flag: self.flag(),
            message: self.message(),
        }
    }
}

/// Per-side permission triple consulted by the sync engine to decide
/// whether a planned hunk is allowed to materialize.
#[derive(Clone, Copy, Debug)]
pub struct SidePermissions {
    pub mailbox: MailboxSidePermissions,
    pub flag: FlagSidePermissions,
    pub message: MessageSidePermissions,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MailboxSyncConfig {
    /// Mailbox-name filter applied symmetrically to both sides.
    #[serde(default)]
    pub filters: MailboxFilter,

    /// Friendly-name → backend-id map (e.g. `inbox = "INBOX"`).
    /// Currently consumed only at display time; sync ignores aliases.
    #[serde(default)]
    pub alias: HashMap<String, String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MessageSyncConfig {}

/// Mailbox-name filter: `Include` keeps only the listed names,
/// `Exclude` drops them, `All` keeps everything.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum MailboxFilter {
    #[default]
    All,
    Include(Vec<String>),
    Exclude(Vec<String>),
}

/// Per-side mailbox permissions. `create`/`delete` flags gate whether
/// the sync engine is allowed to mutate the mailbox set on that side.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MailboxSidePermissions {
    pub create: bool,
    pub delete: bool,
}

impl Default for MailboxSidePermissions {
    fn default() -> Self {
        Self {
            create: true,
            delete: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct FlagSidePermissions {
    pub update: bool,
}

impl Default for FlagSidePermissions {
    fn default() -> Self {
        Self { update: true }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MessageSidePermissions {
    pub create: bool,
    pub delete: bool,
}

impl Default for MessageSidePermissions {
    fn default() -> Self {
        Self {
            create: true,
            delete: true,
        }
    }
}

// ── Protocol configs (mirrored from himalaya v2) ─────────────────────
//
// Each protocol-specific struct carries its own settings plus the
// shared per-side knobs (mailbox/flag/message/pool_size) appended by
// the `side_config!` macro. Accessors on [`SideConfig`] forward to the
// active variant's shared fields so callers never have to match on the
// variant just to read a permission.

side_config! {
    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "kebab-case", deny_unknown_fields)]
    pub struct ImapConfig {
        pub server: String,
        #[serde(default)]
        pub tls: TlsConfig,
        #[serde(default)]
        pub starttls: bool,
        pub sasl: Option<SaslConfig>,
    }
}

side_config! {
    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "kebab-case", deny_unknown_fields)]
    pub struct M2dirConfig {
        #[serde(deserialize_with = "shell_expanded_path")]
        pub root: PathBuf,
    }
}

side_config! {
    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "kebab-case", deny_unknown_fields)]
    pub struct JmapConfig {
        pub server: String,
        #[serde(default)]
        pub tls: TlsConfig,
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

impl From<TlsConfig> for Tls {
    fn from(config: TlsConfig) -> Self {
        Tls {
            provider: config.provider.map(|config| match config {
                TlsProviderConfig::Rustls => TlsProvider::Rustls,
                TlsProviderConfig::NativeTls => TlsProvider::NativeTls,
            }),
            rustls: Rustls {
                crypto: config.rustls.crypto.map(|config| match config {
                    RustlsCryptoConfig::Aws => RustlsCrypto::Aws,
                    RustlsCryptoConfig::Ring => RustlsCrypto::Ring,
                }),
                alpn: Vec::new(),
            },
            cert: config.cert,
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
    pub authcid: String,
    pub passwd: Secret,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SaslOauthbearerConfig {
    #[serde(deserialize_with = "shell_expanded_string")]
    pub username: String,
    pub host: String,
    pub port: u16,
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

impl TryFrom<SaslConfig> for Sasl {
    type Error = anyhow::Error;

    fn try_from(config: SaslConfig) -> Result<Self> {
        Ok(match config {
            SaslConfig::Anonymous(c) => Sasl::Anonymous(SaslAnonymous { message: c.message }),
            SaslConfig::Login(c) => Sasl::Login(SaslLogin {
                username: c.username,
                password: c.password.get()?,
            }),
            SaslConfig::Plain(c) => Sasl::Plain(SaslPlain {
                authzid: c.authzid,
                authcid: c.authcid,
                passwd: c.passwd.get()?,
            }),
            SaslConfig::Oauthbearer(c) => Sasl::Oauthbearer(SaslOauthbearer {
                username: c.username,
                host: c.host,
                port: c.port,
                token: c.token.get()?,
            }),
            SaslConfig::Xoauth2(c) => Sasl::Xoauth2(SaslXoauth2 {
                username: c.username,
                token: c.token.get()?,
            }),
            SaslConfig::ScramSha256(c) => Sasl::ScramSha256(SaslScramSha256 {
                username: c.username,
                password: c.password.get()?,
            }),
        })
    }
}
