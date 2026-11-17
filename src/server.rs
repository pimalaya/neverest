//! # Server
//!
//! Resolves the `server` key of a backend table into the URL its client
//! connects to.

use anyhow::{Context, Result, bail};
use url::Url;

/// Resolves a `server` into a URL, `scheme` filling in for a value with none.
///
/// The presence of `://` tells them apart, and it has to: a bare authority
/// is not a relative URL, `url` reading `dav.example.org:8443` as a scheme
/// with a path, which parses cleanly and carries no host.
pub fn server_url(server: &str, scheme: &str) -> Result<Url> {
    ascii_host(server)?;
    let url = if server.contains("://") {
        Url::parse(server)
    } else {
        Url::parse(&format!("{scheme}://{server}"))
    };

    url.with_context(|| format!("Cannot parse {server} as a server URL"))
}

/// Refuses a URL or an authority whose host is not ASCII, naming the
/// Punycode form the family takes instead.
///
/// No neverest build carries a Unicode IDNA back end, as no family binary
/// does: `bücher.de` is written `xn--bcher-kva.de`.
pub fn ascii_host(url: &str) -> Result<()> {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    if !host.is_ascii() {
        bail!(
            "Host `{host}` is not ASCII: write it in its Punycode form (`xn--...`), \
             as `idn2` or any IDNA converter prints it"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::server::server_url;

    /// A host that is not ASCII is refused naming its Punycode form, on every
    /// scheme: url keeps an `imaps` host opaque, which then failed at DNS.
    #[test]
    fn a_host_that_is_not_ascii_asks_for_its_punycode_form() {
        for server in [
            "bücher.de",
            "imaps://bücher.de:993",
            "https://u@bücher.de/dav/",
        ] {
            let err = server_url(server, "https").unwrap_err().to_string();
            assert!(err.contains("Punycode"), "{server}: {err}");
        }
        let url = server_url("xn--bcher-kva.de", "https").unwrap();
        assert_eq!(url.host_str(), Some("xn--bcher-kva.de"));
    }

    /// A bare authority with a port used to reach a backend as a hostless
    /// URL, `url` reading `dav.example.org:8443` as a scheme and a path.
    #[test]
    fn a_server_resolves_from_an_authority_with_or_without_a_port() {
        for (server, scheme, host, port) in [
            (
                "dav.example.org:8443",
                "https",
                "dav.example.org",
                Some(8443),
            ),
            ("dav.example.org", "https", "dav.example.org", None),
            (
                "imap.example.org:143",
                "imaps",
                "imap.example.org",
                Some(143),
            ),
            ("smtp.example.org", "smtps", "smtp.example.org", None),
        ] {
            let url = server_url(server, scheme).unwrap();
            assert_eq!(url.scheme(), scheme, "{server}");
            assert_eq!(url.host_str(), Some(host), "{server}");
            assert_eq!(url.port(), port, "{server}");
        }
    }

    /// A value carrying a scheme is used verbatim, so an explicit
    /// cleartext or non-default port survives the resolution.
    #[test]
    fn a_server_carrying_a_scheme_is_left_alone() {
        let url = server_url("http://127.0.0.1:5232/dav/", "https").unwrap();
        assert_eq!(url.scheme(), "http");
        assert_eq!(url.port(), Some(5232));
        assert_eq!(url.path(), "/dav/");

        let url = server_url("imap://example.org:143", "imaps").unwrap();
        assert_eq!(url.scheme(), "imap");
    }
}
