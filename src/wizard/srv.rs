//! RFC 6186 SRV probe (`_imap._tcp` / `_imaps._tcp`) feeding the
//! wizard's discovery chain.

use log::debug;
use pimalaya_cli::{
    spinner::Spinner,
    wizard::imap::{Encryption as ImapEncryption, ImapAuth, ImapSecret, WizardImapConfig},
};
use pimconf::rfc6186::{
    client::DiscoverySrvClientStd,
    types::{SrvReport, SrvService},
};

use crate::wizard::discover::{DiscoveryResult, discovery_resolver};

/// Runs SRV discovery for `domain`.
pub fn run(domain: &str) -> Option<SrvReport> {
    let spinner = Spinner::start(format!("Probing SRV records for {domain}\u{2026}"));
    let mut client = DiscoverySrvClientStd::new(discovery_resolver().ok()?);

    match client.discover(domain) {
        Ok(report) if !is_empty(&report) => {
            spinner.success(summary(domain, &report));
            Some(report)
        }
        Ok(_) => {
            spinner.failure(format!("SRV: no records for {domain}"));
            None
        }
        Err(err) => {
            debug!("srv discovery for {domain} failed: {err}");
            spinner.failure(format!("SRV: no records for {domain}"));
            None
        }
    }
}

/// Extracts the preferred IMAP server (TLS first) from a [`SrvReport`].
pub fn defaults(report: &SrvReport) -> DiscoveryResult {
    let imap = report
        .imaps
        .as_ref()
        .map(|s| imap_from_service(s, ImapEncryption::Tls))
        .or_else(|| {
            report
                .imap
                .as_ref()
                .map(|s| imap_from_service(s, ImapEncryption::StartTls))
        });

    DiscoveryResult { imap, jmap: None }
}

fn summary(domain: &str, report: &SrvReport) -> String {
    if report.imap.is_some() || report.imaps.is_some() {
        format!("SRV: discovered IMAP for {domain}")
    } else {
        format!("SRV: no IMAP record for {domain}")
    }
}

fn is_empty(report: &SrvReport) -> bool {
    report.imap.is_none() && report.imaps.is_none()
}

fn imap_from_service(service: &SrvService, encryption: ImapEncryption) -> WizardImapConfig {
    WizardImapConfig {
        host: service.host.clone(),
        port: service.port,
        encryption,
        login: String::new(),
        auth: ImapAuth::Password(ImapSecret::Raw(String::new().into())),
    }
}
