//! # The calendar intents
//!
//! The `calendar-reply` and `calendar-cancel` queue intents (pimdir STORAGE
//! Annex B.2): an invitation answered, a meeting cancelled, through the
//! provider's own verbs rather than a write to the store.
//!
//! ```json
//! {"v":1,"source":"gcal","seq":42,"partstat":"ACCEPTED","comment":"See you"}
//! {"v":1,"source":"msgraph-calendar","seq":42,"comment":"Postponed"}
//! ```
//!
//! An intent addresses one item of the collection it is anchored on, by
//! `seq`, or by `link_id` while the item is still the producer's pending
//! `add`. The source it names performs it on the item it binds, and the
//! effect comes back with the next sync: the new `PARTSTAT`, or the
//! removal. Like a submission, it is at-least-once: a crash between the
//! provider's answer and the acknowledgement performs it again.

use anyhow::anyhow;
use io_pimdir::{client::PimdirStore, codec::PimdirAction};
use serde::Deserialize;

/// The queue action kind of an invitation reply.
pub const REPLY: &str = "calendar-reply";

/// The queue action kind of a meeting cancellation.
pub const CANCEL: &str = "calendar-cancel";

/// What an intent asks of its performer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Invitation {
    /// The account's answer sent to the organizer.
    Reply {
        partstat: Partstat,
        comment: Option<String>,
    },
    /// The meeting the account organises cancelled, the attendees told.
    Cancel { comment: Option<String> },
}

/// The answers a reply may carry (RFC 5545 §3.2.12, the event ones a
/// person gives).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Partstat {
    Accepted,
    Tentative,
    Declined,
}

impl Partstat {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "ACCEPTED" => Some(Self::Accepted),
            "TENTATIVE" => Some(Self::Tentative),
            "DECLINED" => Some(Self::Declined),
            _ => None,
        }
    }
}

/// How an intent names its item.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Target {
    /// A stored item, by its public id.
    Seq(i64),
    /// The producer's own pending `add`, by its key.
    Link(String),
}

/// One pending calendar intent, as read from the queue.
///
/// The payload stays raw so a malformed one parks with its reason rather
/// than hiding the row.
#[derive(Clone, Debug)]
pub struct InvitationIntent {
    /// The queue row's append id, the handle to acknowledge or park it.
    pub id: i64,
    /// The collection of the item it addresses.
    pub collection: String,
    /// [`REPLY`] or [`CANCEL`].
    pub kind: String,
    /// The raw versioned JSON payload.
    pub payload: String,
}

/// The decoded `v: 1` payload of either kind.
#[derive(Debug, Deserialize)]
struct Payload {
    v: u8,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    seq: Option<i64>,
    #[serde(default)]
    link_id: Option<String>,
    #[serde(default)]
    partstat: Option<String>,
    #[serde(default)]
    comment: Option<String>,
}

impl InvitationIntent {
    fn payload(&self) -> Result<Payload, Failure> {
        let payload: Payload = serde_json::from_str(&self.payload)
            .map_err(|err| Failure::Permanent(anyhow!("Malformed {} payload: {err}", self.kind)))?;
        if payload.v != 1 {
            return Err(Failure::Permanent(anyhow!(
                "Unsupported {} payload version {}",
                self.kind,
                payload.v
            )));
        }
        Ok(payload)
    }

    /// The source the producer named to perform it, best effort.
    pub fn source(&self) -> Option<String> {
        self.payload().ok().and_then(|payload| payload.source)
    }

    /// The item it addresses: exactly one of `seq` and `link_id`.
    pub fn target(&self) -> Result<Target, Failure> {
        let payload = self.payload()?;
        match (payload.seq, payload.link_id) {
            (Some(seq), None) => Ok(Target::Seq(seq)),
            (None, Some(link)) => Ok(Target::Link(link)),
            _ => Err(Failure::Permanent(anyhow!(
                "A {} payload names its item by exactly one of seq and link_id",
                self.kind
            ))),
        }
    }

    /// The public id it names, best effort, for the report.
    pub fn seq(&self) -> Option<i64> {
        self.payload().ok().and_then(|payload| payload.seq)
    }

    /// What it asks for.
    pub fn invitation(&self) -> Result<Invitation, Failure> {
        let payload = self.payload()?;
        let comment = payload.comment.filter(|comment| !comment.is_empty());
        match self.kind.as_str() {
            REPLY => {
                let partstat = payload
                    .partstat
                    .as_deref()
                    .and_then(Partstat::parse)
                    .ok_or_else(|| {
                        Failure::Permanent(anyhow!(
                            "A calendar-reply needs a partstat of ACCEPTED, TENTATIVE or DECLINED"
                        ))
                    })?;
                Ok(Invitation::Reply { partstat, comment })
            }
            _ => Ok(Invitation::Cancel { comment }),
        }
    }
}

/// How a failed intent is dispositioned.
#[derive(Debug)]
pub enum Failure {
    /// Retry: the row stays pending for the next run.
    Transient(anyhow::Error),
    /// Park: no run does better; the row keeps its error.
    Permanent(anyhow::Error),
}

impl Failure {
    /// Whether this failure parks the row.
    pub fn parks(&self) -> bool {
        matches!(self, Self::Permanent(_))
    }

    /// The underlying error, for the report and the log.
    pub fn error(&self) -> &anyhow::Error {
        match self {
            Self::Transient(err) | Self::Permanent(err) => err,
        }
    }

    /// Classifies a provider failure by the HTTP status in its chain.
    ///
    /// A [`Refusal`] and a 4xx are permanent, the provider refusing this
    /// intent again, except the statuses saying come back later or read
    /// again (408, 412, 429); a 5xx, a transport error or no status at all
    /// is transient.
    pub fn classify(err: anyhow::Error) -> Self {
        if err.chain().any(|cause| cause.is::<Refusal>()) {
            return Self::Permanent(err);
        }
        match http_status(&err) {
            Some(408 | 412 | 429) => Self::Transient(err),
            Some(status) if (400..500).contains(&status) => Self::Permanent(err),
            _ => Self::Transient(err),
        }
    }
}

/// What a provider will never do for this intent, whatever the run: the
/// account is not an attendee, or does not organise the meeting.
#[derive(Debug)]
pub struct Refusal(pub String);

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Refusal {}

/// The HTTP status a provider answered, wherever it sits in the chain.
fn http_status(err: &anyhow::Error) -> Option<u16> {
    err.chain().find_map(|cause| {
        #[cfg(feature = "gcal")]
        if let Some(io_gcal::v3::client::GcalClientStdError::Send(send)) = cause.downcast_ref() {
            return send.status();
        }
        #[cfg(feature = "msgraph")]
        if let Some(io_msgraph::v1::client::MsgraphClientStdError::Send(send)) =
            cause.downcast_ref()
        {
            return send.status();
        }
        let _ = cause;
        None
    })
}

/// Every pending calendar intent in the store, in queue order.
///
/// The drain leaves the kinds pimdir does not apply itself (they read back
/// as [`PimdirAction::Unknown`]), so this reads exactly what it skipped.
pub fn pending(store: &PimdirStore) -> anyhow::Result<Vec<InvitationIntent>> {
    let rows = store
        .list_pending_actions()
        .map_err(|err| anyhow!("Cannot read the queue: {err}"))?;

    Ok(rows
        .into_iter()
        .filter_map(|row| match row.action {
            PimdirAction::Unknown { kind, payload, .. } if kind == REPLY || kind == CANCEL => {
                Some(InvitationIntent {
                    id: row.id,
                    collection: row.collection,
                    kind,
                    payload,
                })
            }
            _ => None,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intent(kind: &str, payload: &str) -> InvitationIntent {
        InvitationIntent {
            id: 1,
            collection: "gcal/primary".into(),
            kind: kind.into(),
            payload: payload.into(),
        }
    }

    #[test]
    fn a_reply_decodes_its_answer_and_its_comment() {
        let reply = intent(
            REPLY,
            r#"{"v":1,"source":"gcal","seq":7,"partstat":"TENTATIVE","comment":"Maybe"}"#,
        );

        assert_eq!(reply.source().as_deref(), Some("gcal"));
        assert_eq!(reply.target().unwrap(), Target::Seq(7));
        assert_eq!(
            reply.invitation().unwrap(),
            Invitation::Reply {
                partstat: Partstat::Tentative,
                comment: Some("Maybe".into()),
            }
        );
    }

    #[test]
    fn a_cancel_names_a_pending_add_by_its_key() {
        let cancel = intent(CANCEL, r#"{"v":1,"link_id":"uid:abc","comment":""}"#);

        assert_eq!(cancel.target().unwrap(), Target::Link("uid:abc".into()));
        assert_eq!(
            cancel.invitation().unwrap(),
            Invitation::Cancel { comment: None }
        );
    }

    #[test]
    fn a_malformed_intent_parks() {
        let parks = |intent: InvitationIntent| {
            let target = intent.target().map(|_| ());
            let invitation = intent.invitation().map(|_| ());
            target.and(invitation).unwrap_err().parks()
        };

        assert!(parks(intent(REPLY, "{")));
        assert!(parks(intent(
            REPLY,
            r#"{"v":2,"seq":1,"partstat":"ACCEPTED"}"#
        )));
        assert!(parks(intent(
            REPLY,
            r#"{"v":1,"seq":1,"partstat":"MAYBE"}"#
        )));
        assert!(parks(intent(REPLY, r#"{"v":1,"partstat":"ACCEPTED"}"#)));
        assert!(parks(intent(
            CANCEL,
            r#"{"v":1,"seq":1,"link_id":"uid:abc"}"#
        )));
    }

    #[test]
    fn a_refusal_parks_and_an_unknown_failure_retries() {
        let refused = anyhow::Error::new(Refusal("not an attendee".into()));

        assert!(Failure::classify(refused.context("Reply error")).parks());
        assert!(!Failure::classify(anyhow!("connection reset")).parks());
    }
}
