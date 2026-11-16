//! # Source configuration
//!
//! One source of an account: the one backend it talks to, plus the channel
//! its sends leave by.

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::config::{
    dav::{CaldavConfig, CarddavConfig},
    gmail::GmailConfig,
    google::GoogleConfig,
    imap::ImapConfig,
    jmap::JmapConfig,
    msgraph::{MsgraphConfig, MsgraphTable},
    permissions::{
        CollectionSourceConfig, FlagSourcePermissions, ItemSourcePermissions, SourcePermissions,
    },
    proxy::ProxyConfig,
    smtp::SmtpConfig,
};

/// Generates a [`SourceConfig`] accessor forwarding to the shared field on
/// the source's backend variant, by value.
macro_rules! source_accessor {
    ($name:ident, $ty:ty) => {
        pub fn $name(&self) -> $ty {
            match &self.backend {
                SourceBackendConfig::Imap(c) => c.$name,
                SourceBackendConfig::Carddav(c) => c.$name,
                SourceBackendConfig::Caldav(c) => c.$name,
                SourceBackendConfig::Jmap(c) => c.$name,
                SourceBackendConfig::Gmail(c) => c.$name,
                SourceBackendConfig::Msgraph(c) => c.$name,
                SourceBackendConfig::MsgraphContacts(c) => c.$name,
                SourceBackendConfig::MsgraphCalendar(c) => c.$name,
                SourceBackendConfig::Gpeople(c) => c.$name,
                SourceBackendConfig::Gcal(c) => c.$name,
            }
        }
    };
}

/// [`source_accessor`] for a field too large to copy out.
macro_rules! source_ref_accessor {
    ($name:ident, $ty:ty) => {
        pub fn $name(&self) -> &$ty {
            match &self.backend {
                SourceBackendConfig::Imap(c) => &c.$name,
                SourceBackendConfig::Carddav(c) => &c.$name,
                SourceBackendConfig::Caldav(c) => &c.$name,
                SourceBackendConfig::Jmap(c) => &c.$name,
                SourceBackendConfig::Gmail(c) => &c.$name,
                SourceBackendConfig::Msgraph(c) => &c.$name,
                SourceBackendConfig::MsgraphContacts(c) => &c.$name,
                SourceBackendConfig::MsgraphCalendar(c) => &c.$name,
                SourceBackendConfig::Gpeople(c) => &c.$name,
                SourceBackendConfig::Gcal(c) => &c.$name,
            }
        }
    };
}

/// One source: the remote it talks to, plus the channel its sends leave by.
///
/// The channel belongs to the source and not the account, since sending
/// natively (Graph's `sendMail`) or needing a companion SMTP server is a
/// property of the provider. At most one source per account declares one.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", try_from = "SourceTable")]
pub struct SourceConfig {
    /// The one remote this source names.
    #[serde(flatten)]
    pub backend: SourceBackendConfig,
    /// The SMTP server this source's queued submit intents flush through.
    ///
    /// Only meaningful on a backend that cannot send by itself (IMAP), a
    /// Graph source using `sendMail` instead. Absent, and with no source
    /// sending natively, submit intents stay pending.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smtp: Option<SmtpConfig>,
}

/// A source as written, read without `flatten`, which unknown keys slip by.
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct SourceTable {
    imap: Option<ImapConfig>,
    carddav: Option<CarddavConfig>,
    caldav: Option<CaldavConfig>,
    jmap: Option<JmapConfig>,
    gmail: Option<GmailConfig>,
    msgraph: Option<MsgraphTable>,
    gpeople: Option<GoogleConfig>,
    gcal: Option<GoogleConfig>,
    smtp: Option<SmtpConfig>,
}

impl TryFrom<SourceTable> for SourceConfig {
    type Error = String;

    fn try_from(table: SourceTable) -> Result<Self, Self::Error> {
        let backends = [
            table.imap.map(SourceBackendConfig::Imap),
            table.carddav.map(SourceBackendConfig::Carddav),
            table.caldav.map(SourceBackendConfig::Caldav),
            table.jmap.map(SourceBackendConfig::Jmap),
            table.gmail.map(SourceBackendConfig::Gmail),
            table.msgraph.map(MsgraphTable::domain).transpose()?,
            table.gpeople.map(SourceBackendConfig::Gpeople),
            table.gcal.map(SourceBackendConfig::Gcal),
        ];
        let mut backends = backends.into_iter().flatten();

        let Some(backend) = backends.next() else {
            return Err(String::from(
                "A source declares no backend: write one table of `imap`, `carddav`, `caldav`, \
                 `jmap`, `gmail`, `msgraph`, `gpeople` or `gcal` under it",
            ));
        };

        if let Some(other) = backends.next() {
            return Err(format!(
                "A source declares both `{}` and `{}`, and talks to exactly one backend: move \
                 one under a source of its own",
                backend.protocol(),
                other.protocol(),
            ));
        }

        Ok(Self {
            backend,
            smtp: table.smtp,
        })
    }
}

/// The remote backend behind a source; exactly one variant per source.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceBackendConfig {
    Imap(ImapConfig),
    Carddav(CarddavConfig),
    Caldav(CaldavConfig),
    Jmap(JmapConfig),
    Gmail(GmailConfig),
    /// Microsoft Graph mail, the `mail` domain of an `msgraph` table.
    Msgraph(MsgraphConfig),
    /// Microsoft Graph contacts, the `contacts` domain of an `msgraph`
    /// table: the same session, syncing contact folders.
    #[serde(rename = "msgraph-contacts")]
    MsgraphContacts(MsgraphConfig),
    /// Microsoft Graph calendars, the `calendar` domain of an `msgraph`
    /// table: the same session, syncing calendars.
    #[serde(rename = "msgraph-calendar")]
    MsgraphCalendar(MsgraphConfig),
    /// Google contacts through the People API.
    Gpeople(GoogleConfig),
    /// Google calendars through the Calendar API.
    Gcal(GoogleConfig),
}

impl SourceBackendConfig {
    /// The proxy the backend table names, `None` when it names none.
    pub fn proxy(&self) -> Option<&ProxyConfig> {
        match self {
            Self::Imap(c) => c.proxy.as_ref(),
            Self::Carddav(c) => c.proxy.as_ref(),
            Self::Caldav(c) => c.proxy.as_ref(),
            Self::Jmap(c) => c.proxy.as_ref(),
            Self::Gmail(c) => c.proxy.as_ref(),
            Self::Msgraph(c) | Self::MsgraphContacts(c) | Self::MsgraphCalendar(c) => {
                c.proxy.as_ref()
            }
            Self::Gpeople(c) | Self::Gcal(c) => c.proxy.as_ref(),
        }
    }

    /// The backend table this backend is written under, `msgraph` for every
    /// Graph domain.
    pub fn protocol(&self) -> &'static str {
        match self {
            Self::Imap(_) => "imap",
            Self::Carddav(_) => "carddav",
            Self::Caldav(_) => "caldav",
            Self::Jmap(_) => "jmap",
            Self::Gmail(_) => "gmail",
            Self::Msgraph(_) | Self::MsgraphContacts(_) | Self::MsgraphCalendar(_) => "msgraph",
            Self::Gpeople(_) => "gpeople",
            Self::Gcal(_) => "gcal",
        }
    }

    /// The id of the source the sugar builds from this backend: its table.
    ///
    /// A Graph domain keeps the table it had before the domains shared one,
    /// so a store keeps every binding.
    pub fn sugar_name(&self) -> &'static str {
        match self {
            Self::MsgraphContacts(_) => "msgraph-contacts",
            Self::MsgraphCalendar(_) => "msgraph-calendar",
            backend => backend.protocol(),
        }
    }

    /// The Graph domain this backend syncs, `None` off Graph.
    pub fn graph_domain(&self) -> Option<&'static str> {
        match self {
            Self::Msgraph(_) => Some("mail"),
            Self::MsgraphContacts(_) => Some("contacts"),
            Self::MsgraphCalendar(_) => Some("calendar"),
            _ => None,
        }
    }

    /// The cargo feature that builds this backend, named after its table.
    pub fn feature(&self) -> &'static str {
        self.protocol()
    }
}

#[allow(dead_code)]
impl SourceConfig {
    /// Wraps a backend into a source with no send channel of its own.
    pub fn new(backend: SourceBackendConfig) -> Self {
        Self {
            backend,
            smtp: None,
        }
    }

    source_ref_accessor!(collection, CollectionSourceConfig);
    source_accessor!(flag, FlagSourcePermissions);
    source_accessor!(item, ItemSourcePermissions);
    source_accessor!(pool_size, Option<usize>);

    /// Whether this source is the IMAP backend.
    pub fn is_imap(&self) -> bool {
        matches!(self.backend, SourceBackendConfig::Imap(_))
    }

    /// Whether this source talks HTTP, those sharing a smaller default pool.
    pub fn is_http(&self) -> bool {
        matches!(
            self.backend,
            SourceBackendConfig::Jmap(_)
                | SourceBackendConfig::Gmail(_)
                | SourceBackendConfig::Msgraph(_)
                | SourceBackendConfig::MsgraphContacts(_)
                | SourceBackendConfig::MsgraphCalendar(_)
                | SourceBackendConfig::Gpeople(_)
                | SourceBackendConfig::Gcal(_)
                | SourceBackendConfig::Carddav(_)
                | SourceBackendConfig::Caldav(_)
        )
    }

    /// Whether this source sends by itself: Graph's `sendMail` today, on a
    /// mail source only.
    pub fn sends_natively(&self) -> bool {
        matches!(self.backend, SourceBackendConfig::Msgraph(_))
    }

    /// Whether this source can carry a send channel at all.
    ///
    /// A contacts or calendar source cannot: submission is a mail
    /// capability, so an `smtp` table there is an error, not a dead option.
    pub fn carries_mail(&self) -> bool {
        !matches!(
            self.backend,
            SourceBackendConfig::Carddav(_)
                | SourceBackendConfig::Caldav(_)
                | SourceBackendConfig::MsgraphContacts(_)
                | SourceBackendConfig::MsgraphCalendar(_)
                | SourceBackendConfig::Gpeople(_)
                | SourceBackendConfig::Gcal(_)
        )
    }

    /// Whether a body streams straight from this source to another rather
    /// than being staged on the way.
    ///
    /// Only an IMAP pairing can, so this gates [`AccountMode::streams`],
    /// an optimisation of the declared `retain` and never a mode of its own.
    ///
    /// [`AccountMode::streams`]: crate::config::mode::AccountMode::streams
    pub fn is_streamable(&self) -> bool {
        self.is_imap()
    }

    /// Rejects a source whose declared options its backend cannot honour.
    pub fn validate(&self, name: &str) -> Result<()> {
        if self.smtp.is_some() && !self.carries_mail() {
            bail!(
                "The `sources.{name}.smtp` channel is a mail capability and this source syncs \
                 contacts; drop the table, or move it to the source that sends."
            );
        }

        Ok(())
    }

    /// Snapshots the per-source collection/flag/item permissions.
    pub fn permissions(&self) -> SourcePermissions {
        SourcePermissions {
            collection: self.collection().permissions(),
            flag: self.flag(),
            item: self.item(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::AccountConfig;

    #[test]
    fn a_source_pairs_one_backend_with_its_send_channel() {
        let account: AccountConfig = toml::from_str(
            r#"
            msgraph.auth.token.raw = "tok"
            msgraph.mail = true
            "#,
        )
        .unwrap();
        let sources = account.sources().unwrap();
        assert!(sources["msgraph"].sends_natively());
        assert!(sources["msgraph"].smtp.is_none());

        let err = toml::from_str::<AccountConfig>(
            r#"
            sources.a.imapp.server = "imaps://imap.example.org:993"
            "#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("unknown field `imapp`"), "{err}");

        let account: AccountConfig = toml::from_str(
            r#"
            sources.a.imap.server = "imaps://imap.example.org:993"
            sources.a.smtp.server = "smtps://smtp.example.org:465"
            "#,
        )
        .unwrap();
        let source = &account.sources().unwrap()["a"];
        assert!(source.is_imap());
        assert!(source.smtp.is_some());
    }

    /// Each DAV table is built by the cargo feature of its own name, so a
    /// refusal names the one to rebuild with.
    #[test]
    fn each_dav_table_names_its_own_feature() {
        let account: AccountConfig = toml::from_str(
            r#"
            carddav.server = "dav.example.org"
            carddav.auth.bearer.token.raw = "tok"
            caldav.server = "dav.example.org"
            caldav.auth.bearer.token.raw = "tok"
            "#,
        )
        .unwrap();

        let sources = account.sources().unwrap();
        for name in ["carddav", "caldav"] {
            assert_eq!(sources[name].backend.feature(), name);
        }
    }

    #[test]
    fn a_graph_contacts_source_is_neither_mail_nor_a_send_channel() {
        let account: AccountConfig = toml::from_str(
            r#"
            msgraph.auth.token.raw = "tok"
            msgraph.mail = true
            msgraph.contacts = true
            msgraph.calendar = true
            sources.work.msgraph.auth.token.raw = "tok"
            sources.work.msgraph.contacts = true
            "#,
        )
        .unwrap();

        let sources = account.sources().unwrap();
        assert!(sources["msgraph"].sends_natively());
        for name in ["msgraph-contacts", "msgraph-calendar", "work"] {
            assert!(!sources[name].carries_mail(), "{name} does not submit");
            assert!(!sources[name].sends_natively(), "{name} does not send");
        }
    }

    #[test]
    fn a_dav_source_carries_no_send_channel() {
        let account: AccountConfig = toml::from_str(
            r#"
            carddav.server = "https://dav.example.org/"
            carddav.auth.basic.username = "user"
            carddav.auth.basic.password.raw = "pw"
            caldav.server = "https://dav.example.org/"
            caldav.auth.basic.username = "user"
            caldav.auth.basic.password.raw = "pw"
            "#,
        )
        .unwrap();

        let sources = account.sources().unwrap();
        for name in ["carddav", "caldav"] {
            assert!(!sources[name].carries_mail(), "{name} does not submit");
            assert!(!sources[name].sends_natively());
        }
        account.validate().unwrap();

        let account: AccountConfig = toml::from_str(
            r#"
            sources.dav.carddav.server = "https://dav.example.org/"
            sources.dav.carddav.auth.bearer.token.raw = "tok"
            sources.dav.smtp.server = "smtps://smtp.example.org:465"
            "#,
        )
        .unwrap();

        let err = account.validate().unwrap_err().to_string();
        assert!(err.contains("`sources.dav.smtp`"), "got {err}");
    }

    /// A source is one backend: a second one is refused, never dropped.
    #[test]
    fn a_source_declaring_two_backends_is_refused() {
        let err = toml::from_str::<AccountConfig>(
            r#"
            sources.a.imap.server = "imaps://imap.example.org:993"
            sources.a.msgraph.auth.token.raw = "tok"
            sources.a.msgraph.mail = true
            "#,
        )
        .map(|_| String::from("loaded"))
        .unwrap_or_else(|err| err.to_string());
        assert!(err.contains("`imap` and `msgraph`"), "{err}");

        let err = toml::from_str::<AccountConfig>("sources.a.smtp.server = \"x\"")
            .map(|_| String::from("loaded"))
            .unwrap_or_else(|err| err.to_string());
        assert!(err.contains("declares no backend"), "{err}");
    }
}
