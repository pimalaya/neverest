//! # Capabilities
//!
//! What each source declares it can do (pimdir STORAGE §15.6), derived
//! from its backend and its configured rights alone, so a declaration
//! needs no network. JMAP sources stay undeclared, which gates nothing.

use io_pimdir::capability::{
    CALENDAR, CALENDAR_CANCEL, CALENDAR_OCCURRENCE_UPDATE, CALENDAR_ONLINE_MEETING, CALENDAR_REPLY,
    CALENDAR_SCHEDULING, CONTACTS, CONTACTS_CARD_COPY, CONTACTS_CARD_MOVE, MAIL,
    MAIL_FLAGS_ANSWERED, MAIL_FLAGS_DRAFT, MAIL_FLAGS_FLAGGED, MAIL_FLAGS_KEYWORDS,
    MAIL_FLAGS_SEEN, MAIL_MESSAGE_ADD, MAIL_MESSAGE_COPY, MAIL_MESSAGE_MOVE, MAIL_MESSAGE_REMOVE,
    MAIL_SUBMIT, MAIL_SUBMIT_COPY, PimdirCapability, PimdirSupport,
};

use crate::config::{SourceBackendConfig, SourceConfig};

/// A backend's support of one capability, its rights aside.
type Support = (PimdirSupport, Option<&'static str>);

/// A backend's support of each capability by name, its rights aside, and
/// whether the source has an `smtp` table.
type Backend = fn(&str, bool) -> Support;

/// What neverest does not perform yet for any calendar backend.
const NOT_PERFORMED: Support = (
    PimdirSupport::None,
    Some("neverest does not perform it yet"),
);

/// Why a source-wide row refuses an intent its source performs on the
/// calendars it holds.
const HELD_ONLY: &str = "performed on the calendars this source syncs only";

/// The declaration of one source, every capability of its kind included,
/// `none` ones with their reason; empty for a source left undeclared.
///
/// `collections` are the store collections the source syncs. An intent the
/// source performs through its provider's own verbs reaches only the items
/// it holds, so it is declared on each of them and `none` source-wide
/// (pimdir STORAGE §15.6 Implementations).
pub fn declaration(
    source: &SourceConfig,
    one_way: bool,
    collections: &[String],
) -> Vec<PimdirCapability> {
    let (names, backend): (&[&str], Backend) = match &source.backend {
        SourceBackendConfig::Imap(_) => (MAIL, imap),
        SourceBackendConfig::Gmail(_) => (MAIL, gmail),
        SourceBackendConfig::Msgraph(_) => (MAIL, msgraph),
        SourceBackendConfig::Carddav(_) => (CONTACTS, carddav),
        SourceBackendConfig::Gpeople(_) => (CONTACTS, gpeople),
        SourceBackendConfig::MsgraphContacts(_) => (CONTACTS, msgraph_contacts),
        SourceBackendConfig::Caldav(_) => (CALENDAR, caldav),
        SourceBackendConfig::Gcal(_) => (CALENDAR, gcal),
        SourceBackendConfig::MsgraphCalendar(_) => (CALENDAR, msgraph_calendar),
        SourceBackendConfig::Jmap(_) => return Vec::new(),
    };
    let perms = source.permissions();
    let smtp = source.smtp.is_some();

    names
        .iter()
        .flat_map(|&name| {
            let (create, delete) = (perms.item.create, perms.item.delete);
            let refused = match name {
                MAIL_SUBMIT | MAIL_SUBMIT_COPY | CALENDAR_REPLY | CALENDAR_CANCEL => None,
                _ if one_way => Some("one-way copy: the server wins"),
                MAIL_FLAGS_SEEN | MAIL_FLAGS_FLAGGED | MAIL_FLAGS_ANSWERED | MAIL_FLAGS_DRAFT
                | MAIL_FLAGS_KEYWORDS
                    if !perms.flag.update =>
                {
                    Some("flag.update is disabled")
                }
                _ if name.ends_with(".move") && !(create && delete) => {
                    Some("item.create or item.delete is disabled")
                }
                _ if (name.ends_with(".add") || name.ends_with(".copy")) && !create => {
                    Some("item.create is disabled")
                }
                _ if name.ends_with(".remove") && !delete => Some("item.delete is disabled"),
                _ if name.ends_with(".update") && !perms.item.update => {
                    Some("item.update is disabled")
                }
                _ => None,
            };
            let (support, detail) = match refused {
                Some(why) => (PimdirSupport::None, Some(why)),
                None => backend(name, smtp),
            };

            let row =
                |collection: Option<&String>, support, detail: Option<&str>| PimdirCapability {
                    collection: collection.cloned(),
                    name: name.to_string(),
                    support,
                    detail: detail.map(String::from),
                };
            let held_only =
                matches!(name, CALENDAR_REPLY | CALENDAR_CANCEL) && support != PimdirSupport::None;
            match held_only {
                false => vec![row(None, support, detail)],
                true => std::iter::once(row(None, PimdirSupport::None, Some(HELD_ONLY)))
                    .chain(
                        collections
                            .iter()
                            .map(|collection| row(Some(collection), support, detail)),
                    )
                    .collect(),
            }
        })
        .collect()
}

/// Whether the provider a source sends through files the sent message
/// itself, so the copy a `submit` asks for needs no `add` of its own.
pub fn files_sent_copy(source: &SourceConfig) -> bool {
    matches!(
        source.backend,
        SourceBackendConfig::Gmail(_) | SourceBackendConfig::Msgraph(_)
    )
}

fn imap(name: &str, smtp: bool) -> Support {
    match name {
        MAIL_FLAGS_KEYWORDS => (
            PimdirSupport::Partial,
            Some("as far as the server's PERMANENTFLAGS allow"),
        ),
        MAIL_SUBMIT | MAIL_SUBMIT_COPY if !smtp => (PimdirSupport::None, Some("no smtp table")),
        _ => (PimdirSupport::Full, None),
    }
}

fn gmail(name: &str, smtp: bool) -> Support {
    match name {
        MAIL_MESSAGE_REMOVE => (
            PimdirSupport::Partial,
            Some("removes the label, permanent from TRASH only"),
        ),
        MAIL_FLAGS_ANSWERED => (PimdirSupport::None, Some("Gmail has no answered label")),
        MAIL_FLAGS_DRAFT => (
            PimdirSupport::None,
            Some("Gmail drafts are the DRAFT label, not a flag"),
        ),
        MAIL_FLAGS_KEYWORDS => (PimdirSupport::Partial, Some("$Important only")),
        MAIL_SUBMIT | MAIL_SUBMIT_COPY if !smtp => (
            PimdirSupport::None,
            Some("no smtp table: Gmail sends through SMTP here"),
        ),
        MAIL_SUBMIT => (
            PimdirSupport::Full,
            Some("Gmail files every sent message in SENT, asked or not"),
        ),
        MAIL_SUBMIT_COPY => (PimdirSupport::Full, Some("Gmail files it in SENT itself")),
        _ => (PimdirSupport::Full, None),
    }
}

fn msgraph(name: &str, _smtp: bool) -> Support {
    match name {
        MAIL_MESSAGE_ADD | MAIL_MESSAGE_COPY => (
            PimdirSupport::None,
            Some("Graph mail is pull-only: no append"),
        ),
        MAIL_MESSAGE_MOVE => (
            PimdirSupport::None,
            Some("Graph mail is pull-only: no move"),
        ),
        MAIL_MESSAGE_REMOVE => (
            PimdirSupport::Partial,
            Some("a soft delete, out of sight, not into Deleted Items"),
        ),
        MAIL_FLAGS_ANSWERED | MAIL_FLAGS_DRAFT => (
            PimdirSupport::None,
            Some("Graph takes isRead and flag only"),
        ),
        MAIL_FLAGS_KEYWORDS => (PimdirSupport::None, Some("Graph categories are not mapped")),
        MAIL_SUBMIT => (
            PimdirSupport::Full,
            Some("Graph files every sent message in Sent Items, asked or not"),
        ),
        MAIL_SUBMIT_COPY => (
            PimdirSupport::Full,
            Some("Graph files it in Sent Items itself"),
        ),
        _ => (PimdirSupport::Full, None),
    }
}

fn carddav(_name: &str, _smtp: bool) -> Support {
    (PimdirSupport::Full, None)
}

fn gpeople(name: &str, _smtp: bool) -> Support {
    match name {
        CONTACTS_CARD_MOVE | CONTACTS_CARD_COPY => (
            PimdirSupport::None,
            Some("Google People has a single address book"),
        ),
        _ => (PimdirSupport::Full, None),
    }
}

fn msgraph_contacts(_name: &str, _smtp: bool) -> Support {
    (PimdirSupport::Full, None)
}

fn caldav(name: &str, _smtp: bool) -> Support {
    match name {
        CALENDAR_SCHEDULING => (
            PimdirSupport::Partial,
            Some("the server's own scheduling (RFC 6638), when it has one"),
        ),
        CALENDAR_ONLINE_MEETING => (PimdirSupport::None, Some("CalDAV has no online meeting")),
        CALENDAR_REPLY => (
            PimdirSupport::None,
            Some(
                "CalDAV has no verb: an update of the account's PARTSTAT replies through the server's scheduling (RFC 6638)",
            ),
        ),
        CALENDAR_CANCEL => (
            PimdirSupport::None,
            Some("CalDAV has no verb: a remove cancels through the server's scheduling (RFC 6638)"),
        ),
        _ => (PimdirSupport::Full, None),
    }
}

fn gcal(name: &str, _smtp: bool) -> Support {
    match name {
        CALENDAR_OCCURRENCE_UPDATE => (
            PimdirSupport::None,
            Some("a modified instance does not push yet"),
        ),
        CALENDAR_SCHEDULING => (
            PimdirSupport::Partial,
            Some("Google notifies every attendee, SCHEDULE-AGENT aside"),
        ),
        CALENDAR_CANCEL => (
            PimdirSupport::Partial,
            Some("Google sends its own cancellation, without the comment"),
        ),
        CALENDAR_ONLINE_MEETING => NOT_PERFORMED,
        _ => (PimdirSupport::Full, None),
    }
}

fn msgraph_calendar(name: &str, _smtp: bool) -> Support {
    match name {
        CALENDAR_OCCURRENCE_UPDATE => (
            PimdirSupport::None,
            Some("an edited exception does not push yet"),
        ),
        CALENDAR_SCHEDULING => (
            PimdirSupport::Partial,
            Some("Graph notifies every attendee, SCHEDULE-AGENT aside"),
        ),
        CALENDAR_ONLINE_MEETING => NOT_PERFORMED,
        _ => (PimdirSupport::Full, None),
    }
}

#[cfg(test)]
mod tests {
    use io_pimdir::capability::CALENDAR_ITEM_ADD;

    use super::*;
    use crate::config::AccountConfig;

    fn source(toml: &str, name: &str) -> SourceConfig {
        let account: AccountConfig = toml::from_str(toml).unwrap();
        account.sources().unwrap().remove(name).unwrap()
    }

    fn caldav() -> SourceConfig {
        source(
            "caldav.server = \"https://dav.example.org/\"\n\
             caldav.auth.basic.username = \"user\"\n\
             caldav.auth.basic.password.raw = \"pw\"",
            "caldav",
        )
    }

    fn gcal() -> SourceConfig {
        source("gcal.auth.token.raw = \"token\"", "gcal")
    }

    fn msgraph_calendar() -> SourceConfig {
        source(
            "msgraph-calendar.auth.token.raw = \"token\"",
            "msgraph-calendar",
        )
    }

    /// The row of `name` on `collection`, `None` for the source-wide one.
    fn row<'a>(
        declaration: &'a [PimdirCapability],
        name: &str,
        collection: Option<&str>,
    ) -> &'a PimdirCapability {
        declaration
            .iter()
            .find(|row| row.name == name && row.collection.as_deref() == collection)
            .unwrap()
    }

    fn support(declaration: &[PimdirCapability], name: &str) -> PimdirSupport {
        row(declaration, name, None).support
    }

    #[test]
    fn a_calendar_source_declares_every_calendar_capability() {
        let source = caldav();
        let declaration = declaration(&source, false, &["caldav/work".into()]);

        assert_eq!(declaration.len(), CALENDAR.len());
        assert_eq!(
            support(&declaration, CALENDAR_SCHEDULING),
            PimdirSupport::Partial
        );
        assert_eq!(
            support(&declaration, CALENDAR_ONLINE_MEETING),
            PimdirSupport::None
        );
    }

    #[test]
    fn a_one_way_source_refuses_writes_but_not_intents() {
        let collections = ["gcal/primary".to_string()];
        let declaration = declaration(&gcal(), true, &collections);

        assert_eq!(
            support(&declaration, CALENDAR_ITEM_ADD),
            PimdirSupport::None
        );
        assert_eq!(
            row(&declaration, CALENDAR_REPLY, Some("gcal/primary")).support,
            PimdirSupport::Full
        );
    }

    #[test]
    fn a_caldav_source_points_its_intents_at_scheduling() {
        let declaration = declaration(&caldav(), false, &["caldav/work".into()]);

        for name in [CALENDAR_REPLY, CALENDAR_CANCEL] {
            let row = row(&declaration, name, None);
            assert_eq!(row.support, PimdirSupport::None);
            assert!(row.detail.as_deref().unwrap().contains("RFC 6638"));
        }
        assert!(declaration.iter().all(|row| row.collection.is_none()));
    }

    #[test]
    fn a_provider_performs_its_intents_on_the_calendars_it_holds() {
        let collections = ["gcal/primary".to_string(), "gcal/team".to_string()];
        let declaration = declaration(&gcal(), false, &collections);

        assert_eq!(declaration.len(), CALENDAR.len() + 2 * collections.len());
        for name in [CALENDAR_REPLY, CALENDAR_CANCEL] {
            let wide = row(&declaration, name, None);
            assert_eq!(wide.support, PimdirSupport::None);
            assert_eq!(wide.detail.as_deref(), Some(HELD_ONLY));
        }
        for collection in &collections {
            let collection = Some(collection.as_str());
            assert_eq!(
                row(&declaration, CALENDAR_REPLY, collection).support,
                PimdirSupport::Full
            );
            assert_eq!(
                row(&declaration, CALENDAR_CANCEL, collection).support,
                PimdirSupport::Partial
            );
        }
        assert_eq!(
            support(&declaration, CALENDAR_SCHEDULING),
            PimdirSupport::Partial
        );
    }

    #[test]
    fn graph_performs_both_intents_fully() {
        let collections = ["msgraph-calendar/AAMk".to_string()];
        let declaration = declaration(&msgraph_calendar(), false, &collections);

        for name in [CALENDAR_REPLY, CALENDAR_CANCEL] {
            assert_eq!(support(&declaration, name), PimdirSupport::None);
            assert_eq!(
                row(&declaration, name, Some("msgraph-calendar/AAMk")).support,
                PimdirSupport::Full
            );
        }
    }

    #[test]
    fn a_source_with_no_calendar_yet_declares_its_intents_nowhere() {
        let declaration = declaration(&gcal(), false, &[]);

        assert_eq!(declaration.len(), CALENDAR.len());
        assert_eq!(support(&declaration, CALENDAR_REPLY), PimdirSupport::None);
    }
}
