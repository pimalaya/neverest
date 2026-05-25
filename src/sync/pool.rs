// This file is part of Neverest, a CLI to synchronize emails.
//
// Copyright (C) 2024-2026  soywod <pimalaya.org@posteo.net>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

//! Connection pool: opens N independent clients per side and fans
//! mailbox / message hunks out across paired `(left, right)` workers.

use std::{
    sync::{Arc, mpsc},
    thread,
};

use anyhow::{Context, Result, bail};
use crossbeam_queue::SegQueue;
use io_email::client::EmailClientStd;
use log::{trace, warn};

use crate::{
    client,
    config::SideConfig,
    sync::hunk::{EmailHunk, MailboxHunk},
};

// TODO: replace with the server-advertised IMAP LIMIT once `io-imap`
// exposes the capability getter.
const IMAP_SOFT_LIMIT: usize = 10;

/// Both sides' worker pools paired so `Copy` hunks always have read +
/// write ends in hand.
pub struct Pool {
    pub left: Vec<EmailClientStd>,
    pub right: Vec<EmailClientStd>,
}

impl Pool {
    /// Opens both sides' clients in parallel (each side parallel
    /// internally); the first error propagates and drops everything
    /// already opened.
    pub fn open(left: SideConfig, right: SideConfig) -> Result<Pool> {
        Ok(Pool {
            left: open_side(left)?,
            right: open_side(right)?,
        })
    }

    /// Per-side worker count = `min(left.len, right.len).max(1)`.
    pub fn worker_count(&self) -> usize {
        self.left.len().min(self.right.len()).max(1)
    }

    /// Fans email `hunks` out across worker threads, each owning one
    /// `(left, right)` pair for the mailbox duration; per-hunk failures
    /// are collected without stopping other workers.
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

        let queue: Arc<SegQueue<EmailHunk>> = Arc::new(SegQueue::new());
        for hunk in hunks {
            queue.push(hunk);
        }
        let (done_tx, done_rx) = mpsc::channel::<HunkOutcome>();

        let mut workers: Vec<(EmailClientStd, EmailClientStd)> = Vec::with_capacity(worker_count);
        while workers.len() < worker_count {
            match (self.left.pop(), self.right.pop()) {
                (Some(l), Some(r)) => workers.push((l, r)),
                _ => break,
            }
        }

        let mut outcomes: Vec<HunkOutcome> = Vec::with_capacity(total);

        thread::scope(|scope| -> Result<()> {
            let mut handles = Vec::with_capacity(workers.len());
            for (left, right) in workers.drain(..) {
                let q = queue.clone();
                let tx = done_tx.clone();
                let mailbox = mailbox.to_string();

                handles.push(scope.spawn(move || email_worker(left, right, mailbox, q, tx)));
            }
            drop(done_tx);

            let mut applied = 0;
            while let Ok(outcome) = done_rx.recv() {
                applied += 1;
                match &outcome.result {
                    Err(e) => trace!("{mailbox} [{applied}/{total}] {}: {e:#}", outcome.hunk),
                    Ok(_) => trace!("{mailbox} [{applied}/{total}] {}", outcome.hunk),
                }
                outcomes.push(outcome);
                on_progress(applied, total);
            }

            for handle in handles {
                match handle.join() {
                    Ok((left, right)) => {
                        self.left.push(left);
                        self.right.push(right);
                    }
                    Err(_) => bail!("Email worker thread panicked"),
                }
            }
            Ok(())
        })?;

        Ok(outcomes)
    }

    /// Mailbox-hunk counterpart of [`Pool::apply_in_mailbox`].
    pub fn apply_mailbox_hunks<F>(
        &mut self,
        hunks: Vec<MailboxHunk>,
        mut on_progress: F,
    ) -> Result<Vec<MailboxHunkOutcome>>
    where
        F: FnMut(usize, usize),
    {
        let total = hunks.len();
        let worker_count = self.worker_count();
        on_progress(0, total);

        let queue: Arc<SegQueue<MailboxHunk>> = Arc::new(SegQueue::new());
        for hunk in hunks {
            queue.push(hunk);
        }
        let (done_tx, done_rx) = mpsc::channel::<MailboxHunkOutcome>();

        let mut workers: Vec<(EmailClientStd, EmailClientStd)> = Vec::with_capacity(worker_count);
        while workers.len() < worker_count {
            match (self.left.pop(), self.right.pop()) {
                (Some(l), Some(r)) => workers.push((l, r)),
                _ => break,
            }
        }

        let mut outcomes: Vec<MailboxHunkOutcome> = Vec::with_capacity(total);

        thread::scope(|scope| -> Result<()> {
            let mut handles = Vec::with_capacity(workers.len());
            for (left, right) in workers.drain(..) {
                let q = queue.clone();
                let tx = done_tx.clone();
                handles.push(scope.spawn(move || mailbox_worker(left, right, q, tx)));
            }
            drop(done_tx);

            let mut applied = 0;
            while let Ok(outcome) = done_rx.recv() {
                applied += 1;
                match &outcome.result {
                    Err(e) => trace!("mailbox [{applied}/{total}] {}: {e:#}", outcome.hunk),
                    Ok(_) => trace!("mailbox [{applied}/{total}] {}", outcome.hunk),
                }
                outcomes.push(outcome);
                on_progress(applied, total);
            }

            for handle in handles {
                match handle.join() {
                    Ok((left, right)) => {
                        self.left.push(left);
                        self.right.push(right);
                    }
                    Err(_) => bail!("Mailbox worker thread panicked"),
                }
            }
            Ok(())
        })?;

        Ok(outcomes)
    }
}

/// Per-email-hunk outcome; successful `Copy` resolves to `Some(new_id)`,
/// other variants to `None`.
pub struct HunkOutcome {
    pub hunk: EmailHunk,
    pub result: Result<Option<String>>,
}

/// Per-mailbox-hunk outcome.
pub struct MailboxHunkOutcome {
    pub hunk: MailboxHunk,
    pub result: Result<()>,
}

/// Opens one side's client pool in parallel; the first error propagates
/// and drops the partial pool.
fn open_side(config: SideConfig) -> Result<Vec<EmailClientStd>> {
    let size = resolve_size(&config);

    thread::scope(|scope| -> Result<Vec<EmailClientStd>> {
        let handles: Vec<_> = (0..size)
            .map(|_| {
                let cfg = config.clone();
                scope.spawn(move || client::open(cfg))
            })
            .collect();

        let mut clients = Vec::with_capacity(size);
        for h in handles {
            match h.join() {
                Ok(result) => clients.push(result?),
                Err(_) => bail!("Pool open thread panicked"),
            }
        }
        Ok(clients)
    })
}

/// Resolves the per-side pool size from config or the per-backend
/// default (IMAP 8, JMAP 4, m2dir 8); IMAP warns above the soft cap.
fn resolve_size(config: &SideConfig) -> usize {
    let default = if config.is_imap() {
        8
    } else if config.is_jmap() {
        4
    } else {
        8
    };

    let requested = config.pool_size().unwrap_or(default).max(1);

    if config.is_imap() && requested > IMAP_SOFT_LIMIT {
        warn!("imap pool size {requested} exceeds cap {IMAP_SOFT_LIMIT}");
    }

    requested
}

/// One email-hunk worker: drain the queue, apply each hunk against the
/// `(left, right)` pair, return the pair on exit.
fn email_worker(
    mut left: EmailClientStd,
    mut right: EmailClientStd,
    mailbox: String,
    queue: Arc<SegQueue<EmailHunk>>,
    done_tx: mpsc::Sender<HunkOutcome>,
) -> (EmailClientStd, EmailClientStd) {
    while let Some(hunk) = queue.pop() {
        let result = hunk
            .apply(&mut left, &mut right)
            .context(format!("Apply hunk in `{mailbox}`"));

        if done_tx.send(HunkOutcome { hunk, result }).is_err() {
            break;
        }
    }
    (left, right)
}

/// Mailbox-hunk counterpart of [`email_worker`].
fn mailbox_worker(
    mut left: EmailClientStd,
    mut right: EmailClientStd,
    queue: Arc<SegQueue<MailboxHunk>>,
    done_tx: mpsc::Sender<MailboxHunkOutcome>,
) -> (EmailClientStd, EmailClientStd) {
    while let Some(hunk) = queue.pop() {
        let result = hunk
            .apply(&mut left, &mut right)
            .context("Apply mailbox hunk");
        if done_tx.send(MailboxHunkOutcome { hunk, result }).is_err() {
            break;
        }
    }
    (left, right)
}
