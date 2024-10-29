use color_eyre::eyre::Result;
use pimalaya_tui::terminal::prompt;

use crate::backend::{self, BackendSource};

use super::config::TomlAccountConfig;

pub async fn configure() -> Result<(String, TomlAccountConfig)> {
    let name = prompt::text("Account name:", Some("personal"))?;

    let config = TomlAccountConfig {
        default: Some(true),
        folder: None,
        envelope: None,
        left: backend::wizard::configure(&name, BackendSource::Left).await?,
        right: backend::wizard::configure(&name, BackendSource::Right).await?,
    };

    Ok((name, config))
}
