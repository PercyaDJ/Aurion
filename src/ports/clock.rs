use chrono::{DateTime, Local, Utc};
use crate::core::models::TimeRange;

/// Port for clock/time operations.
/// Implementations: ClockMock (PC, virtual time), ClockReal (system clock).
pub trait ClockPort: Send + Sync {
    /// Get the current UTC time.
    fn now(&self) -> DateTime<Utc>;

    /// Current time in the device time zone (schedules are local times).
    fn now_local(&self) -> DateTime<Local> {
        self.now().with_timezone(&Local)
    }

    /// Check if the current time falls within the given range.
    fn is_in_range(&self, range: &TimeRange) -> bool {
        let now_time = self.now().time();
        range.contains(now_time)
    }
}
