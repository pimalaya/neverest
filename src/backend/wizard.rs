use color_eyre::eyre::Result;
use email::autoconfig;
use pimalaya_tui::terminal::{prompt, wizard};

use crate::backend::config::BackendConfig;

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
    let backend = prompt::item(format!("{source}:"), &*DEFAULT_BACKEND_KINDS, None)?;

    let backend = match backend {
        #[cfg(feature = "imap")]
        BackendKind::Imap => {
            let email = prompt::email("Email address:", None)?;

            println!("Discovering IMAP config…");
            let autoconfig = autoconfig::from_addr(&email).await.ok();

            let config = wizard::imap::start(account_name, &email, autoconfig.as_ref()).await?;

            BackendConfig::Imap(config)
        }
        #[cfg(feature = "maildir")]
        BackendKind::Maildir => {
            let config = wizard::maildir::start(account_name)?;
            BackendConfig::Maildir(config)
        }
        #[cfg(feature = "notmuch")]
        BackendKind::Notmuch => {
            let config = wizard::notmuch::start()?;
            BackendConfig::Notmuch(config)
        }
    };

    Ok(BackendGlobalConfig {
        backend,
        folder: None,
        flag: None,
        message: None,
    })
}
