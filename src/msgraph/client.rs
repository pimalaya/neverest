//! # Microsoft Graph client
//!
//! [`GraphClient`] wraps the std blocking io-msgraph client behind the same
//! adapter surface as the IMAP backend, with the folder name map, the delta row
//! cache serving the `Meta` tier and stream reopens.
//!
//! `enumerate` drives the messages delta query, carrying the `@odata.deltaLink`
//! as the engine's opaque checkpoint (HTTP 410 means an expired link and
//! restarts a fresh full round). Folders are listed two levels deep, named
//! `Parent/Child`; deeper nesting is not replicated.
//!
//! Push scope is honest: flag changes and deletes push, appends, moves and
//! mailbox mutations are rejected, so a mirror with a Graph side propagates
//! flags and deletions but no new message into Graph.
//!
//! Graph message ids are mutable across folder moves (no immutable-id support
//! yet), so a moved message surfaces as a removal plus an addition. A delta
//! reset never changes handle identity, so no handle-space rebuild follows,
//! unlike the IMAP UIDVALIDITY path.

use std::{collections::HashMap, io::Write, time::Duration};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, FixedOffset};
use io_msgraph::v1::{
    client::{MsgraphClientStd, MsgraphClientStdConnectOptions, MsgraphClientStdError},
    rest::users::{
        mail_folders::{
            MsgraphMailFolder,
            list::{MsgraphMailFoldersListParams, MsgraphMailFoldersListResponse},
        },
        messages::{
            MsgraphFlagStatus, MsgraphFollowupFlag, MsgraphMessage,
            delta::{MsgraphMessageDelta, MsgraphMessagesDeltaResponse},
        },
    },
    send::{MSGRAPH_API_BASE, MsgraphSend, MsgraphSendOutput},
};
use log::{debug, trace, warn};
use pimalaya_stream::{
    stream::{Stream, TlsConnectOptions},
    tls::Tls,
};
use secrecy::{ExposeSecret, SecretString};
use url::Url;

use crate::{
    client::{EnumEntry, Enumeration},
    item::{
        collection::Collection,
        flag::{Flag, FlagOp, IanaFlag},
        summary::{ItemSummary, normalize_message_id},
    },
};

/// The `$select` projection of the delta query: the envelope fields the meta
/// summary and the flag mapping need, so delta pages stay small.
const DELTA_SELECT: &str = "id,subject,from,toRecipients,receivedDateTime,internetMessageId,isRead,isDraft,flag,parentFolderId";

/// The page size requested when listing mail folders.
const FOLDER_PAGE_SIZE: u32 = 100;

/// The live Microsoft Graph session of one side.
pub struct GraphClient {
    inner: MsgraphClientStd,
    /// The TLS configuration, kept for stream reopens.
    tls: Tls,
    /// Folder display name (or `Parent/Child` path) to folder id, refreshed
    /// from the folder listing when a name misses.
    folders: HashMap<String, String>,
    /// The delta rows of the last enumerations, keyed by collection then
    /// handle, serving the `Meta` tier without re-fetching.
    rows: HashMap<String, HashMap<String, MsgraphMessage>>,
    /// Whether the server allowed reusing the stream after the last exchange;
    /// when false the next operation reopens it.
    alive: bool,
}

impl GraphClient {
    /// Opens the TLS connection to the Graph API with the given bearer token,
    /// scoped to the `user` mailbox owner (`me` or a user id).
    pub fn connect(token: &SecretString, user: &str, tls: Tls) -> Result<Self> {
        let options = MsgraphClientStdConnectOptions {
            tls: tls.clone(),
            user_id: user.to_owned(),
        };
        let inner = MsgraphClientStd::connect(token.expose_secret(), options)
            .context("Cannot connect to Microsoft Graph")?;

        Ok(Self {
            inner,
            tls,
            folders: HashMap::new(),
            rows: HashMap::new(),
            alive: true,
        })
    }

    /// Reopens the stream to the Graph API endpoint, keeping the credential.
    fn reconnect(&mut self) -> Result<()> {
        debug!("reopening the graph stream");

        let url = Url::parse(MSGRAPH_API_BASE).context("Cannot parse the Graph API base URL")?;
        let host = url.host_str().context("Graph API base URL has no host")?;
        let opts = TlsConnectOptions {
            tls: self.tls.clone(),
            ..Default::default()
        };
        let stream = Stream::connect_tls(host, url.port().unwrap_or(443), opts)?;
        stream.set_read_timeout(Some(Duration::from_secs(30)))?;

        self.inner.set_stream(stream);
        self.alive = true;
        Ok(())
    }

    /// Runs one Graph operation, reopening the stream first when the server
    /// closed it, and records the new keep-alive hint.
    fn op<T>(
        &mut self,
        run: impl FnOnce(&mut MsgraphClientStd) -> Result<MsgraphSendOutput<T>, MsgraphClientStdError>,
    ) -> Result<T, MsgraphClientStdError> {
        if !self.alive {
            self.reconnect().map_err(MsgraphClientStdError::Tls)?;
        }

        let out = run(&mut self.inner)?;
        self.alive = out.keep_alive;
        Ok(out.response)
    }

    /// Lists the synced mail folders as shared mailboxes, every top-level
    /// folder plus one level of children named `Parent/Child`.
    ///
    /// The name map is refreshed as a side effect; counts are not populated.
    pub fn list_mailboxes(&mut self, _with_counts: bool) -> Result<Vec<Collection>> {
        Ok(self
            .list_folders()?
            .into_iter()
            .map(|(name, _)| Collection {
                id: name.clone(),
                name,
                total: None,
                unread: None,
            })
            .collect())
    }

    /// Lists the replicated folder names with their ids, refreshing the name
    /// map as a side effect.
    fn list_folders(&mut self) -> Result<Vec<(String, String)>> {
        debug!("begin graph folder listing");

        let params = MsgraphMailFoldersListParams {
            top: Some(FOLDER_PAGE_SIZE),
            ..Default::default()
        };
        let mut entries = Vec::new();
        let mut parents = Vec::new();

        let mut page = self
            .op(|client| client.mail_folders_list(&params))
            .context("List mail folders error")?;
        loop {
            for folder in &page.value {
                if folder.child_folder_count.unwrap_or(0) > 0 {
                    parents.push((folder.display_name.clone(), folder.id.clone()));
                }
            }
            fold_folder_page(&mut entries, None, &page.value);
            let Some(next) = page.next_link else {
                break;
            };
            page = self.folders_from_link(&next)?;
        }

        for (parent_name, parent_id) in parents {
            let mut page = self
                .op(|client| client.mail_child_folders_list(&parent_id, &params))
                .with_context(|| format!("List child folders of {parent_name} error"))?;
            loop {
                fold_folder_page(&mut entries, Some(&parent_name), &page.value);
                let Some(next) = page.next_link else {
                    break;
                };
                page = self.folders_from_link(&next)?;
            }
        }

        self.folders = entries.iter().cloned().collect();

        debug!("end of graph folder listing");
        trace!("folders: {:?}", self.folders.keys());
        Ok(entries)
    }

    /// Follows an OData next link of a folder listing.
    fn folders_from_link(&mut self, link: &str) -> Result<MsgraphMailFoldersListResponse> {
        let url = Url::parse(link).context("Cannot parse the folder paging link")?;
        self.op(|client| {
            let coroutine = MsgraphSend::<MsgraphMailFoldersListResponse>::get(&client.auth, url);
            client.run(coroutine)
        })
        .context("Follow folder paging link error")
    }

    /// Resolves a collection name to its Graph folder id, case-insensitively,
    /// refreshing the folder map on a miss.
    fn folder_id(&mut self, name: &str) -> Result<String> {
        if let Some(id) = lookup_folder(&self.folders, name) {
            return Ok(id);
        }
        self.list_folders()?;
        lookup_folder(&self.folders, name).with_context(|| format!("Unknown Graph folder {name}"))
    }

    /// Enumerates a mailbox through one Graph delta round.
    ///
    /// The opaque `cursor` carries the previous round's `@odata.deltaLink`;
    /// without one (first sync, or an unreadable checkpoint) a fresh full round
    /// runs. The returned checkpoint is the next round's delta link.
    pub fn enumerate(&mut self, mailbox: &str, cursor: Option<&[u8]>) -> Result<Enumeration> {
        let link = cursor.and_then(decode_checkpoint);
        let (rows, fresh, delta_link) = self.delta_round(mailbox, link)?;

        let mut items = Vec::new();
        let mut vanished = Vec::new();
        for row in &rows {
            if row.message.id.is_empty() {
                continue;
            }
            if row.removed.is_some() {
                vanished.push(row.message.id.clone());
            } else {
                items.push(EnumEntry {
                    revision: None,
                    id: row.message.id.clone(),
                    flags: message_flags(&row.message),
                });
            }
        }
        self.cache_rows(mailbox, &rows);

        Ok(Enumeration {
            items,
            vanished,
            complete: fresh,
            checkpoint: encode_checkpoint(&delta_link),
        })
    }

    /// Runs one full delta round over a folder, paging until the delta link
    /// closes it.
    ///
    /// Resumes from the saved link when given, falling back to a fresh round on
    /// an expired one (HTTP 410). Returns the rows, whether the round was a
    /// fresh full one, and the next round's delta link.
    fn delta_round(
        &mut self,
        mailbox: &str,
        link: Option<String>,
    ) -> Result<(Vec<MsgraphMessageDelta>, bool, String)> {
        debug!("begin graph delta round");
        trace!("mailbox: {mailbox}, resumed: {}", link.is_some());

        let mut fresh = link.is_none();
        let mut page = match link {
            None => self.fresh_delta(mailbox)?,
            Some(link) => match self.op(|client| client.messages_delta_from_link(&link)) {
                Ok(page) => page,
                Err(err) if is_expired_link(&err) => {
                    warn!("graph delta link of {mailbox} expired, restarting a full round");
                    fresh = true;
                    self.fresh_delta(mailbox)?
                }
                Err(err) => {
                    return Err(
                        anyhow::Error::new(err).context(format!("Resume delta of {mailbox} error"))
                    );
                }
            },
        };

        let mut rows = Vec::new();
        loop {
            let MsgraphMessagesDeltaResponse {
                value,
                next_link,
                delta_link,
            } = page;
            rows.extend(value);

            if let Some(delta) = delta_link {
                debug!("end of graph delta round");
                trace!("rows: {}", rows.len());
                return Ok((rows, fresh, delta));
            }
            let next = next_link
                .with_context(|| format!("Delta page of {mailbox} carries no paging link"))?;
            page = self
                .op(|client| client.messages_delta_from_link(&next))
                .with_context(|| format!("Page delta of {mailbox} error"))?;
        }
    }

    /// Starts a fresh folder-scoped delta round.
    fn fresh_delta(&mut self, mailbox: &str) -> Result<MsgraphMessagesDeltaResponse> {
        let folder = self.folder_id(mailbox)?;
        self.op(|client| client.messages_delta(Some(&folder), Some(DELTA_SELECT)))
            .with_context(|| format!("Start delta of {mailbox} error"))
    }

    /// Folds a delta round's rows into the per-collection cache: changed rows
    /// are stored by handle, removed rows are dropped.
    fn cache_rows(&mut self, mailbox: &str, rows: &[MsgraphMessageDelta]) {
        let cache = self.rows.entry(mailbox.to_owned()).or_default();
        for row in rows {
            if row.message.id.is_empty() {
                continue;
            }
            if row.removed.is_some() {
                cache.remove(&row.message.id);
            } else {
                cache.insert(row.message.id.clone(), row.message.clone());
            }
        }
    }

    /// The delta row of a handle: the enumeration cache when it holds it, else
    /// a targeted single-message get.
    fn row(&mut self, mailbox: &str, id: &str) -> Result<MsgraphMessage> {
        match self.rows.get(mailbox).and_then(|cache| cache.get(id)) {
            Some(message) => Ok(message.clone()),
            None => self
                .op(|client| client.message_get(id))
                .with_context(|| format!("Get message {id} error")),
        }
    }

    /// Fetches envelopes for a message-id set, served from the cached delta
    /// rows (one message get per handle missing the cache).
    ///
    /// Graph exposes no RFC 5322 octet size, so `size` stays 0 and is filled
    /// from the blob length at the `Full` tier.
    pub fn fetch_envelopes(&mut self, mailbox: &str, ids: &[&str]) -> Result<Vec<ItemSummary>> {
        let mut envelopes = Vec::with_capacity(ids.len());
        for id in ids {
            let message = self.row(mailbox, id)?;
            envelopes.push(message_envelope(id, &message));
        }
        Ok(envelopes)
    }

    /// Streams the bodies of a message-id set: one raw MIME get per message,
    /// Graph having no batched body fetch.
    pub fn fetch_bodies<S: Write>(
        &mut self,
        _mailbox: &str,
        ids: &[&str],
        mut open: impl FnMut(&str) -> std::io::Result<S>,
        mut done: impl FnMut(&str, Option<&str>, S) -> std::io::Result<()>,
    ) -> Result<()> {
        for id in ids {
            let raw = self.message_raw(id)?;
            let mut sink = open(id).with_context(|| format!("Open body sink for {id} error"))?;
            sink.write_all(&raw)
                .with_context(|| format!("Store body {id} error"))?;
            done(id, None, sink).with_context(|| format!("Commit body {id} error"))?;
        }
        Ok(())
    }

    /// Streams one message's raw RFC 5322 bytes into `sink`.
    pub fn get_message_stream(
        &mut self,
        _mailbox: &str,
        id: &str,
        mut sink: impl Write,
    ) -> Result<()> {
        let raw = self.message_raw(id)?;
        sink.write_all(&raw)
            .with_context(|| format!("Stream body {id} error"))?;
        Ok(())
    }

    /// Fetches the raw RFC 5322 MIME content of one message.
    fn message_raw(&mut self, id: &str) -> Result<Vec<u8>> {
        self.op(|client| client.message_get_raw(id))
            .with_context(|| format!("Get raw message {id} error"))
    }

    /// Replaces the flags of a message-id set: `\Seen` maps to `isRead`,
    /// `\Flagged` to the follow-up flagStatus.
    ///
    /// Only [`FlagOp::Set`] is supported, the engine pushing full flag sets.
    /// `\Draft` is read-only on Graph and other keywords have no equivalent,
    /// both ignored.
    pub fn store_flags(&mut self, ids: &[&str], flags: &[Flag], op: FlagOp) -> Result<()> {
        if !matches!(op, FlagOp::Set) {
            bail!("Graph flag updates only support a full set");
        }

        let patch = flags_patch(flags);
        for id in ids {
            self.op(|client| client.message_update(id, &patch))
                .with_context(|| format!("Update flags of {id} error"))?;
        }
        Ok(())
    }

    /// Deletes one message by id.
    pub fn delete_message(&mut self, id: &str) -> Result<()> {
        self.op(|client| client.message_delete(id))
            .with_context(|| format!("Delete message {id} error"))?;
        Ok(())
    }

    /// Sends raw RFC 5322 MIME bytes through the Graph sendMail action, which
    /// saves the message to Sent itself.
    ///
    /// The client error comes back unwrapped, so the caller can read the HTTP
    /// status off it. sendMail derives the recipients from the MIME headers
    /// (Bcc included), so envelope recipients beyond the headers are lost.
    pub fn send_mime(&mut self, raw: &[u8]) -> Result<(), MsgraphClientStdError> {
        self.op(|client| client.mail_send_mime(raw))?;
        Ok(())
    }
}

/// Whether a client error is an expired delta link (HTTP 410), the signal to
/// restart a full round.
fn is_expired_link(err: &MsgraphClientStdError) -> bool {
    matches!(err, MsgraphClientStdError::Send(send) if send.status() == Some(410))
}

/// Folds one folder listing page into `(name, id)` entries, prefixing child
/// folders with their parent name. Folders missing a name or an id are skipped.
fn fold_folder_page(
    entries: &mut Vec<(String, String)>,
    parent: Option<&str>,
    folders: &[MsgraphMailFolder],
) {
    for folder in folders {
        if folder.id.is_empty() || folder.display_name.is_empty() {
            continue;
        }
        let name = match parent {
            Some(parent) => format!("{parent}/{}", folder.display_name),
            None => folder.display_name.clone(),
        };
        entries.push((name, folder.id.clone()));
    }
}

/// Finds a folder id by mailbox name, case-insensitively, as the sync matches
/// mailbox names too.
fn lookup_folder(folders: &HashMap<String, String>, name: &str) -> Option<String> {
    folders
        .iter()
        .find(|(folder, _)| folder.eq_ignore_ascii_case(name))
        .map(|(_, id)| id.clone())
}

/// Maps a delta row to the shared flag set. `\Answered` and `\Deleted` have no
/// Graph delta equivalent and are never produced.
fn message_flags(message: &MsgraphMessage) -> std::collections::BTreeSet<Flag> {
    let mut flags = std::collections::BTreeSet::new();
    if message.is_read == Some(true) {
        flags.insert(Flag::from_iana(IanaFlag::Seen));
    }
    let status = message.flag.as_ref().and_then(|flag| flag.flag_status);
    if status == Some(MsgraphFlagStatus::Flagged) {
        flags.insert(Flag::from_iana(IanaFlag::Flagged));
    }
    if message.is_draft == Some(true) {
        flags.insert(Flag::from_iana(IanaFlag::Draft));
    }
    flags
}

/// The `message_update` patch replacing a message's flags. Every other field
/// stays absent, so the PATCH touches nothing else.
fn flags_patch(flags: &[Flag]) -> MsgraphMessage {
    let seen = flags.iter().any(|f| f.iana() == Some(IanaFlag::Seen));
    let flagged = flags.iter().any(|f| f.iana() == Some(IanaFlag::Flagged));
    MsgraphMessage {
        is_read: Some(seen),
        flag: Some(MsgraphFollowupFlag {
            flag_status: Some(if flagged {
                MsgraphFlagStatus::Flagged
            } else {
                MsgraphFlagStatus::NotFlagged
            }),
        }),
        ..Default::default()
    }
}

/// The author-claimed date of a delta row (Graph emits ISO 8601 date-times).
fn message_date(message: &MsgraphMessage) -> Option<DateTime<FixedOffset>> {
    let raw = message.received_date_time.as_deref()?;
    DateTime::parse_from_rfc3339(raw).ok()
}

/// Folds a delta row into a shared [`ItemSummary`] for the `Meta` tier.
fn message_envelope(id: &str, message: &MsgraphMessage) -> ItemSummary {
    let address = |recipient: &io_msgraph::v1::rest::users::messages::MsgraphRecipient| {
        crate::item::address::Address {
            name: recipient.email_address.name.clone(),
            email: recipient.email_address.address.clone().unwrap_or_default(),
        }
    };
    ItemSummary {
        id: id.to_owned(),
        message_id: message
            .internet_message_id
            .as_deref()
            .and_then(normalize_message_id),
        in_reply_to: Vec::new(),
        flags: message_flags(message),
        subject: message.subject.clone().unwrap_or_default(),
        from: message.from.as_ref().map(address).into_iter().collect(),
        to: message.to_recipients.iter().map(address).collect(),
        date: message_date(message),
        size: 0,
        has_attachment: None,
    }
}

/// Encodes the delta link into checkpoint bytes (plain UTF-8).
fn encode_checkpoint(link: &str) -> Vec<u8> {
    link.as_bytes().to_vec()
}

/// Decodes checkpoint bytes back into the delta link; `None` for an absent,
/// empty or non-UTF-8 checkpoint, which forces a fresh full round.
fn decode_checkpoint(bytes: &[u8]) -> Option<String> {
    let link = std::str::from_utf8(bytes).ok()?;
    (!link.is_empty()).then(|| link.to_owned())
}

#[cfg(test)]
mod tests {
    use io_msgraph::v1::send::MsgraphSendError;

    use super::*;

    /// A delta row fixture as Graph would serialize it, exercising the serde
    /// shape along the way.
    fn fixture_row() -> MsgraphMessage {
        serde_json::from_str(
            r#"{
                "id": "AAMkAD-abc",
                "subject": "Hello",
                "from": {"emailAddress": {"name": "Alice", "address": "alice@example.org"}},
                "toRecipients": [{"emailAddress": {"address": "bob@example.org"}}],
                "receivedDateTime": "2026-07-06T12:00:00Z",
                "internetMessageId": "<m1@example.org>",
                "isRead": true,
                "isDraft": false,
                "flag": {"flagStatus": "flagged"},
                "parentFolderId": "AQMkAD-inbox"
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn flags_map_to_iana_wire_spellings() {
        let message = fixture_row();
        let flags = message_flags(&message);
        assert!(flags.contains(&Flag::from_iana(IanaFlag::Seen)));
        assert!(flags.contains(&Flag::from_iana(IanaFlag::Flagged)));
        assert!(!flags.contains(&Flag::from_iana(IanaFlag::Draft)));

        let bare = MsgraphMessage::default();
        assert!(message_flags(&bare).is_empty());

        let draft: MsgraphMessage = serde_json::from_str(
            r#"{"id": "x", "isRead": false, "isDraft": true, "flag": {"flagStatus": "notFlagged"}}"#,
        )
        .unwrap();
        let flags = message_flags(&draft);
        assert!(flags.contains(&Flag::from_iana(IanaFlag::Draft)));
        assert!(!flags.contains(&Flag::from_iana(IanaFlag::Seen)));
        assert!(!flags.contains(&Flag::from_iana(IanaFlag::Flagged)));
    }

    #[test]
    fn a_delta_row_folds_into_an_envelope() {
        let message = fixture_row();
        let env = message_envelope("AAMkAD-abc", &message);
        assert_eq!(env.id, "AAMkAD-abc");
        assert_eq!(env.message_id.as_deref(), Some("m1@example.org"));
        assert_eq!(env.subject, "Hello");
        assert_eq!(env.from[0].email, "alice@example.org");
        assert_eq!(env.to[0].email, "bob@example.org");
        assert_eq!(
            env.date
                .unwrap()
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "2026-07-06T12:00:00Z"
        );
        assert_eq!(env.size, 0, "Graph exposes no RFC 5322 size");
    }

    #[test]
    fn a_flags_patch_touches_only_the_flag_fields() {
        let patch = flags_patch(&[Flag::from_iana(IanaFlag::Seen)]);
        assert_eq!(patch.is_read, Some(true));
        assert_eq!(
            patch.flag.as_ref().and_then(|f| f.flag_status),
            Some(MsgraphFlagStatus::NotFlagged)
        );
        let body = serde_json::to_value(&patch).unwrap();
        assert_eq!(
            body.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["flag", "isRead"]
        );

        let patch = flags_patch(&[Flag::from_iana(IanaFlag::Flagged)]);
        assert_eq!(patch.is_read, Some(false));
        assert_eq!(
            patch.flag.as_ref().and_then(|f| f.flag_status),
            Some(MsgraphFlagStatus::Flagged)
        );
    }

    #[test]
    fn only_a_410_counts_as_an_expired_link() {
        let expired = MsgraphClientStdError::Send(MsgraphSendError::Api {
            status: 410,
            code: "syncStateNotFound".into(),
            message: "gone".into(),
        });
        assert!(is_expired_link(&expired));

        let denied = MsgraphClientStdError::Send(MsgraphSendError::Api {
            status: 403,
            code: "accessDenied".into(),
            message: "no".into(),
        });
        assert!(!is_expired_link(&denied));
        let io = MsgraphClientStdError::Io(std::io::Error::other("reset"));
        assert!(!is_expired_link(&io));
    }

    #[test]
    fn checkpoint_round_trips_the_delta_link() {
        let link =
            "https://graph.microsoft.com/v1.0/me/mailFolders/x/messages/delta?$deltatoken=abc";
        assert_eq!(
            decode_checkpoint(&encode_checkpoint(link)).as_deref(),
            Some(link)
        );
        assert_eq!(decode_checkpoint(&[]), None);
        assert_eq!(decode_checkpoint(&[0xff, 0xfe]), None);
    }

    #[test]
    fn folder_map_folds_pages_and_child_levels() {
        let page: MsgraphMailFoldersListResponse = serde_json::from_str(
            r#"{
                "value": [
                    {"id": "id-inbox", "displayName": "Inbox", "childFolderCount": 1},
                    {"id": "id-archive", "displayName": "Archive", "childFolderCount": 0},
                    {"id": "id-anon", "displayName": ""}
                ],
                "@odata.nextLink": "https://graph.microsoft.com/v1.0/me/mailFolders?$skip=3"
            }"#,
        )
        .unwrap();
        assert!(page.next_link.is_some());

        let mut entries = Vec::new();
        fold_folder_page(&mut entries, None, &page.value);
        fold_folder_page(
            &mut entries,
            Some("Inbox"),
            &[MsgraphMailFolder {
                id: String::from("id-child"),
                display_name: String::from("Receipts"),
                ..Default::default()
            }],
        );

        let map: HashMap<String, String> = entries.into_iter().collect();
        assert_eq!(map.get("Inbox").map(String::as_str), Some("id-inbox"));
        assert_eq!(map.get("Archive").map(String::as_str), Some("id-archive"));
        assert_eq!(
            map.get("Inbox/Receipts").map(String::as_str),
            Some("id-child")
        );
        assert_eq!(map.len(), 3);

        assert_eq!(
            lookup_folder(&map, "inbox").as_deref(),
            Some("id-inbox"),
            "lookup is case-insensitive"
        );
        assert_eq!(lookup_folder(&map, "Missing"), None);
    }
}
