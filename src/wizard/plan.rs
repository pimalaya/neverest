//! # Plan
//!
//! The options discovery found for each domain of an account, mail,
//! sending, contacts and calendar, ranked so the first one, Auto, is picked
//! without a question, and the words the review shows them in.
//!
//! An option is a [`Route`]: a protocol, a server and a sign-in method.
//! Nothing here prompts or connects, the interactive half belonging to
//! [`crate::wizard::discover`] and [`crate::wizard::signin`].

use std::{fmt, net::IpAddr};

use io_pim_discovery::compose::config::{
    DiscoveryAuthMethod, DiscoveryEndpoint, DiscoverySecurity, DiscoveryService,
    DiscoveryServiceConfig,
};
use url::Url;

/// A domain of the account, as the review rows name it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Domain {
    /// Reading mail.
    Mail,
    /// Sending mail.
    Sending,
    /// Contacts.
    Contacts,
    /// Calendars.
    Calendar,
}

/// A protocol a route reaches its domain over.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Protocol {
    /// IMAP, reading mail.
    Imap,
    /// SMTP, sending mail.
    Smtp,
    /// The Gmail API, reading mail.
    Gmail,
    /// Microsoft Graph mail, reading and sending mail.
    Msgraph,
    /// CardDAV, contacts.
    Carddav,
    /// The People API, contacts.
    Gpeople,
    /// Microsoft Graph contacts.
    MsgraphContacts,
    /// CalDAV, calendars.
    Caldav,
    /// The Calendar API, calendars.
    Gcal,
    /// Microsoft Graph calendars.
    MsgraphCalendar,
}

/// How a route signs in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignIn {
    /// A password, often an app password.
    Password,
    /// A static API token, sent as a bearer.
    Token,
    /// An OAuth 2.0 access token a broker issues.
    Oauth,
}

/// Where a route reaches its server.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Endpoint {
    /// A TCP server, over implicit TLS or STARTTLS.
    Tcp { host: String, port: u16, tls: bool },
    /// An https URL, or an http one on this machine.
    Http(String),
}

/// One way to set a domain up: a protocol, a server and a sign-in method.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Route {
    /// The protocol.
    pub protocol: Protocol,
    /// The server.
    pub endpoint: Endpoint,
    /// The sign-in method.
    pub sign_in: SignIn,
    /// The login the server is known to take.
    pub login: Option<String>,
    /// The authorization server of an OAuth sign-in.
    pub authority: Option<String>,
}

/// The options discovery found for each domain, best first.
#[derive(Clone, Debug, Default)]
pub struct Plan {
    /// The mail routes.
    pub mail: Vec<Route>,
    /// The SMTP routes.
    pub sending: Vec<Route>,
    /// The contacts routes.
    pub contacts: Vec<Route>,
    /// The calendar routes.
    pub calendar: Vec<Route>,
    /// Whether `ortie` is on the `PATH`, which ranks OAuth first and words
    /// it as a browser sign-in.
    pub ortie: bool,
}

/// What the user picked for a domain.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum Pick {
    /// The first ranked route, resolved against the other picks.
    #[default]
    Auto,
    /// The route found at this index.
    Found(usize),
    /// A typed server.
    Custom(Route),
    /// Nothing.
    Off,
}

/// The picks of every domain.
#[derive(Clone, Debug, Default)]
pub struct Picks {
    /// The mail pick.
    pub mail: Pick,
    /// The sending pick.
    pub sending: Pick,
    /// The contacts pick.
    pub contacts: Pick,
    /// The calendar pick.
    pub calendar: Pick,
}

/// What a domain's pick resolves to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Setup {
    /// A route to sign in.
    Route(Route),
    /// Sending over mail's own session.
    ThroughMail,
    /// Nothing.
    Off,
}

/// An entry of the list a change offers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeItem {
    /// What the entry reads.
    pub label: String,
    /// What picking it does.
    pub change: Change,
}

/// What picking a change entry does.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Change {
    /// Sets the domain's pick.
    Pick(Pick),
    /// Asks for a server URL.
    Custom,
}

impl Domain {
    /// Every domain, in review order.
    pub const ALL: [Self; 4] = [Self::Mail, Self::Sending, Self::Contacts, Self::Calendar];

    /// The domain's name, as the review shows it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Mail => "Mail",
            Self::Sending => "Sending",
            Self::Contacts => "Contacts",
            Self::Calendar => "Calendar",
        }
    }

    /// The protocol a typed server URL reaches the domain over.
    pub fn protocol(self) -> Protocol {
        match self {
            Self::Mail => Protocol::Imap,
            Self::Sending => Protocol::Smtp,
            Self::Contacts => Protocol::Carddav,
            Self::Calendar => Protocol::Caldav,
        }
    }

    /// Whether this build can set the domain up at all: one of its
    /// protocols is compiled in, and for sending, mail can be set up too.
    pub fn compiled(self) -> bool {
        let protocols: &[Protocol] = match self {
            Self::Mail => &[Protocol::Imap, Protocol::Gmail, Protocol::Msgraph],
            Self::Sending => &[Protocol::Smtp, Protocol::Msgraph],
            Self::Contacts => &[
                Protocol::Carddav,
                Protocol::Gpeople,
                Protocol::MsgraphContacts,
            ],
            Self::Calendar => &[Protocol::Caldav, Protocol::Gcal, Protocol::MsgraphCalendar],
        };

        let mail = self != Self::Sending || Self::Mail.compiled();

        mail && protocols.iter().any(|protocol| protocol.compiled())
    }
}

impl Protocol {
    /// The protocol's name, as the review shows it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Imap => "IMAP",
            Self::Smtp => "SMTP",
            Self::Gmail => "Gmail API",
            Self::Msgraph | Self::MsgraphContacts | Self::MsgraphCalendar => "Microsoft Graph",
            Self::Carddav => "CardDAV",
            Self::Gpeople => "Google People API",
            Self::Caldav => "CalDAV",
            Self::Gcal => "Google Calendar API",
        }
    }

    /// The service name Ortie takes: io-pim-discovery's wire name.
    pub fn service(self) -> &'static str {
        match self {
            Self::Imap => "imap",
            Self::Smtp => "smtp",
            Self::Gmail => "gmail",
            Self::Msgraph => "msgraph",
            Self::Carddav => "carddav",
            Self::Gpeople => "gpeople",
            Self::MsgraphContacts => "msgraphContacts",
            Self::Caldav => "caldav",
            Self::Gcal => "gcal",
            Self::MsgraphCalendar => "msgraphCalendar",
        }
    }

    /// The configuration key the protocol is written under, which names
    /// its source and the keyring entries.
    pub fn source(self) -> &'static str {
        match self {
            Self::Imap => "imap",
            Self::Smtp => "smtp",
            Self::Gmail => "gmail",
            Self::Msgraph => "msgraph",
            Self::Carddav => "carddav",
            Self::Gpeople => "gpeople",
            Self::MsgraphContacts => "msgraph-contacts",
            Self::Caldav => "caldav",
            Self::Gcal => "gcal",
            Self::MsgraphCalendar => "msgraph-calendar",
        }
    }

    /// The domain the protocol sets up.
    pub fn domain(self) -> Domain {
        match self {
            Self::Imap | Self::Gmail | Self::Msgraph => Domain::Mail,
            Self::Smtp => Domain::Sending,
            Self::Carddav | Self::Gpeople | Self::MsgraphContacts => Domain::Contacts,
            Self::Caldav | Self::Gcal | Self::MsgraphCalendar => Domain::Calendar,
        }
    }

    /// Whether this build carries the protocol's backend.
    pub fn compiled(self) -> bool {
        match self {
            Self::Imap => cfg!(feature = "imap"),
            Self::Smtp => cfg!(feature = "smtp"),
            Self::Gmail => cfg!(feature = "gmail"),
            Self::Msgraph | Self::MsgraphContacts | Self::MsgraphCalendar => {
                cfg!(feature = "msgraph")
            }
            Self::Carddav | Self::Caldav => cfg!(feature = "dav"),
            Self::Gpeople => cfg!(feature = "gpeople"),
            Self::Gcal => cfg!(feature = "gcal"),
        }
    }

    /// Whether mail read over the protocol sends by itself, needing no
    /// SMTP channel.
    pub fn sends(self) -> bool {
        self == Self::Msgraph
    }

    /// Whether the protocol is a provider's own API, reached at a fixed
    /// host through OAuth alone.
    pub fn api(self) -> bool {
        !matches!(self, Self::Imap | Self::Smtp | Self::Carddav | Self::Caldav)
    }

    /// The protocol a discovered service is set up over, `None` for a
    /// service neverest has no backend for.
    fn of(service: DiscoveryService) -> Option<Self> {
        Some(match service {
            DiscoveryService::Imap => Self::Imap,
            DiscoveryService::Smtp => Self::Smtp,
            DiscoveryService::Gmail => Self::Gmail,
            DiscoveryService::Msgraph => Self::Msgraph,
            DiscoveryService::Carddav => Self::Carddav,
            DiscoveryService::Gpeople => Self::Gpeople,
            DiscoveryService::MsgraphContacts => Self::MsgraphContacts,
            DiscoveryService::Caldav => Self::Caldav,
            DiscoveryService::Gcal => Self::Gcal,
            DiscoveryService::MsgraphCalendar => Self::MsgraphCalendar,
            _ => return None,
        })
    }

    /// The provider's own API first, then the rest.
    fn rank(self) -> u8 {
        match self.api() {
            true => 0,
            false => 1,
        }
    }
}

impl SignIn {
    /// The secret's name, as a prompt asks for it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Password => "Password",
            Self::Token => "API token",
            Self::Oauth => "Access token",
        }
    }

    /// OAuth first when Ortie can run it, last otherwise.
    fn rank(self, ortie: bool) -> u8 {
        match (self, ortie) {
            (Self::Oauth, true) | (Self::Password, false) => 0,
            (Self::Token, _) => 1,
            (Self::Password, true) | (Self::Oauth, false) => 2,
        }
    }
}

impl fmt::Display for SignIn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Password => "Password",
            Self::Token => "API token",
            Self::Oauth => "OAuth 2.0",
        })
    }
}

impl Endpoint {
    /// The server's host name, lowercased.
    pub fn host(&self) -> String {
        match self {
            Self::Tcp { host, .. } => host.clone(),
            Self::Http(url) => Url::parse(url)
                .ok()
                .and_then(|url| url.host_str().map(str::to_lowercase))
                .unwrap_or_default(),
        }
    }

    /// Implicit TLS before STARTTLS.
    fn rank(&self) -> u8 {
        match self {
            Self::Tcp { tls: false, .. } => 1,
            _ => 0,
        }
    }
}

impl Route {
    /// The server URL, as a password prompt names it.
    pub fn url(&self) -> String {
        match &self.endpoint {
            Endpoint::Tcp { host, port, tls } => {
                let scheme = match (self.protocol, tls) {
                    (Protocol::Smtp, true) => "smtps",
                    (Protocol::Smtp, false) => "smtp",
                    (_, true) => "imaps",
                    (_, false) => "imap",
                };

                format!("{scheme}://{host}:{port}")
            }
            Endpoint::Http(url) => url.clone(),
        }
    }

    /// Whether the route takes a login, beside its secret.
    pub fn needs_login(&self) -> bool {
        match self.protocol {
            Protocol::Imap | Protocol::Smtp => true,
            Protocol::Carddav | Protocol::Caldav => self.sign_in == SignIn::Password,
            _ => false,
        }
    }

    /// Whether two routes sign in with one secret: one authorization
    /// server for OAuth, one group of servers of one name otherwise.
    pub fn shares_sign_in(&self, other: &Self) -> bool {
        if self.sign_in != other.sign_in {
            return false;
        }

        match (self.sign_in, &self.authority, &other.authority) {
            (SignIn::Oauth, Some(a), Some(b)) => a == b,
            _ => same_host(&self.endpoint.host(), &other.endpoint.host()),
        }
    }

    /// The routes a discovered config offers, one per sign-in method, none
    /// for an unencrypted server or a service neverest does not sync.
    fn from_config(config: &DiscoveryServiceConfig) -> Vec<Self> {
        let Some(protocol) = Protocol::of(config.service) else {
            return Vec::new();
        };

        let endpoint = match &config.endpoint {
            DiscoveryEndpoint::Tcp {
                host,
                port,
                security,
            } if matches!(protocol, Protocol::Imap | Protocol::Smtp)
                && *security != DiscoverySecurity::Plain =>
            {
                Endpoint::Tcp {
                    host: host.trim_end_matches('.').to_lowercase(),
                    port: *port,
                    tls: *security == DiscoverySecurity::Tls,
                }
            }
            DiscoveryEndpoint::Http(url)
                if !matches!(protocol, Protocol::Imap | Protocol::Smtp) =>
            {
                match Url::parse(url) {
                    Ok(url) if url.scheme() == "https" => Endpoint::Http(url.to_string()),
                    _ => return Vec::new(),
                }
            }
            _ => return Vec::new(),
        };

        let login = config
            .username
            .clone()
            .filter(|username| is_address(username));

        let mut sign_ins: Vec<(SignIn, Option<String>)> = Vec::new();

        for method in &config.auth {
            let (sign_in, authority) = match method {
                DiscoveryAuthMethod::Password => (SignIn::Password, None),
                DiscoveryAuthMethod::Bearer => (SignIn::Token, None),
                DiscoveryAuthMethod::OauthAuthorizationCodeGrant { token_endpoint, .. }
                | DiscoveryAuthMethod::OauthDeviceAuthorizationGrant { token_endpoint, .. } => {
                    (SignIn::Oauth, Some(token_endpoint.clone()))
                }
                DiscoveryAuthMethod::OauthIssuer(issuer) => (SignIn::Oauth, Some(issuer.clone())),
            };

            if !sign_ins.iter().any(|(known, _)| *known == sign_in) {
                sign_ins.push((sign_in, authority));
            }
        }

        // A server advertising nothing still takes a secret of some kind.
        if sign_ins.is_empty() {
            sign_ins = vec![(SignIn::Password, None), (SignIn::Token, None)];
        }

        // The provider APIs take OAuth alone.
        if protocol.api() {
            sign_ins.retain(|(sign_in, _)| *sign_in == SignIn::Oauth);
        }

        sign_ins
            .into_iter()
            .map(|(sign_in, authority)| Self {
                protocol,
                endpoint: endpoint.clone(),
                sign_in,
                login: login.clone(),
                authority,
            })
            .collect()
    }

    /// Whether two routes read the same: protocol, server and sign-in.
    fn reads_same(&self, other: &Self) -> bool {
        self.protocol == other.protocol
            && self.endpoint == other.endpoint
            && self.sign_in == other.sign_in
    }

    /// The route's rank among the routes of its domain.
    fn rank(&self, ortie: bool) -> (u8, u8, u8) {
        (
            self.protocol.rank(),
            self.endpoint.rank(),
            self.sign_in.rank(ortie),
        )
    }

    /// The change-list entry, before any server is named.
    fn label(&self) -> String {
        format!("{} · {}", self.protocol.name(), self.sign_in)
    }

    /// What the review says of the route, `shared` when it signs in with
    /// mail's secret.
    fn line(&self, ortie: bool, shared: bool) -> String {
        let how = match (self.sign_in, shared, ortie) {
            (SignIn::Password, false, _) => "with your password",
            (SignIn::Password, true, _) => "with mail's password",
            (SignIn::Token, false, _) => "with an API token",
            (SignIn::Token, true, _) => "with mail's API token",
            (SignIn::Oauth, _, true) => "through your browser",
            (SignIn::Oauth, _, false) => "with a token from a broker",
        };

        match self.protocol.api() {
            true => format!("{}, {how}", self.protocol.name()),
            false => format!(
                "{} at {}, {how}",
                self.protocol.name(),
                self.endpoint.host()
            ),
        }
    }
}

impl Plan {
    /// Builds the routes of `configs`, deduplicated and ranked.
    ///
    /// Two routes reading the same are one, the first keeping its place
    /// and taking the other's login when it had none. The sort is stable,
    /// so routes of one rank keep discovery's priority order.
    pub fn new(configs: &[DiscoveryServiceConfig], ortie: bool) -> Self {
        let mut plan = Self {
            ortie,
            ..Default::default()
        };

        for route in configs.iter().flat_map(Route::from_config) {
            let routes = plan.routes_mut(route.protocol.domain());

            match routes.iter_mut().find(|known| known.reads_same(&route)) {
                Some(known) => {
                    if known.login.is_none() {
                        known.login = route.login;
                    }
                }
                None => routes.push(route),
            }
        }

        for domain in Domain::ALL {
            plan.routes_mut(domain)
                .sort_by_key(|route| route.rank(ortie));
        }

        plan
    }

    /// The routes found for `domain`, best first.
    pub fn routes(&self, domain: Domain) -> &[Route] {
        match domain {
            Domain::Mail => &self.mail,
            Domain::Sending => &self.sending,
            Domain::Contacts => &self.contacts,
            Domain::Calendar => &self.calendar,
        }
    }

    /// Keeps the routes of every domain `keep` accepts.
    pub fn retain(&mut self, keep: impl Fn(&Route) -> bool) {
        for domain in Domain::ALL {
            self.routes_mut(domain).retain(&keep);
        }
    }

    /// Whether discovery found nothing for any domain.
    pub fn is_empty(&self) -> bool {
        Domain::ALL
            .iter()
            .all(|domain| self.routes(*domain).is_empty())
    }

    /// Resolves the pick of `domain`, sending against what mail resolved
    /// to.
    pub fn setup(&self, domain: Domain, picks: &Picks) -> Setup {
        if domain == Domain::Sending {
            let mail = self.setup(Domain::Mail, picks);
            return self.sending(&picks.sending, &mail);
        }

        let routes = self.routes(domain);

        let route = match picks.get(domain) {
            Pick::Auto => routes.first(),
            Pick::Found(index) => routes.get(*index),
            Pick::Custom(route) => Some(route),
            Pick::Off => None,
        };

        route.cloned().map(Setup::Route).unwrap_or(Setup::Off)
    }

    /// What the review says of `domain` set up as `setup`, `picks` telling
    /// a domain discovery missed from one left out.
    pub fn line(&self, domain: Domain, setup: &Setup, picks: &Picks) -> String {
        match setup {
            Setup::Route(route) => {
                let mail = self.setup(Domain::Mail, picks);
                let shared = domain != Domain::Mail
                    && route.sign_in != SignIn::Oauth
                    && mail.route().is_some_and(|mail| mail.shares_sign_in(route));

                route.line(self.ortie, shared)
            }
            Setup::ThroughMail => String::from("Through mail"),
            Setup::Off if domain != Domain::Sending && *picks.get(domain) == Pick::Auto => {
                String::from("Nothing found")
            }
            Setup::Off => String::from("Not set up"),
        }
    }

    /// The entries a change of `domain` offers: Auto naming what it is
    /// when discovery found an option, every route found, the server named
    /// only where two entries would read the same, a custom server, and a
    /// way to leave the domain out.
    pub fn changes(&self, domain: Domain, picks: &Picks) -> Vec<ChangeItem> {
        let routes = self.routes(domain);
        let mut items = Vec::new();

        if !routes.is_empty() {
            let mut auto = picks.clone();
            *auto.get_mut(domain) = Pick::Auto;

            let line = self.line(domain, &self.setup(domain, &auto), &auto);

            items.push(ChangeItem {
                label: format!("Auto ({line})"),
                change: Change::Pick(Pick::Auto),
            });
        }

        let labels: Vec<String> = routes.iter().map(Route::label).collect();

        for (index, route) in routes.iter().enumerate() {
            let label = &labels[index];
            let twins = labels.iter().filter(|other| *other == label).count();

            let label = match twins {
                1 => label.clone(),
                _ => format!("{label} · {}", route.url()),
            };

            items.push(ChangeItem {
                label,
                change: Change::Pick(Pick::Found(index)),
            });
        }

        items.push(ChangeItem {
            label: String::from("Custom server URL"),
            change: Change::Custom,
        });

        let off = match domain {
            Domain::Sending => "No sending",
            _ => "Leave out",
        };

        items.push(ChangeItem {
            label: String::from(off),
            change: Change::Pick(Pick::Off),
        });

        items
    }

    /// Resolves the sending pick against what mail resolved to.
    ///
    /// Sending needs mail: it goes through mail when mail sends by itself,
    /// else over the picked SMTP server, Auto taking the best one and
    /// signing in as mail does when that server offers it.
    fn sending(&self, pick: &Pick, mail: &Setup) -> Setup {
        let Some(mail) = mail.route() else {
            return Setup::Off;
        };

        if mail.protocol.sends() {
            return Setup::ThroughMail;
        }

        let route = match pick {
            Pick::Found(index) => self.sending.get(*index),
            Pick::Custom(route) => Some(route),
            Pick::Off => None,
            Pick::Auto => self.sending.first().map(|best| {
                self.sending
                    .iter()
                    .find(|route| route.endpoint == best.endpoint && route.sign_in == mail.sign_in)
                    .unwrap_or(best)
            }),
        };

        route.cloned().map(Setup::Route).unwrap_or(Setup::Off)
    }

    /// The routes of `domain`, to fill or sort them.
    fn routes_mut(&mut self, domain: Domain) -> &mut Vec<Route> {
        match domain {
            Domain::Mail => &mut self.mail,
            Domain::Sending => &mut self.sending,
            Domain::Contacts => &mut self.contacts,
            Domain::Calendar => &mut self.calendar,
        }
    }
}

impl Picks {
    /// The pick of `domain`.
    pub fn get(&self, domain: Domain) -> &Pick {
        match domain {
            Domain::Mail => &self.mail,
            Domain::Sending => &self.sending,
            Domain::Contacts => &self.contacts,
            Domain::Calendar => &self.calendar,
        }
    }

    /// The pick of `domain`, to change it.
    pub fn get_mut(&mut self, domain: Domain) -> &mut Pick {
        match domain {
            Domain::Mail => &mut self.mail,
            Domain::Sending => &mut self.sending,
            Domain::Contacts => &mut self.contacts,
            Domain::Calendar => &mut self.calendar,
        }
    }
}

impl Setup {
    /// The route to sign in, if any.
    pub fn route(&self) -> Option<&Route> {
        match self {
            Self::Route(route) => Some(route),
            Self::ThroughMail | Self::Off => None,
        }
    }
}

impl fmt::Display for ChangeItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

/// Splits `routes` into sign-in groups, each signing in with one secret:
/// the indices of each group, in the order of their first route.
pub fn groups(routes: &[&Route]) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();

    for (index, route) in routes.iter().enumerate() {
        match groups
            .iter_mut()
            .find(|group| routes[group[0]].shares_sign_in(route))
        {
            Some(group) => group.push(index),
            None => groups.push(vec![index]),
        }
    }

    groups
}

/// Whether two hosts are servers of one name: equal, or equal once their
/// first label is dropped, an IP address or a two-label host never losing
/// one.
pub fn same_host(a: &str, b: &str) -> bool {
    let a = a.trim_end_matches('.').to_lowercase();
    let b = b.trim_end_matches('.').to_lowercase();

    if a == b {
        return true;
    }

    matches!((parent(&a), parent(&b)), (Some(a), Some(b)) if a == b)
}

/// The host without its first label, when that leaves a domain of two
/// labels or more and the host is no IP address.
fn parent(host: &str) -> Option<&str> {
    let bare = host.trim_start_matches('[').trim_end_matches(']');

    if bare.parse::<IpAddr>().is_ok() {
        return None;
    }

    let (_, rest) = host.split_once('.')?;

    rest.contains('.').then_some(rest)
}

/// Whether a string is a full `local@domain` address.
fn is_address(value: &str) -> bool {
    value
        .split_once('@')
        .is_some_and(|(local, domain)| !local.is_empty() && !domain.is_empty())
}

#[cfg(test)]
mod tests {
    use io_pim_discovery::compose::{
        config::{
            DiscoveryAuthMethod, DiscoveryConfigSource, DiscoveryEndpoint, DiscoverySecurity,
            DiscoveryService, DiscoveryServiceConfig,
        },
        providers::DiscoveryKnownProvider,
    };

    use crate::wizard::plan::*;

    const TOKEN_ENDPOINT: &str = "https://auth.example.org/token";

    fn grant() -> DiscoveryAuthMethod {
        DiscoveryAuthMethod::OauthAuthorizationCodeGrant {
            authorization_endpoint: String::from("https://auth.example.org/authorize"),
            token_endpoint: String::from(TOKEN_ENDPOINT),
            scope: None,
            resource: None,
            scopes_supported: Vec::new(),
        }
    }

    fn device_grant() -> DiscoveryAuthMethod {
        DiscoveryAuthMethod::OauthDeviceAuthorizationGrant {
            device_authorization_endpoint: String::from("https://auth.example.org/device"),
            token_endpoint: String::from(TOKEN_ENDPOINT),
            scope: None,
            resource: None,
            scopes_supported: Vec::new(),
        }
    }

    fn tcp(
        service: DiscoveryService,
        host: &str,
        port: u16,
        security: DiscoverySecurity,
        auth: Vec<DiscoveryAuthMethod>,
    ) -> DiscoveryServiceConfig {
        DiscoveryServiceConfig {
            service,
            endpoint: DiscoveryEndpoint::Tcp {
                host: host.to_string(),
                port,
                security,
            },
            username: Some(String::from("jane@example.org")),
            auth,
            source: DiscoveryConfigSource::Srv,
            resolved: false,
        }
    }

    fn http(
        service: DiscoveryService,
        url: &str,
        auth: Vec<DiscoveryAuthMethod>,
    ) -> DiscoveryServiceConfig {
        DiscoveryServiceConfig {
            service,
            endpoint: DiscoveryEndpoint::Http(url.to_string()),
            username: Some(String::from("jane@example.org")),
            auth,
            source: DiscoveryConfigSource::Dav,
            resolved: true,
        }
    }

    /// A provider API, as a provider rule offers it.
    fn api(service: DiscoveryService, url: &str) -> DiscoveryServiceConfig {
        let mut config = http(service, url, vec![grant()]);
        config.source = DiscoveryConfigSource::Provider(DiscoveryKnownProvider::Google);
        config.resolved = false;
        config
    }

    fn imaps(host: &str, auth: Vec<DiscoveryAuthMethod>) -> DiscoveryServiceConfig {
        tcp(
            DiscoveryService::Imap,
            host,
            993,
            DiscoverySecurity::Tls,
            auth,
        )
    }

    fn smtps(host: &str, auth: Vec<DiscoveryAuthMethod>) -> DiscoveryServiceConfig {
        tcp(
            DiscoveryService::Smtp,
            host,
            465,
            DiscoverySecurity::Tls,
            auth,
        )
    }

    fn summary(routes: &[Route]) -> Vec<(Protocol, String, SignIn)> {
        routes
            .iter()
            .map(|route| (route.protocol, route.url(), route.sign_in))
            .collect()
    }

    fn protocol(plan: &Plan, domain: Domain, picks: &Picks) -> Option<Protocol> {
        plan.setup(domain, picks)
            .route()
            .map(|route| route.protocol)
    }

    /// Every option a provider rule offers for the four domains, through
    /// one authorization server, beside the open protocols.
    fn provider() -> Vec<DiscoveryServiceConfig> {
        let mut configs = vec![
            imaps(
                "imap.example.org",
                vec![grant(), DiscoveryAuthMethod::Password],
            ),
            smtps(
                "smtp.example.org",
                vec![grant(), DiscoveryAuthMethod::Password],
            ),
            http(
                DiscoveryService::Carddav,
                "https://dav.example.org/carddav/",
                vec![grant()],
            ),
            http(
                DiscoveryService::Caldav,
                "https://dav.example.org/caldav/",
                vec![grant()],
            ),
            api(DiscoveryService::Gmail, "https://gmail.googleapis.com/"),
            api(DiscoveryService::Gpeople, "https://people.googleapis.com/"),
            api(
                DiscoveryService::Gcal,
                "https://www.googleapis.com/calendar/v3/",
            ),
        ];

        for config in &mut configs {
            config.source = DiscoveryConfigSource::Provider(DiscoveryKnownProvider::Google);
        }

        configs
    }

    #[test]
    fn each_domain_takes_the_routes_of_its_protocols() {
        let plan = Plan::new(
            &[
                imaps("imap.example.org", vec![DiscoveryAuthMethod::Password]),
                smtps("smtp.example.org", vec![DiscoveryAuthMethod::Password]),
                http(
                    DiscoveryService::Carddav,
                    "https://dav.example.org/card/",
                    vec![DiscoveryAuthMethod::Password],
                ),
                http(
                    DiscoveryService::Caldav,
                    "https://dav.example.org/cal/",
                    vec![DiscoveryAuthMethod::Password],
                ),
                http(
                    DiscoveryService::Jmap,
                    "https://api.example.org/jmap/session",
                    vec![DiscoveryAuthMethod::Password],
                ),
            ],
            false,
        );

        assert_eq!(summary(&plan.mail)[0].0, Protocol::Imap);
        assert_eq!(summary(&plan.sending)[0].0, Protocol::Smtp);
        assert_eq!(
            summary(&plan.contacts),
            [(
                Protocol::Carddav,
                String::from("https://dav.example.org/card/"),
                SignIn::Password
            )]
        );
        assert_eq!(summary(&plan.calendar)[0].0, Protocol::Caldav);

        // JMAP has no backend in neverest, so it is no option.
        assert_eq!(plan.mail.len(), 1);
    }

    #[test]
    fn oauth_through_several_grants_is_one_option() {
        let plan = Plan::new(
            &[http(
                DiscoveryService::Carddav,
                "https://dav.example.org/",
                vec![
                    grant(),
                    DiscoveryAuthMethod::Password,
                    device_grant(),
                    DiscoveryAuthMethod::Bearer,
                ],
            )],
            true,
        );

        let sign_ins: Vec<SignIn> = plan.contacts.iter().map(|route| route.sign_in).collect();
        assert_eq!(sign_ins, [SignIn::Oauth, SignIn::Token, SignIn::Password]);
        assert_eq!(plan.contacts[0].authority.as_deref(), Some(TOKEN_ENDPOINT));
    }

    #[test]
    fn unencrypted_servers_are_not_offered() {
        let plan = Plan::new(
            &[
                tcp(
                    DiscoveryService::Imap,
                    "imap.example.org",
                    143,
                    DiscoverySecurity::Plain,
                    vec![DiscoveryAuthMethod::Password],
                ),
                http(
                    DiscoveryService::Caldav,
                    "http://dav.example.org/",
                    vec![DiscoveryAuthMethod::Password],
                ),
            ],
            false,
        );

        assert!(plan.is_empty());
    }

    #[test]
    fn oauth_ranks_first_only_with_ortie() {
        let configs = [http(
            DiscoveryService::Carddav,
            "https://dav.example.org/",
            vec![DiscoveryAuthMethod::Password, grant()],
        )];

        let with = Plan::new(&configs, true);
        let without = Plan::new(&configs, false);

        assert_eq!(with.contacts[0].sign_in, SignIn::Oauth);
        assert_eq!(without.contacts[0].sign_in, SignIn::Password);
    }

    #[test]
    fn implicit_tls_ranks_before_starttls() {
        let plan = Plan::new(
            &[
                tcp(
                    DiscoveryService::Imap,
                    "imap.example.org",
                    143,
                    DiscoverySecurity::Starttls,
                    vec![DiscoveryAuthMethod::Password],
                ),
                imaps("imap.example.org", vec![DiscoveryAuthMethod::Password]),
                tcp(
                    DiscoveryService::Smtp,
                    "smtp.example.org",
                    587,
                    DiscoverySecurity::Starttls,
                    vec![DiscoveryAuthMethod::Password],
                ),
                smtps("smtp.example.org", vec![DiscoveryAuthMethod::Password]),
            ],
            false,
        );

        assert_eq!(plan.mail[0].url(), "imaps://imap.example.org:993");
        assert_eq!(plan.sending[0].url(), "smtps://smtp.example.org:465");
    }

    /// A provider rule's own APIs win every domain they carry, with or
    /// without Ortie, and the three sign in through one grant.
    #[test]
    fn a_provider_account_signs_its_domains_in_through_one_grant() {
        for ortie in [true, false] {
            let plan = Plan::new(&provider(), ortie);
            let picks = Picks::default();

            assert_eq!(protocol(&plan, Domain::Mail, &picks), Some(Protocol::Gmail));
            assert_eq!(
                protocol(&plan, Domain::Contacts, &picks),
                Some(Protocol::Gpeople)
            );
            assert_eq!(
                protocol(&plan, Domain::Calendar, &picks),
                Some(Protocol::Gcal)
            );

            let setups: Vec<Setup> = [Domain::Mail, Domain::Contacts, Domain::Calendar]
                .into_iter()
                .map(|domain| plan.setup(domain, &picks))
                .collect();
            let routes: Vec<&Route> = setups.iter().filter_map(Setup::route).collect();
            assert_eq!(groups(&routes), [vec![0, 1, 2]]);
        }
    }

    /// The Gmail API reads mail and sends nothing, so sending takes the
    /// provider's SMTP server under mail's grant; Graph mail sends by
    /// itself.
    #[test]
    fn sending_goes_through_mail_only_when_mail_sends() {
        let plan = Plan::new(&provider(), true);
        let picks = Picks::default();

        let sending = plan.setup(Domain::Sending, &picks);
        let route = sending.route().expect("an SMTP channel");
        assert_eq!(route.protocol, Protocol::Smtp);
        assert_eq!(route.sign_in, SignIn::Oauth);

        let mail = plan.setup(Domain::Mail, &picks);
        let mail = mail.route().expect("a mail route");
        assert_eq!(groups(&[mail, route]), [vec![0, 1]]);

        let graph = Plan::new(
            &[
                api(DiscoveryService::Msgraph, "https://graph.microsoft.com/"),
                smtps("smtp.example.org", vec![grant()]),
            ],
            true,
        );
        assert_eq!(graph.setup(Domain::Sending, &picks), Setup::ThroughMail);
    }

    #[test]
    fn sending_needs_mail() {
        let plan = Plan::new(
            &[
                imaps("imap.example.org", vec![DiscoveryAuthMethod::Password]),
                smtps("smtp.example.org", vec![DiscoveryAuthMethod::Password]),
            ],
            false,
        );

        let picks = Picks {
            mail: Pick::Off,
            ..Default::default()
        };
        assert_eq!(plan.setup(Domain::Sending, &picks), Setup::Off);

        let alone = Plan::new(
            &[smtps(
                "smtp.example.org",
                vec![DiscoveryAuthMethod::Password],
            )],
            false,
        );
        assert_eq!(alone.setup(Domain::Sending, &Picks::default()), Setup::Off);
    }

    #[test]
    fn sending_signs_in_as_mail_when_its_best_server_can() {
        let plan = Plan::new(
            &[
                imaps(
                    "imap.example.org",
                    vec![grant(), DiscoveryAuthMethod::Password],
                ),
                smtps(
                    "smtp.example.org",
                    vec![grant(), DiscoveryAuthMethod::Password],
                ),
            ],
            true,
        );

        let sign_in = |picks: &Picks| {
            plan.setup(Domain::Sending, picks)
                .route()
                .map(|route| route.sign_in)
        };

        assert_eq!(sign_in(&Picks::default()), Some(SignIn::Oauth));
        assert_eq!(
            sign_in(&Picks {
                mail: Pick::Found(1),
                ..Default::default()
            }),
            Some(SignIn::Password)
        );
    }

    #[test]
    fn the_review_reads_in_plain_words() {
        let plan = Plan::new(
            &[
                imaps("imap.example.org", vec![DiscoveryAuthMethod::Password]),
                smtps("smtp.example.org", vec![DiscoveryAuthMethod::Password]),
                http(
                    DiscoveryService::Carddav,
                    "https://dav.example.org/",
                    vec![DiscoveryAuthMethod::Password],
                ),
            ],
            false,
        );
        let picks = Picks::default();
        let line = |domain| plan.line(domain, &plan.setup(domain, &picks), &picks);

        assert_eq!(
            line(Domain::Mail),
            "IMAP at imap.example.org, with your password"
        );
        assert_eq!(
            line(Domain::Sending),
            "SMTP at smtp.example.org, with mail's password"
        );
        assert_eq!(
            line(Domain::Contacts),
            "CardDAV at dav.example.org, with mail's password"
        );
        assert_eq!(line(Domain::Calendar), "Nothing found");

        let left = Picks {
            contacts: Pick::Off,
            sending: Pick::Off,
            ..Default::default()
        };
        let line = |domain| plan.line(domain, &plan.setup(domain, &left), &left);
        assert_eq!(line(Domain::Contacts), "Not set up");
        assert_eq!(line(Domain::Sending), "Not set up");

        let apis = Plan::new(&provider(), true);
        let line = |domain| apis.line(domain, &apis.setup(domain, &picks), &picks);
        assert_eq!(line(Domain::Mail), "Gmail API, through your browser");
        assert_eq!(
            line(Domain::Sending),
            "SMTP at smtp.example.org, through your browser"
        );
        assert_eq!(
            line(Domain::Calendar),
            "Google Calendar API, through your browser"
        );

        let broker = Plan::new(&provider(), false);
        assert_eq!(
            broker.line(
                Domain::Contacts,
                &broker.setup(Domain::Contacts, &picks),
                &picks
            ),
            "Google People API, with a token from a broker"
        );
    }

    #[test]
    fn a_change_names_the_server_only_between_twins() {
        let plan = Plan::new(
            &[
                imaps("imap.example.org", vec![DiscoveryAuthMethod::Password]),
                tcp(
                    DiscoveryService::Imap,
                    "imap.example.org",
                    143,
                    DiscoverySecurity::Starttls,
                    vec![DiscoveryAuthMethod::Password, DiscoveryAuthMethod::Bearer],
                ),
                smtps("smtp.example.org", vec![DiscoveryAuthMethod::Password]),
            ],
            false,
        );

        let labels = |domain| -> Vec<String> {
            plan.changes(domain, &Picks::default())
                .into_iter()
                .map(|item| item.label)
                .collect()
        };

        assert_eq!(
            labels(Domain::Mail),
            [
                "Auto (IMAP at imap.example.org, with your password)",
                "IMAP · Password · imaps://imap.example.org:993",
                "IMAP · Password · imap://imap.example.org:143",
                "IMAP · API token",
                "Custom server URL",
                "Leave out",
            ]
        );
        assert_eq!(
            labels(Domain::Sending),
            [
                "Auto (SMTP at smtp.example.org, with mail's password)",
                "SMTP · Password",
                "Custom server URL",
                "No sending",
            ]
        );

        // A domain discovery missed offers a server URL or nothing.
        assert_eq!(labels(Domain::Calendar), ["Custom server URL", "Leave out"]);
    }

    #[test]
    fn hosts_of_one_name_share_a_password() {
        assert!(same_host("imap.example.org", "smtp.example.org"));
        assert!(same_host("imap.example.org", "dav.example.org"));
        assert!(same_host("Mail.Example.org.", "mail.example.org"));

        assert!(!same_host("imap.example.org", "dav.example.net"));
        assert!(!same_host("imap.example.org", "example.org"));
        assert!(!same_host("a.org", "b.org"));
        assert!(!same_host("192.0.2.1", "192.0.2.2"));
        assert!(!same_host("[2001:db8::1]", "[2001:db8::2]"));
    }

    /// Mail, sending, contacts and calendar on servers of one name share
    /// one password; a host of another name, or another kind of secret,
    /// signs in on its own.
    #[test]
    fn passwords_group_across_the_four_domains() {
        let plan = Plan::new(
            &[
                imaps("imap.example.org", vec![DiscoveryAuthMethod::Password]),
                smtps("smtp.example.org", vec![DiscoveryAuthMethod::Password]),
                http(
                    DiscoveryService::Carddav,
                    "https://dav.example.org/card/",
                    vec![DiscoveryAuthMethod::Password],
                ),
                http(
                    DiscoveryService::Caldav,
                    "https://dav.example.org/cal/",
                    vec![DiscoveryAuthMethod::Password],
                ),
            ],
            false,
        );
        let picks = Picks::default();
        let setups: Vec<Setup> = Domain::ALL
            .into_iter()
            .map(|domain| plan.setup(domain, &picks))
            .collect();
        let routes: Vec<&Route> = setups.iter().filter_map(Setup::route).collect();
        assert_eq!(groups(&routes), [vec![0, 1, 2, 3]]);

        let elsewhere = Plan::new(
            &[
                imaps("imap.example.org", vec![DiscoveryAuthMethod::Password]),
                smtps("smtp.example.org", vec![DiscoveryAuthMethod::Password]),
                http(
                    DiscoveryService::Carddav,
                    "https://dav.example.net/",
                    vec![DiscoveryAuthMethod::Password],
                ),
                http(
                    DiscoveryService::Caldav,
                    "https://dav.example.org/",
                    vec![DiscoveryAuthMethod::Bearer],
                ),
            ],
            false,
        );
        let setups: Vec<Setup> = Domain::ALL
            .into_iter()
            .map(|domain| elsewhere.setup(domain, &picks))
            .collect();
        let routes: Vec<&Route> = setups.iter().filter_map(Setup::route).collect();
        assert_eq!(groups(&routes), [vec![0, 1], vec![2], vec![3]]);
    }
}
