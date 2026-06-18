//! Account configuration: each account pairs a `left` and a `right`
//! [`SideConfig`] plus mailbox/message sync settings.

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

/// Splices the per-side shared fields (`mailbox`, `flag`, `message`,
/// `pool_size`) onto every protocol-specific config struct.
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
            /// Per-side connection pool size override; defaults are
            /// picked per backend.
            #[serde(default)]
            pub pool_size: Option<usize>,
        }
    };
}

/// Generates a [`SideConfig`] accessor that forwards to the matching
/// shared field on the active variant.
macro_rules! side_accessor {
    ($name:ident, $ty:ty) => {
        pub fn $name(&self) -> $ty {
            match self {
                Self::Imap(c) => c.$name,
                Self::Jmap(c) => c.$name,
                Self::Gmail(c) => c.$name,
                Self::Msgraph(c) => c.$name,
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
    /// Loads `Config` from `config_paths`, or runs the wizard when no
    /// file exists.
    pub fn load_or_wizard(config_paths: &[PathBuf]) -> Result<Config> {
        match Config::from_paths_or_default(config_paths)? {
            Some(config) => Ok(config),
            None => wizard::discover::run(&Config::target_path(config_paths)?),
        }
    }

    /// Serializes `self` to TOML at `path`, creating missing parent
    /// directories.
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

    /// Mailbox-level sync settings shared by both sides.
    #[serde(default)]
    pub mailbox: MailboxSyncConfig,

    // TODO: message-level sync filters (date range, sender, subject).
    #[serde(default)]
    pub message: MessageSyncConfig,
}

/// One side of the bidirectional sync; exactly one variant per side.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub enum SideConfig {
    Imap(ImapConfig),
    Jmap(JmapConfig),
    Gmail(GmailConfig),
    Msgraph(MsgraphConfig),
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

    /// Whether this side talks a remote HTTP backend (JMAP, Gmail or
    /// Microsoft Graph); these share the smaller default pool size.
    pub fn is_http(&self) -> bool {
        matches!(self, Self::Jmap(_) | Self::Gmail(_) | Self::Msgraph(_))
    }

    /// Snapshots the per-side mailbox/flag/message permissions.
    pub fn permissions(&self) -> SidePermissions {
        SidePermissions {
            mailbox: self.mailbox(),
            flag: self.flag(),
            message: self.message(),
        }
    }
}

/// Per-side permission triple gating which sync hunks may materialize.
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

    /// Friendly-name → backend-id map (e.g. `inbox = "INBOX"`); used
    /// for display only, sync ignores aliases.
    #[serde(default)]
    pub alias: HashMap<String, String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MessageSyncConfig {}

/// Mailbox-name filter: include-list, exclude-list, or keep all.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum MailboxFilter {
    #[default]
    All,
    Include(Vec<String>),
    Exclude(Vec<String>),
}

/// Per-side mailbox permissions gating mailbox-set mutations.
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

side_config! {
    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "kebab-case", deny_unknown_fields)]
    pub struct ImapConfig {
        pub server: String,
        #[serde(default)]
        pub tls: TlsConfig,
        #[serde(default)]
        pub starttls: bool,
        /// ALPN protocol identifiers offered during the TLS handshake.
        /// Defaults to `["imap"]`; set to `[]` to skip ALPN.
        #[serde(default = "io_imap::client::default_alpn")]
        pub alpn: Vec<String>,
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
        /// ALPN protocol identifiers offered during the TLS handshake.
        /// Defaults to `["http/1.1"]`; set to `[]` to skip ALPN.
        #[serde(default = "io_jmap::client::default_alpn")]
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
        #[serde(default = "default_http_alpn")]
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
        #[serde(default = "default_http_alpn")]
        pub alpn: Vec<String>,
        pub auth: MsgraphAuthConfig,
    }
}

/// Microsoft Graph authentication; only OAuth 2.0 bearer tokens are
/// accepted.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MsgraphAuthConfig {
    /// OAuth 2.0 bearer access token; the client adds the `Bearer `
    /// prefix itself. Refresh is the caller's responsibility.
    pub token: Secret,
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

impl SaslConfig {
    /// Resolves the SASL config into a runtime [`Sasl`]. `host` and
    /// `port` come from the live server URL; they are only used by
    /// OAUTHBEARER (echoed in the GS2 header) and ignored by every
    /// other mechanism.
    pub fn try_into_sasl(self, host: impl ToString, port: u16) -> Result<Sasl> {
        Ok(match self {
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
                host: host.to_string(),
                port,
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
