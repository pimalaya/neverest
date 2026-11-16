//! # Filter configuration
//!
//! Which collections and items a sync lists: a source's collection filter
//! and an account's mail scope floor.

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Days, Months, NaiveDate, NaiveTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use crate::config::is_default;

/// Item-level sync options.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ItemSyncConfig {
    /// Which items a sync lists.
    #[serde(default, skip_serializing_if = "is_default")]
    pub filter: ItemFilter,
}

/// Which items a sync lists (pimdir SYNC §5).
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ItemFilter {
    /// The floor of the `Date` scope a mail collection syncs under: `30d`,
    /// `12w`, `6mo`, `1y`, a date (midnight UTC) or an RFC 3339 instant.
    ///
    /// Mail outside it is neither listed nor deleted: what a wider sync
    /// stored stays. Refused beside contacts or calendars; `sync --since`
    /// overrides it for one run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
}

/// The RFC 3339 instant in UTC a scope floor `raw` names at `now`.
///
/// A duration (`30d`, `30 days`) counts back to the start of its UTC day,
/// so a run resumes the round an earlier run that day opened; a date is its
/// midnight UTC; an instant is normalised to UTC.
pub fn resolve_since(raw: &str, now: DateTime<Utc>) -> Result<String> {
    let raw = raw.trim();
    let format = |at: DateTime<Utc>| at.to_rfc3339_opts(SecondsFormat::Secs, true);

    if let Ok(at) = DateTime::parse_from_rfc3339(raw) {
        return Ok(format(at.to_utc()));
    }
    if let Ok(day) = NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
        return Ok(format(day.and_time(NaiveTime::MIN).and_utc()));
    }

    let digits = raw.find(|c: char| !c.is_ascii_digit()).unwrap_or(raw.len());
    let (count, unit) = raw.split_at(digits);
    let count: u32 = count.parse().with_context(|| {
        format!("{raw:?} is neither a duration (`30d`), a date (`2026-01-01`) nor an instant")
    })?;
    let today = now.date_naive();
    let floor = match unit.trim().to_ascii_lowercase().as_str() {
        "d" | "day" | "days" => today.checked_sub_days(Days::new(u64::from(count))),
        "w" | "week" | "weeks" => today.checked_sub_days(Days::new(u64::from(count) * 7)),
        "mo" | "month" | "months" => today.checked_sub_months(Months::new(count)),
        "y" | "year" | "years" => today.checked_sub_months(Months::new(count.saturating_mul(12))),
        unit => bail!(
            "{raw:?} counts in {unit:?}, which is none of d (days), w (weeks), mo (months), y \
             (years)"
        ),
    }
    .with_context(|| format!("{raw:?} reaches before any date"))?;

    Ok(format(floor.and_time(NaiveTime::MIN).and_utc()))
}

/// Collection-name filter: include-list, exclude-list, or keep all.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum CollectionFilter {
    /// Every collection the source lists.
    #[default]
    All,
    /// Only the named collections.
    Include(Vec<String>),
    /// Every collection but the named ones.
    Exclude(Vec<String>),
}

#[cfg(test)]
mod tests {
    use crate::config::filter::*;

    fn at(instant: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(instant).unwrap().to_utc()
    }

    /// A duration counts back from today to the start of its UTC day.
    ///
    /// So two runs of one day list one scope; a date is its midnight UTC,
    /// and an instant is normalised to UTC.
    #[test]
    fn a_scope_floor_reads_as_a_duration_a_date_or_an_instant() {
        let now = at("2026-10-07T15:42:10+02:00");

        assert_eq!(resolve_since("30d", now).unwrap(), "2026-09-07T00:00:00Z");
        assert_eq!(
            resolve_since(" 30 days ", now).unwrap(),
            "2026-09-07T00:00:00Z"
        );
        assert_eq!(resolve_since("2w", now).unwrap(), "2026-09-23T00:00:00Z");
        assert_eq!(resolve_since("6mo", now).unwrap(), "2026-04-07T00:00:00Z");
        assert_eq!(resolve_since("1y", now).unwrap(), "2025-10-07T00:00:00Z");
        assert_eq!(resolve_since("0d", now).unwrap(), "2026-10-07T00:00:00Z");
        assert_eq!(
            resolve_since("2026-01-01", now).unwrap(),
            "2026-01-01T00:00:00Z"
        );
        assert_eq!(
            resolve_since("2026-01-01T10:00:00+02:00", now).unwrap(),
            "2026-01-01T08:00:00Z"
        );

        assert!(resolve_since("30m", now).is_err(), "minutes are not a unit");
        assert!(resolve_since("soon", now).is_err());
        assert!(resolve_since("", now).is_err());
    }
}
