//! # TLS
//!
//! Turns the `tls` tables into the runtime [`Tls`] handle the network
//! backends connect with.

use pimalaya_stream::tls::{Rustls, RustlsCrypto, Tls, TlsProvider, TlsServer};

use crate::config::tls::{RustlsCryptoConfig, TlsConfig, TlsProviderConfig};

impl TlsConfig {
    /// Builds the runtime [`Tls`] handle the connect helpers expect.
    ///
    /// `alpn` is the protocol-level list, empty to skip ALPN. The schema
    /// never exposes `tls.rustls.alpn`: the per-protocol `*.alpn` field is
    /// folded in here.
    pub fn into_tls(self, alpn: Vec<String>) -> Tls {
        Tls {
            provider: self.provider.map(|p| match p {
                TlsProviderConfig::Rustls => TlsProvider::Rustls,
                TlsProviderConfig::NativeTls => TlsProvider::NativeTls,
            }),
            rustls: Rustls {
                crypto: self.rustls.crypto.map(|c| match c {
                    RustlsCryptoConfig::Aws => RustlsCrypto::Aws,
                    RustlsCryptoConfig::Ring => RustlsCrypto::Ring,
                }),
                alpn,
            },
            server: self.cert.map(|cert| TlsServer { cert }),
            client: None,
        }
    }
}
