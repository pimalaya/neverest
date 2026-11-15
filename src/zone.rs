//! # The user's zone
//!
//! The zone a floating time is read in, `TZ` then the system's: the search
//! index places floating times and dates in it (pimdir SEARCH §7, §8.1),
//! and the Google Calendar and Graph sources anchor a floating time in it
//! before a write when the calendar states no zone of its own, so the time
//! a person reads stays the time they meant.

use std::sync::OnceLock;

use jiff::tz::TimeZone;
use log::warn;

/// The zone a floating time is read in when the user's has no IANA name.
pub const UTC: &str = "UTC";

/// The user's zone, `TZ` then the system's, resolved once per run: UTC,
/// with a warning, when it has no IANA name.
pub fn floating_zone() -> &'static str {
    static ZONE: OnceLock<String> = OnceLock::new();
    ZONE.get_or_init(|| zone_name(TimeZone::try_system()))
}

/// The IANA name of `zone`, UTC with a warning when it has none.
fn zone_name(zone: Result<TimeZone, jiff::Error>) -> String {
    match zone.map(|zone| zone.iana_name().map(String::from)) {
        Ok(Some(name)) => name,
        Ok(None) => {
            warn!("the time zone has no IANA name, floating times are read in UTC");
            UTC.into()
        }
        Err(err) => {
            warn!("cannot read the time zone, floating times are read in UTC: {err}");
            UTC.into()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{env, process::Command};

    use super::*;

    /// The marker a child run of [`the_zone_is_read_from_tz`] checks.
    const CHILD: &str = "NEVEREST_TEST_ZONE";

    /// The zone stated is the user's IANA name, UTC for a POSIX `TZ`
    /// rule or a zone that cannot be read.
    #[test]
    fn a_zone_without_an_iana_name_reads_as_utc() {
        let paris = TimeZone::get("Europe/Paris");
        let rule = TimeZone::posix("CET-1CEST,M3.5.0,M10.5.0/3");
        let unread = TimeZone::get("Nowhere/Atlantis");

        assert_eq!(zone_name(paris), "Europe/Paris");
        assert_eq!(zone_name(rule), "UTC");
        assert_eq!(zone_name(unread), "UTC");
    }

    /// `TZ` names the zone, in every build reading one: this test runs
    /// itself again under each `TZ`, the zone being read once per process.
    #[test]
    fn the_zone_is_read_from_tz() {
        if let Ok(expected) = env::var(CHILD) {
            assert_eq!(floating_zone(), expected);
            return;
        }

        for zone in ["Europe/Paris", "America/New_York"] {
            let output = Command::new(env::current_exe().unwrap())
                .args(["--exact", "zone::tests::the_zone_is_read_from_tz"])
                .env("TZ", zone)
                .env(CHILD, zone)
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                output.status.success() && stdout.contains("1 passed"),
                "TZ={zone}: {stdout}"
            );
        }
    }
}
