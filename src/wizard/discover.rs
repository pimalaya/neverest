//! # Discovery
//!
//! The half of the wizard deciding what the account is, what becomes of it
//! belonging to [`crate::cli::configure`].
//!
//! One prompt takes an email address (a bare domain is accepted), and
//! nothing else is asked before discovery. Each domain, mail, sending,
//! contacts and calendar, gets the best option found without a question
//! ([`crate::wizard::plan`]). The review shows the picks in plain words and
//! changes one on demand; the sign-ins follow, grouped by identity
//! ([`crate::wizard::signin`]), then the collections each source syncs
//! ([`crate::wizard::settings`]).
//!
//! The account holds one source per domain set up, in the direct-backend
//! sugar, plus the mail source's `smtp` channel. Finding nothing asks for a
//! server URL, an empty answer stopping the wizard.

use anyhow::{Result, bail};
use pimalaya_cli::{
    prompt,
    spinner::Spinner,
    wizard::{
        keyring::TokenBroker,
        review::{self, ReviewAction, ReviewRow},
    },
};

use crate::{
    config::{AccountConfig, CONFIG_SAMPLE_URL},
    wizard::{
        plan::{Change, Domain, Pick, Picks, Plan, Route, Setup, SignIn},
        search,
        server::TypedServer,
        settings,
        signin::{Backend, Identity, Outcome, SignIns},
    },
};

/// The one prompt of the wizard. Discovery also accepts a bare domain, but
/// the label names what users actually have.
const EMAIL_PROMPT: &str = "Email address:";

/// Discovers one account from a single prompt, signs it in, and hands it
/// back under the name `free_name` makes of the one the address suggests.
///
/// The name is settled before any sign-in, the keyring entries and the
/// Ortie account carrying it.
pub fn run(free_name: impl FnOnce(&str) -> String) -> Result<(String, AccountConfig)> {
    let domains: Vec<Domain> = Domain::ALL
        .into_iter()
        .filter(|domain| domain.compiled())
        .collect();

    if domains.is_empty() {
        bail!("This build has no backend the wizard can set up: {CONFIG_SAMPLE_URL}");
    }

    let input = prompt::text(EMAIL_PROMPT, None)?;
    let target = Target::parse(&input)?;
    let name = free_name(&default_account_name(&target.domain));

    let plan = target.look_up(TokenBroker::Ortie.installed());
    let mut picks = Picks::default();

    if plan.is_empty() {
        for route in target.ask_servers()? {
            let domain = route.protocol.domain();
            *picks.get_mut(domain) = Pick::Custom(route);
        }
    }

    let identity = Identity {
        name: name.clone(),
        address: target.address.clone(),
        domain: Some(target.domain.clone()),
    };

    let mut sign_ins = SignIns::default();

    let mut signed = loop {
        let setups: Vec<(Domain, Setup)> = domains
            .iter()
            .map(|domain| (*domain, plan.setup(*domain, &picks)))
            .collect();

        let rows: Vec<ReviewRow> = setups
            .iter()
            .map(|(domain, setup)| ReviewRow {
                name: domain.name().to_string(),
                line: plan.line(*domain, setup, &picks),
            })
            .collect();

        eprintln!();

        match review::prompt(&rows)? {
            ReviewAction::Continue if setups.iter().all(|(_, setup)| *setup == Setup::Off) => {
                eprintln!("Set up mail, contacts or calendar to continue");
            }
            ReviewAction::Continue => {
                if let Outcome::Done(signed) = sign_ins.run(&identity, &setups, &mut picks)? {
                    break signed;
                }
            }
            ReviewAction::Change(index) => change(&plan, domains[index], &mut picks)?,
        }
    };

    signed.sort_by_key(|signed| {
        Domain::ALL
            .iter()
            .position(|domain| *domain == signed.domain)
    });

    eprintln!();

    let mut account = AccountConfig::default();

    for signed in signed {
        match signed.backend {
            Backend::Source {
                mut config,
                collections,
            } => {
                config.collection_mut().filter = settings::pick(signed.domain, &collections)?;
                account.set_direct_backend(config);
            }
            Backend::Smtp(config) => account.smtp = Some(config),
        }
    }

    Ok((name, account))
}

/// What the first prompt was answered with, read for discovery.
struct Target {
    /// The answer, as typed.
    input: String,
    /// What discovery looks up: the address, or `@<domain>`.
    lookup: String,
    /// The domain looked up.
    domain: String,
    /// The typed address.
    address: Option<String>,
}

impl Target {
    /// Reads an address or a bare domain.
    fn parse(input: &str) -> Result<Self> {
        let input = input.trim();

        let (local, domain) = match input.rsplit_once('@') {
            Some((local, domain)) => (local, domain),
            None => ("", input),
        };

        let domain = domain.trim_end_matches('.').to_lowercase();

        if domain.is_empty() {
            bail!("Empty domain: enter an email address");
        }

        let address = (!local.is_empty()).then(|| input.to_string());

        Ok(Self {
            input: input.to_string(),
            lookup: address.clone().unwrap_or_else(|| format!("@{domain}")),
            domain,
            address,
        })
    }

    /// Looks the target up under a spinner, answering the options this
    /// build can set up.
    ///
    /// A resolver that does not answer reads as nothing discovered, the
    /// reason shown.
    fn look_up(&self, ortie: bool) -> Plan {
        let spinner = Spinner::start(format!("Looking up {}", self.domain));

        let mut plan = match search::search(&self.lookup) {
            Ok(configs) => Plan::new(&configs, ortie),
            Err(err) => {
                spinner.failure(format!("Cannot look up {}: {err:#}", self.domain));

                return Plan {
                    ortie,
                    ..Default::default()
                };
            }
        };

        plan.retain(|route| route.protocol.compiled());

        match plan.is_empty() {
            true => spinner.failure(format!("Nothing could be discovered for {}", self.domain)),
            false => spinner.success(format!("Found settings for {}", self.domain)),
        }

        plan
    }

    /// Asks for a server URL when discovery found nothing, its scheme
    /// naming the domains it sets up, then its sign-in method; an empty
    /// answer stops the wizard.
    fn ask_servers(&self) -> Result<Vec<Route>> {
        loop {
            let input = prompt::text("Server URL:", None)?;
            let input = input.trim();

            if input.is_empty() {
                bail!(
                    "Could not automatically discover a configuration for `{}`.\n\n\
                     Write your account configuration by hand instead, starting from the \
                     documented sample:\n  {CONFIG_SAMPLE_URL}",
                    self.input
                );
            }

            let servers = match TypedServer::parse_any(input) {
                Ok(servers) => servers,
                Err(err) => {
                    eprintln!("{err}");
                    continue;
                }
            };

            let servers: Vec<TypedServer> = servers
                .into_iter()
                .filter(|server| server.domain.protocol().compiled())
                .collect();

            if servers.is_empty() {
                eprintln!("This build has no backend for {input}");
                continue;
            }

            let sign_in = ask_sign_in()?;

            return Ok(servers
                .into_iter()
                .map(|server| server.route(sign_in))
                .collect());
        }
    }
}

/// Changes the pick of `domain` among the entries the plan offers, the
/// current one selected.
///
/// Sending rides on mail: it is set up once mail is, and a mail source
/// sending by itself leaves nothing to change.
fn change(plan: &Plan, domain: Domain, picks: &mut Picks) -> Result<()> {
    if domain == Domain::Sending {
        match plan.setup(Domain::Mail, picks).route() {
            None => {
                eprintln!("Sending goes through the mail source: set mail up first");
                return Ok(());
            }
            Some(mail) if mail.protocol.sends() => {
                eprintln!("{} sends mail over its own session", mail.protocol.name());
                return Ok(());
            }
            Some(_) => {}
        }
    }

    let custom = domain.protocol().compiled();
    let items: Vec<_> = plan
        .changes(domain, picks)
        .into_iter()
        .filter(|item| custom || item.change != Change::Custom)
        .collect();

    let current = Change::Pick(picks.get(domain).clone());
    let default = items.iter().find(|item| item.change == current).cloned();

    let item = prompt::item(format!("{}:", domain.name()), items, default)?;

    match item.change {
        Change::Pick(pick) => *picks.get_mut(domain) = pick,
        Change::Custom => {
            let label = match domain {
                Domain::Sending => "Sending server URL:",
                _ => "Server URL:",
            };

            if let Some(route) = ask_server(label, domain)? {
                *picks.get_mut(domain) = Pick::Custom(route);
            }
        }
    }

    Ok(())
}

/// Asks for a server URL for `domain` until one reads, then its sign-in
/// method; `None` on an empty answer.
fn ask_server(label: &str, domain: Domain) -> Result<Option<Route>> {
    loop {
        let input = prompt::text(label, None)?;
        let input = input.trim();

        if input.is_empty() {
            return Ok(None);
        }

        match TypedServer::parse(input, domain) {
            Ok(server) => return Ok(Some(server.route(ask_sign_in()?))),
            Err(err) => eprintln!("{err}"),
        }
    }
}

/// Asks how a typed server signs in, a password (the default) or an API
/// token.
fn ask_sign_in() -> Result<SignIn> {
    Ok(prompt::item(
        "Sign in with:",
        [SignIn::Password, SignIn::Token],
        Some(SignIn::Password),
    )?)
}

/// Proposes a default account name from the email: the first label of
/// its domain, never the local part.
pub fn default_account_name(email: &str) -> String {
    let domain = match email.rsplit_once('@') {
        Some((_, domain)) => domain,
        None => email,
    };

    let label = domain.split('.').next().unwrap_or(domain);

    match label.is_empty() {
        true => String::from("default"),
        false => label.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use crate::wizard::discover::*;

    #[test]
    fn account_name_defaults_to_the_first_domain_label() {
        assert_eq!(default_account_name("clement.douin@posteo.net"), "posteo");
        assert_eq!(default_account_name("alice@mail.example.co.uk"), "mail");

        assert_eq!(default_account_name("@posteo.net"), "posteo");
        assert_eq!(default_account_name("posteo.net"), "posteo");

        assert_eq!(default_account_name("@"), "default");
    }

    #[test]
    fn the_input_says_what_discovery_looks_up() {
        let address = Target::parse(" jane@Example.org ").expect("an address");
        assert_eq!(address.lookup, "jane@Example.org");
        assert_eq!(address.domain, "example.org");
        assert_eq!(address.address.as_deref(), Some("jane@Example.org"));

        let domain = Target::parse("example.org.").expect("a bare domain");
        assert_eq!(domain.lookup, "@example.org");
        assert_eq!(domain.address, None);

        let at = Target::parse("@example.org").expect("an at domain");
        assert_eq!(at.lookup, "@example.org");
        assert_eq!(at.address, None);

        assert!(Target::parse("jane@").is_err());
        assert!(Target::parse("  ").is_err());
    }
}
