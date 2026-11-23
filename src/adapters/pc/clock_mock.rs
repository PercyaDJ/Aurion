use crate::core::time::{SignedDuration, Timestamp};
use std::sync::Mutex;
use crate::ports::clock::ClockPort;

/// PC mock clock: virtual accelerated time.
/// Allows jumping forward in time for testing.
pub struct ClockMock {
    start_time: Timestamp,
    offset: Mutex<SignedDuration>,
    _speed_factor: f64,
}

impl ClockMock {
    /// Create a clock starting at a specific time.
    pub fn new(start_time: Timestamp) -> Self {
        Self {
            start_time,
            offset: Mutex::new(SignedDuration::ZERO),
            _speed_factor: 1.0,
        }
    }

    /// Create a clock with accelerated time (e.g., 60x = 1 minute per second).
    pub fn accelerated(start_time: Timestamp, speed_factor: f64) -> Self {
        Self {
            start_time,
            offset: Mutex::new(SignedDuration::ZERO),
            _speed_factor: speed_factor,
        }
    }

    /// Advance the virtual clock by a given duration.
    pub fn advance(&self, duration: SignedDuration) {
        let mut offset = self.offset.lock().unwrap();
        *offset += duration;
    }

    /// Advance by seconds (convenience).
    pub fn advance_secs(&self, secs: i64) {
        self.advance(SignedDuration::from_secs(secs));
    }

    /// Advance by minutes (convenience).
    pub fn advance_minutes(&self, mins: i64) {
        self.advance(SignedDuration::from_mins(mins));
    }
}

impl ClockPort for ClockMock {
    fn now(&self) -> Timestamp {
        let offset = self.offset.lock().unwrap();
        self.start_time + *offset
    }
}

/// Virtual clock driven by the Tokio timer: `now = start + elapsed tokio
/// time`. With `#[tokio::test(start_paused = true)]` every `sleep` of the
/// orchestrator advances this clock instantly, so a whole night can be
/// simulated in milliseconds.
pub struct TokioClock {
    start: Timestamp,
    origin: tokio::time::Instant,
}

impl TokioClock {
    pub fn new(start: Timestamp) -> Self {
        Self { start, origin: tokio::time::Instant::now() }
    }
}

impl ClockPort for TokioClock {
    fn now(&self) -> Timestamp {
        let elapsed = tokio::time::Instant::now().duration_since(self.origin);
        self.start + SignedDuration::try_from(elapsed).unwrap_or(SignedDuration::ZERO)
    }
}

/// Real system clock (used in production).
pub struct ClockReal;

impl Default for ClockReal {
    fn default() -> Self {
        Self::new()
    }
}

impl ClockReal {
    pub fn new() -> Self {
        Self
    }
}

impl ClockPort for ClockReal {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::TimeRange;
    use crate::core::time::hm;

    fn utc(h: i8) -> Timestamp {
        crate::core::time::utc(jiff::civil::date(2025, 1, 15).at(h, 0, 0, 0))
    }

    #[test]
    fn test_mock_clock_advance() {
        let start = utc(21);
        let clock = ClockMock::new(start);
        assert_eq!(clock.now(), start);

        clock.advance_secs(3600); // +1 hour
        assert_eq!(clock.now(), utc(22));
    }

    #[tokio::test(start_paused = true)]
    async fn test_tokio_clock_follows_virtual_time() {
        let start = utc(21);
        let clock = TokioClock::new(start);
        tokio::time::sleep(std::time::Duration::from_secs(3 * 3600)).await;
        assert_eq!(clock.now(), start + SignedDuration::from_hours(3));
    }

    #[test]
    fn test_clock_in_range() {
        let clock = ClockMock::new(utc(23));

        let range = TimeRange { start: hm(21, 0), end: hm(6, 0) };
        assert!(clock.is_in_range(&range)); // 23:00 is in [21:00, 06:00]

        clock.advance_secs(8 * 3600); // +8h → 07:00
        assert!(!clock.is_in_range(&range)); // 07:00 is NOT in [21:00, 06:00]
    }
}
