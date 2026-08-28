//! The `text/calendar` kind: how a calendar object resource derives its link
//! id and its `v:1` summary.
//!
//! Like a [card](super::vcard) and unlike [mail](super::mail), a calendar
//! resource has **one** derivation: a DAV `sync-collection` REPORT returns
//! hrefs and ETags but no `UID`, so it resolves at the `Full` tier only and
//! [`parse_summary`](super::Kind::parse_summary) is `None` for this kind.
//! There is therefore no two-derivations hazard to keep in agreement.
//!
//! The item is the **resource**, not the component: RFC 4791 §4.1 keeps every
//! component sharing a `UID` in one resource, so a recurring series and its
//! overrides are one item, summarised from the master.
//!
//! Unlike the card scanner beside it, this one is
//! [`io_pimdir::conventions::calendar`] outright. Delegating costs nothing
//! here: io-pimdir reads the properties a summary needs the way pimdir SPEC
//! Annex A.3 spells them, **verbatim**, which is the same reading a frontend
//! wants, and it resolves the sort key through the `VTIMEZONE` the resource
//! carries, which is the one genuinely hard part and the one two writers of a
//! store must not answer differently. A second implementation here would buy
//! nothing and could only drift.

use io_pimdir::conventions::{
    PimdirDerivation,
    calendar::{self, PimdirCalendarMeta},
};
use io_replica::placement::{ReplicaLinkId, ReplicaMeta, ReplicaSortKey};

/// The `Full`-tier derivation: link id, summary and sort key from a raw
/// calendar object resource. `size` is the whole resource's octet length,
/// known from the stream, since `raw` is only the prefix it carried.
pub fn parse_body(raw: &[u8], size: u64) -> (ReplicaLinkId, ReplicaMeta, ReplicaSortKey) {
    let PimdirDerivation {
        link_id,
        meta,
        sort_key,
    } = calendar::derive(raw);

    // io-pimdir sizes the summary from the bytes it was handed, which are the
    // whole resource unless the stream capped its prefix; only then is there
    // anything to restate, so a whole body pays no round-trip.
    let meta = match size == raw.len() as u64 {
        true => meta,
        false => with_size(meta, size),
    };

    (link_id, meta, sort_key)
}

/// Restates a summary's `size` as the resource's true octet length, leaving it
/// as it stands when it does not parse (the summary is then whatever io-pimdir
/// wrote, which is still better than none).
fn with_size(meta: ReplicaMeta, size: u64) -> ReplicaMeta {
    let Ok(mut summary) = serde_json::from_str::<PimdirCalendarMeta>(&meta.0) else {
        return meta;
    };

    summary.size = Some(size);

    serde_json::to_string(&summary)
        .map(ReplicaMeta)
        .unwrap_or(meta)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EVENT: &str = "BEGIN:VCALENDAR\r\n\
         VERSION:2.0\r\n\
         BEGIN:VEVENT\r\n\
         UID:event-1@example.org\r\n\
         SUMMARY:Stand-up\r\n\
         DTSTART:20260814T090000Z\r\n\
         DTEND:20260814T093000Z\r\n\
         END:VEVENT\r\n\
         END:VCALENDAR\r\n";

    fn meta(raw: &str) -> PimdirCalendarMeta {
        let (_, meta, _) = parse_body(raw.as_bytes(), raw.len() as u64);
        serde_json::from_str(&meta.0).unwrap()
    }

    #[test]
    fn the_uid_is_the_link_id_and_a_resource_without_one_falls_back_to_the_marked_digest() {
        let (link, _, _) = parse_body(EVENT.as_bytes(), EVENT.len() as u64);
        assert_eq!(link.0, "event-1@example.org");

        let anonymous = EVENT.replace("UID:event-1@example.org\r\n", "");
        let (link, _, _) = parse_body(anonymous.as_bytes(), anonymous.len() as u64);

        // `hash:` is the prefix `Kind::split_link_id` reads as "no server has
        // heard of this id"; a fallback spelled otherwise would be pushed as a
        // `UID`.
        assert!(link.0.starts_with("hash:"), "got {}", link.0);
    }

    #[test]
    fn the_sort_key_is_the_start_resolved_to_utc() {
        let (_, _, key) = parse_body(EVENT.as_bytes(), EVENT.len() as u64);
        assert_eq!(key.0, "2026-08-14T09:00:00Z");

        let all_day = EVENT.replace("DTSTART:20260814T090000Z", "DTSTART;VALUE=DATE:20260814");
        let (_, _, key) = parse_body(all_day.as_bytes(), all_day.len() as u64);
        assert_eq!(key.0, "2026-08-14T00:00:00Z");
    }

    #[test]
    fn the_summary_carries_what_a_reader_renders_an_agenda_from() {
        let summary = meta(EVENT);

        assert_eq!(summary.v, 1);
        assert_eq!(summary.component.as_deref(), Some("VEVENT"));
        assert_eq!(summary.summary, "Stand-up");
        assert_eq!(summary.dtstart.as_deref(), Some("20260814T090000Z"));
        assert!(!summary.recurring);
        assert_eq!(summary.size, Some(EVENT.len() as u64));
    }

    /// A resource longer than the streamed prefix derives from what arrived,
    /// so io-pimdir sizes the summary from a fraction of the body. The octet
    /// count the stream reported is the one a reader must see.
    #[test]
    fn a_truncated_body_still_reports_the_whole_resource_size() {
        let (_, meta, _) = parse_body(EVENT.as_bytes(), 4096);
        let summary: PimdirCalendarMeta = serde_json::from_str(&meta.0).unwrap();

        assert_eq!(summary.size, Some(4096));
        assert_eq!(summary.summary, "Stand-up");
    }
}
