//! # Store configuration
//!
//! The `store` table of an account: where its pimdir store lives, how long
//! a retained item survives and how long an index refresh runs.

use std::{path::PathBuf, time::Duration};

use chrono::{DateTime, SecondsFormat, Utc};
use pimalaya_config::toml::opt_shell_expanded_path;
use serde::{Deserialize, Serialize};

/// The local pimdir store an account syncs through, the cache a frontend
/// reads.
///
/// Implicit, and shared by every account naming no root; this table only
/// customises it.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct StoreConfig {
    /// The store directory, holding `pimdir.db` and `objects/`.
    ///
    /// Defaults to `neverest/` under the platform's state location (XDG,
    /// Application Support, `%LOCALAPPDATA%`), shared by every account
    /// naming no root; accounts naming one root share that store.
    #[serde(default, deserialize_with = "opt_shell_expanded_path")]
    pub root: Option<PathBuf>,
    /// How long a retained item survives: `store.purge-after = "90d"`, or an
    /// integer of seconds.
    ///
    /// A pimdir store retains a deleted row, body kept, and neverest is the
    /// sweeper. Unset never purges, `0` purges at once (deliberately no
    /// boolean); months and years are refused for having no fixed length.
    #[serde(
        default,
        with = "pimalaya_config::duration::option",
        skip_serializing_if = "Option::is_none"
    )]
    pub purge_after: Option<Duration>,
    /// How long the search index may be refreshed after a sync:
    /// `store.index-timeout = "60s"`, the default, or an integer of seconds.
    ///
    /// A large first build spreads over runs, each resuming the last; `sync
    /// --index-only` builds it whole. `--queue-only` refreshes 2 s at most,
    /// `0` turns it off, and a build without `search` ignores it.
    #[serde(
        default,
        with = "pimalaya_config::duration::option",
        skip_serializing_if = "Option::is_none"
    )]
    pub index_timeout: Option<Duration>,
}

impl StoreConfig {
    /// The RFC 3339 cutoff at `now`: an older retained item is reclaimed.
    ///
    /// `None` when `purge-after` is unset, or so large no instant precedes
    /// it, which means the same. The format matches what the store stamps
    /// `retained_at` with, so the comparison is plain lexicographic.
    pub fn purge_cutoff(&self, now: DateTime<Utc>) -> Option<String> {
        let after = chrono::Duration::from_std(self.purge_after?).ok()?;
        let cutoff = now.checked_sub_signed(after)?;
        Some(cutoff.to_rfc3339_opts(SecondsFormat::Millis, true))
    }

    /// The longest a search index refresh runs after a sync, 60 seconds
    /// unless `index-timeout` says otherwise; zero turns it off.
    pub fn index_timeout(&self) -> Duration {
        self.index_timeout.unwrap_or(Duration::from_secs(60))
    }
}

#[cfg(test)]
mod tests {
    use crate::config::{AccountConfig, store::*};

    #[test]
    fn the_purge_delay_is_a_human_duration_and_drives_the_cutoff() {
        let now: DateTime<Utc> = "2026-08-07T12:00:00Z".parse().unwrap();

        let account: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://imap.example.org:993"
            "#,
        )
        .unwrap();
        assert!(account.store.purge_after.is_none());
        assert!(account.store.purge_cutoff(now).is_none());

        let account: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://imap.example.org:993"
            store.purge-after = "90d"
            "#,
        )
        .unwrap();
        assert_eq!(
            account.store.purge_cutoff(now).as_deref(),
            Some("2026-05-09T12:00:00.000Z")
        );

        let account: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://imap.example.org:993"
            store.purge-after = "0"
            "#,
        )
        .unwrap();
        assert_eq!(
            account.store.purge_cutoff(now).as_deref(),
            Some("2026-08-07T12:00:00.000Z")
        );

        let err = toml::from_str::<AccountConfig>(
            r#"
            imap.server = "imaps://imap.example.org:993"
            store.purge-after = "90 days"
            "#,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("90 days"), "{err}");
    }

    /// A duration is pimalaya-config's: an integer of seconds or a number
    /// and its unit, written back in the largest unit dividing it.
    #[test]
    fn a_duration_reads_seconds_or_a_unit_and_round_trips() {
        let cases = [
            ("0", 0, r#""0""#),
            (r#""0""#, 0, r#""0""#),
            ("45", 45, r#""45s""#),
            (r#""45s""#, 45, r#""45s""#),
            (r#""30m""#, 1800, r#""30m""#),
            (r#""12h""#, 43200, r#""12h""#),
            ("7776000", 7776000, r#""90d""#),
            (r#""90d""#, 7776000, r#""90d""#),
            (r#""2w""#, 1209600, r#""2w""#),
            ("90061", 90061, r#""90061s""#),
        ];

        for (raw, secs, written) in cases {
            let store: StoreConfig = toml::from_str(&format!("purge-after = {raw}")).unwrap();
            assert_eq!(store.purge_after, Some(Duration::from_secs(secs)), "{raw}");
            let document = toml::to_string(&store).unwrap();
            assert_eq!(document.trim(), format!("purge-after = {written}"));

            let store: StoreConfig = toml::from_str(&format!("index-timeout = {raw}")).unwrap();
            assert_eq!(
                store.index_timeout,
                Some(Duration::from_secs(secs)),
                "{raw}"
            );
        }

        for raw in [r#""""#, r#""90""#, r#""90y""#, r#""d""#, "-1"] {
            let toml = format!("purge-after = {raw}");
            assert!(toml::from_str::<StoreConfig>(&toml).is_err(), "{raw}");
        }
    }
}
