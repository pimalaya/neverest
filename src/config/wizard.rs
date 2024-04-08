use anyhow::Result;
use dialoguer::Input;
use shellexpand_utils::expand;
use std::{fs, path::PathBuf};
use toml_edit::{DocumentMut, Table};

use crate::{account, ui::THEME};

use super::Config;

#[macro_export]
macro_rules! wizard_warn {
    ($($arg:tt)*) => {
	println!("{}", console::style(format!($($arg)*)).yellow().bold());
    };
}

#[macro_export]
macro_rules! wizard_prompt {
    ($($arg:tt)*) => {
	format!("{}", console::style(format!($($arg)*)).italic())
    };
}

#[macro_export]
macro_rules! wizard_log {
    ($($arg:tt)*) => {
	println!();
	println!("{}", console::style(format!($($arg)*)).underlined());
	println!();
    };
}

pub async fn configure(path: &PathBuf) -> Result<Config> {
    wizard_log!("Configuring your default account");

    let mut config = Config::default();

    let (account_name, account_config) = account::wizard::configure().await?;
    config.accounts.insert(account_name, account_config);

    let path = Input::with_theme(&*THEME)
        .with_prompt(wizard_prompt!(
            "Where would you like to save your configuration?"
        ))
        .default(path.to_string_lossy().to_string())
        .interact()?;
    let path = expand::path(path);

    println!("Writing the configuration to {path:?}…");
    let toml = pretty_serialize(&config)?;
    fs::create_dir_all(path.parent().unwrap_or(&path))?;
    fs::write(path, toml)?;

    println!("Exiting the wizard…");
    Ok(config)
}

fn pretty_serialize(config: &Config) -> Result<String> {
    let mut doc: DocumentMut = toml::to_string(&config)?.parse()?;

    doc.iter_mut().for_each(|(_, item)| {
        if let Some(table) = item.as_table_mut() {
            table.iter_mut().for_each(|(_, item)| {
                if let Some(table) = item.as_table_mut() {
                    set_table_dotted(table);
                }
            })
        }
    });

    Ok(doc.to_string())
}

fn set_table_dotted(table: &mut Table) {
    let keys: Vec<String> = table.iter().map(|(key, _)| key.to_string()).collect();
    for ref key in keys {
        if let Some(table) = table.get_mut(key).unwrap().as_table_mut() {
            table.set_dotted(true);
            set_table_dotted(table)
        }
    }
}

#[cfg(test)]
mod test {
    use email::{
        account::config::passwd::PasswdConfig,
        flag::sync::config::FlagSyncPermissions,
        folder::sync::config::{FolderSyncPermissions, FolderSyncStrategy},
        imap::config::{ImapAuthConfig, ImapConfig},
        maildir::config::MaildirConfig,
        message::sync::config::MessageSyncPermissions,
    };
    use secret::Secret;
    use std::collections::{BTreeSet, HashMap};

    use crate::{
        account::config::{AccountConfig, FolderConfig},
        backend::config::{
            BackendConfig, BackendGlobalConfig, FlagBackendConfig, FolderBackendConfig,
            MessageBackendConfig,
        },
        config::Config,
    };

    fn assert_eq(config: AccountConfig, expected_toml: &str) {
        let config = Config {
            accounts: HashMap::from_iter([("test".into(), config)]),
            ..Default::default()
        };

        let toml = super::pretty_serialize(&config).expect("serialize error");
        assert_eq!(toml, expected_toml);

        let expected_config = toml::from_str(&toml).expect("deserialize error");
        assert_eq!(config, expected_config);
    }

    #[test]
    fn pretty_serialize() {
        assert_eq(
            AccountConfig {
                default: Some(true),
                folder: Some(FolderConfig {
                    filter: FolderSyncStrategy::Include(BTreeSet::from_iter(["INBOX".into()])),
                }),
                // TODO: test me
                envelope: None,
                left: BackendGlobalConfig {
                    backend: BackendConfig::Imap(ImapConfig {
                        host: "localhost".into(),
                        port: 143,
                        login: "test".into(),
                        auth: ImapAuthConfig::Passwd(PasswdConfig(Secret::Raw("password".into()))),
                        ..Default::default()
                    }),
                    folder: Some(FolderBackendConfig {
                        permissions: FolderSyncPermissions {
                            create: true,
                            delete: false,
                        },
                    }),
                    flag: Some(FlagBackendConfig {
                        permissions: FlagSyncPermissions { update: true },
                    }),
                    message: Some(MessageBackendConfig {
                        permissions: MessageSyncPermissions {
                            create: true,
                            delete: false,
                        },
                    }),
                },
                right: BackendGlobalConfig {
                    backend: BackendConfig::Maildir(MaildirConfig {
                        root_dir: "/tmp/test".into(),
                    }),
                    folder: None,
                    flag: None,
                    message: None,
                },
            },
            r#"[accounts.test]
default = true
folder.filter.include = ["INBOX"]
left.backend.type = "imap"
left.backend.host = "localhost"
left.backend.port = 143
left.backend.login = "test"
left.backend.passwd.raw = "password"
left.folder.permissions.create = true
left.folder.permissions.delete = false
left.flag.permissions.update = true
left.message.permissions.create = true
left.message.permissions.delete = false
right.backend.type = "maildir"
right.backend.root-dir = "/tmp/test"
"#,
        )
    }
}
