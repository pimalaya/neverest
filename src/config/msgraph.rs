//! # Microsoft Graph configuration
//!
//! The `msgraph` table: one Graph session and the mail, contact and
//! calendar domains it syncs, each a source of its own.

use std::fmt;

use pimalaya_config::secret::Secret;
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, MapAccess, Visitor, value::MapAccessDeserializer},
};

use crate::config::{
    default_http_alpn, is_default, is_default_http_alpn,
    permissions::{CollectionSourceConfig, FlagSourcePermissions, ItemSourcePermissions},
    proxy::ProxyConfig,
    source::SourceBackendConfig,
    tls::TlsConfig,
};

source_config! {
    /// One domain of a Microsoft Graph source: the session of its `msgraph`
    /// table with the domain's own keys, built by [`MsgraphTable::domains`].
    ///
    /// The API host is fixed, so only the mailbox owner, TLS and the
    /// OAuth 2.0 credential are configurable.
    #[derive(Clone, Debug, Serialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct MsgraphConfig {
        /// Graph user id, `me` by default: the authenticated user.
        #[serde(default = "default_msgraph_user_id")]
        pub user_id: String,
        /// How the connection is secured.
        #[serde(default)]
        pub tls: TlsConfig,
        /// ALPN identifiers offered during the TLS handshake, `["http/1.1"]`
        /// by default; `[]` skips ALPN.
        #[serde(
            default = "default_http_alpn",
            skip_serializing_if = "is_default_http_alpn"
        )]
        /// ALPN identifiers offered during the TLS handshake.
        pub alpn: Vec<String>,
        /// How the Graph session authenticates.
        pub auth: MsgraphAuthConfig,
        /// The domain's connection pool size, its `pool-size`.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub pool_size: Option<usize>,
    }
}

/// Microsoft Graph authentication: OAuth 2.0 bearer tokens only, neverest
/// never running an OAuth flow itself.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MsgraphAuthConfig {
    /// OAuth 2.0 bearer token, the `Bearer ` prefix added by the client.
    ///
    /// Acquiring and refreshing it is the caller's job: point
    /// `token.command` at any command printing a valid token, ortie say.
    pub token: Secret,
}

/// The refusal of an `msgraph` table naming no domain.
const MSGRAPH_NO_DOMAIN: &str = "The `msgraph` table names no domain: write `msgraph.mail = \
                                 true` to sync mail, `msgraph.contact` or `msgraph.calendar` for \
                                 the others";

/// The `msgraph` table: one Graph session and the domains it syncs, each
/// domain a source of its own.
///
/// Graph carries mail, contacts and calendars behind one host and token,
/// so the session is written once; what a domain syncs and may write sits
/// in its own table.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MsgraphTable {
    /// Graph user id, `me` by default: the authenticated user.
    #[serde(default = "default_msgraph_user_id")]
    pub user_id: String,
    /// How the connection is secured.
    #[serde(default)]
    pub tls: TlsConfig,
    /// ALPN identifiers offered during the TLS handshake, `["http/1.1"]` by
    /// default; `[]` skips ALPN.
    #[serde(
        default = "default_http_alpn",
        skip_serializing_if = "is_default_http_alpn"
    )]
    pub alpn: Vec<String>,
    /// How the Graph session authenticates.
    pub auth: MsgraphAuthConfig,
    /// The proxy every domain's connections go through.
    ///
    /// Falls back to the account's, then to the `all_proxy` and
    /// `https_proxy` environment variables.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy: Option<ProxyConfig>,
    /// The mail folders, the source `msgraph-mail` under the account.
    #[serde(
        default,
        deserialize_with = "msgraph_domain",
        skip_serializing_if = "Option::is_none"
    )]
    pub mail: Option<MsgraphDomainConfig>,
    /// The contact folders, the source `msgraph-contact` under the
    /// account.
    #[serde(
        default,
        deserialize_with = "msgraph_domain",
        skip_serializing_if = "Option::is_none"
    )]
    pub contact: Option<MsgraphDomainConfig>,
    /// The calendars, the source `msgraph-calendar` under the account.
    #[serde(
        default,
        deserialize_with = "msgraph_domain",
        skip_serializing_if = "Option::is_none"
    )]
    pub calendar: Option<MsgraphDomainConfig>,
}

impl MsgraphTable {
    /// One source backend a domain the table names, mail first, each with
    /// the table's session.
    pub fn domains(&self) -> Vec<SourceBackendConfig> {
        let source = |domain: &MsgraphDomainConfig| MsgraphConfig {
            user_id: self.user_id.clone(),
            tls: self.tls.clone(),
            alpn: self.alpn.clone(),
            auth: self.auth.clone(),
            collection: domain.collection.clone(),
            flag: domain.flag,
            item: domain.item,
            pool_size: domain.pool_size,
            proxy: self.proxy.clone(),
        };

        let mail = self.mail.as_ref().map(source);
        let contact = self.contact.as_ref().map(source);
        let calendar = self.calendar.as_ref().map(source);
        [
            mail.map(SourceBackendConfig::Msgraph),
            contact.map(SourceBackendConfig::MsgraphContacts),
            calendar.map(SourceBackendConfig::MsgraphCalendar),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    /// The one domain a named source's table names: a source is one
    /// pimdir source id, of one kind.
    pub fn domain(self) -> Result<SourceBackendConfig, String> {
        let mut domains = self.domains();
        if domains.len() == 1 {
            return Ok(domains.remove(0));
        }

        let mut names: Vec<String> = domains
            .iter()
            .filter_map(SourceBackendConfig::graph_domain)
            .map(|name| format!("`{name}`"))
            .collect();
        let Some(last) = names.pop() else {
            return Err(MSGRAPH_NO_DOMAIN.to_owned());
        };
        Err(format!(
            "A source's `msgraph` table names {} and {last}, and a source syncs one domain: \
             give each a source of its own, or write `msgraph` under the account, which names \
             each after its domain",
            names.join(", "),
        ))
    }
}

/// What one Graph domain syncs and may write: the keys every backend table
/// holds beside its connection.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MsgraphDomainConfig {
    /// Which collections the domain syncs, and what may be created, renamed
    /// or deleted on it.
    #[serde(default, skip_serializing_if = "is_default")]
    pub collection: CollectionSourceConfig,
    /// Whether flag updates may be pushed.
    #[serde(default, skip_serializing_if = "is_default")]
    pub flag: FlagSourcePermissions,
    /// Which item mutations may be pushed.
    #[serde(default, skip_serializing_if = "is_default")]
    pub item: ItemSourcePermissions,
    /// Connection pool size override, 3 by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pool_size: Option<usize>,
}

/// Reads a Graph domain written `true`, taking its defaults, or as a
/// table; `false` reads as absent.
fn msgraph_domain<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<MsgraphDomainConfig>, D::Error> {
    struct Domain;

    impl<'de> Visitor<'de> for Domain {
        type Value = Option<MsgraphDomainConfig>;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("`true`, `false` or a table")
        }

        fn visit_bool<E: de::Error>(self, synced: bool) -> Result<Self::Value, E> {
            Ok(synced.then(MsgraphDomainConfig::default))
        }

        fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
            MsgraphDomainConfig::deserialize(MapAccessDeserializer::new(map)).map(Some)
        }
    }

    deserializer.deserialize_any(Domain)
}

/// Reads the account's `msgraph` table, refusing one naming no domain.
pub fn msgraph_sugar<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<MsgraphTable>, D::Error> {
    let table = MsgraphTable::deserialize(deserializer)?;
    if table.domains().is_empty() {
        return Err(de::Error::custom(MSGRAPH_NO_DOMAIN));
    }
    Ok(Some(table))
}

fn default_msgraph_user_id() -> String {
    String::from("me")
}

#[cfg(test)]
mod tests {
    use crate::config::{AccountConfig, filter::CollectionFilter, msgraph::*};

    #[test]
    fn msgraph_auth_is_bearer_token_only() {
        let config: MsgraphTable = toml::from_str(
            r#"
            auth.token.raw = "tok"
            "#,
        )
        .unwrap();
        assert_eq!(config.user_id, "me");

        let config: MsgraphTable = toml::from_str(
            r#"
            user-id = "user@example.org"
            auth.token.command = ["ortie", "-a", "msgraph", "token", "show", "--auto-refresh"]
            "#,
        )
        .unwrap();
        assert_eq!(config.user_id, "user@example.org");

        let err = toml::from_str::<MsgraphTable>(
            r#"
            [auth.device-code]
            client-id = "id"
            "#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("device-code"));
    }

    /// A domain is `true`, taking its defaults, or a table of its keys;
    /// `false` is no domain. Each carries the one session.
    #[test]
    fn a_graph_domain_is_true_or_a_table() {
        let account: AccountConfig = toml::from_str(
            r#"
            msgraph.auth.token.raw = "tok"
            msgraph.user-id = "user@example.org"
            msgraph.mail = true
            msgraph.contact.collection.filter.include = ["AAMk"]
            msgraph.contact.pool-size = 2
            msgraph.calendar = false
            "#,
        )
        .unwrap();
        let sources = account.sources().unwrap();

        let mut ids: Vec<&String> = sources.keys().collect();
        ids.sort();
        assert_eq!(ids, ["msgraph-contact", "msgraph-mail"]);
        let contacts = &sources["msgraph-contact"];
        assert_eq!(
            contacts.collection().filter,
            CollectionFilter::Include(vec![String::from("AAMk")])
        );
        assert_eq!(contacts.pool_size(), Some(2));
        assert_eq!(
            sources["msgraph-mail"].collection().filter,
            CollectionFilter::All
        );
        for source in sources.values() {
            let (SourceBackendConfig::Msgraph(graph) | SourceBackendConfig::MsgraphContacts(graph)) =
                &source.backend
            else {
                panic!("a Graph source");
            };
            assert_eq!(graph.user_id, "user@example.org");
        }
        assert_eq!(account.backends(), ["msgraph"]);

        let err = toml::from_str::<AccountConfig>(
            "msgraph.auth.token.raw = \"tok\"\nmsgraph.contact.collection-typo = 1",
        )
        .map(|_| String::from("loaded"))
        .unwrap_or_else(|err| err.to_string());
        assert!(err.contains("unknown field `collection-typo`"), "{err}");
    }

    /// An `msgraph` table naming no domain is refused, every v0.3 mail-only
    /// table among them, and a named source holds one domain.
    #[test]
    fn a_graph_table_names_its_domains() {
        for document in [
            "msgraph.auth.token.raw = \"tok\"",
            "msgraph.auth.token.raw = \"tok\"\nmsgraph.mail = false",
            "sources.a.msgraph.auth.token.raw = \"tok\"",
        ] {
            let err = toml::from_str::<AccountConfig>(document)
                .map(|_| String::from("loaded"))
                .unwrap_or_else(|err| err.to_string());
            assert!(
                err.contains("The `msgraph` table names no domain: write `msgraph.mail = true`"),
                "{document}: {err}"
            );
        }

        let err = toml::from_str::<AccountConfig>(
            "sources.a.msgraph.auth.token.raw = \"tok\"\nsources.a.msgraph.mail = true\n\
             sources.a.msgraph.contact = true\nsources.a.msgraph.calendar = true",
        )
        .map(|_| String::from("loaded"))
        .unwrap_or_else(|err| err.to_string());
        assert!(
            err.contains(
                "A source's `msgraph` table names `mail`, `contact` and `calendar`, and a source \
                 syncs one domain"
            ),
            "{err}"
        );
    }
}
