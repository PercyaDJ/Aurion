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

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::core::models::TimeRange;

/// Seconds the hotspot stays up before an interrupted night resumes.
pub const RESUME_DELAY_SECS: u64 = 300;
/// A failing battery must not cause an endless reboot / resume loop.
pub const MAX_RESUMES: u32 = 3;
/// In time-range mode, a marker older than this belongs to a past night.
const RANGE_MAX_AGE_HOURS: i64 = 12;

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
    pub fn new(now: DateTime<Local>, duration_hours: Option<f64>) -> Self {
        let started = now.timestamp_millis();
        Self {
            started_epoch_ms: started,
            end_epoch_ms: duration_hours.map(|h| started + (h * 3_600_000.0) as i64),
            duration_hours,
            resumes: 0,
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
    pub fn resume_plan(&self, now: DateTime<Local>, range: &TimeRange) -> Option<ResumePlan> {
        if self.resumes >= MAX_RESUMES {
            return None;
        }
        let now_ms = now.timestamp_millis();
        match (self.end_epoch_ms, self.duration_hours) {
            (Some(end), Some(total)) => {
                let remaining = (end - now_ms) as f64 / 3_600_000.0;
                // Less than a minute left: the night is over.
                (remaining > 1.0 / 60.0).then(|| ResumePlan::Timer(remaining.min(total)))
            }
            _ => {
                let age_h = (now_ms - self.started_epoch_ms) / 3_600_000;
                let recent = (0..RANGE_MAX_AGE_HOURS).contains(&age_h);
                (range.contains(now.time()) || recent).then_some(ResumePlan::Range)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveTime, TimeZone};

    fn at(h: u32, m: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 1, 15, h, m, 0).unwrap()
    }

    fn range() -> TimeRange {
        TimeRange { start: NaiveTime::from_hms_opt(21, 0, 0).unwrap(), end: NaiveTime::from_hms_opt(6, 0, 0).unwrap() }
    }

    #[test]
    fn timer_night_resumes_for_the_remaining_time_only() {
        let m = NightMarker::new(at(21, 0), Some(6.0));
        assert_eq!(m.resume_plan(at(23, 0), &range()), Some(ResumePlan::Timer(4.0)));
        assert_eq!(m.resume_plan(at(3, 0) + chrono::Duration::days(1), &range()), None, "night over");
    }

    #[test]
    fn clock_behind_never_extends_the_night() {
        // After a power cut the Pi 4 clock can be far in the past
        let m = NightMarker::new(at(21, 0), Some(6.0));
        let past = Local.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
        assert_eq!(m.resume_plan(past, &range()), Some(ResumePlan::Timer(6.0)));
    }

    #[test]
    fn range_night_resumes_inside_the_range_or_shortly_after_launch() {
        let m = NightMarker::new(at(18, 0), None);
        assert_eq!(m.resume_plan(at(19, 0), &range()), Some(ResumePlan::Range), "launched early, still waiting");
        assert_eq!(m.resume_plan(at(23, 30), &range()), Some(ResumePlan::Range));
        let next_morning = at(10, 0) + chrono::Duration::days(1);
        assert_eq!(m.resume_plan(next_morning, &range()), None, "past night");
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
