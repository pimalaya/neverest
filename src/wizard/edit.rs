//! Interactive configuration wizard for editing (or creating) an
//! existing account. Skips provider discovery entirely: this is meant
//! for accounts the user already configured. Pre-fills the wizard
//! prompts with the account's current values; the auth secret is
//! never reused, the user is re-prompted for it.

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use log::info;
use pimalaya_cli::{
    prompt,
    wizard::{
        imap::{
            self as imap_wizard, Encryption as ImapEncryption, ImapAuth, ImapSecret,
            WizardImapConfig,
        },
        jmap::{self as jmap_wizard, JmapAuth, JmapSecret, WizardJmapConfig},
    },
};

use crate::{
    config::{
        AccountConfig, Config, FlagSidePermissions, ImapConfig, JmapAuthConfig, JmapConfig,
        M2dirConfig, MailboxSidePermissions, MessageSidePermissions, SaslConfig, SideConfig,
    },
    wizard::account::{imap_to_config, jmap_to_config},
};

/// Edits (or creates) the account named `account_name`. Uses the
/// account's current `left` and `right` blocks as defaults; an
/// existing JMAP variant routes to the JMAP wizard, an IMAP variant
/// routes to the IMAP wizard, and m2dir re-prompts for the store
/// root. Writes the updated config to `target` before returning.
pub fn edit_account(target: &Path, mut config: Config, account_name: &str) -> Result<Config> {
    let existing = config.accounts.remove(account_name);

    let (left_default, right_default) = match existing.as_ref() {
        Some(a) => (Some(&a.left), Some(&a.right)),
        None => (None, None),
    };

    let default_email = right_default
        .and_then(side_email)
        .or_else(|| left_default.and_then(side_email));

    let email = prompt::text("Email address:", default_email.as_deref())?;
    let (local_part, domain) = email
        .rsplit_once('@')
        .ok_or_else(|| anyhow!("Invalid email address `{email}`: missing `@`"))?;

    let is_first_account = config.accounts.is_empty() && existing.is_none();
    let default = existing
        .as_ref()
        .map(|a| a.default)
        .unwrap_or(is_first_account);
    let mailbox = existing
        .as_ref()
        .map(|a| a.mailbox.clone())
        .unwrap_or_default();
    let message = existing
        .as_ref()
        .map(|a| a.message.clone())
        .unwrap_or_default();

    let left = prompt_side("left", local_part, domain, account_name, left_default)?;
    let right = prompt_side("right", local_part, domain, account_name, right_default)?;

    let account = AccountConfig {
        default,
        left,
        right,
        mailbox,
        message,
    };

    config.accounts.insert(account_name.to_owned(), account);
    config.write(target)?;
    info!("configuration written to {}", target.display());

    Ok(config)
}

/// Re-runs the wizard for one side. The default backend kind (and the
/// pre-filled values for IMAP/JMAP host/port/login or the m2dir root)
/// come from the existing config when present.
fn prompt_side(
    label: &str,
    local_part: &str,
    domain: &str,
    account_name: &str,
    existing: Option<&SideConfig>,
) -> Result<SideConfig> {
    match existing {
        Some(SideConfig::Jmap(c)) => {
            let defaults = jmap_to_wizard(c);
            let wizard_name = format!("{account_name} {label}");
            let jmap = jmap_wizard::run(&wizard_name, local_part, domain, Some(&defaults))?;
            Ok(SideConfig::Jmap(jmap_to_config(jmap)?))
        }
        Some(SideConfig::Imap(c)) => {
            let defaults = imap_to_wizard(c);
            let wizard_name = format!("{account_name} {label}");
            let imap = imap_wizard::run(&wizard_name, local_part, domain, Some(&defaults))?;
            Ok(SideConfig::Imap(imap_to_config(imap)?))
        }
        Some(SideConfig::M2dir(c)) => {
            let root = prompt::text(
                format!("{label} m2dir store root:"),
                Some(c.root.display().to_string()),
            )?;
            Ok(SideConfig::M2dir(M2dirConfig {
                root: PathBuf::from(root),
                mailbox: c.mailbox,
                flag: c.flag,
                message: c.message,
                pool_size: c.pool_size,
            }))
        }
        None => {
            let default_root = format!("~/Mail/{account_name}-{label}");
            let root = prompt::text(format!("{label} m2dir store root:"), Some(default_root))?;
            Ok(SideConfig::M2dir(M2dirConfig {
                root: PathBuf::from(root),
                mailbox: MailboxSidePermissions::default(),
                flag: FlagSidePermissions::default(),
                message: MessageSidePermissions::default(),
                pool_size: None,
            }))
        }
    }
}

/// Returns the user-facing email for a side, if extractable. Used to
/// default the "Email address:" prompt when editing.
fn side_email(side: &SideConfig) -> Option<String> {
    match side {
        SideConfig::Imap(c) => Some(sasl_login(c.sasl.as_ref())).filter(|s| !s.is_empty()),
        SideConfig::Jmap(c) => match &c.auth {
            JmapAuthConfig::Basic { username, .. } if !username.is_empty() => {
                Some(username.clone())
            }
            _ => None,
        },
        SideConfig::M2dir(_) => None,
    }
}

/// Derives default [`WizardImapConfig`] values from an existing
/// [`ImapConfig`]. The auth secret is never reused; the wizard
/// re-prompts the user for it.
pub fn imap_to_wizard(c: &ImapConfig) -> WizardImapConfig {
    let (scheme, host, port_from_url) = parse_server(&c.server, "imaps");
    let encryption = match scheme.as_str() {
        "imaps" => ImapEncryption::Tls,
        _ if c.starttls => ImapEncryption::StartTls,
        _ => ImapEncryption::None,
    };
    let port = port_from_url.unwrap_or(match encryption {
        ImapEncryption::Tls => 993,
        _ => 143,
    });
    let login = sasl_login(c.sasl.as_ref());

    WizardImapConfig {
        host,
        port,
        encryption,
        login,
        auth: ImapAuth::Password(ImapSecret::Raw(String::new().into())),
    }
}

/// Same as [`imap_to_wizard`] but for JMAP. Auth is reset to a
/// placeholder; the wizard re-prompts the user for it.
pub fn jmap_to_wizard(c: &JmapConfig) -> WizardJmapConfig {
    let auth = match &c.auth {
        JmapAuthConfig::Basic { username, .. } => JmapAuth::Basic {
            login: username.clone(),
            secret: JmapSecret::Raw(String::new().into()),
        },
        JmapAuthConfig::Bearer { .. } | JmapAuthConfig::Header(_) => JmapAuth::Bearer {
            secret: JmapSecret::Raw(String::new().into()),
        },
    };

    WizardJmapConfig {
        server: c.server.clone(),
        auth,
    }
}

/// Extracts the user-facing login (PLAIN authcid, LOGIN username,
/// XOAUTH2/OAUTHBEARER/SCRAM username) from a SASL block so the
/// wizard can pre-fill the prompt when editing an existing account.
/// Returns an empty string when the block is absent or carries no
/// username (e.g. ANONYMOUS).
fn sasl_login(sasl: Option<&SaslConfig>) -> String {
    match sasl {
        Some(SaslConfig::Plain(p)) => p.authcid.clone(),
        Some(SaslConfig::Login(l)) => l.username.clone(),
        Some(SaslConfig::Oauthbearer(o)) => o.username.clone(),
        Some(SaslConfig::Xoauth2(x)) => x.username.clone(),
        Some(SaslConfig::ScramSha256(s)) => s.username.clone(),
        Some(SaslConfig::Anonymous(_)) | None => String::new(),
    }
}

/// Best-effort URL split into `(scheme, host, port?)`. Tolerates
/// bare authorities by defaulting the scheme.
fn parse_server(server: &str, default_scheme: &'static str) -> (String, String, Option<u16>) {
    if let Ok(url) = url::Url::parse(server) {
        let scheme = url.scheme().to_owned();
        let host = url.host_str().map(str::to_owned).unwrap_or_default();
        let port = url.port_or_known_default();
        return (scheme, host, port);
    }
    if let Ok(url) = url::Url::parse(&format!("{default_scheme}://{server}")) {
        let scheme = url.scheme().to_owned();
        let host = url.host_str().map(str::to_owned).unwrap_or_default();
        let port = url.port();
        return (scheme, host, port);
    }
    (default_scheme.to_owned(), server.to_owned(), None)
}
