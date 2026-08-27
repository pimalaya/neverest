//! IMAP + SMTP wizard.
//!
//! A discovery entry pins the endpoints, so [`configure_discovered`]
//! picks the SASL mechanism, prompts its credentials and tests the IMAP
//! connection, then, when discovery also found a submission endpoint,
//! asks whether the account's send channel shares them: if so the same
//! `sasl` table backs both, otherwise a mechanism is picked again (IMAP
//! and SMTP may advertise different auth), and the SMTP connection is
//! tested last. The wizard never invents an SMTP host: with no discovered
//! submission endpoint the account has no send channel, and the user adds
//! one by hand.
//!
//! IMAP is the sync side; SMTP is only the channel the queued submit
//! intents are performed through. Both configure one `sasl` table of the
//! same six mechanisms, so reuse is a copy of the IMAP one whatever it
//! names, an OAuth token included.
//!
//! Only IMAP is probed for its advertised mechanisms. io-imap reads them
//! off `CAPABILITY` as [`SaslMechanism`] values; io-smtp exposes the
//! `AUTH` line as strings and no such reader, so the SMTP prompt offers
//! the list discovery advertised rather than one the server confirmed.

use anyhow::{Result, bail};
use io_pim_discovery::compose::config::DiscoverySecurity;
use io_sasl::mechanism::SaslMechanism;
use pimalaya_cli::{prompt, spinner::Spinner};

use crate::{
    client,
    config::{
        ImapConfig, SaslAnonymousConfig, SaslConfig, SaslLoginConfig, SaslOauthbearerConfig,
        SaslPlainConfig, SaslScramSha256Config, SaslXoauth2Config, SmtpConfig, SourceBackendConfig,
        SourceConfig, TlsConfig,
    },
    wizard::{
        search::{AuthCaps, Discovered, DiscoveredKind, TcpEndpoint},
        secret,
    },
};

const PLAIN: &str = "PLAIN (username + password)";
const LOGIN: &str = "LOGIN (username + password)";
const SCRAM_SHA_256: &str = "SCRAM-SHA-256 (username + password)";
const ANONYMOUS: &str = "ANONYMOUS (no credentials)";
const OAUTHBEARER: &str = "OAUTHBEARER (username + API token)";
const XOAUTH2: &str = "XOAUTH2 (username + API token)";

/// Configures the IMAP side and the SMTP send channel from a discovered
/// entry: pick the SASL mechanism and credentials for IMAP, test the
/// connection, then ask whether SMTP reuses them (prompting a mechanism
/// of its own when it does not) and test SMTP last. Both connections are
/// validated here, so the caller writes a configuration that is known to
/// connect.
pub fn configure_discovered(
    account_name: &str,
    email: &str,
    discovered: &Discovered,
) -> Result<(ImapConfig, Option<SmtpConfig>)> {
    let DiscoveredKind::ImapSmtp { imap, smtp } = &discovered.kind else {
        bail!("Expected an IMAP + SMTP configuration");
    };

    let login_hint = discovered.login_default(email);

    let probed = probe_mechanisms(imap);
    let imap_sasl = prompt_sasl(
        account_name,
        "imap",
        login_hint.as_deref(),
        discovered.auth,
        probed.as_deref(),
    )?;
    let imap = imap_config(imap, imap_sasl.clone());
    test_connection("IMAP", || {
        client::open(SourceConfig::new(SourceBackendConfig::Imap(imap.clone()))).map(|_| ())
    })?;

    let smtp = configure_smtp(
        account_name,
        login_hint.as_deref(),
        discovered.auth,
        &imap_sasl,
        smtp.as_ref(),
    )?;

    Ok((imap, smtp))
}

/// Configures and tests the send channel from the discovered submission
/// endpoint, reusing the IMAP `sasl` table on confirmation. `None` when
/// discovery found no endpoint: neverest never invents a submission host.
///
/// Declining reuse asks whether the server authenticates at all before
/// offering a mechanism, since a submission relay on a trusted network
/// takes no `AUTH`, which the config spells by omitting `sasl`.
#[cfg(feature = "smtp")]
fn configure_smtp(
    account_name: &str,
    login_hint: Option<&str>,
    caps: AuthCaps,
    imap_sasl: &SaslConfig,
    endpoint: Option<&TcpEndpoint>,
) -> Result<Option<SmtpConfig>> {
    let Some(endpoint) = endpoint else {
        return Ok(None);
    };

    let sasl = if prompt::bool("Use the same credentials for SMTP?", true)? {
        Some(imap_sasl.clone())
    } else if prompt::bool("Does the submission server authenticate?", true)? {
        // NOTE: no probed list, io-smtp exposing the `AUTH` line as strings
        // and no mechanism reader, so the discovered capabilities key the
        // menu instead.
        Some(prompt_sasl(account_name, "smtp", login_hint, caps, None)?)
    } else {
        None
    };

    let smtp = smtp_config(endpoint, sasl);
    test_connection("SMTP", || {
        crate::offline::submit::connect_smtp(&smtp).map(|_| ())
    })?;

    Ok(Some(smtp))
}

/// Without the `smtp` feature the build has no send channel, so a
/// discovered submission endpoint is reported and dropped.
#[cfg(not(feature = "smtp"))]
fn configure_smtp(
    _account_name: &str,
    _login_hint: Option<&str>,
    _caps: AuthCaps,
    _imap_sasl: &SaslConfig,
    endpoint: Option<&TcpEndpoint>,
) -> Result<Option<SmtpConfig>> {
    if endpoint.is_some() {
        eprintln!("A submission server was discovered, but this build has no `smtp` feature.");
    }

    Ok(None)
}

/// Runs a connection `test` behind a labelled spinner, surfacing a
/// failure as the wizard's error so a bad credential stops here instead
/// of yielding a config that cannot connect.
fn test_connection(label: &str, test: impl FnOnce() -> Result<()>) -> Result<()> {
    let spinner = Spinner::start(format!("Testing {label} connection"));

    if let Err(err) = test() {
        spinner.failure(format!("{label} connection failed"));
        return Err(err);
    }

    spinner.success(format!("{label} connection succeeded"));
    Ok(())
}

/// Prompts the SASL mechanism then its credentials. When `probed` is
/// `Some` (a live IMAP CAPABILITY probe) only those mechanisms are
/// offered, most preferred first and LOGIN last; otherwise the full list
/// keyed on `caps` is offered, so a failed probe never leaves the user
/// stuck. The token mechanisms' OAuth brokers appear only when a token
/// or OAuth grant was advertised.
///
/// `service` names the protocol the credentials are for, so a keyring
/// entry says which of an account's two it holds.
fn prompt_sasl(
    account_name: &str,
    service: &str,
    login_hint: Option<&str>,
    caps: AuthCaps,
    probed: Option<&[SaslMechanism]>,
) -> Result<SaslConfig> {
    let mechanism = prompt_mechanism(caps, probed)?;
    build_sasl(mechanism, account_name, service, login_hint, caps)
}

/// Prompts the authentication mechanism: the probed list when the server
/// advertised one, otherwise the full fallback list. A single candidate
/// is selected without prompting.
fn prompt_mechanism(caps: AuthCaps, probed: Option<&[SaslMechanism]>) -> Result<SaslMechanism> {
    // NOTE: io-sasl names more mechanisms than [`SaslConfig`] can spell, so a
    // probed one this wizard cannot write a config for is dropped rather than
    // offered. A probe left with nothing falls back like a failed one.
    let probed: Vec<SaslMechanism> = probed
        .unwrap_or_default()
        .iter()
        .copied()
        .filter(|mechanism| mechanism_label(mechanism).is_some())
        .collect();
    let mechanisms = if probed.is_empty() {
        fallback_mechanisms(caps)
    } else {
        probed
    };

    let labels: Vec<&str> = mechanisms.iter().filter_map(mechanism_label).collect();
    let label = if labels.len() == 1 {
        labels[0]
    } else {
        prompt::item("SASL mechanism:", labels, None)?
    };

    Ok(mechanisms
        .into_iter()
        .find(|m| mechanism_label(m) == Some(label))
        .expect("chosen label matches a mechanism"))
}

/// Prompts the credentials for `mechanism` and builds its SASL config.
/// ANONYMOUS carries no login; every other mechanism needs one, plus a
/// password (basic family) or an API token (OAuth family).
fn build_sasl(
    mechanism: SaslMechanism,
    account_name: &str,
    service: &str,
    login_hint: Option<&str>,
    caps: AuthCaps,
) -> Result<SaslConfig> {
    if let SaslMechanism::Anonymous = mechanism {
        let message = prompt::text("ANONYMOUS message (optional):", None::<&str>)?;
        let message = Some(message).filter(|m| !m.trim().is_empty());
        return Ok(SaslConfig::Anonymous(SaslAnonymousConfig { message }));
    }

    let login = prompt::text("Login:", login_hint)?;
    let key = format!("{account_name}-{service}");

    Ok(match mechanism {
        SaslMechanism::Plain => {
            let passwd = secret::configure_password("Password", &key)?;
            SaslConfig::Plain(SaslPlainConfig {
                authzid: None,
                authcid: login,
                passwd,
            })
        }
        SaslMechanism::Login => {
            let password = secret::configure_password("Password", &key)?;
            SaslConfig::Login(SaslLoginConfig {
                username: login,
                password,
            })
        }
        SaslMechanism::ScramSha256 => {
            let password = secret::configure_password("Password", &key)?;
            SaslConfig::ScramSha256(SaslScramSha256Config {
                username: login,
                password,
            })
        }
        SaslMechanism::OAuthBearer => {
            let token = secret::configure_token("API token", &key, caps.oauth || !caps.any())?;
            SaslConfig::Oauthbearer(SaslOauthbearerConfig {
                username: login,
                token,
            })
        }
        SaslMechanism::XOAuth2 => {
            let token = secret::configure_token("API token", &key, caps.oauth || !caps.any())?;
            SaslConfig::Xoauth2(SaslXoauth2Config {
                username: login,
                token,
            })
        }
        SaslMechanism::Anonymous => unreachable!("handled above"),
        mechanism => bail!("Unsupported SASL mechanism {}", mechanism.as_str()),
    })
}

/// The menu label for a mechanism, split by the credential it needs, or
/// `None` for one [`SaslConfig`] cannot spell (io-sasl names every
/// registered mechanism, neverest configures six of them).
fn mechanism_label(mechanism: &SaslMechanism) -> Option<&'static str> {
    match mechanism {
        SaslMechanism::ScramSha256 => Some(SCRAM_SHA_256),
        SaslMechanism::Plain => Some(PLAIN),
        SaslMechanism::OAuthBearer => Some(OAUTHBEARER),
        SaslMechanism::XOAuth2 => Some(XOAUTH2),
        SaslMechanism::Anonymous => Some(ANONYMOUS),
        SaslMechanism::Login => Some(LOGIN),
        _ => None,
    }
}

/// The mechanisms offered when no live probe is available, keyed on what
/// discovery advertised (every family when nothing was): most preferred
/// first, LOGIN last, token mechanisms only when a token or OAuth grant
/// was advertised.
fn fallback_mechanisms(caps: AuthCaps) -> Vec<SaslMechanism> {
    let mut mechanisms = Vec::new();

    if caps.basic || !caps.any() {
        mechanisms.extend([SaslMechanism::ScramSha256, SaslMechanism::Plain]);
    }
    if caps.token() || !caps.any() {
        mechanisms.extend([SaslMechanism::OAuthBearer, SaslMechanism::XOAuth2]);
    }
    if caps.basic || !caps.any() {
        mechanisms.extend([SaslMechanism::Anonymous, SaslMechanism::Login]);
    }

    mechanisms
}

/// Opens an unauthenticated IMAP connection to the discovered endpoint
/// purely to read the server's CAPABILITY, and returns the mechanisms it
/// advertises (most preferred first, LOGIN last), so only what the server
/// supports is offered. `None` (offer the full list) when the probe fails
/// or advertises nothing usable: the error is logged, never surfaced, so
/// the wizard falls back rather than stopping.
fn probe_mechanisms(endpoint: &TcpEndpoint) -> Option<Vec<SaslMechanism>> {
    use io_imap::{
        client::{ImapClientStd, default_alpn},
        rfc3501::capability::available_auth_mechanisms,
        session::ImapSessionOpenOptions,
    };
    use io_sasl::mechanism::Sasl;

    let probe = || -> Result<Vec<SaslMechanism>> {
        let tls = TlsConfig::default().into_tls(default_alpn());
        let server = url::Url::parse(&endpoint_server(endpoint))?;
        let opts = ImapSessionOpenOptions {
            starttls: endpoint.security == DiscoverySecurity::Starttls,
            ..Default::default()
        };
        let (_client, capabilities) = ImapClientStd::connect(&server, &tls, None::<Sasl>, opts)?;
        Ok(available_auth_mechanisms(&capabilities))
    };

    match probe() {
        Ok(mechanisms) if !mechanisms.is_empty() => Some(mechanisms),
        Ok(_) => None,
        Err(err) => {
            log::warn!("could not probe IMAP capabilities, offering all mechanisms: {err:#}");
            None
        }
    }
}

/// The `scheme://host:port` string for a discovered IMAP endpoint,
/// matching how [`imap_config`] builds the server URL.
fn endpoint_server(endpoint: &TcpEndpoint) -> String {
    let scheme = if endpoint.security == DiscoverySecurity::Tls {
        "imaps"
    } else {
        "imap"
    };

    format!("{scheme}://{}:{}", endpoint.host, endpoint.port)
}

fn imap_config(endpoint: &TcpEndpoint, sasl: SaslConfig) -> ImapConfig {
    ImapConfig {
        server: endpoint_server(endpoint),
        tls: Default::default(),
        starttls: endpoint.security == DiscoverySecurity::Starttls,
        // NOTE: unset, so io-imap keeps owning the default rather than the
        // value being frozen into the written config.
        alpn: None,
        sasl: Some(sasl),
        collection: Default::default(),
        flag: Default::default(),
        item: Default::default(),
        pool_size: None,
    }
}

#[cfg(feature = "smtp")]
fn smtp_config(endpoint: &TcpEndpoint, sasl: Option<SaslConfig>) -> SmtpConfig {
    let scheme = if endpoint.security == DiscoverySecurity::Tls {
        "smtps"
    } else {
        "smtp"
    };

    SmtpConfig {
        server: format!("{scheme}://{}:{}", endpoint.host, endpoint.port),
        tls: Default::default(),
        starttls: endpoint.security == DiscoverySecurity::Starttls,
        // NOTE: unset, so io-smtp keeps owning the default rather than the
        // value being frozen into the written config.
        alpn: None,
        sasl,
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "smtp")]
    use pimalaya_config::secret::Secret;

    use super::*;

    fn endpoint(security: DiscoverySecurity, port: u16) -> TcpEndpoint {
        TcpEndpoint {
            host: "imap.example.org".into(),
            port,
            security,
        }
    }

    #[test]
    fn imap_endpoint_maps_security_onto_scheme_and_starttls() {
        let tls = imap_config(
            &endpoint(DiscoverySecurity::Tls, 993),
            SaslConfig::Anonymous(SaslAnonymousConfig::default()),
        );
        assert_eq!(tls.server, "imaps://imap.example.org:993");
        assert!(!tls.starttls);
        assert!(tls.alpn.is_none());

        let starttls = imap_config(
            &endpoint(DiscoverySecurity::Starttls, 143),
            SaslConfig::Anonymous(SaslAnonymousConfig::default()),
        );
        assert_eq!(starttls.server, "imap://imap.example.org:143");
        assert!(starttls.starttls);
    }

    /// The offered menu, as the labels the user actually sees, since
    /// `SaslMechanism` implements no `PartialEq`.
    fn labels(caps: AuthCaps) -> Vec<&'static str> {
        fallback_mechanisms(caps)
            .iter()
            .filter_map(mechanism_label)
            .collect()
    }

    #[test]
    fn fallback_mechanisms_follow_the_advertised_capabilities() {
        let basic = labels(AuthCaps {
            basic: true,
            ..Default::default()
        });
        assert!(!basic.contains(&XOAUTH2));
        assert!(basic.contains(&PLAIN));
        assert_eq!(basic.last(), Some(&LOGIN));

        let token = labels(AuthCaps {
            bearer: true,
            ..Default::default()
        });
        assert_eq!(token, vec![OAUTHBEARER, XOAUTH2]);

        let unknown = labels(AuthCaps::default());
        assert!(unknown.contains(&PLAIN));
        assert!(unknown.contains(&XOAUTH2));
        assert_eq!(unknown.last(), Some(&LOGIN));
    }

    /// Reuse is a copy of the IMAP table, so a token mechanism backs the
    /// send channel as readily as a password one: the case the old
    /// LOGIN-only channel had to refuse.
    #[cfg(feature = "smtp")]
    #[test]
    fn any_mechanism_backs_the_send_channel() {
        let token = SaslConfig::Xoauth2(SaslXoauth2Config {
            username: "user@example.org".into(),
            token: Secret::Raw(String::from("tok").into()),
        });

        let smtp = smtp_config(&endpoint(DiscoverySecurity::Tls, 465), Some(token));
        assert!(matches!(smtp.sasl, Some(SaslConfig::Xoauth2(_))));
    }

    #[cfg(feature = "smtp")]
    #[test]
    fn a_credential_less_send_channel_omits_the_sasl_table() {
        let relay = smtp_config(&endpoint(DiscoverySecurity::Starttls, 587), None);
        assert_eq!(relay.server, "smtp://imap.example.org:587");
        assert!(relay.starttls);
        assert!(relay.sasl.is_none());
        assert!(relay.alpn.is_none());
    }
}
