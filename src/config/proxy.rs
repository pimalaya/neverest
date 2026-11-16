//! # Proxy configuration
//!
//! The `proxy` table of an account, a backend or a send channel.

use pimalaya_config::secret::Secret;
use serde::{Deserialize, Serialize};

/// The proxy a connection goes through.
///
/// `url` is a `socks5://`, `socks5h://` or `http://` proxy URL. Its user
/// info authenticates too, but `username` and `password` keep the secret
/// out of the URL.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ProxyConfig {
    /// The proxy URL.
    pub url: String,
    /// The proxy username, required by `password`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// The proxy password.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<Secret>,
}

#[cfg(test)]
mod tests {
    use crate::config::AccountConfig;

    /// A proxy table is spelled as the siblings spell it: a typo is refused.
    #[test]
    fn a_proxy_table_refuses_an_unknown_key() {
        let err = toml::from_str::<AccountConfig>("proxy.uri = \"http://p:1\"").unwrap_err();
        assert!(err.to_string().contains("uri"), "{err}");
    }
}
