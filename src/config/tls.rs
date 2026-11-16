//! # TLS configuration
//!
//! The `tls` table every network backend and send channel holds.

use std::path::PathBuf;

use pimalaya_config::toml::opt_shell_expanded_path;
use serde::{Deserialize, Serialize};

/// How a connection is secured, shared by every backend.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct TlsConfig {
    /// Which TLS provider to use, the build's own default when unset.
    pub provider: Option<TlsProviderConfig>,
    /// Options the rustls provider reads.
    #[serde(default)]
    pub rustls: RustlsConfig,
    /// Path to an extra CA certificate to trust, shell-expanded.
    #[serde(default, deserialize_with = "opt_shell_expanded_path")]
    pub cert: Option<PathBuf>,
}

/// Which TLS implementation carries the connection.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum TlsProviderConfig {
    /// The rustls provider.
    Rustls,
    /// The platform's own TLS stack.
    NativeTls,
}

/// Options the rustls provider reads.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct RustlsConfig {
    /// Which crypto backend rustls uses, its default when unset.
    pub crypto: Option<RustlsCryptoConfig>,
}

/// Which crypto backend rustls uses.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum RustlsCryptoConfig {
    /// The aws-lc-rs backend.
    Aws,
    /// The ring backend.
    Ring,
}
