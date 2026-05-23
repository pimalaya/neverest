//! Stable cross-protocol identifier for messages.
//!
//! `Envelope.id` is backend-native (IMAP UID, JMAP id, Maildir
//! filename) so it cannot match across sides. To dedupe a message on
//! both sides we hash the `Message-ID:` header when the backend
//! surfaced one. Message-ID is byte-stable across every backend that
//! stores the message and is unique per RFC 5322 §3.6.4, so a
//! collision means two distinct mails really do share the header
//! (server-side RFC violation) and the engine fails loud.
//!
//! When no Message-ID is available, the key degrades to a hash of
//! `(subject, date, from)`. The fallback is collision-prone (digests,
//! auto-replies, headerless mails) so the engine likewise hard-fails
//! on collisions; users see the bad pair and decide whether to forward
//! the missing header upstream or filter the offending messages out.
//!
//! `Envelope.size` is deliberately excluded: it is `RFC822.SIZE` on
//! IMAP/JMAP but local file size on Maildir/m2dir. Tools that rewrite
//! local bytes (e.g. OfflineIMAP prepending `X-Keywords:` headers,
//! MDA line-ending normalization) shift the local size by a
//! deterministic delta, hashing into a different bucket than the wire
//! envelope, defeating dedupe.

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use io_email::envelope::Envelope;

/// 64-bit hash of the message's content-stable fields. Used as the
/// cross-side identifier for sync diffing and the cache index.
pub fn message_key(env: &Envelope) -> u64 {
    let mut hasher = DefaultHasher::new();
    if let Some(message_id) = env.message_id.as_deref() {
        // Tag so a Message-ID-keyed hash cannot collide with a
        // legacy-fallback hash for a different message.
        b"mid".hash(&mut hasher);
        message_id.hash(&mut hasher);
        return hasher.finish();
    }
    b"legacy".hash(&mut hasher);
    env.subject.hash(&mut hasher);
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
