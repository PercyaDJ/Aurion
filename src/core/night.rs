//! Night marker: lets a night interrupted by a power cut resume on its own.
//!
//! When a night starts, a small `night.json` is written next to the config
//! (on the SD card). It is removed only when the night ends normally. If the
//! Pi boots and finds it, the previous night was interrupted (battery empty
//! or swapped, cable pulled): the orchestrator keeps the hotspot up for
//! [`RESUME_DELAY_SECS`] so the user can cancel from the phone, then resumes
//! the capture without anyone touching the camera.
//!
//! The Raspberry Pi 4 has no battery-backed clock: after a power cut the
//! time can be wrong until a phone connects. The plan is therefore
//! re-evaluated at the end of the waiting window (a phone that connected
//! meanwhile has corrected the clock), the remaining time is capped by the
//! planned duration, and a night resumes at most [`MAX_RESUMES`] times.

use std::path::Path;

use crate::core::time::{local_earliest, Time, Timestamp, ZonedDateTime};
use serde::{Deserialize, Serialize};

use crate::core::models::TimeRange;

/// Seconds the hotspot stays up before an interrupted night resumes.
pub const RESUME_DELAY_SECS: u64 = 300;
/// A failing battery must not cause an endless reboot / resume loop.
pub const MAX_RESUMES: u32 = 3;
/// Expedition mode: nobody used the interface for this long → the night
/// starts on its own.
pub const AUTO_START_IDLE_SECS: u64 = 300;
/// Pi 5: sleep (powered off) while the night starts in more than this.
pub const SLEEP_IF_START_IN_SECS: i64 = 2 * 3600;
/// Wake-up programmed this long before the start of the night (boot, then
/// the automatic start window).
pub const WAKE_LEAD_SECS: i64 = 10 * 60;

/// Next time `t` occurs strictly after `after` (local time).
/// A time skipped by a daylight saving change is skipped that day; a repeated
/// one gives its first occurrence.
pub fn next_occurrence(after: ZonedDateTime, t: Time) -> ZonedDateTime {
    let tz = after.time_zone().clone();
    let mut day = after.date();
    loop {
        if let Some(dt) = local_earliest(day.to_datetime(t), &tz) {
            if dt > after {
                return dt;
            }
        }
        day = day.tomorrow().expect("date in range");
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NightMarker {
    /// When the night was launched (Unix milliseconds).
    pub started_epoch_ms: i64,
    /// Timer mode: planned end (Unix milliseconds). None = time range mode.
    pub end_epoch_ms: Option<i64>,
    /// Timer mode: planned duration, upper bound of any resumed capture.
    pub duration_hours: Option<f64>,
    /// Number of times this night already resumed.
    #[serde(default)]
    pub resumes: u32,
    /// Night folder on the key: a resumed night continues in it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

/// What an interrupted night should do now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResumePlan {
    /// Time range mode: resume, the range decides when to stop.
    Range,
    /// Timer mode: capture for this many more hours.
    Timer(f64),
}

impl NightMarker {
    pub fn new(now: ZonedDateTime, duration_hours: Option<f64>) -> Self {
        let started = now.timestamp().as_millisecond();
        Self {
            started_epoch_ms: started,
            end_epoch_ms: duration_hours.map(|h| started + (h * 3_600_000.0) as i64),
            duration_hours,
            resumes: 0,
            session: None,
        }
    }

    pub fn load(path: &Path) -> Option<Self> {
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_vec(self).map_err(std::io::Error::other)?;
        crate::core::config::write_atomic(path, &json, 0o600)
    }

    pub fn remove(path: &Path) {
        let _ = std::fs::remove_file(path);
    }

    /// Whether the night should resume at `now`, and how.
    pub fn resume_plan(&self, now: ZonedDateTime, range: &TimeRange) -> Option<ResumePlan> {
        if self.resumes >= MAX_RESUMES {
            return None;
        }
        let now_ms = now.timestamp().as_millisecond();
        match (self.end_epoch_ms, self.duration_hours) {
            (Some(end), Some(total)) => {
                let remaining = (end - now_ms) as f64 / 3_600_000.0;
                // Less than a minute left: the night is over.
                (remaining > 1.0 / 60.0).then(|| ResumePlan::Timer(remaining.min(total)))
            }
            _ => {
                // The night launched at `started` ends at the first range end
                // after it (launched at 14:00 for 21:00 → 06:00 tomorrow).
                let started = Timestamp::from_millisecond(self.started_epoch_ms).ok()?.to_zoned_datetime(now.time_zone().clone());
                let cycle_end = next_occurrence(started, range.end);
                (now < cycle_end).then_some(ResumePlan::Range)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::time::{hm, local_tz, plus_secs};

    const DAY: i64 = 24 * 3600;

    fn at(h: i8, m: i8) -> ZonedDateTime {
        local_earliest(jiff::civil::date(2026, 1, 15).at(h, m, 0, 0), &local_tz()).unwrap()
    }

    fn range() -> TimeRange {
        TimeRange { start: hm(21, 0), end: hm(6, 0) }
    }

    #[test]
    fn timer_night_resumes_for_the_remaining_time_only() {
        let m = NightMarker::new(at(21, 0), Some(6.0));
        assert_eq!(m.resume_plan(at(23, 0), &range()), Some(ResumePlan::Timer(4.0)));
        assert_eq!(m.resume_plan(plus_secs(&at(3, 0), DAY), &range()), None, "night over");
    }

    #[test]
    fn clock_behind_never_extends_the_night() {
        // After a power cut the Pi 4 clock can be far in the past
        let m = NightMarker::new(at(21, 0), Some(6.0));
        let past = local_earliest(jiff::civil::date(2020, 1, 1).at(0, 0, 0, 0), &local_tz()).unwrap();
        assert_eq!(m.resume_plan(past, &range()), Some(ResumePlan::Timer(6.0)));
    }

    #[test]
    fn range_night_resumes_inside_the_range_or_shortly_after_launch() {
        let m = NightMarker::new(at(18, 0), None);
        assert_eq!(m.resume_plan(at(19, 0), &range()), Some(ResumePlan::Range), "launched early, still waiting");
        assert_eq!(m.resume_plan(at(23, 30), &range()), Some(ResumePlan::Range));
        let next_morning = plus_secs(&at(10, 0), DAY);
        assert_eq!(m.resume_plan(next_morning, &range()), None, "past night");
    }

    #[test]
    fn range_night_launched_in_the_morning_still_resumes_in_the_evening() {
        // Pi 5 powered off until the night, or battery swapped in the afternoon
        let m = NightMarker::new(at(7, 0), None);
        assert_eq!(m.resume_plan(at(20, 50), &range()), Some(ResumePlan::Range));
        assert_eq!(m.resume_plan(plus_secs(&at(5, 59), DAY), &range()), Some(ResumePlan::Range));
        assert_eq!(m.resume_plan(plus_secs(&at(6, 1), DAY), &range()), None, "that night is over");
    }

    #[test]
    fn next_occurrences() {
        let t = hm(21, 0);
        assert_eq!(next_occurrence(at(14, 0), t), at(21, 0));
        assert_eq!(next_occurrence(at(21, 0), t), plus_secs(&at(21, 0), DAY), "strictly after");
        assert_eq!(next_occurrence(at(23, 0), t), plus_secs(&at(21, 0), DAY));
    }

    #[test]
    fn resumes_are_limited() {
        let mut m = NightMarker::new(at(21, 0), Some(6.0));
        m.resumes = MAX_RESUMES;
        assert_eq!(m.resume_plan(at(22, 0), &range()), None);
    }

    #[test]
    fn marker_roundtrip_and_garbage() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("night.json");
        assert!(NightMarker::load(&p).is_none());
        let m = NightMarker::new(at(21, 0), Some(2.5));
        m.save(&p).unwrap();
        assert_eq!(NightMarker::load(&p), Some(m));
        std::fs::write(&p, b"{broken").unwrap();
        assert!(NightMarker::load(&p).is_none(), "a corrupt marker is ignored");
        NightMarker::remove(&p);
        assert!(!p.exists());
    }
}
