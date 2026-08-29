//! # Service discovery
//!
//! Email-driven discovery for the wizard. The address feeds
//! io-pim-discovery's parallel search (fixed provider rules, PACC, Mozilla
//! autoconfig, RFC 6186 SRV, with a final WWW-Authenticate probe refining the
//! advertised schemes).
//!
//! Every reachable service becomes one selectable entry carrying the
//! authentication capabilities it advertised, and a detected Microsoft
//! account also offers the Graph API.
//!
//! Only services neverest has a backend for are proposed, so the wizard never
//! writes a source [`crate::client::open`] would refuse.

#![cfg_attr(not(feature = "imap"), allow(dead_code, unused_imports))]

use std::{collections::BTreeSet, env, fmt, time::Duration};

use anyhow::Result;
use io_pim_discovery::{
    compose::{
        client::DiscoveryComposeClientStd,
        config::{
            DiscoveryAuthMethod, DiscoveryConfigSource, DiscoveryEndpoint, DiscoverySecurity,
            DiscoveryService, DiscoveryServiceConfig,
        },
        providers::DiscoveryKnownProvider,
    },
    shared::dns::system_resolver,
};
use pimalaya_stream::tls::{Rustls, Tls};
use url::Url;

/// DNS-over-TCP resolver backing discovery when `NEVEREST_DNS_RESOLVER` is
/// unset and no system resolver is found: Cloudflare's `1.1.1.1`.
const DEFAULT_RESOLVER: &str = "tcp://1.1.1.1:53";

/// Upper bound on the parallel discovery fan-out.
///
/// An unreachable endpoint (a firewalled port, a black-hole host) must not
/// stall the interactive wizard, so mechanisms that have not reported by then
/// are abandoned and only what completed in time is offered.
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(8);

/// One selectable service to reach the account, with the authentication
/// capabilities it advertised.
///
/// The concrete method (SASL mechanism, HTTP scheme) is picked in a second
/// prompt once the service is chosen, so a service appears exactly once.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Discovered {
    /// The service this entry configures.
    pub kind: DiscoveredKind,
    /// Login hint advertised by the mechanism (usually the email).
    pub username: Option<String>,
    /// What the service accepts, folded across its discovered methods.
    pub auth: AuthCaps,
}

/// The discovered service kind, carrying its endpoints for IMAP + SMTP
/// (the Graph API has a fixed one).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiscoveredKind {
    /// An IMAP endpoint for the sync side, paired with the SMTP endpoint
    /// backing the account's send channel when one was discovered.
    ImapSmtp {
        imap: TcpEndpoint,
        smtp: Option<TcpEndpoint>,
    },
    /// The Microsoft Graph API (Microsoft accounts only).
    Msgraph,
    /// A CardDAV endpoint (RFC 6352), discovered through RFC 6764: the
    /// contacts kind.
    Carddav { url: String },
    /// A CalDAV endpoint (RFC 4791), discovered the same way: the calendar
    /// kind.
    Caldav { url: String },
}

/// A discovered TCP service endpoint (IMAP or SMTP).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TcpEndpoint {
    pub host: String,
    pub port: u16,
    pub security: DiscoverySecurity,
}

/// The authentication capabilities a service advertised, folded across all
/// its discovered methods.
///
/// It drives the per-service auth prompt. Neverest reads a token an external
/// manager issues but never runs a grant, so OAuth is no method of its own
/// here: it only unlocks the brokers behind the API token flow.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AuthCaps {
    /// Basic/password auth: SASL PLAIN/LOGIN/SCRAM. Often an app
    /// password (e.g. Fastmail, Gmail).
    pub basic: bool,
    /// A static bearer/API token: SASL OAUTHBEARER/XOAUTH2.
    pub bearer: bool,
    /// An OAuth 2.0 grant is advertised, so a broker can issue the token.
    pub oauth: bool,
}

impl AuthCaps {
    /// Whether any capability was advertised. When none was, the auth prompt
    /// offers every method so the user is never left without a choice.
    pub fn any(self) -> bool {
        self.basic || self.bearer || self.oauth
    }

    /// Whether a token (static or broker-issued) is on offer.
    pub fn token(self) -> bool {
        self.bearer || self.oauth
    }
}

impl fmt::Display for Discovered {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            DiscoveredKind::ImapSmtp { imap, .. } => write!(f, "IMAP + SMTP {}", imap.host),
            DiscoveredKind::Msgraph => write!(f, "Microsoft Graph API"),
            DiscoveredKind::Carddav { url } => write!(f, "CardDAV (contacts) {url}"),
            DiscoveredKind::Caldav { url } => write!(f, "CalDAV (calendar) {url}"),
        }
    }
}

impl Discovered {
    /// Best default login for the credential prompt: the advertised username
    /// when it looks like an address, else the searched email when the user
    /// typed a full one, else nothing.
    pub fn login_default(&self, email: &str) -> Option<String> {
        self.username
            .clone()
            .filter(|username| looks_like_address(username))
            .or_else(|| looks_like_address(email).then(|| email.to_string()))
    }

    /// Ranks an entry for the selection list: the open protocols first,
    /// then the proprietary API.
    fn rank(&self) -> u8 {
        match self.kind {
            DiscoveredKind::ImapSmtp { .. } => 0,
            DiscoveredKind::Msgraph => 1,
            DiscoveredKind::Carddav { .. } => 2,
            DiscoveredKind::Caldav { .. } => 3,
        }
    }
}

/// Searches every mail service reachable from `email` and returns one
/// selectable entry per service, ordered by [`Discovered::rank`].
pub fn search(email: &str) -> Result<Vec<Discovered>> {
    let client = DiscoveryComposeClientStd::new(discovery_resolver(), discovery_tls());
    let services = BTreeSet::from([
        DiscoveryService::Imap,
        DiscoveryService::Smtp,
        DiscoveryService::Carddav,
        DiscoveryService::Caldav,
    ]);
    let configs = client.compose_all_within(email, services, DISCOVERY_TIMEOUT)?;

    let provider = provider_of(email, &configs);
    let mut found = Vec::new();

    if let Some(imap) = best(&configs, DiscoveryService::Imap, provider)
        && let Some(endpoint) = tcp_endpoint(imap)
    {
        let smtp = best(&configs, DiscoveryService::Smtp, provider);
        let mut auth = caps_of(&imap.auth);
        if let Some(smtp) = smtp {
            let smtp_auth = caps_of(&smtp.auth);
            auth.basic |= smtp_auth.basic;
            auth.bearer |= smtp_auth.bearer;
            auth.oauth |= smtp_auth.oauth;
        }
        found.push(Discovered {
            kind: DiscoveredKind::ImapSmtp {
                imap: endpoint,
                smtp: smtp.and_then(tcp_endpoint),
            },
            username: imap.username.clone(),
            auth,
        });
    }

    if let Some(DiscoveryKnownProvider::Microsoft) = provider {
        found.push(Discovered {
            kind: DiscoveredKind::Msgraph,
            username: Some(email.to_string()),
            auth: AuthCaps {
                oauth: true,
                ..Default::default()
            },
        });
    }

    if let Some(carddav) = best(&configs, DiscoveryService::Carddav, provider)
        && let Some(url) = http_endpoint(carddav)
    {
        found.push(Discovered {
            kind: DiscoveredKind::Carddav { url },
            username: carddav.username.clone(),
            auth: caps_of(&carddav.auth),
        });
    }

    if let Some(caldav) = best(&configs, DiscoveryService::Caldav, provider)
        && let Some(url) = http_endpoint(caldav)
    {
        found.push(Discovered {
            kind: DiscoveredKind::Caldav { url },
            username: caldav.username.clone(),
            auth: caps_of(&caldav.auth),
        });
    }

    found.sort_by_key(Discovered::rank);
    Ok(found)
}

/// Drops the discovered entries whose backend is not compiled in. The IMAP +
/// SMTP entry only needs `imap`: a build without `smtp` still syncs, it just
/// gets no send channel.
pub fn retain_supported(found: &mut Vec<Discovered>) {
    found.retain(|entry| match entry.kind {
        DiscoveredKind::ImapSmtp { .. } => cfg!(feature = "imap"),
        DiscoveredKind::Msgraph => cfg!(feature = "msgraph"),
        DiscoveredKind::Carddav { .. } | DiscoveredKind::Caldav { .. } => cfg!(feature = "dav"),
    });
}

/// Resolves the provider from the email domain, falling back to any
/// provider-tagged config, which catches custom domains detected by MX.
fn provider_of(email: &str, configs: &[DiscoveryServiceConfig]) -> Option<DiscoveryKnownProvider> {
    let by_domain = email
        .rsplit_once('@')
        .and_then(|(_, domain)| DiscoveryKnownProvider::from_domain(domain));

    by_domain.or_else(|| {
        configs.iter().find_map(|config| match config.source {
            DiscoveryConfigSource::Provider(provider) => Some(provider),
            _ => None,
        })
    })
}

/// Folds a service's advertised methods into its [`AuthCaps`]: password into
/// `basic`, bearer into `bearer`, every OAuth grant into `oauth`.
fn caps_of(auth: &[DiscoveryAuthMethod]) -> AuthCaps {
    let mut caps = AuthCaps::default();

    for method in auth {
        match method {
            DiscoveryAuthMethod::Password => caps.basic = true,
            DiscoveryAuthMethod::Bearer => caps.bearer = true,
            _ => caps.oauth = true,
        }
    }

    caps
}

/// Picks the best config for a TCP service, restricted to the detected
/// provider's own configs when there is one: the most secure endpoint wins,
/// so a domain advertising both implicit TLS and STARTTLS keeps the former.
fn best(
    configs: &[DiscoveryServiceConfig],
    service: DiscoveryService,
    provider: Option<DiscoveryKnownProvider>,
) -> Option<&DiscoveryServiceConfig> {
    configs
        .iter()
        .filter(|config| config.service == service)
        .filter(|config| match provider {
            Some(provider) => config.source == DiscoveryConfigSource::Provider(provider),
            None => true,
        })
        .max_by_key(|config| match &config.endpoint {
            DiscoveryEndpoint::Tcp {
                security: DiscoverySecurity::Tls,
                ..
            } => 2,
            DiscoveryEndpoint::Tcp {
                security: DiscoverySecurity::Starttls,
                ..
            } => 1,
            _ => 0,
        })
}

/// Whether a string is a full `local@domain` address (both parts
/// non-empty), rejecting the bare-domain `@domain` form.
fn looks_like_address(value: &str) -> bool {
    value
        .split_once('@')
        .is_some_and(|(local, domain)| !local.is_empty() && !domain.is_empty())
}

/// The HTTP endpoint of a discovered service, for the DAV kinds (a DAV
/// service is addressed by URL, never by host and port).
fn http_endpoint(config: &DiscoveryServiceConfig) -> Option<String> {
    match &config.endpoint {
        DiscoveryEndpoint::Http(url) => Some(url.to_string()),
        DiscoveryEndpoint::Tcp { .. } => None,
    }
}

/// Extracts a [`TcpEndpoint`] from a config, or `None` for an HTTP one.
fn tcp_endpoint(config: &DiscoveryServiceConfig) -> Option<TcpEndpoint> {
    match &config.endpoint {
        DiscoveryEndpoint::Tcp {
            host,
            port,
            security,
        } => Some(TcpEndpoint {
            host: host.clone(),
            port: *port,
            security: *security,
        }),
        DiscoveryEndpoint::Http(_) => None,
    }
}

/// Resolver used by discovery: the `NEVEREST_DNS_RESOLVER` override, then the
/// system resolver, then the Cloudflare default.
///
/// Preferring the system one avoids leaking the email domain to a third-party
/// resolver; the override works around networks that block the default.
fn discovery_resolver() -> Url {
    if let Ok(resolver) = env::var("NEVEREST_DNS_RESOLVER")
        && let Ok(url) = resolver.parse()
    {
        return url;
    }

    if let Some(url) = system_resolver() {
        return url;
    }

    DEFAULT_RESOLVER
        .parse()
        .expect("DEFAULT_RESOLVER must be a valid URL")
}

/// TLS profile for the HTTPS-bound discovery mechanisms; they only speak
/// HTTP/1.1 to `_well-known` endpoints.
fn discovery_tls() -> Tls {
    Tls {
        rustls: Rustls {
            alpn: vec!["http/1.1".into()],
            ..Default::default()
        },
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_fold_each_method_onto_its_axis() {
        let oauth = DiscoveryAuthMethod::OauthIssuer("https://issuer".into());

        assert_eq!(
            caps_of(&[DiscoveryAuthMethod::Password]),
            AuthCaps {
                basic: true,
                ..Default::default()
            }
        );
        assert_eq!(
            caps_of(&[DiscoveryAuthMethod::Bearer]),
            AuthCaps {
                bearer: true,
                ..Default::default()
            }
        );
        assert_eq!(
            caps_of(std::slice::from_ref(&oauth)),
            AuthCaps {
                oauth: true,
                ..Default::default()
            }
        );

        let fastmail = caps_of(&[DiscoveryAuthMethod::Bearer, oauth]);
        assert_eq!(
            fastmail,
            AuthCaps {
                bearer: true,
                oauth: true,
                ..Default::default()
            }
        );
        assert!(fastmail.token());
        assert!(!fastmail.basic);
    }

    #[test]
    fn caps_report_emptiness_and_token_offer() {
        assert!(!AuthCaps::default().any());
        assert!(!AuthCaps::default().token());

        let basic = AuthCaps {
            basic: true,
            ..Default::default()
        };
        assert!(basic.any());
        assert!(!basic.token());

        let oauth = AuthCaps {
            oauth: true,
            ..Default::default()
        };
        assert!(oauth.token());
    }

    #[test]
    fn login_default_prefers_an_advertised_address() {
        let imap = TcpEndpoint {
            host: "imap.example.org".into(),
            port: 993,
            security: DiscoverySecurity::Tls,
        };
        let entry = |username: Option<&str>| Discovered {
            kind: DiscoveredKind::ImapSmtp {
                imap: imap.clone(),
                smtp: None,
            },
            username: username.map(str::to_string),
            auth: AuthCaps::default(),
        };

        assert_eq!(
            entry(Some("advertised@example.org")).login_default("typed@example.org"),
            Some("advertised@example.org".into())
        );
        assert_eq!(
            entry(Some("@example.org")).login_default("typed@example.org"),
            Some("typed@example.org".into())
        );

        assert_eq!(entry(None).login_default("@example.org"), None);
    }
}
