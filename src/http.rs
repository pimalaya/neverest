//! # HTTP streams
//!
//! The TLS stream a REST backend (Gmail, Google Calendar, Google People,
//! Microsoft Graph) runs its requests over, opened to the host of its API
//! base URL through the configured proxy.

use std::time::Duration;

use anyhow::{Context, Result};
use pimalaya_stream::{
    proxy::Proxy,
    stream::{Stream, TlsConnectOptions},
    tls::Tls,
};
use url::Url;

/// How long a read waits for the server before the exchange fails.
pub const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Opens a TLS stream to the host of `url`, through `proxy`.
pub fn open_tls(url: &str, tls: &Tls, proxy: &Proxy) -> Result<Stream> {
    let url = Url::parse(url).with_context(|| format!("Cannot parse {url}"))?;
    let host = url
        .host_str()
        .with_context(|| format!("{url} has no host"))?;
    let opts = TlsConnectOptions {
        tls: tls.clone(),
        proxy: proxy.clone(),
        ..Default::default()
    };
    let stream = Stream::connect_tls(host, url.port().unwrap_or(443), opts)?;
    stream.set_read_timeout(Some(READ_TIMEOUT))?;
    Ok(stream)
}
