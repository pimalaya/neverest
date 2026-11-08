//! # Sign-ins
//!
//! Signs in the domains the review settled, once per identity: one OAuth
//! hand-off per authorization server, one password or API token per group
//! of servers of one name.
//!
//! Each group is tested as soon as it is typed, a source by `check`'s
//! per-endpoint test (open it, list its collections with their roles) and
//! the send channel by `check`'s channel test. A refusal is reported for
//! its domain and offers a way on, the domains that passed being kept, and
//! a typed secret is stored once a test passed, under one keyring entry for
//! every source it signs in.
//!
//! Neverest runs no OAuth 2.0 grant of its own: Ortie signs in when it is
//! installed, and a token broker the user picks otherwise.

use std::fmt;

use anyhow::{Error, Result, anyhow, bail};
use pimalaya_cli::{
    prompt,
    spinner::Spinner,
    wizard::{
        keyring,
        ortie::{self, OrtieSignInError},
    },
};
use pimalaya_config::secret::Secret;
use secrecy::SecretString;

#[cfg(feature = "imap")]
use crate::wizard::imap;
#[cfg(feature = "smtp")]
use crate::wizard::smtp;
use crate::{
    account::SourceAccount,
    cli::check::{self, CheckedCollection},
    config::{
        CaldavConfig, CarddavConfig, DavAuthConfig, GmailAuthConfig, GmailConfig, GoogleAuthConfig,
        GoogleConfig, MsgraphAuthConfig, MsgraphConfig, SaslConfig, SaslLoginConfig,
        SaslOauthbearerConfig, SaslPlainConfig, SaslScramSha256Config, SaslXoauth2Config,
        SmtpConfig, SourceBackendConfig, SourceConfig,
    },
    wizard::{
        plan::{self, Domain, Endpoint, Pick, Picks, Protocol, Route, Setup, SignIn},
        secret,
    },
};

/// What the sign-ins know of the account being set up.
pub struct Identity {
    /// The account name, naming the keyring entries and the Ortie account.
    pub name: String,
    /// The typed address.
    pub address: Option<String>,
    /// The domain of the address, or the bare domain typed, flagging a
    /// server off it.
    pub domain: Option<String>,
}

/// The sign-ins of one wizard run, kept across returns to the review so a
/// domain that passed is not asked again.
#[derive(Default)]
pub struct SignIns {
    /// The domains signed in, with the route each signed in over.
    settled: Vec<Settled>,
    /// The secrets typed and stored.
    typed: Vec<Typed>,
    /// Whether the token broker line was printed.
    broker_told: bool,
}

/// How the sign-ins ended.
pub enum Outcome {
    /// Every domain set up is signed in, or left out.
    Done(Vec<Signed>),
    /// The user went back to the review.
    Review,
}

/// A domain signed in, and the block it is written as.
#[derive(Clone, Debug)]
pub struct Signed {
    /// The domain.
    pub domain: Domain,
    /// Its block.
    pub backend: Backend,
}

/// A signed-in block, as the account holds it.
#[derive(Clone, Debug)]
pub enum Backend {
    /// A source, with the collections its test listed.
    Source {
        config: SourceBackendConfig,
        collections: Vec<CheckedCollection>,
    },
    /// The send channel.
    Smtp(SmtpConfig),
}

impl SignIns {
    /// Signs in the domains `setups` resolved to, a domain left out
    /// turning its pick off.
    pub fn run(
        &mut self,
        identity: &Identity,
        setups: &[(Domain, Setup)],
        picks: &mut Picks,
    ) -> Result<Outcome> {
        let routes: Vec<(Domain, &Route)> = setups
            .iter()
            .filter_map(|(domain, setup)| setup.route().map(|route| (*domain, route)))
            .collect();

        let oauth: Vec<&Route> = routes
            .iter()
            .map(|(_, route)| *route)
            .filter(|route| route.sign_in == SignIn::Oauth)
            .collect();
        let suffix = plan::groups(&oauth).len() > 1;

        let mut run = Run {
            domains: routes.iter().map(|(domain, _)| *domain).collect(),
            out: Vec::new(),
        };

        let mut signed = Vec::new();
        let mut units = Vec::new();

        for (domain, route) in routes {
            let settled = self
                .settled
                .iter()
                .find(|settled| settled.domain == domain && settled.route == *route);

            match settled {
                Some(settled) => signed.push(Signed {
                    domain,
                    backend: settled.backend.clone(),
                }),
                None => units.push(Unit {
                    domain,
                    login: route.login.clone().or_else(|| identity.address.clone()),
                    route: route.clone(),
                }),
            }
        }

        let groups = plan::groups(&units.iter().map(|unit| &unit.route).collect::<Vec<_>>());
        let mut units: Vec<Option<Unit>> = units.into_iter().map(Some).collect();

        for group in groups {
            let group: Vec<Unit> = group
                .into_iter()
                .filter_map(|index| units[index].take())
                .filter(|unit| !run.out.contains(&unit.domain))
                .collect();

            if group.is_empty() {
                continue;
            }

            match self.sign_in(identity, group, suffix, picks, &mut run)? {
                Some(done) => signed.extend(done),
                None => return Ok(Outcome::Review),
            }
        }

        signed.retain(|signed| !run.out.contains(&signed.domain));

        Ok(Outcome::Done(signed))
    }

    /// Signs one group in, answering its blocks, or `None` when the user
    /// went back to the review.
    ///
    /// The group's secret is tested against each domain in turn. Retyping
    /// after a refusal concerns the refused domain and the ones after it,
    /// so the domains that passed keep the secret they passed with.
    fn sign_in(
        &mut self,
        identity: &Identity,
        mut pending: Vec<Unit>,
        suffix: bool,
        picks: &mut Picks,
        run: &mut Run,
    ) -> Result<Option<Vec<Signed>>> {
        let mut signed = Vec::new();
        let mut retry = false;

        while !pending.is_empty() {
            ask_login(&mut pending)?;

            let credential = match self.credential(identity, &pending, suffix, retry)? {
                Ok(credential) => credential,
                Err(err) => {
                    eprintln!("{err:#}");

                    match Way::ask(&pending[0], false, run.others(pending[0].domain))? {
                        Way::Review => return Ok(None),
                        Way::LeaveOut(domain) => {
                            run.leave_out(domain, picks);
                            pending.retain(|unit| !run.out.contains(&unit.domain));
                        }
                        Way::TryAgain | Way::ChangeLogin => {}
                    }

                    retry = true;
                    continue;
                }
            };

            let secret = credential.secret();
            let mut passed = Vec::new();
            let mut rest = Vec::new();
            let mut units = pending.into_iter();

            while let Some(unit) = units.next() {
                if run.out.contains(&unit.domain) {
                    continue;
                }

                if let Some(backend) = unit.test(&secret) {
                    passed.push((unit, backend));
                    continue;
                }

                match Way::ask(&unit, unit.route.needs_login(), run.others(unit.domain))? {
                    Way::TryAgain => {
                        rest.push(unit);
                        rest.extend(units);
                        break;
                    }
                    Way::ChangeLogin => {
                        let login = prompt::text("Login:", unit.login.as_deref())?;

                        rest.push(unit);
                        rest.extend(units);

                        for unit in rest.iter_mut().filter(|unit| unit.route.needs_login()) {
                            unit.login = Some(login.clone());
                        }

                        break;
                    }
                    Way::LeaveOut(domain) => run.leave_out(domain, picks),
                    Way::Review => return Ok(None),
                }
            }

            // Leaving mail out takes sending with it, passed or not.
            passed.retain(|(unit, _)| !run.out.contains(&unit.domain));
            rest.retain(|unit| !run.out.contains(&unit.domain));

            if let Some((first, _)) = passed.first() {
                let stored = self.store(identity, credential, first)?;

                for (unit, backend) in passed {
                    let backend = match &stored {
                        Some(stored) => backend.with_secret(stored.clone()),
                        None => backend,
                    };

                    self.settled.push(Settled {
                        domain: unit.domain,
                        route: unit.route,
                        backend: backend.clone(),
                    });

                    signed.push(Signed {
                        domain: unit.domain,
                        backend,
                    });
                }
            }

            pending = rest;
            retry = true;
        }

        Ok(Some(signed))
    }

    /// The secret `pending` signs in with: issued through OAuth, reused,
    /// or typed. The outer error is a prompt failing, the inner one a
    /// sign-in the user can retry.
    ///
    /// A secret typed earlier for a server of the same name is reused
    /// silently, one for another server only when the user says so. A
    /// retry always asks again.
    fn credential(
        &mut self,
        identity: &Identity,
        pending: &[Unit],
        suffix: bool,
        retry: bool,
    ) -> Result<Result<Credential, Error>> {
        let route = &pending[0].route;

        if route.sign_in == SignIn::Oauth {
            return self.oauth(identity, pending, suffix);
        }

        let host = route.endpoint.host();

        if !retry {
            let earlier = || {
                self.typed
                    .iter()
                    .rev()
                    .filter(|typed| typed.sign_in == route.sign_in)
            };

            if let Some(typed) = earlier().find(|typed| plan::same_host(&typed.host, &host)) {
                return Ok(Ok(typed.reused()));
            }

            if let Some(typed) = earlier().next() {
                let noun = match route.sign_in {
                    SignIn::Token => "API token",
                    _ => "password",
                };

                if prompt::bool(format!("Use the same {noun} for {host}?"), false)? {
                    return Ok(Ok(typed.reused()));
                }
            }
        }

        let urls: Vec<String> = pending.iter().map(|unit| unit.route.url()).collect();
        let urls: Vec<&str> = urls.iter().map(String::as_str).collect();
        let secret =
            keyring::prompt_password(route.sign_in.label(), &urls, identity.domain.as_deref())?;

        Ok(Ok(Credential::Typed(secret)))
    }

    /// Signs `pending` in through Ortie, one account covering every
    /// service of the group, or through the token broker the user picks
    /// when Ortie is not installed.
    fn oauth(
        &mut self,
        identity: &Identity,
        pending: &[Unit],
        suffix: bool,
    ) -> Result<Result<Credential, Error>> {
        let mut services: Vec<&str> = Vec::new();

        for unit in pending {
            let service = unit.route.protocol.service();

            if !services.contains(&service) {
                services.push(service);
            }
        }

        let account = match suffix {
            true => format!("neverest-{}-{}", identity.name, services[0]),
            false => format!("neverest-{}", identity.name),
        };

        let input = identity
            .address
            .clone()
            .or_else(|| identity.domain.clone())
            .unwrap_or_else(|| pending[0].route.endpoint.host());

        match ortie::sign_in(&input, &account, &services) {
            Ok(argv) => Ok(Ok(Credential::Read(secret::command_secret(argv)?))),
            Err(OrtieSignInError::NotInstalled) => {
                if !self.broker_told {
                    eprintln!(
                        "A token broker issues the access token: \
                         install Ortie and the wizard signs you in, \
                         or pick the broker you use"
                    );
                    self.broker_told = true;
                }

                let token = secret::configure_token(SignIn::Oauth.label(), &account)?;

                Ok(Ok(Credential::Read(token)))
            }
            Err(err) => Ok(Err(err.into())),
        }
    }

    /// Stores a typed secret once a test passed, under the entry of the
    /// first source it signed in, answering what the configuration reads
    /// instead, `None` when it reads the tested secret already.
    fn store(
        &mut self,
        identity: &Identity,
        credential: Credential,
        first: &Unit,
    ) -> Result<Option<Secret>> {
        match credential {
            Credential::Read(_) => Ok(None),
            Credential::Reused { stored, .. } => Ok(Some(stored)),
            Credential::Typed(secret) => {
                let sign_in = first.route.sign_in;
                let key = format!(
                    "neverest-{}-{}",
                    identity.name,
                    first.route.protocol.source()
                );

                let choice = keyring::prompt_store(sign_in.label(), &key, &secret)?;
                let stored = secret::to_secret(choice)?;

                self.typed.push(Typed {
                    sign_in,
                    host: first.route.endpoint.host(),
                    secret,
                    stored: stored.clone(),
                });

                Ok(Some(stored))
            }
        }
    }
}

impl Backend {
    /// The block reading `secret` where it read the tested one.
    fn with_secret(self, secret: Secret) -> Self {
        match self {
            Self::Smtp(mut config) => {
                config.sasl = config.sasl.map(|sasl| sasl_with_secret(sasl, secret));
                Self::Smtp(config)
            }
            Self::Source {
                config,
                collections,
            } => {
                let config = match config {
                    SourceBackendConfig::Imap(mut config) => {
                        config.sasl = config.sasl.map(|sasl| sasl_with_secret(sasl, secret));
                        SourceBackendConfig::Imap(config)
                    }
                    SourceBackendConfig::Carddav(mut config) => {
                        config.auth = dav_with_secret(config.auth, secret);
                        SourceBackendConfig::Carddav(config)
                    }
                    SourceBackendConfig::Caldav(mut config) => {
                        config.auth = dav_with_secret(config.auth, secret);
                        SourceBackendConfig::Caldav(config)
                    }
                    config => config,
                };

                Self::Source {
                    config,
                    collections,
                }
            }
        }
    }
}

/// What one run of the sign-ins went through.
struct Run {
    /// The domains the run signs in.
    domains: Vec<Domain>,
    /// The domains left out during the run.
    out: Vec<Domain>,
}

impl Run {
    /// Leaves `domain` out, and sending with mail, which it needs.
    fn leave_out(&mut self, domain: Domain, picks: &mut Picks) {
        *picks.get_mut(domain) = Pick::Off;
        self.out.push(domain);

        if domain == Domain::Mail {
            self.out.push(Domain::Sending);
        }
    }

    /// Whether a domain is set up beside `domain`, which leaving it out
    /// keeps.
    fn others(&self, domain: Domain) -> bool {
        self.domains.iter().any(|other| {
            *other != domain
                && !self.out.contains(other)
                && !(domain == Domain::Mail && *other == Domain::Sending)
        })
    }
}

/// A domain signed in during the run, and the route it signed in over.
struct Settled {
    domain: Domain,
    route: Route,
    backend: Backend,
}

/// A secret typed during the run, and what the configuration reads it
/// with.
struct Typed {
    sign_in: SignIn,
    host: String,
    secret: SecretString,
    stored: Secret,
}

/// A domain to sign in.
struct Unit {
    domain: Domain,
    route: Route,
    login: Option<String>,
}

/// The secret a group signs in with.
enum Credential {
    /// Typed now: tested raw, stored once a test passed.
    Typed(SecretString),
    /// Typed earlier in the run, and stored already.
    Reused {
        secret: SecretString,
        stored: Secret,
    },
    /// Read by a command, kept as it is.
    Read(Secret),
}

/// How to go on after a refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Way {
    /// Retype the secret, or sign in again.
    TryAgain,
    /// Ask the login, then retype.
    ChangeLogin,
    /// Save the account without the domain.
    LeaveOut(Domain),
    /// Return to the review.
    Review,
}

impl Typed {
    /// The secret, signing another group in.
    fn reused(&self) -> Credential {
        Credential::Reused {
            secret: self.secret.clone(),
            stored: self.stored.clone(),
        }
    }
}

impl Unit {
    /// Tests the route signing in with `secret` under a spinner, answering
    /// the block that passed, or `None` once the failure is shown.
    fn test(&self, secret: &Secret) -> Option<Backend> {
        let domain = self.domain.name();
        let protocol = self.route.protocol.name();
        let what = format!("{domain} over {protocol}");
        let spinner = Spinner::start(format!("Testing {} over {protocol}", domain.to_lowercase()));

        match self.connect(secret) {
            Ok(backend) => {
                spinner.success(format!("{what} signed in"));
                Some(backend)
            }
            Err(err) => {
                spinner.failure(format!("{what} failed: {err:#}"));
                None
            }
        }
    }

    /// Signs the route in with `secret`, a source listing its collections
    /// as `check` does and the send channel authenticating then quitting.
    fn connect(&self, secret: &Secret) -> Result<Backend> {
        let login = self.login.clone().unwrap_or_default();

        match self.route.protocol {
            #[cfg(feature = "smtp")]
            Protocol::Smtp => smtp::connect(
                &self.route.endpoint,
                self.route.sign_in,
                &login,
                secret.clone(),
            )
            .map(Backend::Smtp),
            protocol => {
                let config = self.source(&login, secret.clone())?;
                let source =
                    SourceAccount::resolve(protocol.source(), &SourceConfig::new(config.clone()))?;
                let collections = check::list(&source)?;

                Ok(Backend::Source {
                    config,
                    collections,
                })
            }
        }
    }

    /// The source block signing `login` in with `secret`. The provider
    /// APIs read the signed-in user's data, `me`.
    fn source(&self, login: &str, secret: Secret) -> Result<SourceBackendConfig> {
        let route = &self.route;
        let alpn = vec![String::from("http/1.1")];

        let server = || match &route.endpoint {
            Endpoint::Http(url) => Ok(url.clone()),
            endpoint => Err(anyhow!("Expected a DAV server, got {endpoint:?}")),
        };

        let dav = |secret| match route.sign_in {
            SignIn::Password => DavAuthConfig::Basic {
                username: login.to_string(),
                password: secret,
            },
            SignIn::Token | SignIn::Oauth => DavAuthConfig::Bearer { token: secret },
        };

        let google = |token| GoogleConfig {
            tls: Default::default(),
            alpn: alpn.clone(),
            auth: GoogleAuthConfig { token },
            collection: Default::default(),
            flag: Default::default(),
            item: Default::default(),
            pool_size: None,
        };

        let msgraph = |token| MsgraphConfig {
            user_id: String::from("me"),
            tls: Default::default(),
            alpn: alpn.clone(),
            auth: MsgraphAuthConfig { token },
            collection: Default::default(),
            flag: Default::default(),
            item: Default::default(),
            pool_size: None,
        };

        Ok(match route.protocol {
            #[cfg(feature = "imap")]
            Protocol::Imap => SourceBackendConfig::Imap(imap::config(
                &route.endpoint,
                route.sign_in,
                login,
                secret,
            )?),
            Protocol::Carddav => SourceBackendConfig::Carddav(CarddavConfig {
                server: server()?,
                tls: Default::default(),
                alpn: alpn.clone(),
                auth: dav(secret),
                collection: Default::default(),
                flag: Default::default(),
                item: Default::default(),
                pool_size: None,
            }),
            Protocol::Caldav => SourceBackendConfig::Caldav(CaldavConfig {
                server: server()?,
                tls: Default::default(),
                alpn: alpn.clone(),
                auth: dav(secret),
                collection: Default::default(),
                flag: Default::default(),
                item: Default::default(),
                pool_size: None,
            }),
            Protocol::Gmail => SourceBackendConfig::Gmail(GmailConfig {
                user_id: String::from("me"),
                tls: Default::default(),
                alpn: alpn.clone(),
                auth: GmailAuthConfig { token: secret },
                batch_size: None,
                batches_in_flight: None,
                units_per_minute: None,
                collection: Default::default(),
                flag: Default::default(),
                item: Default::default(),
                pool_size: None,
            }),
            Protocol::Gpeople => SourceBackendConfig::Gpeople(google(secret)),
            Protocol::Gcal => SourceBackendConfig::Gcal(google(secret)),
            Protocol::Msgraph => SourceBackendConfig::Msgraph(msgraph(secret)),
            Protocol::MsgraphContacts => SourceBackendConfig::MsgraphContacts(msgraph(secret)),
            Protocol::MsgraphCalendar => SourceBackendConfig::MsgraphCalendar(msgraph(secret)),
            protocol => bail!("{} is not compiled in", protocol.name()),
        })
    }
}

impl Credential {
    /// The secret the tests read.
    fn secret(&self) -> Secret {
        match self {
            Self::Typed(secret) | Self::Reused { secret, .. } => Secret::Raw(secret.clone()),
            Self::Read(secret) => secret.clone(),
        }
    }
}

impl Way {
    /// Asks how to go on after `unit` was refused, offering to change the
    /// login when `login`, and to leave the domain out when `others` says
    /// another one is set up or still to come.
    fn ask(unit: &Unit, login: bool, others: bool) -> Result<Self> {
        let mut ways = vec![Self::TryAgain];

        if login {
            ways.push(Self::ChangeLogin);
        }

        if others {
            ways.push(Self::LeaveOut(unit.domain));
        }

        ways.push(Self::Review);

        Ok(prompt::item("What next?", ways, None)?)
    }
}

impl fmt::Display for Way {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TryAgain => f.write_str("Try again"),
            Self::ChangeLogin => f.write_str("Change the login"),
            Self::LeaveOut(domain) => write!(f, "Leave {} out", domain.name().to_lowercase()),
            Self::Review => f.write_str("Back to the review"),
        }
    }
}

/// Asks the login once for the domains needing one nobody knows.
fn ask_login(pending: &mut [Unit]) -> Result<()> {
    let mut missing = pending
        .iter_mut()
        .filter(|unit| unit.route.needs_login() && unit.login.is_none())
        .peekable();

    if missing.peek().is_none() {
        return Ok(());
    }

    let login = prompt::text("Login:", None)?;

    for unit in missing {
        unit.login = Some(login.clone());
    }

    Ok(())
}

/// The SASL block reading `secret` where it read the tested one.
fn sasl_with_secret(sasl: SaslConfig, secret: Secret) -> SaslConfig {
    match sasl {
        SaslConfig::Anonymous(config) => SaslConfig::Anonymous(config),
        SaslConfig::Login(config) => SaslConfig::Login(SaslLoginConfig {
            password: secret,
            ..config
        }),
        SaslConfig::Plain(config) => SaslConfig::Plain(SaslPlainConfig {
            passwd: secret,
            ..config
        }),
        SaslConfig::Oauthbearer(config) => SaslConfig::Oauthbearer(SaslOauthbearerConfig {
            token: secret,
            ..config
        }),
        SaslConfig::Xoauth2(config) => SaslConfig::Xoauth2(SaslXoauth2Config {
            token: secret,
            ..config
        }),
        SaslConfig::ScramSha256(config) => SaslConfig::ScramSha256(SaslScramSha256Config {
            password: secret,
            ..config
        }),
    }
}

/// The DAV authentication reading `secret` where it read the tested one.
fn dav_with_secret(auth: DavAuthConfig, secret: Secret) -> DavAuthConfig {
    match auth {
        DavAuthConfig::Basic { username, .. } => DavAuthConfig::Basic {
            username,
            password: secret,
        },
        DavAuthConfig::Bearer { .. } => DavAuthConfig::Bearer { token: secret },
    }
}

#[cfg(test)]
mod tests {
    use pimalaya_config::{command::CommandConfig, secret::Secret};
    use secrecy::SecretString;

    use crate::{
        config::{CarddavConfig, DavAuthConfig, ImapConfig, SaslConfig, SaslPlainConfig},
        wizard::{plan::Domain, signin::*},
    };

    fn stored() -> Secret {
        Secret::Command(CommandConfig::Argv {
            program: String::from("pass"),
            args: vec![String::from("show"), String::from("neverest-perso-imap")],
        })
    }

    fn typed() -> Secret {
        Secret::Raw(SecretString::from("typed"))
    }

    /// Every block a password signed in reads the stored entry rather than
    /// the typed value, its login kept.
    #[test]
    fn the_stored_secret_replaces_the_typed_one() {
        let imap = Backend::Source {
            config: SourceBackendConfig::Imap(ImapConfig {
                server: String::from("imaps://imap.example.org:993"),
                tls: Default::default(),
                starttls: false,
                alpn: None,
                sasl: Some(SaslConfig::Plain(SaslPlainConfig {
                    authzid: None,
                    authcid: String::from("jane@example.org"),
                    passwd: typed(),
                })),
                collection: Default::default(),
                flag: Default::default(),
                item: Default::default(),
                pool_size: None,
            }),
            collections: Vec::new(),
        };

        let Backend::Source {
            config: SourceBackendConfig::Imap(imap),
            ..
        } = imap.with_secret(stored())
        else {
            panic!("expected an IMAP block");
        };
        let Some(SaslConfig::Plain(plain)) = imap.sasl else {
            panic!("expected a PLAIN block");
        };
        assert_eq!(plain.authcid, "jane@example.org");
        assert!(matches!(plain.passwd, Secret::Command(_)));

        let carddav = Backend::Source {
            config: SourceBackendConfig::Carddav(CarddavConfig {
                server: String::from("https://dav.example.org/"),
                tls: Default::default(),
                alpn: Vec::new(),
                auth: DavAuthConfig::Basic {
                    username: String::from("jane@example.org"),
                    password: typed(),
                },
                collection: Default::default(),
                flag: Default::default(),
                item: Default::default(),
                pool_size: None,
            }),
            collections: Vec::new(),
        };

        let Backend::Source {
            config: SourceBackendConfig::Carddav(carddav),
            ..
        } = carddav.with_secret(stored())
        else {
            panic!("expected a CardDAV block");
        };
        let DavAuthConfig::Basic { username, password } = carddav.auth else {
            panic!("expected Basic");
        };
        assert_eq!(username, "jane@example.org");
        assert!(matches!(password, Secret::Command(_)));
    }

    /// Leaving mail out takes sending with it, and a domain is left out
    /// only while another one stays.
    #[test]
    fn leaving_mail_out_leaves_sending_out() {
        let mut run = Run {
            domains: vec![Domain::Mail, Domain::Sending, Domain::Calendar],
            out: Vec::new(),
        };
        let mut picks = Picks::default();

        assert!(run.others(Domain::Mail));

        run.leave_out(Domain::Mail, &mut picks);
        assert_eq!(picks.mail, Pick::Off);
        assert!(run.out.contains(&Domain::Sending));
        assert!(!run.others(Domain::Calendar));

        let alone = Run {
            domains: vec![Domain::Mail, Domain::Sending],
            out: Vec::new(),
        };
        assert!(!alone.others(Domain::Mail));
        assert!(alone.others(Domain::Sending));
    }
}
