//! Mozilla Thunderbird Autoconfiguration step of the wizard's
//! discovery chain. Tries ISP main, ISP fallback, and Thunderbird
//! ISPDB in series (secure variants only); each probe owns its own
//! spinner. neverest only cares about IMAP servers.

use io_discovery::autoconfig::{
    client::DiscoveryAutoconfigClientStd,
    types::{Autoconfig, SecurityType, Server, ServerType},
};
use log::debug;
use pimalaya_cli::{
    spinner::Spinner,
    wizard::imap::{Encryption as ImapEncryption, ImapAuth, ImapSecret, WizardImapConfig},
};

use crate::wizard::discover::{DiscoveryResult, discovery_resolver, discovery_tls};

pub fn run(local_part: &str, domain: &str) -> Option<Autoconfig> {
    let mut client =
        DiscoveryAutoconfigClientStd::new(discovery_resolver()).with_tls(discovery_tls());

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
