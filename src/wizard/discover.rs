//! Interactive configuration wizard with discovery-based defaults.
//!
//! Triggered by `cli::load_or_wizard` when no config file is found
//! ([`pimalaya_config::toml::TomlConfig::from_paths_or_default`]
//! returned `Ok(None)`).
//!
//! Flow:
//!
//! 1. Confirm with the user. Exit if they decline.
//! 2. Ask for an account name and email address.
//! 3. Try PACC, then Autoconfig (ISP main / fallback / ISPDB, secure
//!    variants only), then RFC 6186 SRV; each probe owns its own
//!    spinner and first hit wins.
//! 4. If PACC returned a JMAP endpoint, ask the user whether to use
//!    it instead of IMAP and run the matching protocol wizard.
//! 5. Ask for the local m2dir store root; the result is the `left`
//!    side and the remote backend is the `right` side.
//! 6. Build a [`Config`], write it to `target`, return it.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::exit,
};

use anyhow::{Result, anyhow};
use log::info;
use pimalaya_cli::{
    prompt,
    wizard::{
        imap::{self as imap_wizard, WizardImapConfig},
        jmap::{self as jmap_wizard, WizardJmapConfig},
    },
};
use pimalaya_stream::tls::Tls;
use url::Url;

use crate::{
    config::{
        AccountConfig, Config, FlagSidePermissions, M2dirConfig, MailboxSidePermissions,
        MessageSidePermissions, SideConfig,
    },
    wizard::{
        account::{imap_to_config, jmap_to_config},
        autoconfig, pacc, srv,
    },
};

/// DNS resolver used by PACC, Autoconfig, and SRV discovery.
/// Cloudflare's `1.1.1.1` is a reasonable default; we'll make this
/// configurable later.
const DEFAULT_RESOLVER: &str = "tcp://1.1.1.1:53";

/// Parses [`DEFAULT_RESOLVER`] into a [`Url`]. The const is fixed at
/// build time, so a parse failure is a static bug.
pub fn discovery_resolver() -> Url {
    DEFAULT_RESOLVER
        .parse()
        .expect("DEFAULT_RESOLVER must be a valid URL")
}

/// Builds the [`Tls`] profile passed to the per-mechanism discovery
/// clients via `with_tls`. Discovery only speaks HTTPS to `_well-known`
/// endpoints, so `http/1.1` is the only ALPN protocol we offer.
pub fn discovery_tls() -> Tls {
    let mut tls = Tls::default();
    tls.rustls.alpn = vec!["http/1.1".into()];
    tls
}

#[derive(Default)]
pub struct DiscoveryResult {
    pub jmap: Option<WizardJmapConfig>,
    pub imap: Option<WizardImapConfig>,
}

impl DiscoveryResult {
    pub fn is_empty(&self) -> bool {
        self.imap.is_none() && self.jmap.is_none()
    }
}

pub fn run(target: &Path) -> Result<Config> {
    let prompt = format!(
        "No configuration found, create one at {}?",
        target.display(),
    );

    if !prompt::bool(&prompt, true)? {
        exit(0);
    }

    let account_name = prompt::text("Account name:", Some("default"))?;
    let email = prompt::text::<&str>("Email address:", None)?;

    let (local_part, domain) = email
        .rsplit_once('@')
        .ok_or_else(|| anyhow!("Invalid email address `{email}`: missing `@`"))?;

    let discovery = discover(local_part, domain);
    let account = build_account_from_discovery(&account_name, local_part, domain, discovery)?;

    let config = Config {
        accounts: HashMap::from([(account_name, account)]),
    };

    config.write(target)?;
    info!("configuration written to {}", target.display());

    Ok(config)
}

/// Runs PACC, then Autoconfig, then SRV in series; first non-empty
/// `DiscoveryResult` wins. Each mechanism reports its own spinner
/// line. Returns an empty `DiscoveryResult` when every mechanism
/// failed; the caller falls back to pure manual entry in that case.
fn discover(local_part: &str, domain: &str) -> DiscoveryResult {
    if let Some(result) = pacc::run(domain)
        .map(|c| pacc::defaults(&c))
        .filter(|r| !r.is_empty())
    {
        return result;
    }

    if let Some(result) = autoconfig::run(local_part, domain)
        .map(|c| autoconfig::defaults(&c))
        .filter(|r| !r.is_empty())
    {
        return result;
    }

    if let Some(result) = srv::run(domain)
        .map(|r| srv::defaults(&r))
        .filter(|r| !r.is_empty())
    {
        return result;
    }

    DiscoveryResult::default()
}

/// Picks the remote backend (JMAP when offered and accepted, else
/// IMAP), runs its credential wizard, then prompts for the local
/// m2dir root and assembles the [`AccountConfig`] with the local side
/// as `left` and the remote side as `right`.
fn build_account_from_discovery(
    account_name: &str,
    local_part: &str,
    domain: &str,
    discovery: DiscoveryResult,
) -> Result<AccountConfig> {
    let DiscoveryResult { imap, jmap } = discovery;

    let prefer_jmap = match (&jmap, imap.is_some()) {
        (Some(_), true) => prompt::bool(
            "A JMAP server was discovered, use it instead of IMAP?",
            true,
        )?,
        (Some(_), false) => true,
        (None, _) => false,
    };

    let right = if prefer_jmap {
        let jmap = jmap_wizard::run(account_name, local_part, domain, jmap.as_ref())?;
        SideConfig::Jmap(jmap_to_config(jmap)?)
    } else {
        let imap = imap_wizard::run(account_name, local_part, domain, imap.as_ref())?;
        SideConfig::Imap(imap_to_config(imap)?)
    };

    let default_root = format!("~/Mail/{account_name}");
    let root = prompt::text("Local m2dir store root:", Some(default_root.as_str()))?;
    let left = SideConfig::M2dir(M2dirConfig {
        root: PathBuf::from(root),
        mailbox: MailboxSidePermissions::default(),
        flag: FlagSidePermissions::default(),
        message: MessageSidePermissions::default(),
        pool_size: None,
    });

    Ok(AccountConfig {
        default: true,
        left,
        right,
        mailbox: Default::default(),
        message: Default::default(),
    })
}
