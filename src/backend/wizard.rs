use anyhow::Result;
use dialoguer::Select;

#[cfg(feature = "imap")]
use crate::imap;
#[cfg(feature = "maildir")]
use crate::maildir;
#[cfg(feature = "notmuch")]
use crate::notmuch;
use crate::ui::THEME;

use super::{config::BackendGlobalConfig, BackendKind, BackendSource};

static DEFAULT_BACKEND_KINDS: &[BackendKind] = &[
    #[cfg(feature = "imap")]
    BackendKind::Imap,
    #[cfg(feature = "maildir")]
    BackendKind::Maildir,
    #[cfg(feature = "notmuch")]
    BackendKind::Notmuch,
];

pub async fn configure(account_name: &str, source: BackendSource) -> Result<BackendGlobalConfig> {
    let default_kind = if source.is_left() { 0 } else { 1 };

    let kind = Select::with_theme(&*THEME)
        .with_prompt(source)
        .items(DEFAULT_BACKEND_KINDS)
        .default(default_kind)
        .interact_opt()?
        .unwrap();

    let backend = match &DEFAULT_BACKEND_KINDS[kind] {
        #[cfg(feature = "imap")]
        BackendKind::Imap => imap::wizard::configure(account_name).await,
        #[cfg(feature = "maildir")]
        BackendKind::Maildir => maildir::wizard::configure(account_name),
        #[cfg(feature = "notmuch")]
        BackendKind::Notmuch => notmuch::wizard::configure(),
    }?;

    Ok(BackendGlobalConfig {
        backend,
        folder: None,
        flag: None,
        message: None,
    })
}
