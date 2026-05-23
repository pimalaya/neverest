//! Worker dispatch for per-mailbox hunk application.
//!
//! [`PairedPools`] is the engine-facing view over both side pools.
//! Mailbox hunks (one create/delete per name) stay on the main thread;
//! email hunks fan out across `min(left.size, right.size)` worker
//! threads inside a scoped `std::thread::scope`. Workers contend on
//! a shared `mpsc::Receiver` guarded by `Mutex`; lock contention is
//! bounded by network apply cost.

use std::sync::{Arc, Mutex, mpsc};
use std::thread;

use anyhow::{Context, Result};
use io_email::client::EmailClientStd;
use log::debug;

use crate::sync::hunk::EmailHunk;

/// Paired-pool view over the two side pools. The engine builds this
/// once after mailbox patch application; subsequent per-mailbox calls
/// reuse the same N `(left, right)` client pairs without
/// reconnecting.
pub struct PairedPools {
    pub left: Vec<EmailClientStd>,
    pub right: Vec<EmailClientStd>,
}

impl PairedPools {
    /// Per-mailbox worker count = `min(left.size, right.size)`, capped
    /// at 1 so `apply_in_mailbox` always has at least one pair to hand
    /// out.
    pub fn worker_count(&self) -> usize {
        self.left.len().min(self.right.len()).max(1)
    }

    /// Fans `hunks` out across worker threads inside a
    /// `std::thread::scope`. Each worker owns one `(left, right)`
    /// pair for the duration of the mailbox so `Copy` hunks (one
    /// connection reads, the other writes) always have both ends in
    /// hand. Workers return their clients via `returned_tx` on exit;
    /// the method reclaims them into `self.left` / `self.right` before
    /// returning.
    ///
    /// Errors from individual hunks are collected; one failure does
    /// not stop the other workers.
    pub fn apply_in_mailbox<F>(
        &mut self,
        mailbox: &str,
        hunks: Vec<EmailHunk>,
        mut on_progress: F,
    ) -> Result<Vec<HunkOutcome>>
    where
        F: FnMut(usize, usize),
    {
        let total = hunks.len();
        let worker_count = self.worker_count();
        on_progress(0, total);

        let (hunk_tx, hunk_rx) = mpsc::channel::<EmailHunk>();
        let (done_tx, done_rx) = mpsc::channel::<HunkOutcome>();
        let hunk_rx = Arc::new(Mutex::new(hunk_rx));

        let mut workers: Vec<(EmailClientStd, EmailClientStd)> = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            let left = self.left.pop().expect("worker_count <= self.left.len()");
            let right = self.right.pop().expect("worker_count <= self.right.len()");
            workers.push((left, right));
        }

        let (returned_tx, returned_rx) = mpsc::channel::<(EmailClientStd, EmailClientStd)>();

        let mut outcomes: Vec<HunkOutcome> = Vec::with_capacity(total);

        thread::scope(|scope| -> Result<()> {
            for (left, right) in workers.drain(..) {
                let rx = hunk_rx.clone();
                let tx = done_tx.clone();
                let return_tx = returned_tx.clone();
                let mailbox = mailbox.to_string();

                scope.spawn(move || Self::worker_loop(left, right, mailbox, rx, tx, return_tx));
            }
            drop(done_tx);
            drop(returned_tx);

            for hunk in hunks {
                hunk_tx
                    .send(hunk)
                    .expect("workers stay alive until hunk_tx drops");
            }
            drop(hunk_tx);

            let mut applied = 0;
            while let Ok(outcome) = done_rx.recv() {
                applied += 1;
                match &outcome.result {
                    Err(e) => {
                        debug!(
                            "[{mailbox} {applied}/{total}] {hunk}: failed ({e:#})",
                            hunk = outcome.hunk,
                        )
                    }
                    Ok(_) => debug!("[{mailbox} {applied}/{total}] {hunk}", hunk = outcome.hunk,),
                }
                outcomes.push(outcome);
                on_progress(applied, total);
            }
            Ok(())
        })?;

        while let Ok((left, right)) = returned_rx.recv() {
            self.left.push(left);
            self.right.push(right);
        }

        Ok(outcomes)
    }

    /// One worker's loop. Pulls hunks off the shared receiver, applies
    /// each one against this worker's `(left, right)` pair via
    /// [`EmailHunk::apply`], and ships the outcome back. Hands the
    /// clients back through `returned_tx` on exit so the pool can
    /// reuse them on the next mailbox.
    fn worker_loop(
        mut left: EmailClientStd,
        mut right: EmailClientStd,
        mailbox: String,
        hunk_rx: Arc<Mutex<mpsc::Receiver<EmailHunk>>>,
        done_tx: mpsc::Sender<HunkOutcome>,
        returned_tx: mpsc::Sender<(EmailClientStd, EmailClientStd)>,
    ) {
        loop {
            // Unpoison the mutex if a sibling worker panicked while
            // holding it: the protected receiver carries no invariants
            // (it is itself a thread-safe primitive), so the only
            // sensible reaction is to keep draining. Surviving workers
            // continue to apply hunks and the original error reaches
            // the report, instead of every worker re-panicking with a
            // `PoisonError` that obscures the root cause.
            let hunk = match hunk_rx.lock().unwrap_or_else(|e| e.into_inner()).recv() {
                Ok(hunk) => hunk,
                Err(_) => break,
            };

            let result = hunk
                .apply(&mut left, &mut right)
                .context(format!("Apply hunk in `{mailbox}`"));

            if done_tx.send(HunkOutcome { hunk, result }).is_err() {
                break;
            }
        }

        let _ = returned_tx.send((left, right));
    }
}

/// Per-hunk apply result returned by [`PairedPools::apply_in_mailbox`].
/// `result` carries `Some(new_id)` on a successful `Copy` (the
/// backend-assigned id on the target side) so the engine can record
/// it in the snapshot at the hunk's `content_key`; the other variants
/// resolve to `Ok(None)` on success.
pub struct HunkOutcome {
    pub hunk: EmailHunk,
    pub result: Result<Option<String>>,
}
