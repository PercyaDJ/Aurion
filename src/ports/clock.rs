use crate::core::models::TimeRange;
use crate::core::time::{to_local, TimeZone, Timestamp, ZonedDateTime};

/// Port for clock/time operations.
/// Implementations: ClockMock (PC, virtual time), ClockReal (system clock).
pub trait ClockPort: Send + Sync {
    /// Get the current instant.
    fn now(&self) -> Timestamp;

    /// Current time in the device time zone (schedules are local times).
    fn now_local(&self) -> ZonedDateTime {
        to_local(self.now())
    }

    /// Check if the current time (UTC wall clock) falls within the given range.
    fn is_in_range(&self, range: &TimeRange) -> bool {
        let now_time = self.now().to_zoned_datetime(TimeZone::UTC).time();
        range.contains(now_time)
    }
}
