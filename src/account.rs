//! The runtime account: the endpoints a run connects to, with every
//! secret already resolved.
//!
//! [`crate::config`] is what the TOML says, and it says where a
//! credential comes from rather than what it is: a `password.command`
//! is a `pass` or `gpg` invocation waiting to be spawned. An [`Account`]
//! is what a run acts on, and it holds the values themselves, so
//! nothing below this module can spawn a process to authenticate.
//!
//! That split is what makes the connection layer cheap. Resolution used
//! to happen inside `client::open`, once per opened connection, so an
//! account with a four-connection IMAP source paid four key unlocks
//! before its first request. It happens here instead, once per run, and
//! the pool re-opens a connection from material it already holds.
//!
//! ## What resolving buys, and what it costs
//!
//! Every endpoint is resolved up front, through one
//! [`pimalaya_config::secret::SecretResolver`] shared by all of them, so
//! a command named by several endpoints (an account's IMAP and SMTP
//! tables, its CardDAV and CalDAV ones) is spawned once for the whole
//! account.
//!
//! Resolving up front means a broken credential is found before the run
//! rather than during it, which is the point, but it must not turn one
//! broken entry into a dead account: sources are independent, and a
//! stale `pass` entry for calendars is no reason to leave mail unsynced.
//! So a failure is kept per endpoint ([`Account::get`] raises it) rather
//! than failing the resolution, and the driver reports it where it
//! already reports a source that could not be opened.
//!
//! ## Freshness
//!
//! An account is resolved once and never re-read, which is exact for a
//! one-shot run and would not be for a long-lived one: a token with a
//! lifetime shorter than the process cannot be refreshed from a value
//! resolved at startup. neverest is one-shot, so nothing needs it yet;
//! a daemon would resolve a new account rather than mutate this one.
//!
//! None of these types derive `Debug`: what they hold is exactly what
//! must not reach a log line.

use std::collections::HashMap;

use anyhow::{Context, Result, bail};
#[cfg(any(feature = "imap", feature = "smtp"))]
use io_sasl::mechanism::Sasl;
use pimalaya_cli::spinner::Spinner;
use pimalaya_config::secret::SecretResolver;
#[cfg(any(
    feature = "imap",
    feature = "msgraph",
    feature = "smtp",
    feature = "dav"
))]
use pimalaya_stream::tls::Tls;
#[cfg(feature = "msgraph")]
use secrecy::SecretString;
#[cfg(any(feature = "imap", feature = "smtp", feature = "dav"))]
use url::Url;

#[cfg(feature = "smtp")]
use crate::config::SmtpConfig;
#[cfg(any(feature = "imap", feature = "smtp", feature = "dav"))]
use crate::config::server_url;
use crate::config::{AccountConfig, SourceBackendConfig, SourceConfig};
#[cfg(feature = "dav")]
use crate::dav::client::DavKind;

/// An account's endpoints, resolved once for the run.
///
/// Built by [`Account::resolve`] and read by name through
/// [`Account::get`], which is where an endpoint that failed to resolve
/// raises its error, so one broken credential stops one source rather
/// than the account.
pub struct Account {
    /// Every endpoint the account declares, sources and targets alike,
    /// keyed by the name that is also its pimdir source id.
    ///
    /// A failure is kept as its rendered message: an endpoint is read
    /// once per source that syncs against it, and an error carrying a
    /// cause chain cannot be handed out twice.
    endpoints: HashMap<String, Result<SourceAccount, String>>,
}

impl Account {
    /// Resolves every endpoint `config` declares, spawning each distinct
    /// secret command once.
    ///
    /// Fails only when the endpoints cannot be enumerated at all, an
    /// endpoint that fails to resolve being kept for [`Account::get`].
    /// The count of those is what the spinner reports, so a resolution
    /// that lost an endpoint does not read as a clean one.
    ///
    /// The spinner is here rather than at each of the three commands
    /// that resolve, because this is the wait it exists for: a locked
    /// `gpg-agent` answers in seconds, and every one of them used to
    /// spend those seconds with nothing on screen.
    pub fn resolve(config: &AccountConfig) -> Result<Self> {
        let endpoints = config.endpoints()?;
        let s = Spinner::start("Resolving credentials…");

        let mut resolver = SecretResolver::new();
        let endpoints: HashMap<_, _> = endpoints
            .into_iter()
            .map(|(name, config)| {
                let resolved = SourceAccount::resolve_with(&name, &config, &mut resolver)
                    .map_err(|err| format!("{err:#}"));
                (name, resolved)
            })
            .collect();

        match endpoints.values().filter(|end| end.is_err()).count() {
            0 => s.success("Resolved credentials"),
            failed => s.success(format!("Resolved credentials, {failed} endpoint(s) failed")),
        }

        Ok(Self { endpoints })
    }

    /// The endpoint named `name`, raising what its resolution failed
    /// with when it failed.
    pub fn get(&self, name: &str) -> Result<SourceAccount> {
        match self.endpoints.get(name) {
            Some(Ok(account)) => Ok(account.clone()),
            Some(Err(err)) => bail!("{err}"),
            None => bail!("This account declares no endpoint named {name}"),
        }
    }
}

/// One endpoint with every secret resolved: what a connection is opened
/// from.
#[derive(Clone)]
pub struct SourceAccount {
    /// The backend to connect to, with its credential.
    pub backend: SourceAccountBackend,
    /// The send channel this endpoint declares, `None` when it declares
    /// none or when the build has no `smtp` feature.
    #[cfg(feature = "smtp")]
    pub smtp: Option<SmtpAccount>,
}

impl SourceAccount {
    /// Resolves one endpoint on its own, for a caller holding a single
    /// configuration rather than a whole account (the wizard's
    /// connection checks).
    #[cfg_attr(
        not(any(feature = "imap", feature = "msgraph", feature = "dav")),
        allow(dead_code)
    )]
    pub fn resolve(name: &str, config: &SourceConfig) -> Result<Self> {
        Self::resolve_with(name, config, &mut SecretResolver::new())
    }

    /// Resolves one endpoint through `resolver`, so several endpoints
    /// naming one command spawn it once.
    fn resolve_with(
        name: &str,
        config: &SourceConfig,
        resolver: &mut SecretResolver,
    ) -> Result<Self> {
        let backend = SourceAccountBackend::resolve(&config.backend, resolver)
            .with_context(|| format!("Resolve the credentials of {name}"))?;

        #[cfg(feature = "smtp")]
        let smtp = config
            .smtp
            .as_ref()
            .map(|smtp| SmtpAccount::resolve_with(smtp, resolver))
            .transpose()
            .with_context(|| format!("Resolve the send credentials of {name}"))?;

        Ok(Self {
            backend,
            #[cfg(feature = "smtp")]
            smtp,
        })
    }
}

/// The resolved counterpart of [`SourceBackendConfig`]: one variant per
/// compiled-in backend, each holding exactly what its `connect` takes.
#[derive(Clone)]
pub enum SourceAccountBackend {
    #[cfg(feature = "imap")]
    Imap(ImapAccount),
    #[cfg(feature = "dav")]
    Dav(DavAccount),
    #[cfg(feature = "msgraph")]
    Msgraph(MsgraphAccount),
    /// Keeps the type inhabited when no backend is compiled in. It is
    /// never constructed: resolution refuses every backend first.
    #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "dav")))]
    #[allow(dead_code)]
    Unavailable,
}

impl SourceAccountBackend {
    /// Resolves a backend configuration, refusing a backend this build
    /// cannot open at all rather than leaving it to fail at connect.
    #[cfg_attr(
        not(any(feature = "imap", feature = "msgraph", feature = "dav")),
        allow(unused_variables)
    )]
    fn resolve(config: &SourceBackendConfig, resolver: &mut SecretResolver) -> Result<Self> {
        match config {
            #[cfg(feature = "imap")]
            SourceBackendConfig::Imap(config) => {
                let alpn = config
                    .alpn
                    .clone()
                    .unwrap_or_else(io_imap::client::default_alpn);
                let server = server_url(&config.server, "imaps")?;
                let sasl = config
                    .sasl
                    .clone()
                    .map(|sasl| {
                        let host = server.host_str().unwrap_or_default();
                        let port = server
                            .port()
                            .unwrap_or_else(|| io_imap::client::default_port(server.scheme()));
                        sasl.try_into_sasl(host, port, resolver)
                    })
                    .transpose()?;

                Ok(Self::Imap(ImapAccount {
                    server,
                    tls: config.tls.clone().into_tls(alpn),
                    starttls: config.starttls,
                    sasl,
                }))
            }
            #[cfg(feature = "dav")]
            SourceBackendConfig::Carddav(config) => Ok(Self::Dav(DavAccount {
                kind: DavKind::Card,
                server: server_url(&config.server, "https")?,
                tls: config.tls.clone().into_tls(config.alpn.clone()),
                auth: config.auth.clone().try_into_dav_auth(resolver)?,
            })),
            #[cfg(feature = "dav")]
            SourceBackendConfig::Caldav(config) => Ok(Self::Dav(DavAccount {
                kind: DavKind::Cal,
                server: server_url(&config.server, "https")?,
                tls: config.tls.clone().into_tls(config.alpn.clone()),
                auth: config.auth.clone().try_into_dav_auth(resolver)?,
            })),
            #[cfg(feature = "msgraph")]
            SourceBackendConfig::Msgraph(config) => Ok(Self::Msgraph(MsgraphAccount {
                token: resolver.resolve(config.auth.token.clone())?,
                user_id: config.user_id.clone(),
                tls: config.tls.clone().into_tls(config.alpn.clone()),
            })),
            #[allow(unreachable_patterns)]
            _ => bail!(
                "This side's backend is not available in this build (rebuild with the matching cargo feature; only the imap, msgraph and dav backends exist for now)"
            ),
        }
    }
}

/// A resolved IMAP endpoint: the arguments of an IMAP session open.
#[cfg(feature = "imap")]
#[derive(Clone)]
pub struct ImapAccount {
    /// The server URL, a configured bare authority already read as one.
    pub server: Url,
    /// The TLS handle, ALPN folded in.
    pub tls: Tls,
    /// Whether a cleartext connection is upgraded through STARTTLS.
    pub starttls: bool,
    /// The credential the session authenticates with, `None` for a
    /// preauthenticated server.
    pub sasl: Option<Sasl>,
}

/// A resolved DAV endpoint, CardDAV and CalDAV alike, `kind` being what
/// tells them apart.
#[cfg(feature = "dav")]
#[derive(Clone)]
pub struct DavAccount {
    /// Which home set the session discovers from the server URL.
    pub kind: DavKind,
    /// The DAV entry point, a configured bare authority already read as
    /// a URL.
    pub server: Url,
    /// The TLS handle, ALPN folded in.
    pub tls: Tls,
    /// The credential every request carries.
    pub auth: io_webdav::rfc4918::WebdavAuth,
}

/// A resolved Microsoft Graph endpoint.
#[cfg(feature = "msgraph")]
#[derive(Clone)]
pub struct MsgraphAccount {
    /// The OAuth 2.0 bearer access token, as the configured command
    /// printed it.
    pub token: SecretString,
    /// The mailbox owner, `me` for the authenticated user.
    pub user_id: String,
    /// The TLS handle, ALPN folded in.
    pub tls: Tls,
}

/// A resolved SMTP submission channel: the arguments of a submission
/// session open.
#[cfg(feature = "smtp")]
#[derive(Clone)]
pub struct SmtpAccount {
    /// The submission server URL, a configured bare authority already
    /// read as one.
    pub server: Url,
    /// The TLS handle, ALPN folded in.
    pub tls: Tls,
    /// Whether a cleartext connection is upgraded through STARTTLS.
    pub starttls: bool,
    /// The credential the session authenticates with, `None` for an
    /// unauthenticated relay.
    pub sasl: Option<Sasl>,
}

#[cfg(feature = "smtp")]
impl SmtpAccount {
    /// Resolves a send channel on its own, for a caller holding a single
    /// configuration (the wizard's connection check).
    #[cfg_attr(not(feature = "imap"), allow(dead_code))]
    pub fn resolve(config: &SmtpConfig) -> Result<Self> {
        Self::resolve_with(config, &mut SecretResolver::new())
    }

    /// Resolves a send channel through `resolver`, so a channel sharing
    /// its credential with the source it belongs to spawns nothing.
    fn resolve_with(config: &SmtpConfig, resolver: &mut SecretResolver) -> Result<Self> {
        let server = server_url(&config.server, "smtps")?;
        let alpn = config
            .alpn
            .clone()
            .unwrap_or_else(io_smtp::client::SmtpClientStd::default_alpn);
        let sasl = config
            .sasl
            .clone()
            .map(|sasl| {
                let host = server.host_str().unwrap_or_default();
                let port = server.port().unwrap_or_else(|| {
                    io_smtp::client::SmtpClientStd::default_port(server.scheme())
                });
                sasl.try_into_sasl(host, port, resolver)
            })
            .transpose()?;

        Ok(Self {
            server,
            tls: config.tls.clone().into_tls(alpn),
            starttls: config.starttls,
            sasl,
        })
    }
}

#[cfg(all(test, unix, feature = "imap", feature = "smtp", feature = "dav"))]
mod tests {
    use std::{env::temp_dir, fs, process};

    use super::Account;
    use crate::config::AccountConfig;

    /// The reason the resolver exists: an account naming one password
    /// entry from four places reads it once.
    #[test]
    fn one_password_command_named_by_four_endpoints_is_spawned_once() {
        let path = temp_dir().join(format!("neverest-resolve-once-{}", process::id()));
        let _ = fs::remove_file(&path);

        // Counts its own runs, one byte per spawn, and prints a secret.
        let command = format!("printf x >> {path}; printf s3cr3t", path = path.display());

        let config: AccountConfig = toml::from_str(&format!(
            r#"
            imap.server = "imaps://localhost"
            imap.sasl.plain.username = "user"
            imap.sasl.plain.password.command = "{command}"
            smtp.server = "smtps://localhost"
            smtp.sasl.plain.username = "user"
            smtp.sasl.plain.password.command = "{command}"
            carddav.server = "https://localhost"
            carddav.auth.basic.username = "user"
            carddav.auth.basic.password.command = "{command}"
            caldav.server = "https://localhost"
            caldav.auth.basic.username = "user"
            caldav.auth.basic.password.command = "{command}"
            "#
        ))
        .unwrap();

        Account::resolve(&config).unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"x");

        fs::remove_file(&path).unwrap();
    }
}
