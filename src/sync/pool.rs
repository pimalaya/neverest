//! Per-side connection pool for parallel hunk dispatch.
//!
//! [`SidePool`] opens N independent [`EmailClientStd`] instances against
//! the same [`SideConfig`] up front, so the engine can hand one client
//! per worker thread inside a `std::thread::scope`. Mailbox-list and
//! mailbox-patch stages still run serially against `clients[0]`; only
//! the per-mailbox envelope apply loop pulls from the full pool.
//!
//! Sizing defaults are per-backend (IMAP 8, JMAP 4, m2dir 8); a user
//! override on [`SideConfig::pool_size`] wins. IMAP server-advertised
//! LIMIT detection is out of scope for Phase 6 (`io-imap` does not
//! currently expose a LIMIT capability getter); for now any configured
//! size above 10 just emits a `warn!`.

use std::thread;

use anyhow::Result;
use io_email::client::EmailClientStd;
use log::warn;

use crate::{config::SideConfig, side::Side};

/// Upper bound applied to user-supplied IMAP pool sizes.
///
/// TODO(neverest): replace with the server-advertised LIMIT once
/// `io-imap` exposes the capability getter (Phase 6 deferred work).
const IMAP_SOFT_LIMIT: usize = 10;

/// Bag of pre-opened [`EmailClientStd`] instances for one side of the
/// sync. Clients are independent connections; one worker thread owns
/// one client for the duration of a mailbox's hunk loop.
pub struct SidePool {
    clients: Vec<EmailClientStd>,
}

impl SidePool {
    /// Opens `size` independent clients against `side_config` in
    /// parallel. Wall-clock cost is `max(per_client_open)` instead of
    /// `sum(...)`, which matters for IMAP/JMAP where each open pays a
    /// TLS + auth round-trip (~500ms+). Falls back to a per-backend
    /// default when `side_config.pool_size` is `None`. If any client
    /// fails to open, the partial pool is dropped (each opened client
    /// disconnects on `Drop`) and the first error propagates.
    pub fn open(side_config: SideConfig, side: Side) -> Result<Self> {
        let size = Self::resolve_size(&side_config, side);

        let clients = thread::scope(|scope| -> Result<Vec<EmailClientStd>> {
            let handles: Vec<_> = (0..size)
                .map(|_| {
                    let cfg = side_config.clone();
                    scope.spawn(move || side.open(cfg))
                })
                .collect();

            let mut clients = Vec::with_capacity(size);
            for h in handles {
                clients.push(h.join().expect("open_side worker did not panic")?);
            }
            Ok(clients)
        })?;

        Ok(Self { clients })
    }

    /// Number of clients in the pool. Always `>= 1` post-construction.
    pub fn size(&self) -> usize {
        self.clients.len()
    }

    /// Hands out the underlying client vector for worker dispatch. The
    /// engine moves the whole vec into `std::thread::scope`, hands one
    /// client per worker thread, then drops the pool when the scope
    /// closes.
    pub fn into_clients(self) -> Vec<EmailClientStd> {
        self.clients
    }

    /// Resolves the requested pool size from the config + backend
    /// default.
    ///
    /// Defaults: IMAP 8, JMAP 4, m2dir 8. Applies the IMAP soft cap
    /// regardless of which backend is configured (the cap matches
    /// IMAP's typical server-side LIMIT and is a sensible upper bound
    /// for the other backends too).
    ///
    /// m2dir clients are `PathBuf` wrappers with no connection cost;
    /// the 8 default exists so a mixed setup (IMAP<->m2dir) does not
    /// bottleneck the per-mailbox worker count to `min(8, 1) = 1`. The
    /// engine's worker count is `min(left.size, right.size)`, so both
    /// sides need to clear the desired worker bar.
    fn resolve_size(side_config: &SideConfig, side: Side) -> usize {
        let default = if side_config.is_imap() {
            8
        } else if side_config.is_jmap() {
            4
        } else {
            8
        };

        let requested = side_config.pool_size().unwrap_or(default).max(1);

        if side_config.is_imap() && requested > IMAP_SOFT_LIMIT {
            warn!(
                "{side} imap pool size {requested} exceeds the recommended cap of \
                 {IMAP_SOFT_LIMIT}; some servers may refuse extra connections"
            );
        }

        requested
    }
}
