//! PACC probe (IMAP + JMAP blocks) feeding the wizard's discovery
//! chain.

use log::debug;
use pimalaya_cli::{
    spinner::Spinner,
    wizard::{
        imap::{Encryption as ImapEncryption, ImapAuth, ImapSecret, WizardImapConfig},
        jmap::{JmapAuth, JmapSecret, WizardJmapConfig},
    },
};
use pimconf::pacc::{client::DiscoveryPaccClientStd, types::PaccConfig};

use crate::wizard::discover::{DiscoveryResult, discovery_resolver, discovery_tls};

/// Runs PACC discovery against `domain`.
pub fn run(domain: &str) -> Option<PaccConfig> {
    let spinner = Spinner::start(format!("Probing PACC for {domain}\u{2026}"));
    let mut client =
        DiscoveryPaccClientStd::new(discovery_resolver().ok()?).with_tls(discovery_tls());

    match client.discover(domain) {
        Ok(config) => {
            spinner.success(summary(domain, &config));
            Some(config)
        }
        Err(err) => {
            debug!("pacc discovery for {domain} failed: {err}");
            spinner.failure(format!("PACC: no valid configuration for {domain}"));
            None
        }
    }
}

/// Extracts the IMAP / JMAP servers from a [`PaccConfig`].
pub fn defaults(config: &PaccConfig) -> DiscoveryResult {
    let imap = config.protocols.imap.as_ref().map(|p| WizardImapConfig {
        host: p.host.clone(),
        port: 993,
        encryption: ImapEncryption::Tls,
        login: String::new(),
        auth: ImapAuth::Password(ImapSecret::Raw(String::new().into())),
    });

    let jmap = config.protocols.jmap.as_ref().map(|p| WizardJmapConfig {
        server: p.url.clone(),
        auth: JmapAuth::Basic {
            login: String::new(),
            secret: JmapSecret::Raw(String::new().into()),
        },
    });

    DiscoveryResult { imap, jmap }
}

fn summary(domain: &str, config: &PaccConfig) -> String {
    let p = &config.protocols;
    let mut protos = Vec::with_capacity(2);
    if p.jmap.is_some() {
        protos.push("JMAP");
    }
    if p.imap.is_some() {
        protos.push("IMAP");
    }
    if protos.is_empty() {
        format!("PACC: configuration found for {domain} (no IMAP/JMAP fields)")
    } else {
        format!("PACC: discovered {} for {domain}", protos.join(" + "))
    }
}
