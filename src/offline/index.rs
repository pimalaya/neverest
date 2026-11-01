//! # Search index
//!
//! The store's owner keeps its search index, index.db beside pimdir.db
//! (pimdir SEARCH §2, §3), and frontends only query it: neverest refreshes
//! it after a sync and after a drain, under a deadline, and
//! [`crate::cli::index`] builds it whole.
//!
//! The index is derived from the store, so a refresh failing never fails
//! the run it follows: it is reported and the next run tries again. A
//! refresh cut by its deadline leaves the index behind the store, and the
//! next one goes on from where it stopped.

use std::{
    path::Path,
    time::{Duration, Instant},
};

use io_pimdir::client::{
    PimdirError,
    search::{PimdirIndex, PimdirIndexRefreshOptions},
};
use log::{debug, info, warn};

use crate::{config::StoreConfig, sync::report::IndexRefresh};

/// How long a refresh may run after a run a frontend waits on: a drain, a
/// run performing the queued intents alone or declaring the collections.
pub const QUICK: Duration = Duration::from_secs(2);

/// Refreshes the index of the store rooted at `dir` after a run, for at
/// most the account's `store.index-timeout`, or [`QUICK`] when `quick`,
/// and says what it did in `report`.
///
/// Nothing runs when the timeout is zero, which turns the refresh off. A
/// failure is reported, never raised.
pub fn after_run(dir: &Path, config: &StoreConfig, quick: bool, report: &mut Option<IndexRefresh>) {
    let mut timeout = config.index_timeout();
    if timeout.is_zero() {
        debug!("search index refresh turned off");
        return;
    }
    if quick {
        timeout = timeout.min(QUICK);
    }

    let refreshed = PimdirIndex::open(dir).and_then(|mut index| refresh(&mut index, Some(timeout)));
    let refreshed = match refreshed {
        Ok(refreshed) => refreshed,
        Err(PimdirError::Indexing(_)) => IndexRefresh {
            busy: true,
            ..Default::default()
        },
        Err(err) => {
            warn!("cannot refresh the search index, the next run tries again: {err}");
            IndexRefresh {
                error: Some(err.to_string()),
                ..Default::default()
            }
        }
    };

    info!("{refreshed}");
    *report = Some(refreshed);
}

/// Folds the store's change feed into `index` until it is current or
/// `timeout` passes, `None` folding all of it.
pub fn refresh(
    index: &mut PimdirIndex,
    timeout: Option<Duration>,
) -> Result<IndexRefresh, PimdirError> {
    let options = PimdirIndexRefreshOptions {
        deadline: timeout.map(|timeout| Instant::now() + timeout),
        ..Default::default()
    };
    let report = index.refresh(&options)?;

    Ok(IndexRefresh {
        folded: report.folded,
        indexed: report.indexed,
        behind: report.head.saturating_sub(report.cursor).max(0) as u64,
        busy: report.busy,
        error: None,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use io_pimdir::{
        change::PimdirWriteOp,
        client::{PimdirStore, search::PimdirSearchClock},
        collection::PimdirCollectionId,
        object::PimdirObject,
        placement::{
            PimdirFlags, PimdirHandle, PimdirLevel, PimdirLinkId, PimdirPlacement, PimdirSortKey,
            PimdirStatus,
        },
        search::query::PimdirQuery,
        summary,
    };

    use super::*;

    /// Files a message whose subject is `subject` into the inbox, as a
    /// sync lands one.
    fn land(dir: &Path, id: &str, subject: &str) {
        let body = format!(
            "Message-ID: <{id}>\r\nSubject: {subject}\r\nFrom: Ann <ann@example.org>\r\n\
             Date: Sat, 01 Aug 2026 12:00:00 +0000\r\n\r\nHello\r\n"
        )
        .into_bytes();
        let mut store = PimdirStore::open(dir).unwrap().for_source("imap");
        store
            .ensure_collection("imap/INBOX", "message/rfc822")
            .unwrap();
        let derived = summary::derive("message/rfc822", &body).unwrap();
        let hash = store.hash(&body);
        store
            .write(vec![
                PimdirWriteOp::StoreObject {
                    object: PimdirObject {
                        hash: hash.clone(),
                        size: body.len(),
                    },
                    body: Some(body),
                },
                PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                    collection: PimdirCollectionId("imap/INBOX".into()),
                    handle: PimdirHandle(id.into()),
                    link_id: Some(PimdirLinkId(derived.link_id.0.clone())),
                    object: Some(hash),
                    level: PimdirLevel::Full,
                    summary: derived.summary,
                    sort_key: PimdirSortKey(derived.sort_key.0),
                    flags: PimdirFlags::default(),
                    status: PimdirStatus::Clean,
                    conflict_revision: None,
                    conflict_object: None,
                    base: None,
                    origin: None,
                }),
            ])
            .unwrap();
    }

    /// How many messages a frontend finds for `query`, opening the index
    /// for queries alone as himalaya does.
    fn found(dir: &Path, query: &str) -> u64 {
        PimdirIndex::open_query(dir)
            .unwrap()
            .count(
                &PimdirQuery::parse(query).unwrap(),
                &PimdirSearchClock::default(),
            )
            .unwrap()
            .count
    }

    /// The refresh after a run indexes what the run landed, so a frontend
    /// opening the index for queries alone finds it.
    #[test]
    fn a_refresh_after_a_run_indexes_what_it_landed() {
        let dir = tempfile::tempdir().unwrap();
        land(dir.path(), "m1@x", "Quarterly report");

        let mut refreshed = None;
        after_run(dir.path(), &StoreConfig::default(), false, &mut refreshed);

        let refreshed = refreshed.unwrap();
        assert_eq!(refreshed.error, None);
        assert_eq!(refreshed.behind, 0, "{refreshed:?}");
        assert!(refreshed.folded > 0, "{refreshed:?}");
        assert_eq!(found(dir.path(), "quarterly"), 1);

        land(dir.path(), "m2@x", "Quarterly review");
        let mut refreshed = None;
        after_run(dir.path(), &StoreConfig::default(), true, &mut refreshed);
        assert_eq!(found(dir.path(), "quarterly"), 2, "{refreshed:?}");
    }

    /// An index of another shape, which an earlier build left and a
    /// frontend refuses to query, is rebuilt by the next refresh and
    /// reported as a whole fold, not as a failure.
    #[test]
    fn an_index_of_another_shape_is_rebuilt_by_the_next_refresh() {
        let dir = tempfile::tempdir().unwrap();
        land(dir.path(), "m1@x", "Quarterly report");
        after_run(dir.path(), &StoreConfig::default(), false, &mut None);

        let conn = rusqlite::Connection::open(dir.path().join("index.db")).unwrap();
        conn.execute("DROP INDEX placement_by_link", []).unwrap();
        drop(conn);
        let Err(err) = PimdirIndex::open_query(dir.path()) else {
            panic!("an index of another shape opens for queries");
        };
        assert_eq!(err.code(), "index-outdated");

        let mut refreshed = None;
        after_run(dir.path(), &StoreConfig::default(), false, &mut refreshed);

        let refreshed = refreshed.unwrap();
        assert_eq!(refreshed.error, None);
        assert!(refreshed.folded > 0, "folded again: {refreshed:?}");
        assert_eq!(refreshed.behind, 0, "{refreshed:?}");
        assert_eq!(found(dir.path(), "quarterly"), 1);
    }

    /// A refresh its deadline cut leaves the index behind, and the next one
    /// goes on from there.
    #[test]
    fn a_refresh_its_deadline_cut_is_continued_by_the_next() {
        let dir = tempfile::tempdir().unwrap();
        land(dir.path(), "m1@x", "Quarterly report");
        let mut index = PimdirIndex::open(dir.path()).unwrap();

        let cut = refresh(&mut index, Some(Duration::ZERO)).unwrap();
        assert_eq!(cut.folded, 0);
        assert!(cut.behind > 0, "{cut:?}");

        let next = refresh(&mut index, Some(Duration::from_secs(60))).unwrap();
        assert!(next.folded > 0, "{next:?}");
        assert_eq!(next.behind, 0, "{next:?}");
        assert_eq!(found(dir.path(), "quarterly"), 1);
    }

    /// An index that cannot be opened is reported, never raised: the run
    /// it follows stands.
    #[test]
    fn an_index_failure_is_reported_not_raised() {
        let dir = tempfile::tempdir().unwrap();
        land(dir.path(), "m1@x", "Quarterly report");
        fs::create_dir(dir.path().join("index.db")).unwrap();

        let mut refreshed = None;
        after_run(dir.path(), &StoreConfig::default(), false, &mut refreshed);

        let refreshed = refreshed.unwrap();
        assert!(refreshed.error.is_some(), "{refreshed:?}");
    }

    /// `store.index-timeout = "0"` turns the refresh off: no index is made.
    #[test]
    fn a_zero_timeout_refreshes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        land(dir.path(), "m1@x", "Quarterly report");
        let config: StoreConfig = toml::from_str("index-timeout = \"0\"").unwrap();

        let mut refreshed = None;
        after_run(dir.path(), &config, false, &mut refreshed);

        assert_eq!(refreshed, None);
        assert!(!dir.path().join("index.db").exists());
    }
}
