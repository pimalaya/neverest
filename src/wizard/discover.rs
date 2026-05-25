// This file is part of Neverest, a CLI to synchronize emails.
//
// Copyright (C) 2024-2026  soywod <pimalaya.org@posteo.net>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

//! Interactive configuration wizard with PACC / Autoconfig / SRV
//! discovery-based defaults.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::exit,
};

use anyhow::{Context, Result, anyhow};
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

// TODO: make the discovery DNS resolver configurable.
const DEFAULT_RESOLVER: &str = "tcp://1.1.1.1:53";

/// Parses [`DEFAULT_RESOLVER`] into a [`Url`].
pub fn discovery_resolver() -> Result<Url> {
    Url::parse(DEFAULT_RESOLVER).context("Parse DEFAULT_RESOLVER")
}

/// HTTPS-only [`Tls`] profile shared by every discovery mechanism.
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

/// Runs the wizard, writes the result to `target`, and returns it.
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

/// Runs PACC, Autoconfig and SRV in series; first non-empty result
/// wins, otherwise returns an empty [`DiscoveryResult`].
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

/// Picks JMAP or IMAP for the remote side, prompts for the local
/// m2dir root, then assembles the [`AccountConfig`].
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
