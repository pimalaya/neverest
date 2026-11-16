//! # Proxy
//!
//! Turns the `proxy` tables into the runtime [`Proxy`] a connection goes
//! through.

use anyhow::{Result, bail};
use pimalaya_config::secret::SecretResolver;
use pimalaya_stream::proxy::{Proxy, ProxyAuth};
use secrecy::SecretString;

use crate::config::proxy::ProxyConfig;

impl ProxyConfig {
    /// Resolves the first configuration of `configs`, the one closest to
    /// the connection first, none reading the environment at connect time.
    pub fn resolve(configs: &[Option<&Self>], resolver: &mut SecretResolver) -> Result<Proxy> {
        let Some(config) = configs.iter().flatten().next() else {
            return Ok(Proxy::System);
        };

        let mut proxy = Proxy::from_url(&config.url)?;

        let auth = match (&config.username, &config.password) {
            (None, None) => return Ok(proxy),
            (None, Some(_)) => bail!("Proxy password requires a username"),
            (Some(user), pass) => ProxyAuth {
                user: user.clone(),
                pass: match pass {
                    Some(pass) => resolver.resolve(pass.clone())?,
                    None => SecretString::default(),
                },
            },
        };

        match &mut proxy {
            Proxy::Socks5 { auth: slot, .. } | Proxy::Http { auth: slot, .. } => {
                *slot = Some(auth);
            }
            Proxy::None | Proxy::System => {}
        }

        Ok(proxy)
    }
}

#[cfg(test)]
mod tests {
    use pimalaya_config::secret::SecretResolver;
    use pimalaya_stream::proxy::Proxy;

    use crate::config::proxy::ProxyConfig;

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
}
