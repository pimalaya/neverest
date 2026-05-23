//! RFC 6186 SRV step of the wizard's discovery chain. neverest only
//! consumes the IMAP records (`_imap._tcp` / `_imaps._tcp`); the
//! `_submission._tcp` SRV is ignored because sync does not send.

use io_discovery::rfc6186::{
    client::DiscoverySrvClientStd,
    types::{SrvReport, SrvService},
};
use log::debug;
use pimalaya_cli::{
    spinner::Spinner,
    wizard::imap::{Encryption as ImapEncryption, ImapAuth, ImapSecret, WizardImapConfig},
};

use crate::wizard::discover::{DiscoveryResult, discovery_resolver};

pub fn run(domain: &str) -> Option<SrvReport> {
    let spinner = Spinner::start(format!("Probing SRV records for {domain}\u{2026}"));
    let mut client = DiscoverySrvClientStd::new(discovery_resolver());

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
            debug!("SRV discovery for {domain} failed: {err}");
            spinner.failure(format!("SRV: no records for {domain}"));
            None
        }
    }
}

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
