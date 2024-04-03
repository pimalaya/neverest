use anyhow::Result;
use dialoguer::Input;

use crate::{
    backend::{self, BackendSource},
    ui::THEME,
};

use super::config::AccountConfig;

pub async fn configure() -> Result<(String, AccountConfig)> {
    let name = Input::with_theme(&*THEME)
        .with_prompt("Account name")
        .default(String::from("personal"))
        .interact()?;

    let config = AccountConfig {
        default: Some(true),
        folder: None,
        envelope: None,
        left: backend::wizard::configure(&name, BackendSource::Left).await?,
        right: backend::wizard::configure(&name, BackendSource::Right).await?,
    };

    Ok((name, config))
}
