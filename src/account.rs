//! # Runtime account
//!
//! The endpoints a run connects to, with every secret already resolved.
//! [`crate::config`] says where a credential comes from, an [`Account`]
//! holds the value itself, so nothing below this module spawns a process
//! to authenticate.
//!
//! Resolution happens once per run through one shared
//! [`pimalaya_config::secret::SecretResolver`], not once per opened
//! connection as it used to: a four-connection source paid four key
//! unlocks, and a command named by several endpoints now runs once.
//!
//! A failure is kept per endpoint ([`Account::get`] raises it) rather than
//! failing the whole resolution, a stale entry for calendars being no
//! reason to leave mail unsynced. Values are never re-read, which is exact
//! for a one-shot run; a daemon would resolve a new account.
//!
//! None of these types derive `Debug`: what they hold is exactly what must
//! not reach a log line.

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
/// Read by name through [`Account::get`], where a failed endpoint raises
/// its error, so one broken credential stops one source and not the whole
/// account.
pub struct Account {
    /// Every endpoint declared, keyed by the name that is also its pimdir
    /// source id.
    ///
    /// A failure is kept as its rendered message: an endpoint is read once
    /// per source syncing against it, and an error carrying a cause chain
    /// cannot be handed out twice.
    endpoints: HashMap<String, Result<SourceAccount, String>>,
}

impl Account {
    /// Resolves every endpoint `config` declares, spawning each distinct
    /// secret command once.
    ///
    /// Fails only when the endpoints cannot be enumerated at all, a failed
    /// endpoint being kept for [`Account::get`] and counted by the spinner,
    /// so a lost one does not read as clean. The wait is a locked agent.
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

    /// The endpoint named `name`, raising what its resolution failed with.
    pub fn get(&self, name: &str) -> Result<SourceAccount> {
        match self.endpoints.get(name) {
            Some(Ok(account)) => Ok(account.clone()),
            Some(Err(err)) => bail!("{err}"),
            None => bail!("This account declares no endpoint named {name}"),
        }
    }
}

/// One endpoint with every secret resolved: what a connection opens from.
#[derive(Clone)]
pub struct SourceAccount {
    /// The backend to connect to, with its credential.
    pub backend: SourceAccountBackend,
    /// The send channel this endpoint declares, if any.
    #[cfg(feature = "smtp")]
    pub smtp: Option<SmtpAccount>,
}

impl SourceAccount {
    /// Resolves one endpoint on its own, for the wizard's checks.
    #[cfg_attr(
        not(any(feature = "imap", feature = "msgraph", feature = "dav")),
        allow(dead_code)
    )]
    pub fn resolve(name: &str, config: &SourceConfig) -> Result<Self> {
        Self::resolve_with(name, config, &mut SecretResolver::new())
    }

    /// Resolves one endpoint, so endpoints naming one command spawn it once.
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

/// The resolved [`SourceBackendConfig`]: one variant per compiled-in
/// backend, holding exactly what its `connect` takes.
#[derive(Clone)]
pub enum SourceAccountBackend {
    #[cfg(feature = "imap")]
    Imap(ImapAccount),
    #[cfg(feature = "dav")]
    Dav(DavAccount),
    #[cfg(feature = "msgraph")]
    Msgraph(MsgraphAccount),
    /// Keeps the type inhabited when no backend is compiled in.
    ///
    /// Never constructed: resolution refuses every backend first.
    #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "dav")))]
    #[allow(dead_code)]
    Unavailable,
}

impl SourceAccountBackend {
    /// Resolves a backend configuration, refusing one this build cannot
    /// open rather than letting it fail at connect.
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
    /// The credential to authenticate with, `None` if preauthenticated.
    pub sasl: Option<Sasl>,
}

/// A resolved DAV endpoint, `kind` telling CardDAV and CalDAV apart.
#[cfg(feature = "dav")]
#[derive(Clone)]
pub struct DavAccount {
    /// Which home set the session discovers from the server URL.
    pub kind: DavKind,
    /// The DAV entry point, a configured authority already read as a URL.
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
    /// The OAuth 2.0 bearer token, as the configured command printed it.
    pub token: SecretString,
    /// The mailbox owner, `me` for the authenticated user.
    pub user_id: String,
    /// The TLS handle, ALPN folded in.
    pub tls: Tls,
}

/// A resolved SMTP submission channel: the arguments of a session open.
#[cfg(feature = "smtp")]
#[derive(Clone)]
pub struct SmtpAccount {
    /// The submission server URL, a configured authority read as one.
    pub server: Url,
    /// The TLS handle, ALPN folded in.
    pub tls: Tls,
    /// Whether a cleartext connection is upgraded through STARTTLS.
    pub starttls: bool,
    /// The credential to authenticate with, `None` for an open relay.
    pub sasl: Option<Sasl>,
}

#[cfg(feature = "smtp")]
impl SmtpAccount {
    /// Resolves a send channel on its own, for the wizard's check.
    #[cfg_attr(not(feature = "imap"), allow(dead_code))]
    pub fn resolve(config: &SmtpConfig) -> Result<Self> {
        Self::resolve_with(config, &mut SecretResolver::new())
    }

    /// Resolves a send channel, spawning nothing when it shares its
    /// source's credential.
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

    /// The reason the resolver exists: four endpoints, one entry, one read.
    #[test]
    fn one_password_command_named_by_four_endpoints_is_spawned_once() {
        let path = temp_dir().join(format!("neverest-resolve-once-{}", process::id()));
        let _ = fs::remove_file(&path);

        // NOTE: counts its own runs, one byte per spawn, and prints a secret.
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
