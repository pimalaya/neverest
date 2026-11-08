//! # Configure command
//!
//! The wizard generates, it never edits: it discovers an account from one
//! prompt (see [`crate::wizard::discover`]), signs it in, then hands the
//! resulting `[accounts.<name>]` table back as a file to create, a block
//! to append, or a document on stdout.
//!
//! It runs from `neverest configure`, and from the offer a bare
//! `neverest` or a command needing an account raises when it finds no
//! configuration. That offer is the only place the wizard introduces
//! itself, the command asked for by name going straight to the prompts.
//!
//! One account, one source per domain set up (mail, contacts, calendar), in
//! the direct-backend sugar (`imap.server = …`), plus the mail source's
//! `smtp` channel. A mirror, a fan-in and every field discovery does not
//! cover are written by hand against the documented sample, and so is a
//! change to an account already there.
//!
//! Appending is a plain text append rather than a re-serialization, so
//! comments, ordering and hand-written formatting come out untouched.
//!
//! One rule guards it: the account name has to be free, two tables of one
//! name making the document fail to parse. The account writes no `default`:
//! every command runs over every account unless `-a` names some.

use std::{
    fmt,
    fs::{self, OpenOptions},
    io::{IsTerminal, Write, stdin, stdout},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clap::Parser;
use pimalaya_cli::{printer::Printer, prompt};
use pimalaya_config::toml::TomlConfig;
use schemars::JsonSchema;
use serde::Serialize;

use crate::{
    config::{CONFIG_SAMPLE_URL, Config},
    wizard::discover,
};

/// Configure an account interactively.
///
/// Discovers your mail, sending, contacts and calendar servers from an
/// email address, reviews what it picked, signs each identity in once and
/// tests it, asks which collections to sync, then saves the resulting
/// account to the configuration file, appends it to the one already there,
/// or prints it for you to place by hand.
///
/// Anything discovery does not cover, a mirror or a change to an account
/// already configured, is written by hand.
#[derive(Debug, Parser)]
pub struct ConfigureCommand;

impl ConfigureCommand {
    /// Runs the wizard, then saves, appends or prints the account.
    ///
    /// The account name is not asked, being only the TOML table key, but it
    /// is settled before the sign-ins, which name the keyring entries and
    /// the Ortie account after it. `-a` names nothing here: the wizard
    /// generates rather than edits. A redirected stdout or the JSON output
    /// stays non-interactive, the prompts rendering on stderr and no file
    /// being touched.
    pub fn execute(self, printer: &mut impl Printer, config_paths: &[PathBuf]) -> Result<()> {
        if !stdin().is_terminal() {
            bail!(
                "Configuring needs a terminal to prompt on, \
                 write the configuration by hand instead: {CONFIG_SAMPLE_URL}"
            );
        }

        let path = Config::target_path(config_paths)?;
        let existing = ExistingConfig::read(&path)?;

        let (name, account) = discover::run(|base| account_name(base, existing.as_ref()))?;

        let config = ConfigureOutput {
            document: account.render(&name)?,
            name,
            default: false,
        };

        if printer.is_json() || !stdout().is_terminal() {
            return printer.out(config);
        }

        match existing {
            Some(_) => append_or_print(printer, &path, config),
            None => save_or_print(printer, &path, config),
        }
    }
}

/// What a configuration already on disk constrains in the new account.
struct ExistingConfig {
    names: Vec<String>,
}

impl ExistingConfig {
    /// Reads the configuration at `path`, or `None` when no file is there.
    ///
    /// A file that fails to parse is an error rather than a `None`, since
    /// appending to a broken document would bury the actual problem under
    /// a second one.
    fn read(path: &Path) -> Result<Option<Self>> {
        if !path.exists() {
            return Ok(None);
        }

        let config = Config::from_paths(&[path.to_path_buf()])
            .with_context(|| format!("Read the configuration at {}", path.display()))?;

        Ok(Some(Self {
            names: config.accounts.keys().cloned().collect(),
        }))
    }
}

/// The generated account, as the printer takes it.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigureOutput {
    /// The account name, which is the `[accounts.<name>]` table key.
    name: String,
    /// Always false: a command runs over every account unless `-a` names
    /// some, so the generated account claims no default. Kept for the
    /// shape every pimalaya wizard prints.
    default: bool,
    /// The rendered TOML document.
    document: String,
}

impl fmt::Display for ConfigureOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // NOTE: the trailing newline terminates the document, and it is
        // also what flushes the line-buffered stdout.
        writeln!(f, "{}", self.document.trim_end())
    }
}

/// Welcomes, offers to generate a first configuration, and returns
/// whether the wizard ran.
///
/// Raised from the two places nothing can happen without a configuration,
/// a bare invocation and a command needing an account. It is a hook
/// rather than a gate, so what a declined offer leads to is the caller's
/// business, and for a command that is carrying on.
pub fn offer_configuration(
    printer: &mut impl Printer,
    config_paths: &[PathBuf],
    path: &Path,
) -> Result<bool> {
    print_welcome(path);

    if !prompt::bool("Create a configuration with an account?", true)? {
        return Ok(false);
    }

    ConfigureCommand.execute(printer, config_paths)?;

    Ok(true)
}

/// Frames Neverest, names the missing configuration file, and points at
/// the sample for everything the wizard does not cover.
///
/// Printed before the offer, so the wizard introduces itself to someone
/// who did not ask for it; `configure` skips it. On stderr, so a
/// redirected stdout holds the document alone.
pub fn print_welcome(path: &Path) {
    eprintln!();
    eprintln!("Welcome to Neverest, the CLI to synchronize PIM collections.");
    eprintln!();
    eprintln!("Neverest reconciles what you already have, mail over IMAP, the Gmail API or");
    eprintln!("Microsoft Graph and contacts and calendars over DAV or their provider's");
    eprintln!("API, into a local pimdir store the apps read and edit. It needs one account");
    eprintln!("to know what to reconcile, and no configuration file was found at:");
    eprintln!();
    eprintln!("  {}", path.display());
    eprintln!();
    eprintln!("The wizard discovers your mail, contacts and calendar servers from your");
    eprintln!("email address, signs each in once and generates one ready-to-use account");
    eprintln!("covering them. Everything discovery does not cover is written by hand, and");
    eprintln!("every field is documented at:");
    eprintln!();
    eprintln!("  {CONFIG_SAMPLE_URL}");
    eprintln!();
    eprintln!("At anytime, you can create a new account with the command:");
    eprintln!();
    eprintln!("  neverest configure");
    eprintln!();
}

/// The name discovery proposes, suffixed until the configuration does not
/// already hold it.
///
/// Not prompted: the name is only the TOML table key. It still has to be
/// free, a second `[accounts.<name>]` table making the whole document
/// fail to parse and taking the working accounts down with it.
fn account_name(base: &str, existing: Option<&ExistingConfig>) -> String {
    let taken = existing
        .map(|config| config.names.as_slice())
        .unwrap_or(&[]);

    if !taken.iter().any(|name| name == base) {
        return base.to_string();
    }

    let mut suffix = 2;

    loop {
        let name = format!("{base}-{suffix}");

        if !taken.contains(&name) {
            return name;
        }

        suffix += 1;
    }
}

/// Offers to write the generated account to a configuration file that
/// does not exist yet, printing it instead when the offer is declined.
fn save_or_print(printer: &mut impl Printer, path: &Path, config: ConfigureOutput) -> Result<()> {
    let prompt = format!("Save this account to {}?", path.display());

    if !prompt::bool(prompt, true)? {
        return printer.out(config);
    }

    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("Create the config directory {}", parent.display()))?;
    }

    fs::write(path, config.to_string())
        .with_context(|| format!("Write the config file {}", path.display()))?;

    print_saved(path, &config);

    Ok(())
}

/// Offers to append the generated account to the configuration file
/// already there, printing it instead when the offer is declined.
fn append_or_print(printer: &mut impl Printer, path: &Path, config: ConfigureOutput) -> Result<()> {
    let prompt = format!("Append account `{}` to {}?", config.name, path.display());

    if !prompt::bool(prompt, true)? {
        return printer.out(config);
    }

    let mut file = OpenOptions::new()
        .append(true)
        .open(path)
        .with_context(|| format!("Open the config file {}", path.display()))?;

    // NOTE: the leading newline separates the two tables, and terminates
    // the last line when the file ends without one.
    write!(file, "\n{config}")
        .with_context(|| format!("Append to the config file {}", path.display()))?;

    print_saved(path, &config);

    Ok(())
}

/// Tells where the account landed, under which name, and what to run next.
///
/// The name matters because it was never asked for, and `-a` takes it to
/// run one account alone; without it every command runs over every one.
fn print_saved(path: &Path, config: &ConfigureOutput) {
    let name = &config.name;

    eprintln!();
    eprintln!("Account `{name}` saved to {}.", path.display());
    eprintln!("Run `neverest init` to prepare it, then `neverest sync`.");
}

#[cfg(test)]
mod tests {
    use std::{
        env,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use pimalaya_config::{command::CommandConfig, secret::Secret};

    use crate::{
        cli::configure::*,
        config::{
            AccountConfig, CaldavConfig, CarddavConfig, CollectionFilter, DavAuthConfig,
            ImapConfig, SaslConfig, SaslPlainConfig, SmtpConfig, SourceBackendConfig,
        },
    };

    static NEXT_CONFIG: AtomicUsize = AtomicUsize::new(0);

    /// A path in the temporary directory no other test writes to.
    fn config_path() -> PathBuf {
        let id = NEXT_CONFIG.fetch_add(1, Ordering::Relaxed);
        env::temp_dir().join(format!("neverest-configure-{id}.toml"))
    }

    /// A minimal account naming one IMAP source, the shape the wizard
    /// writes.
    fn account() -> AccountConfig {
        let mut account = AccountConfig::default();
        account.set_direct_backend(SourceBackendConfig::Imap(imap()));
        account
    }

    fn imap() -> ImapConfig {
        ImapConfig {
            server: String::from("imaps://imap.example.org:993"),
            tls: Default::default(),
            starttls: false,
            alpn: None,
            sasl: None,
            collection: Default::default(),
            flag: Default::default(),
            item: Default::default(),
            pool_size: None,
        }
    }

    /// The keyring entry a typed password was stored under.
    fn stored() -> Secret {
        Secret::Command(CommandConfig::Argv {
            program: String::from("pass"),
            args: vec![String::from("show"), String::from("neverest-perso-imap")],
        })
    }

    /// A generated account claims no default: every command runs over
    /// every account unless `-a` names some.
    #[test]
    fn a_generated_account_parses_back() {
        let document = account().render("perso").expect("render the account");
        let config: Config = toml::from_str(&document).expect("parse the generated config");
        let account = &config.accounts["perso"];

        assert_eq!(config.accounts.len(), 1);
        assert!(!account.default);
        assert!(!document.contains("default"), "{document}");
        assert_eq!(
            account.imap.as_ref().map(|config| config.server.as_str()),
            Some("imaps://imap.example.org:993")
        );

        // NOTE: a generated document holds what was configured, every
        // other field being left at its default.
        assert!(!document.contains("carddav"));
        assert!(!document.contains("conflict"));

        let lines: Vec<&str> = document.lines().collect();
        assert_eq!(lines[0], "[accounts.perso]");
        assert_eq!(lines[1], "imap.server = \"imaps://imap.example.org:993\"");
    }

    /// The account the wizard writes for mail, sending, contacts and
    /// calendar of one provider: one source per domain under one header,
    /// read in domain order, the mail folders left out as an exclusion and
    /// no permission written beside it, every source reading the one
    /// stored password.
    #[test]
    fn an_account_covering_every_domain_parses_back() {
        let sasl = Some(SaslConfig::Plain(SaslPlainConfig {
            authzid: None,
            authcid: String::from("jane@example.org"),
            passwd: stored(),
        }));
        let auth = DavAuthConfig::Basic {
            username: String::from("jane@example.org"),
            password: stored(),
        };

        let mut imap = ImapConfig {
            sasl: sasl.clone(),
            ..imap()
        };
        imap.collection.filter =
            CollectionFilter::Exclude(vec![String::from("Junk"), String::from("Trash")]);

        let mut account = AccountConfig::default();
        account.set_direct_backend(SourceBackendConfig::Imap(imap));
        account.set_direct_backend(SourceBackendConfig::Carddav(CarddavConfig {
            server: String::from("https://dav.example.org/"),
            tls: Default::default(),
            alpn: vec![String::from("http/1.1")],
            auth: auth.clone(),
            collection: Default::default(),
            flag: Default::default(),
            item: Default::default(),
            pool_size: None,
        }));
        account.set_direct_backend(SourceBackendConfig::Caldav(CaldavConfig {
            server: String::from("https://dav.example.org/"),
            tls: Default::default(),
            alpn: vec![String::from("http/1.1")],
            auth,
            collection: Default::default(),
            flag: Default::default(),
            item: Default::default(),
            pool_size: None,
        }));
        account.smtp = Some(SmtpConfig {
            server: String::from("smtps://smtp.example.org:465"),
            tls: Default::default(),
            starttls: false,
            alpn: None,
            sasl,
        });

        let document = account.render("perso").expect("render the account");

        assert_eq!(document.matches("[accounts").count(), 1, "{document}");
        assert!(
            document.contains("imap.collection.filter.exclude = [\"Junk\", \"Trash\"]"),
            "{document}"
        );
        assert!(!document.contains("collection.create"), "{document}");
        assert!(!document.contains("collection.delete"), "{document}");
        assert!(!document.contains("default"), "{document}");

        let position = |group: &str| document.find(&format!("\n{group}.")).expect(group);
        assert!(position("imap") < position("smtp"));
        assert!(position("smtp") < position("carddav"));
        assert!(position("carddav") < position("caldav"));

        let config: Config = toml::from_str(&document).expect("parse the rendered account");
        let account = &config.accounts["perso"];
        account.validate().expect("a valid account");

        let sources = account.sources().expect("the account's sources");
        let mut names: Vec<&String> = sources.keys().collect();
        names.sort();
        assert_eq!(names, ["caldav", "carddav", "imap"]);
        assert!(sources["imap"].smtp.is_some(), "the channel completes mail");
        assert_eq!(
            sources["imap"].collection().filter,
            CollectionFilter::Exclude(vec![String::from("Junk"), String::from("Trash")])
        );
        assert_eq!(
            sources["carddav"].collection().filter,
            CollectionFilter::All
        );
        assert_eq!(
            document.matches("neverest-perso-imap").count(),
            4,
            "{document}"
        );
    }

    /// A hand-written account naming several sources still renders as
    /// dotted keys under one header, so appending never opens a table the
    /// account after it would fall into.
    #[test]
    fn an_account_naming_several_sources_renders_as_one_table() {
        let account: AccountConfig = toml::from_str(
            r#"
            default = true
            sources.left.imap.server = "imaps://a.example.org:993"
            sources.right.caldav.server = "https://b.example.org"
            sources.right.caldav.auth.bearer.token.raw = "tok"
            "#,
        )
        .expect("parse the account");

        let document = account.render("perso").expect("render the account");

        assert_eq!(document.matches('[').count(), 1);
        assert!(document.starts_with("[accounts.perso]\n"));

        let config: Config = toml::from_str(&document).expect("parse the rendered account");
        assert_eq!(config.accounts["perso"].sources.len(), 2);
    }

    #[test]
    fn an_appended_account_keeps_the_existing_one() {
        let path = config_path();

        // NOTE: no trailing newline, the shape an appended block has to
        // survive without merging into the last line.
        fs::write(
            &path,
            "# my accounts\n[accounts.work]\ndefault = true\nimap.server = \"imaps://w.org:993\"",
        )
        .expect("write the existing config");

        let existing = ExistingConfig::read(&path)
            .expect("read the existing config")
            .expect("an existing config");

        assert_eq!(existing.names, ["work"]);

        let document = account().render("perso").expect("render the account");
        let mut file = OpenOptions::new().append(true).open(&path).expect("open");
        write!(file, "\n{document}").expect("append the generated account");
        drop(file);

        let content = fs::read_to_string(&path).expect("read back");
        let config: Config = toml::from_str(&content).expect("parse the appended config");

        assert_eq!(config.accounts.len(), 2);

        let defaults = config
            .accounts
            .values()
            .filter(|account| account.default)
            .count();
        assert_eq!(defaults, 1);
        assert!(config.accounts["work"].default);
        assert!(content.starts_with("# my accounts"));

        fs::remove_file(&path).expect("remove the config");
    }

    #[test]
    fn a_taken_name_gets_a_suffix() {
        let existing = ExistingConfig {
            names: vec![String::from("posteo"), String::from("posteo-2")],
        };

        assert_eq!(account_name("posteo", None), "posteo");
        assert_eq!(account_name("posteo", Some(&existing)), "posteo-3");
        assert_eq!(account_name("fastmail", Some(&existing)), "fastmail");
    }

    #[test]
    fn a_missing_configuration_constrains_nothing() {
        let existing = ExistingConfig::read(&config_path()).expect("read a missing config");

        assert!(existing.is_none());
    }
}
