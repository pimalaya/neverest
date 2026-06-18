//! Mozilla Thunderbird Autoconfiguration probe (ISP main / fallback /
//! ISPDB, IMAP only) feeding the wizard's discovery chain.

use log::debug;
use pimalaya_cli::{
    spinner::Spinner,
    wizard::imap::{Encryption as ImapEncryption, ImapAuth, ImapSecret, WizardImapConfig},
};
use pimconf::autoconfig::{
    client::DiscoveryAutoconfigClientStd,
    types::{Autoconfig, SecurityType, Server, ServerType},
};

use crate::wizard::discover::{DiscoveryResult, discovery_resolver, discovery_tls};

/// Tries ISP main, ISP fallback, then ISPDB in series; first hit wins.
pub fn run(local_part: &str, domain: &str) -> Option<Autoconfig> {
    let mut client =
        DiscoveryAutoconfigClientStd::new(discovery_resolver().ok()?).with_tls(discovery_tls());

    let attempts: [(&str, &dyn Fn(&mut DiscoveryAutoconfigClientStd) -> _); 3] = [
        ("Autoconfig ISP main URL", &|c| {
            c.isp(local_part, domain, true)
        }),
        ("Autoconfig ISP fallback URL", &|c| {
            c.isp_fallback(domain, true)
        }),
        ("Thunderbird ISPDB", &|c| c.ispdb(domain, true)),
    ];

    for (label, run) in attempts {
        let spinner = Spinner::start(format!("Probing {label} for {domain}\u{2026}"));

        match run(&mut client) {
            Ok(config) => {
                spinner.success(summary(domain, &config));
                return Some(config);
            }
            Err(err) => {
                debug!("{label} for {domain} failed: {err}");
                spinner.failure(format!("{label}: not available for {domain}"));
            }
        }
    }

    None
}

/// Extracts the IMAP server (if any) from an [`Autoconfig`] result.
pub fn defaults(ac: &Autoconfig) -> DiscoveryResult {
    let imap = ac
        .email_provider
        .incoming_server
        .iter()
        .find(|s| matches!(s.r#type, ServerType::Imap))
        .and_then(imap_from_server);

    DiscoveryResult { imap, jmap: None }
}

fn summary(domain: &str, ac: &Autoconfig) -> String {
    let has_imap = ac
        .email_provider
        .incoming_server
        .iter()
        .any(|s| matches!(s.r#type, ServerType::Imap));

    if has_imap {
        format!("Autoconfig: discovered IMAP for {domain}")
    } else {
        format!("Autoconfig: configuration found for {domain} (no IMAP fields)")
    }
}

fn imap_from_server(server: &Server) -> Option<WizardImapConfig> {
    let host = server.hostname.clone()?;
    let encryption = match server.socket_type {
        Some(SecurityType::Tls) => ImapEncryption::Tls,
        Some(SecurityType::Starttls) => ImapEncryption::StartTls,
        _ => ImapEncryption::None,
    };
    let port = server.port.unwrap_or(match encryption {
        ImapEncryption::Tls => 993,
        _ => 143,
    });

    Some(WizardImapConfig {
        host,
        port,
        encryption,
        login: String::new(),
        auth: ImapAuth::Password(ImapSecret::Raw(String::new().into())),
    })
}
