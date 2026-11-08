//! # Typed server URL
//!
//! A server the user typed, when discovery found nothing or a custom one
//! was asked for. The scheme names the protocol and its transport, a bare
//! host taking the protocol's default port.
//!
//! Encrypted servers only: `http://` is refused rather than sending a
//! password in clear, unless its host is this machine, and `imap://` and
//! `smtp://` mean STARTTLS.

use std::{error, fmt, net::IpAddr};

use percent_encoding::percent_decode_str;
use url::Url;

use crate::wizard::plan::{Domain, Endpoint, Route, SignIn};

/// A typed server, before its sign-in method is picked.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypedServer {
    /// The domain the server sets up.
    pub domain: Domain,
    /// The server.
    pub endpoint: Endpoint,
    /// The user name the URL carried.
    pub login: Option<String>,
}

/// Why a typed server URL is refused.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ServerUrlError {
    /// No `scheme://` prefix.
    MissingScheme,
    /// A cleartext URL, as typed.
    Cleartext(String),
    /// A scheme the domain does not take, or no domain at all when none
    /// was named.
    Unsupported {
        scheme: String,
        domain: Option<Domain>,
    },
    /// A password written in the URL.
    Password,
    /// No host after the scheme.
    MissingHost,
    /// Not a URL at all.
    Invalid(String),
}

impl TypedServer {
    /// Reads a server URL typed for `domain`.
    pub fn parse(input: &str, domain: Domain) -> Result<Self, ServerUrlError> {
        let input = input.trim();

        let Some((scheme, _)) = input.split_once("://") else {
            return Err(ServerUrlError::MissingScheme);
        };

        let scheme = scheme.to_ascii_lowercase();
        let url = Url::parse(input).map_err(|err| ServerUrlError::Invalid(err.to_string()))?;

        if url.password().is_some() {
            return Err(ServerUrlError::Password);
        }

        let host = match url.host_str() {
            Some(host) if !host.is_empty() => host.trim_end_matches('.').to_lowercase(),
            _ => return Err(ServerUrlError::MissingHost),
        };

        let local = is_loopback(&host);

        let transport = match (scheme.as_str(), domain) {
            ("http", _) if !local => return Err(ServerUrlError::Cleartext(input.to_string())),
            ("http", Domain::Contacts | Domain::Calendar) => Transport::Web("http"),
            ("imaps", Domain::Mail) => Transport::Tcp {
                tls: true,
                port: 993,
            },
            ("imap", Domain::Mail) => Transport::Tcp {
                tls: false,
                port: 143,
            },
            ("smtps", Domain::Sending) => Transport::Tcp {
                tls: true,
                port: 465,
            },
            ("smtp", Domain::Sending) => Transport::Tcp {
                tls: false,
                port: 587,
            },
            ("https" | "carddavs" | "carddav", Domain::Contacts)
            | ("https" | "caldavs" | "caldav", Domain::Calendar) => Transport::Web("https"),
            _ => {
                return Err(ServerUrlError::Unsupported {
                    scheme,
                    domain: Some(domain),
                });
            }
        };

        let login = Some(
            percent_decode_str(url.username())
                .decode_utf8_lossy()
                .into_owned(),
        )
        .filter(|login| !login.is_empty());

        let endpoint = match transport {
            Transport::Tcp { tls, port } => Endpoint::Tcp {
                host,
                port: url.port().unwrap_or(port),
                tls,
            },
            Transport::Web(web) => {
                let port = url
                    .port()
                    .map(|port| format!(":{port}"))
                    .unwrap_or_default();
                let web = format!("{web}://{host}{port}{}", url.path());
                let web =
                    Url::parse(&web).map_err(|err| ServerUrlError::Invalid(err.to_string()))?;
                Endpoint::Http(web.to_string())
            }
        };

        Ok(Self {
            domain,
            endpoint,
            login,
        })
    }

    /// Reads a server URL typed when discovery found nothing, its scheme
    /// deciding the domains it sets up: an https URL is a DAV entry point
    /// for contacts and calendar alike. Sending needs mail, so it is no
    /// answer there.
    pub fn parse_any(input: &str) -> Result<Vec<Self>, ServerUrlError> {
        let input = input.trim();

        let Some((scheme, _)) = input.split_once("://") else {
            return Err(ServerUrlError::MissingScheme);
        };

        let scheme = scheme.to_ascii_lowercase();

        let domains: &[Domain] = match scheme.as_str() {
            "imaps" | "imap" => &[Domain::Mail],
            "carddavs" | "carddav" => &[Domain::Contacts],
            "caldavs" | "caldav" => &[Domain::Calendar],
            "https" | "http" => &[Domain::Contacts, Domain::Calendar],
            _ => {
                return Err(ServerUrlError::Unsupported {
                    scheme,
                    domain: None,
                });
            }
        };

        domains
            .iter()
            .map(|domain| Self::parse(input, *domain))
            .collect()
    }

    /// The route signing in to the server with `sign_in`.
    pub fn route(self, sign_in: SignIn) -> Route {
        Route {
            protocol: self.domain.protocol(),
            endpoint: self.endpoint,
            sign_in,
            login: self.login,
            authority: None,
        }
    }
}

impl fmt::Display for ServerUrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingScheme => write!(
                f,
                "Missing scheme, as in imaps://imap.example.org or https://dav.example.org"
            ),
            Self::Cleartext(url) => {
                write!(f, "Refused {url}: only encrypted servers are accepted")
            }
            Self::Unsupported { scheme, domain } => {
                let schemes = match domain {
                    Some(Domain::Mail) => "imaps:// or imap://",
                    Some(Domain::Sending) => "smtps:// or smtp://",
                    Some(Domain::Contacts) => "https://, carddavs:// or carddav://",
                    Some(Domain::Calendar) => "https://, caldavs:// or caldav://",
                    None => "imaps://, imap://, https://, carddavs:// or caldavs://",
                };

                match domain {
                    Some(domain) => write!(
                        f,
                        "Unsupported scheme `{scheme}` for {}: use {schemes}",
                        domain.name().to_lowercase()
                    ),
                    None => write!(f, "Unsupported scheme `{scheme}`: use {schemes}"),
                }
            }
            Self::Password => write!(f, "Type the password when asked rather than in the URL"),
            Self::MissingHost => write!(f, "Missing host in the server URL"),
            Self::Invalid(err) => write!(f, "Invalid server URL: {err}"),
        }
    }
}

impl error::Error for ServerUrlError {}

/// How a scheme reaches its server.
enum Transport {
    /// TCP, over implicit TLS or STARTTLS, on this port by default.
    Tcp { tls: bool, port: u16 },
    /// HTTP, under this scheme.
    Web(&'static str),
}

/// Whether `host` is this machine: `localhost` or a loopback address.
fn is_loopback(host: &str) -> bool {
    let bare = host.trim_start_matches('[').trim_end_matches(']');

    bare == "localhost" || bare.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

#[cfg(test)]
mod tests {
    use crate::wizard::{
        plan::{Domain, Endpoint, Protocol, SignIn},
        server::*,
    };

    fn tcp(input: &str, domain: Domain) -> (String, u16, bool) {
        let server = TypedServer::parse(input, domain).expect("a valid server URL");

        match server.endpoint {
            Endpoint::Tcp { host, port, tls } => (host, port, tls),
            Endpoint::Http(url) => panic!("expected a TCP server, got {url}"),
        }
    }

    fn http(input: &str, domain: Domain) -> String {
        match TypedServer::parse(input, domain).map(|server| server.endpoint) {
            Ok(Endpoint::Http(url)) => url,
            other => panic!("expected an HTTP URL, got {other:?}"),
        }
    }

    #[test]
    fn a_mail_or_sending_scheme_names_its_transport_and_default_port() {
        assert_eq!(
            tcp("imaps://mail.example.org", Domain::Mail),
            (String::from("mail.example.org"), 993, true)
        );
        assert_eq!(
            tcp("imap://Mail.Example.org", Domain::Mail),
            (String::from("mail.example.org"), 143, false)
        );
        assert_eq!(
            tcp("smtps://mail.example.org", Domain::Sending),
            (String::from("mail.example.org"), 465, true)
        );
        assert_eq!(
            tcp("smtp://mail.example.org", Domain::Sending),
            (String::from("mail.example.org"), 587, false)
        );
        assert_eq!(
            tcp("IMAPS://mail.example.org:1993", Domain::Mail),
            (String::from("mail.example.org"), 1993, true)
        );

        let route = TypedServer::parse("smtp://mail.example.org", Domain::Sending)
            .expect("a valid server URL")
            .route(SignIn::Password);
        assert_eq!(route.protocol, Protocol::Smtp);
    }

    #[test]
    fn dav_schemes_read_as_https() {
        assert_eq!(
            http("https://dav.example.org", Domain::Contacts),
            "https://dav.example.org/"
        );
        assert_eq!(
            http("carddav://dav.example.org/card", Domain::Contacts),
            "https://dav.example.org/card"
        );
        assert_eq!(
            http("carddavs://dav.example.org:8443/", Domain::Contacts),
            "https://dav.example.org:8443/"
        );
        assert_eq!(
            http("caldav://dav.example.org", Domain::Calendar),
            "https://dav.example.org/"
        );
        assert_eq!(
            http("caldavs://dav.example.org/cal/", Domain::Calendar),
            "https://dav.example.org/cal/"
        );

        let route = TypedServer::parse("https://dav.example.org", Domain::Calendar)
            .expect("a valid server URL")
            .route(SignIn::Token);
        assert_eq!(route.protocol, Protocol::Caldav);
    }

    #[test]
    fn a_cleartext_server_is_refused_by_name() {
        let err = TypedServer::parse("http://dav.example.org", Domain::Contacts).unwrap_err();

        assert_eq!(
            err,
            ServerUrlError::Cleartext(String::from("http://dav.example.org"))
        );
        assert_eq!(
            err.to_string(),
            "Refused http://dav.example.org: only encrypted servers are accepted"
        );
        assert_eq!(
            TypedServer::parse("http://imap.example.org", Domain::Mail),
            Err(ServerUrlError::Cleartext(String::from(
                "http://imap.example.org"
            )))
        );
        assert_eq!(
            TypedServer::parse("http://localhost.example.org", Domain::Calendar),
            Err(ServerUrlError::Cleartext(String::from(
                "http://localhost.example.org"
            )))
        );
    }

    #[test]
    fn a_cleartext_server_on_this_machine_is_taken() {
        assert_eq!(
            http("http://localhost:5232/", Domain::Calendar),
            "http://localhost:5232/"
        );
        assert_eq!(
            http("http://127.0.0.2", Domain::Contacts),
            "http://127.0.0.2/"
        );
        assert_eq!(
            http("http://[::1]:5232", Domain::Contacts),
            "http://[::1]:5232/"
        );

        // A loopback host does not make HTTP a mail protocol.
        assert_eq!(
            TypedServer::parse("http://localhost", Domain::Mail),
            Err(ServerUrlError::Unsupported {
                scheme: String::from("http"),
                domain: Some(Domain::Mail),
            })
        );
    }

    #[test]
    fn a_domain_takes_its_own_schemes_only() {
        let unsupported = |input, domain| {
            TypedServer::parse(input, domain)
                == Err(ServerUrlError::Unsupported {
                    scheme: input.split_once("://").unwrap().0.to_string(),
                    domain: Some(domain),
                })
        };

        assert!(unsupported("smtps://smtp.example.org", Domain::Mail));
        assert!(unsupported("https://jmap.example.org", Domain::Mail));
        assert!(unsupported("imaps://imap.example.org", Domain::Sending));
        assert!(unsupported("caldavs://dav.example.org", Domain::Contacts));
        assert!(unsupported("carddav://dav.example.org", Domain::Calendar));
        assert!(unsupported("pop3s://pop.example.org", Domain::Mail));
    }

    #[test]
    fn a_malformed_url_is_refused() {
        assert_eq!(
            TypedServer::parse("mail.example.org", Domain::Mail),
            Err(ServerUrlError::MissingScheme)
        );
        assert_eq!(
            TypedServer::parse("imaps://", Domain::Mail),
            Err(ServerUrlError::MissingHost)
        );
        assert_eq!(
            TypedServer::parse("https://jane:secret@dav.example.org", Domain::Contacts),
            Err(ServerUrlError::Password)
        );
    }

    #[test]
    fn a_user_name_is_the_login() {
        let server =
            TypedServer::parse("imaps://jane%40example.org@imap.example.org", Domain::Mail)
                .expect("a valid server URL");

        assert_eq!(server.login.as_deref(), Some("jane@example.org"));
        assert_eq!(
            TypedServer::parse("https://dav.example.org", Domain::Contacts)
                .map(|server| server.login),
            Ok(None)
        );
    }

    /// With nothing discovered, the scheme decides the domain, an https
    /// URL being a DAV entry point for both contacts and calendar, and
    /// sending, which needs mail, no answer at all.
    #[test]
    fn a_first_server_url_names_its_domains_by_its_scheme() {
        let domains = |input| -> Result<Vec<Domain>, ServerUrlError> {
            TypedServer::parse_any(input)
                .map(|servers| servers.into_iter().map(|server| server.domain).collect())
        };

        assert_eq!(domains("imaps://imap.example.org"), Ok(vec![Domain::Mail]));
        assert_eq!(
            domains("carddavs://dav.example.org"),
            Ok(vec![Domain::Contacts])
        );
        assert_eq!(
            domains("caldav://dav.example.org"),
            Ok(vec![Domain::Calendar])
        );
        assert_eq!(
            domains("https://dav.example.org"),
            Ok(vec![Domain::Contacts, Domain::Calendar])
        );
        assert_eq!(
            domains("http://dav.example.org"),
            Err(ServerUrlError::Cleartext(String::from(
                "http://dav.example.org"
            )))
        );
        assert_eq!(
            domains("smtps://smtp.example.org"),
            Err(ServerUrlError::Unsupported {
                scheme: String::from("smtps"),
                domain: None,
            })
        );
    }
}
