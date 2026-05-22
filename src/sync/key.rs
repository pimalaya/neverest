//! Stable cross-protocol identifier for envelopes.
//!
//! `Envelope.id` is backend-native (IMAP UID, JMAP id, Maildir
//! filename) so it cannot match across sides. To dedupe a message on
//! both sides without paying a `BODY[HEADER.FIELDS (MESSAGE-ID)]`
//! fetch on every envelope, we derive a content key from the fields
//! every backend already populates: subject, sender list, date, size.
//!
//! Collisions are theoretically possible (newsletter blasts with
//! identical subject + sender + second-precision date + size) but
//! extremely unlikely in practice. Real `Message-ID` dedupe can layer
//! on top later — start with the cheap key.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use io_email::envelope::Envelope;

/// 64-bit hash of the envelope's content-stable fields. Used as the
/// cross-side identifier for sync diffing and the cache index.
pub fn envelope_key(env: &Envelope) -> u64 {
    let mut hasher = DefaultHasher::new();
    env.subject.hash(&mut hasher);
    env.size.hash(&mut hasher);
    if let Some(date) = env.date {
        date.timestamp().hash(&mut hasher);
    } else {
        0_i64.hash(&mut hasher);
    }
    for addr in &env.from {
        addr.email.hash(&mut hasher);
    }
    hasher.finish()
}
