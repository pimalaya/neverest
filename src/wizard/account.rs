//! Converters from wizard answers to on-disk IMAP / JMAP configs.

use std::process::Command;

use anyhow::{Result, bail};
use pimalaya_cli::wizard::{
    imap::{Encryption as ImapEncryption, ImapAuth, ImapSecret, WizardImapConfig},
    jmap::{JmapAuth, JmapSecret, WizardJmapConfig},
};
use pimalaya_config::{command::shell, secret::Secret};

use crate::config::{
    FlagSidePermissions, ImapConfig, JmapAuthConfig, JmapConfig, MailboxSidePermissions,
    MessageSidePermissions, SaslConfig, SaslPlainConfig,
};

/// Converts wizard IMAP answers into an on-disk [`ImapConfig`].
pub fn imap_to_config(w: WizardImapConfig) -> Result<ImapConfig> {
    let scheme = match w.encryption {
        ImapEncryption::Tls => "imaps",
        ImapEncryption::StartTls | ImapEncryption::None => "imap",
    };
    let server = format!("{scheme}://{}:{}", w.host, w.port);
    let starttls = matches!(w.encryption, ImapEncryption::StartTls);
    let sasl = Some(build_sasl_imap(&w.login, w.auth)?);

    Ok(ImapConfig {
        server,
        tls: Default::default(),
        starttls,
        alpn: io_imap::client::default_alpn(),
        sasl,
        mailbox: MailboxSidePermissions::default(),
        flag: FlagSidePermissions::default(),
        message: MessageSidePermissions::default(),
        pool_size: None,
    })
}

/// Converts wizard JMAP answers into an on-disk [`JmapConfig`].
pub fn jmap_to_config(w: WizardJmapConfig) -> Result<JmapConfig> {
    let auth = match w.auth {
        JmapAuth::Basic { login, secret } => JmapAuthConfig::Basic {
            username: login,
            password: jmap_secret_to_secret(secret)?,
        },
        JmapAuth::Bearer { secret } => JmapAuthConfig::Bearer {
            token: jmap_secret_to_secret(secret)?,
        },
    };

    Ok(JmapConfig {
        server: w.server,
        tls: Default::default(),
        alpn: io_jmap::client::default_alpn(),
        auth,
        identity_id: None,
        drafts_mailbox_id: None,
        mailbox: MailboxSidePermissions::default(),
        flag: FlagSidePermissions::default(),
        message: MessageSidePermissions::default(),
        pool_size: None,
    })
}

fn build_sasl_imap(login: &str, auth: ImapAuth) -> Result<SaslConfig> {
    let ImapAuth::Password(secret) = auth;
    let passwd = match secret {
        ImapSecret::Raw(s) => Secret::Raw(s),
        ImapSecret::Command(cmd) => Secret::Command(parse_cmd(&cmd)?),
    };

    Ok(SaslConfig::Plain(SaslPlainConfig {
        authzid: None,
        authcid: login.to_owned(),
        passwd,
    }))
}

fn jmap_secret_to_secret(secret: JmapSecret) -> Result<Secret> {
    Ok(match secret {
        JmapSecret::Raw(s) => Secret::Raw(s),
        JmapSecret::Command(cmd) => Secret::Command(parse_cmd(&cmd)?),
    })
}

fn parse_cmd(cmd: &str) -> Result<Command> {
    let line = cmd.trim();
    if line.is_empty() {
        bail!("Empty shell command for secret");
    }
    Ok(shell(line))
}
