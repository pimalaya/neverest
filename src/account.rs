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

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};

use anyhow::{Context, Result, bail};
#[cfg(feature = "imap")]
use io_imap::types::core::{IString, NString};
#[cfg(any(feature = "imap", feature = "smtp"))]
use io_sasl::mechanism::Sasl;
use pimalaya_cli::{error::CodedError, spinner::Spinner};
use pimalaya_config::secret::SecretResolver;
#[cfg(any(
    feature = "imap",
    feature = "msgraph",
    feature = "smtp",
    feature = "carddav",
    feature = "caldav",
    feature = "gpeople",
    feature = "gcal",
    feature = "gmail"
))]
use pimalaya_stream::{proxy::Proxy, tls::Tls};
#[cfg(any(
    feature = "msgraph",
    feature = "gpeople",
    feature = "gcal",
    feature = "gmail"
))]
use secrecy::SecretString;
#[cfg(any(
    feature = "imap",
    feature = "smtp",
    feature = "carddav",
    feature = "caldav"
))]
use url::Url;

#[cfg(feature = "smtp")]
use crate::config::SmtpConfig;
#[cfg(any(
    feature = "imap",
    feature = "smtp",
    feature = "carddav",
    feature = "caldav"
))]
use crate::config::server_url;
use crate::config::{AccountConfig, ProxyConfig, SourceBackendConfig, SourceConfig};
#[cfg(any(feature = "carddav", feature = "caldav"))]
use crate::dav::client::DavKind;
use crate::error::{ErrorCode, Failure};
#[cfg(feature = "msgraph")]
use crate::msgraph::client::{GraphKind, SizeHints};
use crate::throttle::Throttle;

/// An account's endpoints, resolved once for the run.
///
/// Read by name through [`Account::get`], where a failed endpoint raises
/// its error, so one broken credential stops one source and not the whole
/// account.
pub struct Account {
    /// Every endpoint declared, keyed by the name that is its pimdir source id.
    ///
    /// A failure is kept as its rendered message: an endpoint is read once
    /// per source syncing against it, and an error carrying a cause chain
    /// cannot be handed out twice.
    endpoints: HashMap<String, Result<SourceAccount, Failure>>,
}

impl Account {
    /// Resolves every declared endpoint, spawning each secret command once.
    ///
    /// Fails only when the endpoints cannot be enumerated at all, a failed
    /// endpoint being kept for [`Account::get`] and counted by the spinner,
    /// so a lost one does not read as clean. The wait is a locked agent.
    pub fn resolve(config: &AccountConfig) -> Result<Self> {
        Self::resolve_waiting(config, crate::throttle::MAX_WAIT)
    }

    /// [`Account::resolve`], each source waiting at most `max_wait` at once
    /// on a throttling provider (`sync --max-wait`).
    pub fn resolve_waiting(config: &AccountConfig, max_wait: Duration) -> Result<Self> {
        let endpoints = config.endpoints()?;
        let proxy = config.proxy.as_ref();
        let s = Spinner::start("Resolving credentials…");

        let mut resolver = SecretResolver::new();
        let endpoints: HashMap<_, _> = endpoints
            .into_iter()
            .map(|(name, config)| {
                let resolved =
                    SourceAccount::resolve_with(&name, &config, proxy, &mut resolver, max_wait)
                        .map_err(|err| Failure::of(&err));
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
            Some(Err(failure)) => Err(failure.to_error()),
            None => bail!("This account declares no endpoint named {name}"),
        }
    }

    /// The endpoints that gave up after being throttled this run, with
    /// until when, by name.
    pub fn throttled(&self) -> Vec<(String, SystemTime)> {
        let mut throttled: Vec<_> = self
            .endpoints
            .iter()
            .filter_map(|(name, account)| {
                let until = account.as_ref().ok()?.throttle.gave_up()?;
                Some((name.clone(), until))
            })
            .collect();
        throttled.sort();
        throttled
    }

    /// The octets each endpoint that received any received this run, by
    /// name: the bodies and the meta of its listings, not the wire's own.
    pub fn downloaded(&self) -> Vec<(String, u64)> {
        let mut downloaded: Vec<_> = self
            .endpoints
            .iter()
            .filter_map(|(name, account)| {
                let bytes = account.as_ref().ok()?.downloaded.load(Ordering::Relaxed);
                (bytes > 0).then(|| (name.clone(), bytes))
            })
            .collect();
        downloaded.sort();
        downloaded
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
    /// How the endpoint answers its provider's throttling, shared by every
    /// connection opened from it.
    pub throttle: Arc<Throttle>,
    /// The octets of bodies and meta the run received from the endpoint,
    /// counted across every connection opened from it.
    pub downloaded: Arc<AtomicU64>,
}

impl SourceAccount {
    /// Resolves one endpoint, so endpoints naming one command spawn it once,
    /// its throttle waiting at most `max_wait` at once. `proxy` is the
    /// account's, which a table naming none goes through.
    fn resolve_with(
        name: &str,
        config: &SourceConfig,
        proxy: Option<&ProxyConfig>,
        resolver: &mut SecretResolver,
        max_wait: Duration,
    ) -> Result<Self> {
        let backend = SourceAccountBackend::resolve(&config.backend, proxy, resolver)
            .with_context(|| format!("Resolve the credentials of {name}"))?;

        #[cfg(feature = "smtp")]
        let smtp = config
            .smtp
            .as_ref()
            .map(|smtp| SmtpAccount::resolve_with(smtp, proxy, resolver))
            .transpose()
            .with_context(|| format!("Resolve the send credentials of {name}"))?;

        // NOTE: Gmail meters each user by the second and by the minute, so
        // it is paced as well; every other provider is only followed when
        // it refuses.
        #[cfg(feature = "gmail")]
        let throttle = match &backend {
            SourceAccountBackend::Gmail(gmail) => Throttle::paced_per_minute(
                name,
                crate::gmail::client::UNITS_PER_SECOND,
                gmail.run.settings.units_per_minute,
            ),
            #[allow(unreachable_patterns)]
            _ => Throttle::new(name),
        };
        #[cfg(not(feature = "gmail"))]
        let throttle = Throttle::new(name);

        Ok(Self {
            backend,
            #[cfg(feature = "smtp")]
            smtp,
            throttle: Arc::new(throttle.waiting(max_wait)),
            downloaded: Arc::new(AtomicU64::new(0)),
        })
    }
}

/// The resolved [`SourceBackendConfig`]: one variant per compiled-in
/// backend, holding exactly what its `connect` takes.
#[derive(Clone)]
pub enum SourceAccountBackend {
    #[cfg(feature = "imap")]
    Imap(ImapAccount),
    #[cfg(any(feature = "carddav", feature = "caldav"))]
    Dav(DavAccount),
    #[cfg(feature = "msgraph")]
    Msgraph(MsgraphAccount),
    #[cfg(feature = "gpeople")]
    Gpeople(GoogleAccount),
    #[cfg(feature = "gcal")]
    Gcal(GoogleAccount),
    #[cfg(feature = "gmail")]
    Gmail(GmailAccount),
    /// Keeps the type inhabited when no backend is compiled in.
    ///
    /// Never constructed: resolution refuses every backend first.
    #[cfg(not(any(
        feature = "imap",
        feature = "msgraph",
        feature = "carddav",
        feature = "caldav",
        feature = "gpeople",
        feature = "gcal",
        feature = "gmail"
    )))]
    #[allow(dead_code)]
    Unavailable,
}

impl SourceAccountBackend {
    /// Resolves a backend configuration, refusing one this build cannot
    /// open rather than letting it fail at connect.
    #[cfg_attr(
        not(any(
            feature = "imap",
            feature = "msgraph",
            feature = "carddav",
            feature = "caldav",
            feature = "gpeople",
            feature = "gcal",
            feature = "gmail"
        )),
        allow(unused_variables)
    )]
    fn resolve(
        config: &SourceBackendConfig,
        account_proxy: Option<&ProxyConfig>,
        resolver: &mut SecretResolver,
    ) -> Result<Self> {
        // NOTE: the backend's own proxy first, then the account's, then the
        // environment's, read when a connection opens.
        let proxy = |resolver: &mut SecretResolver| {
            ProxyConfig::resolve(&[config.proxy(), account_proxy], resolver)
        };

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
                    proxy: proxy(resolver)?,
                    sasl_ir: config.sasl_ir,
                    auto_id: crate::imap::client::auto_id(&config.id)?,
                }))
            }
            #[cfg(feature = "carddav")]
            SourceBackendConfig::Carddav(config) => Ok(Self::Dav(DavAccount {
                kind: DavKind::Card,
                endpoint: DavEndpoint::of(config.server.as_deref(), config.home.as_deref())?,
                tls: config.tls.clone().into_tls(config.alpn.clone()),
                auth: config.auth.clone().try_into_dav_auth(resolver)?,
                proxy: proxy(resolver)?,
            })),
            #[cfg(feature = "caldav")]
            SourceBackendConfig::Caldav(config) => Ok(Self::Dav(DavAccount {
                kind: DavKind::Cal,
                endpoint: DavEndpoint::of(config.server.as_deref(), config.home.as_deref())?,
                tls: config.tls.clone().into_tls(config.alpn.clone()),
                auth: config.auth.clone().try_into_dav_auth(resolver)?,
                proxy: proxy(resolver)?,
            })),
            #[cfg(feature = "msgraph")]
            SourceBackendConfig::Msgraph(config) => Ok(Self::Msgraph(MsgraphAccount {
                kind: GraphKind::Mail,
                token: resolver.resolve(config.auth.token.clone())?,
                user_id: config.user_id.clone(),
                tls: config.tls.clone().into_tls(config.alpn.clone()),
                sizes: SizeHints::default(),
                proxy: proxy(resolver)?,
            })),
            #[cfg(feature = "msgraph")]
            SourceBackendConfig::MsgraphContacts(config) => Ok(Self::Msgraph(MsgraphAccount {
                kind: GraphKind::Contacts,
                token: resolver.resolve(config.auth.token.clone())?,
                user_id: config.user_id.clone(),
                tls: config.tls.clone().into_tls(config.alpn.clone()),
                sizes: SizeHints::default(),
                proxy: proxy(resolver)?,
            })),
            #[cfg(feature = "msgraph")]
            SourceBackendConfig::MsgraphCalendar(config) => Ok(Self::Msgraph(MsgraphAccount {
                kind: GraphKind::Calendar,
                token: resolver.resolve(config.auth.token.clone())?,
                user_id: config.user_id.clone(),
                tls: config.tls.clone().into_tls(config.alpn.clone()),
                sizes: SizeHints::default(),
                proxy: proxy(resolver)?,
            })),
            #[cfg(feature = "gpeople")]
            SourceBackendConfig::Gpeople(config) => Ok(Self::Gpeople(GoogleAccount {
                token: resolver.resolve(config.auth.token.clone())?,
                tls: config.tls.clone().into_tls(config.alpn.clone()),
                proxy: proxy(resolver)?,
            })),
            #[cfg(feature = "gcal")]
            SourceBackendConfig::Gcal(config) => Ok(Self::Gcal(GoogleAccount {
                token: resolver.resolve(config.auth.token.clone())?,
                tls: config.tls.clone().into_tls(config.alpn.clone()),
                proxy: proxy(resolver)?,
            })),
            #[cfg(feature = "gmail")]
            SourceBackendConfig::Gmail(config) => {
                let settings = crate::gmail::run::GmailSettings::new(
                    config.batch_size,
                    config.batches_in_flight,
                    config.units_per_minute,
                )?;
                Ok(Self::Gmail(GmailAccount {
                    token: resolver.resolve(config.auth.token.clone())?,
                    user_id: config.user_id.clone(),
                    tls: config.tls.clone().into_tls(config.alpn.clone()),
                    proxy: proxy(resolver)?,
                    run: Arc::new(crate::gmail::run::GmailRun::new(settings)),
                }))
            }
            SourceBackendConfig::Jmap(_) => {
                bail!("A JMAP source parses but cannot be opened yet")
            }
            #[allow(unreachable_patterns)]
            config => Err(backend_not_built(config.feature())),
        }
    }
}

/// The failure of a source whose backend this build leaves out, coded
/// `backend-not-built` and naming the cargo feature: its table loads, so
/// one file serves every build.
fn backend_not_built(feature: &str) -> anyhow::Error {
    let message = format!(
        "This build leaves out the `{feature}` backend, rebuild with `--features {feature}`"
    );
    CodedError::new(ErrorCode::BackendNotBuilt, message).into()
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
    /// How the connection reaches the server.
    pub proxy: Proxy,
    /// Whether the SASL initial response rides the `AUTHENTICATE` command,
    /// `None` following the advertised `SASL-IR`.
    pub sasl_ir: Option<bool>,
    /// The parameters of the `ID` sent after authentication, `None` sending
    /// none.
    pub auto_id: Option<Vec<(IString<'static>, NString<'static>)>>,
}

/// A resolved DAV endpoint, `kind` telling CardDAV and CalDAV apart.
#[cfg(any(feature = "carddav", feature = "caldav"))]
#[derive(Clone)]
pub struct DavAccount {
    /// Which home set the session discovers, or is given.
    pub kind: DavKind,
    /// Where the session starts.
    pub endpoint: DavEndpoint,
    /// The TLS handle, ALPN folded in.
    pub tls: Tls,
    /// The credential every request carries.
    pub auth: io_webdav::rfc4918::WebdavAuth,
    /// How the connections reach the server.
    pub proxy: Proxy,
}

/// Where a DAV session starts: an entry point it discovers the home set
/// from, or the home set itself.
#[cfg(any(feature = "carddav", feature = "caldav"))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DavEndpoint {
    /// The DAV entry point, a configured authority already read as a URL.
    Server(Url),
    /// The home set, where the session lists its collections.
    Home(Url),
}

#[cfg(any(feature = "carddav", feature = "caldav"))]
impl DavEndpoint {
    /// The endpoint a table names: its `home` when it has one, else its
    /// `server`, read as `https://` when bare.
    pub fn of(server: Option<&str>, home: Option<&str>) -> Result<Self> {
        if let Some(home) = home {
            let url =
                Url::parse(home).with_context(|| format!("Read the home set `{home}` as a URL"))?;
            return Ok(Self::Home(url));
        }

        match server {
            Some(server) => Ok(Self::Server(server_url(server, "https")?)),
            None => bail!("A DAV source needs `server`, or the `home` set it lists"),
        }
    }

    /// The URL the session connects to.
    pub fn url(&self) -> &Url {
        match self {
            Self::Server(url) | Self::Home(url) => url,
        }
    }
}

/// A resolved Microsoft Graph endpoint.
#[cfg(feature = "msgraph")]
#[derive(Clone)]
pub struct MsgraphAccount {
    /// Which domain the session syncs.
    pub kind: GraphKind,
    /// The OAuth 2.0 bearer token, as the configured command printed it.
    pub token: SecretString,
    /// The mailbox owner, `me` for the authenticated user.
    pub user_id: String,
    /// The TLS handle, ALPN folded in.
    pub tls: Tls,
    /// The message sizes its listings stated, shared by its connections.
    pub sizes: SizeHints,
    /// How the connections reach the server.
    pub proxy: Proxy,
}

/// A resolved Google endpoint, People or Calendar.
#[cfg(any(feature = "gpeople", feature = "gcal"))]
#[derive(Clone)]
pub struct GoogleAccount {
    /// The OAuth 2.0 bearer token, as the configured command printed it.
    pub token: SecretString,
    /// The TLS handle, ALPN folded in.
    pub tls: Tls,
    /// How the connections reach the server.
    pub proxy: Proxy,
}

/// A resolved Gmail endpoint.
#[cfg(feature = "gmail")]
#[derive(Clone)]
pub struct GmailAccount {
    /// The OAuth 2.0 bearer token, as the configured command printed it.
    pub token: SecretString,
    /// The mailbox owner, `me` for the authenticated user.
    pub user_id: String,
    /// The TLS handle, ALPN folded in.
    pub tls: Tls,
    /// How the connections reach the server.
    pub proxy: Proxy,
    /// What the source's connections share for the run: its batch
    /// settings and gate, its caches, its history.
    pub run: Arc<crate::gmail::run::GmailRun>,
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
    /// How the connection reaches the server.
    pub proxy: Proxy,
}

#[cfg(feature = "smtp")]
impl SmtpAccount {
    /// Resolves a send channel on its own, for the submission tests.
    #[cfg(test)]
    pub fn resolve(config: &SmtpConfig) -> Result<Self> {
        Self::resolve_with(config, None, &mut SecretResolver::new())
    }

    /// Resolves a send channel, spawning nothing when it shares its
    /// source's credential, through its own proxy, else `proxy`, the
    /// account's.
    fn resolve_with(
        config: &SmtpConfig,
        proxy: Option<&ProxyConfig>,
        resolver: &mut SecretResolver,
    ) -> Result<Self> {
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
            proxy: ProxyConfig::resolve(&[config.proxy.as_ref(), proxy], resolver)?,
        })
    }
}

#[cfg(all(
    test,
    unix,
    feature = "imap",
    feature = "smtp",
    feature = "carddav",
    feature = "caldav"
))]
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

    /// `sync --max-wait` reaches the throttle every connection shares.
    #[test]
    fn the_run_s_longest_wait_reaches_each_source() {
        let config: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://localhost"
            caldav.server = "https://localhost"
            caldav.auth.basic.username = "user"
            caldav.auth.basic.password.raw = "secret"
            "#,
        )
        .unwrap();
        let wait = std::time::Duration::from_secs(7);
        let account = Account::resolve_waiting(&config, wait).unwrap();
        for source in ["imap", "caldav"] {
            assert_eq!(account.get(source).unwrap().throttle.max_wait(), wait);
        }
    }
}

#[cfg(all(
    test,
    any(
        not(feature = "gcal"),
        not(feature = "carddav"),
        not(feature = "caldav")
    )
))]
mod build_tests {
    use pimalaya_cli::error::code_of;

    use super::Account;
    use crate::{config::AccountConfig, error::ErrorCode};

    /// Opens the source `table` of the one-table account `toml`, which the
    /// build leaves out, and checks the refusal names the feature `table`.
    fn assert_names_its_feature(toml: &str, table: &str) {
        let config: AccountConfig = toml::from_str(toml).unwrap();

        let Err(err) = Account::resolve(&config).unwrap().get(table) else {
            panic!("a {table} source opened in a build without {table}");
        };

        assert_eq!(code_of(&err), Some(ErrorCode::BackendNotBuilt));
        let message = format!("{err:#}");
        assert!(message.contains(&format!("`{table}` backend")), "{message}");
        assert!(
            message.ends_with(&format!("--features {table}`")),
            "{message}"
        );
    }

    /// A source of a backend the build leaves out loads, and opening it
    /// fails coded `backend-not-built`, naming the cargo feature.
    #[cfg(not(feature = "gcal"))]
    #[test]
    fn a_backend_left_out_of_the_build_names_its_feature() {
        assert_names_its_feature("gcal.auth.token.raw = \"token\"", "gcal");
    }

    /// Each DAV table names its own feature, not a shared `dav` one.
    #[cfg(not(feature = "carddav"))]
    #[test]
    fn a_carddav_source_left_out_names_carddav() {
        assert_names_its_feature("carddav.auth.bearer.token.raw = \"tok\"", "carddav");
    }

    #[cfg(not(feature = "caldav"))]
    #[test]
    fn a_caldav_source_left_out_names_caldav() {
        assert_names_its_feature("caldav.auth.bearer.token.raw = \"tok\"", "caldav");
    }
}

#[cfg(test)]
mod connection_tests {
    use pimalaya_config::secret::SecretResolver;
    use pimalaya_stream::proxy::Proxy;

    use crate::config::ProxyConfig;

    fn proxy(toml: &str) -> ProxyConfig {
        toml::from_str(toml).unwrap()
    }

    /// The table closest to the connection names the proxy, the account's
    /// next; none reads the environment when the connection opens.
    #[test]
    fn the_closest_proxy_wins_and_none_reads_the_environment() {
        let mut resolver = SecretResolver::new();
        let own = proxy(r#"url = "socks5h://127.0.0.1:1080""#);
        let account = proxy(r#"url = "http://proxy.example.org:3128""#);

        let resolved = ProxyConfig::resolve(&[Some(&own), Some(&account)], &mut resolver);
        assert!(matches!(
            resolved.unwrap(),
            Proxy::Socks5 { port: 1080, .. }
        ));

        let resolved = ProxyConfig::resolve(&[None, Some(&account)], &mut resolver);
        assert!(matches!(resolved.unwrap(), Proxy::Http { port: 3128, .. }));

        let resolved = ProxyConfig::resolve(&[None, None], &mut resolver);
        assert!(matches!(resolved.unwrap(), Proxy::System));
    }

    /// A password with no username is refused, a username alone sent with
    /// an empty password.
    #[test]
    fn a_proxy_password_needs_a_username() {
        let mut resolver = SecretResolver::new();
        let config = proxy("url = \"http://p.example.org:3128\"\npassword.raw = \"pw\"");
        let err = ProxyConfig::resolve(&[Some(&config)], &mut resolver).unwrap_err();
        assert!(err.to_string().contains("requires a username"), "{err}");

        let config = proxy("url = \"http://p.example.org:3128\"\nusername = \"user\"");
        let resolved = ProxyConfig::resolve(&[Some(&config)], &mut resolver).unwrap();
        let Proxy::Http {
            auth: Some(auth), ..
        } = resolved
        else {
            panic!("an authenticated HTTP proxy");
        };
        assert_eq!(auth.user, "user");
    }

    /// A DAV table's `home` is where the session starts, its `server` the
    /// entry point otherwise, and one of the two is required.
    #[cfg(any(feature = "carddav", feature = "caldav"))]
    #[test]
    fn a_dav_home_set_is_taken_over_the_server() {
        use crate::account::DavEndpoint;

        let home = "https://dav.example.org/card/user/";
        let endpoint = DavEndpoint::of(Some("dav.example.org"), Some(home)).unwrap();
        assert!(matches!(&endpoint, DavEndpoint::Home(url) if url.as_str() == home));

        let endpoint = DavEndpoint::of(Some("dav.example.org"), None).unwrap();
        let server = "https://dav.example.org/";
        assert!(matches!(&endpoint, DavEndpoint::Server(url) if url.as_str() == server));

        let err = DavEndpoint::of(None, None).unwrap_err();
        assert!(err.to_string().contains("`home`"), "{err}");
    }

    /// `id.auto` sends the fields asked for, neverest's value for a known
    /// key set to true and `NIL` otherwise; off, no `ID` is sent.
    #[cfg(feature = "imap")]
    #[test]
    fn the_imap_id_sends_the_fields_asked_for() {
        use io_imap::types::core::NString;

        use crate::{config::ImapIdConfig, imap::client::auto_id};

        let off: ImapIdConfig = toml::from_str("fields.name = true").unwrap();
        assert!(auto_id(&off).unwrap().is_none());

        let config: ImapIdConfig = toml::from_str(
            "auto = true\nfields.name = true\nfields.os = true\nfields.vendor = false",
        )
        .unwrap();
        let params = auto_id(&config).unwrap().unwrap();
        let params: Vec<(String, Option<String>)> = params
            .into_iter()
            .map(|(key, value)| {
                let key = String::from_utf8_lossy(key.as_ref()).into_owned();
                let value = match value {
                    NString(Some(value)) => Some(String::from_utf8_lossy(value.as_ref()).into()),
                    NString(None) => None,
                };
                (key, value)
            })
            .collect();
        let expected = [
            (String::from("name"), Some(String::from("neverest"))),
            (String::from("os"), None),
            (String::from("vendor"), None),
        ];
        assert_eq!(params, expected);
    }
}
