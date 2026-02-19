use chrono::{DateTime, Duration, Utc};
use std::sync::Mutex;
use crate::ports::clock::ClockPort;

/// PC mock clock: virtual accelerated time.
/// Allows jumping forward in time for testing.
pub struct ClockMock {
    start_time: DateTime<Utc>,
    offset: Mutex<Duration>,
    speed_factor: f64,
}

impl ClockMock {
    /// Create a clock starting at a specific time.
    pub fn new(start_time: DateTime<Utc>) -> Self {
        Self {
            start_time,
            offset: Mutex::new(Duration::zero()),
            speed_factor: 1.0,
        }
    }

    /// Create a clock with accelerated time (e.g., 60x = 1 minute per second).
    pub fn accelerated(start_time: DateTime<Utc>, speed_factor: f64) -> Self {
        Self {
            start_time,
            offset: Mutex::new(Duration::zero()),
            speed_factor,
        }
    }

    /// Advance the virtual clock by a given duration.
    pub fn advance(&self, duration: Duration) {
        let mut offset = self.offset.lock().unwrap();
        *offset = *offset + duration;
    }

    /// Advance by seconds (convenience).
    pub fn advance_secs(&self, secs: i64) {
        self.advance(Duration::seconds(secs));
    }

    /// Advance by minutes (convenience).
    pub fn advance_minutes(&self, mins: i64) {
        self.advance(Duration::minutes(mins));
    }
}

impl ClockPort for ClockMock {
    fn now(&self) -> DateTime<Utc> {
        let offset = self.offset.lock().unwrap();
        self.start_time + *offset
    }
}

/// Real system clock (used in production).
pub struct ClockReal;

impl ClockReal {
    pub fn new() -> Self {
        Self
    }
}

impl ClockPort for ClockReal {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveTime, TimeZone};
    use crate::core::models::TimeRange;

    #[test]
    fn test_mock_clock_advance() {
        let start = Utc.with_ymd_and_hms(2025, 1, 15, 21, 0, 0).unwrap();
        let clock = ClockMock::new(start);
        assert_eq!(clock.now(), start);

        clock.advance_secs(3600); // +1 hour
        let expected = Utc.with_ymd_and_hms(2025, 1, 15, 22, 0, 0).unwrap();
        assert_eq!(clock.now(), expected);
    }

    #[test]
    fn test_clock_in_range() {
        let start = Utc.with_ymd_and_hms(2025, 1, 15, 23, 0, 0).unwrap();
        let clock = ClockMock::new(start);

        let range = TimeRange {
            start: NaiveTime::from_hms_opt(21, 0, 0).unwrap(),
            end: NaiveTime::from_hms_opt(6, 0, 0).unwrap(),
        };
        assert!(clock.is_in_range(&range)); // 23:00 is in [21:00, 06:00]

        clock.advance_secs(8 * 3600); // +8h → 07:00
        assert!(!clock.is_in_range(&range)); // 07:00 is NOT in [21:00, 06:00]
    }
}
