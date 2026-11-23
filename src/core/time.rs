//! Time helpers shared by the whole application (jiff).
//!
//! Instants are [`Timestamp`]s; schedules are local wall times in the Pi time
//! zone ([`ZonedDateTime`] in [`local_tz`]). The rules that matter around a
//! daylight saving change live here, in one place:
//! - a local time skipped by the change does not exist ([`local_earliest`]
//!   gives `None`), a repeated one gives its first occurrence;
//! - durations are counted in real time (seconds between instants).

pub use jiff::civil::{DateTime as CivilDateTime, Time};
pub use jiff::tz::TimeZone;
pub use jiff::{SignedDuration, Timestamp, ZonedDateTime};

use jiff::tz::AmbiguousOffset;

/// Pi time zone (`TZ`, else `/etc/localtime`).
pub fn local_tz() -> TimeZone {
    TimeZone::system()
}

/// Forget the cached Pi time zone, right after it was changed (jiff keeps it
/// for 5 minutes otherwise): the night schedule must follow at once.
pub fn reset_local_tz() {
    jiff::tz::reset_system_time_zone_cache();
}

/// Current local time.
pub fn now_local() -> ZonedDateTime {
    Timestamp::now().to_zoned_datetime(local_tz())
}

/// An instant as a local time.
pub fn to_local(ts: Timestamp) -> ZonedDateTime {
    ts.to_zoned_datetime(local_tz())
}

/// A file date (modification time) as a local time; a date before 1970 or
/// out of range reads as 1970.
pub fn local_from_system(t: std::time::SystemTime) -> ZonedDateTime {
    to_local(Timestamp::try_from(t).unwrap_or(Timestamp::UNIX_EPOCH))
}

/// `dt` in `tz`: `None` when it is skipped by a daylight saving change, its
/// first occurrence when it is repeated.
pub fn local_earliest(dt: CivilDateTime, tz: &TimeZone) -> Option<ZonedDateTime> {
    let ambiguous = tz.to_ambiguous_zoned_datetime(dt);
    if matches!(ambiguous.offset(), AmbiguousOffset::Gap { .. }) {
        return None;
    }
    ambiguous.earlier().ok()
}

/// Whole seconds from `from` to `to` (negative if `to` is before),
/// truncated toward zero.
pub fn secs_between(from: &ZonedDateTime, to: &ZonedDateTime) -> i64 {
    to.timestamp().duration_since(from.timestamp()).as_secs()
}

/// `t` moved by `secs` seconds of real time.
pub fn plus_secs(t: &ZonedDateTime, secs: i64) -> ZonedDateTime {
    t + SignedDuration::from_secs(secs)
}

/// The system clock was never set (no phone, no hardware clock): it starts
/// from 1970 or from the image build date, before 2024 in both cases.
pub fn clock_unset() -> bool {
    Timestamp::now().to_zoned_datetime(TimeZone::UTC).year() < 2024
}

/// A UTC wall time as an instant (tests, simulation).
pub fn utc(dt: CivilDateTime) -> Timestamp {
    TimeZone::UTC.to_timestamp(dt).expect("UTC date in range")
}

/// Wall time `hh:mm:00`.
pub fn hm(hour: i8, minute: i8) -> Time {
    Time::new(hour, minute, 0, 0).expect("valid hour and minute")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paris() -> TimeZone {
        TimeZone::get("Europe/Paris").unwrap()
    }

    #[test]
    fn skipped_and_repeated_local_times() {
        let gap = jiff::civil::date(2026, 3, 29).at(2, 30, 0, 0);
        assert!(local_earliest(gap, &paris()).is_none());
        let fold = jiff::civil::date(2026, 10, 25).at(2, 30, 0, 0);
        assert_eq!(local_earliest(fold, &paris()).unwrap().timestamp().as_second(), 1_792_888_200, "02:30 CEST");
        let plain = jiff::civil::date(2026, 1, 15).at(21, 0, 0, 0);
        assert_eq!(local_earliest(plain, &paris()).unwrap().timestamp().as_second(), 1_768_507_200);
    }

    #[test]
    fn real_time_arithmetic() {
        let a = local_earliest(jiff::civil::date(2026, 3, 29).at(1, 0, 0, 0), &paris()).unwrap();
        let b = local_earliest(jiff::civil::date(2026, 3, 29).at(4, 0, 0, 0), &paris()).unwrap();
        assert_eq!(secs_between(&a, &b), 2 * 3600, "one hour skipped");
        assert_eq!(secs_between(&b, &a), -2 * 3600);
        assert_eq!(plus_secs(&a, 2 * 3600), b);
    }
}
