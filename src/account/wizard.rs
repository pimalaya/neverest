use color_eyre::eyre::Result;
use pimalaya_tui::prompt;

use crate::backend::{self, BackendSource};

use super::config::AccountConfig;

pub async fn configure() -> Result<(String, AccountConfig)> {
    let name = prompt::text("Account name:", Some("personal"))?;

    let config = AccountConfig {
        default: Some(true),
        folder: None,
        envelope: None,
        left: backend::wizard::configure(&name, BackendSource::Left).await?,
        right: backend::wizard::configure(&name, BackendSource::Right).await?,
        pool_size: None,
    };

    Ok((name, config))
}
