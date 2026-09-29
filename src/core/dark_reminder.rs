//! "Faites vos darks" reminder.
//!
//! Darks must be taken with the same ISO, the same exposure time and, as far
//! as possible, the same sensor temperature as the night. During the
//! capture, [`NightStats`] accumulates the settings of every saved frame; at
//! the end of the night the averages are written as a [`DarkReminder`] in
//! `dark_reminder.json`, next to the config (SD card). At the next power-on
//! the home screen shows it with a button that starts the darks at these
//! settings. The file is removed once a dark series at these settings
//! succeeds, or when the user dismisses the reminder.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Settings accumulated over the saved frames of a night.
#[derive(Debug, Clone, Default)]
pub struct NightStats {
    frames: u32,
    iso_sum: u64,
    shutter_sum: u64,
    iso_min: u32,
    iso_max: u32,
    shutter_min: u64,
    shutter_max: u64,
    temp_sum: f64,
    temp_count: u32,
}

impl NightStats {
    pub fn new() -> Self {
        Self::default()
    }

    /// One saved frame, with the processor temperature when known.
    pub fn add(&mut self, iso: u32, shutter_us: u64, temp_c: Option<f64>) {
        if self.frames == 0 {
            self.iso_min = iso;
            self.iso_max = iso;
            self.shutter_min = shutter_us;
            self.shutter_max = shutter_us;
        }
        self.frames += 1;
        self.iso_sum += iso as u64;
        self.shutter_sum += shutter_us;
        self.iso_min = self.iso_min.min(iso);
        self.iso_max = self.iso_max.max(iso);
        self.shutter_min = self.shutter_min.min(shutter_us);
        self.shutter_max = self.shutter_max.max(shutter_us);
        if let Some(t) = temp_c.filter(|t| t.is_finite()) {
            self.temp_sum += t;
            self.temp_count += 1;
        }
    }

    pub fn frames(&self) -> u32 {
        self.frames
    }

    /// Averages of the night, None when no frame was saved (nothing to
    /// calibrate). The exposure is rounded to the millisecond.
    pub fn reminder(&self, night: Option<String>, ended_epoch_ms: i64) -> Option<DarkReminder> {
        if self.frames == 0 {
            return None;
        }
        let n = self.frames as u64;
        let shutter_ms = (self.shutter_sum + n * 500) / n / 1000;
        Some(DarkReminder {
            night,
            ended_epoch_ms,
            frames: self.frames,
            iso: ((self.iso_sum + n / 2) / n) as u32,
            shutter_us: shutter_ms.max(1) * 1000,
            iso_min: self.iso_min,
            iso_max: self.iso_max,
            shutter_min_us: self.shutter_min,
            shutter_max_us: self.shutter_max,
            temp_c: (self.temp_count > 0).then(|| round1(self.temp_sum / self.temp_count as f64)),
        })
    }
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// Darks to take after a night, shown on the home screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DarkReminder {
    /// Night folder on the key (`sessions/<night>`).
    pub night: Option<String>,
    /// End of the night (Unix milliseconds).
    pub ended_epoch_ms: i64,
    /// Number of saved frames the averages come from.
    pub frames: u32,
    /// Average ISO: the darks are taken at this value.
    pub iso: u32,
    /// Average exposure time (µs, rounded to the millisecond).
    pub shutter_us: u64,
    pub iso_min: u32,
    pub iso_max: u32,
    pub shutter_min_us: u64,
    pub shutter_max_us: u64,
    /// Average processor temperature (°C), close to the sensor one. None
    /// when not available (PC, no thermal sensor).
    pub temp_c: Option<f64>,
}

impl DarkReminder {
    /// Merge with the reminder of the same night written before a resume
    /// (power cut then restart): averages weighted by the number of frames.
    pub fn merge(self, previous: Option<DarkReminder>) -> Self {
        let Some(p) = previous.filter(|p| p.night.is_some() && p.night == self.night) else {
            return self;
        };
        let total = (p.frames + self.frames).max(1) as f64;
        let weighted = |a: f64, b: f64| (a * p.frames as f64 + b * self.frames as f64) / total;
        let shutter_ms = (weighted(p.shutter_us as f64, self.shutter_us as f64) / 1000.0).round().max(1.0) as u64;
        Self {
            night: self.night.clone(),
            ended_epoch_ms: self.ended_epoch_ms,
            frames: p.frames + self.frames,
            iso: weighted(p.iso as f64, self.iso as f64).round() as u32,
            shutter_us: shutter_ms * 1000,
            iso_min: p.iso_min.min(self.iso_min),
            iso_max: p.iso_max.max(self.iso_max),
            shutter_min_us: p.shutter_min_us.min(self.shutter_min_us),
            shutter_max_us: p.shutter_max_us.max(self.shutter_max_us),
            temp_c: match (p.temp_c, self.temp_c) {
                (Some(a), Some(b)) => Some(round1(weighted(a, b))),
                (a, b) => b.or(a),
            },
        }
    }

    /// A dark series at these settings answers the reminder.
    pub fn matches(&self, iso: u32, shutter_us: u64) -> bool {
        self.iso == iso && self.shutter_us == shutter_us
    }

    /// One line for `session.log` and the web log.
    pub fn summary(&self) -> String {
        let mut s = format!("Darks à faire : ISO {} / {:.1} s", self.iso, self.shutter_us as f64 / 1e6);
        if let Some(t) = self.temp_c {
            s.push_str(&format!(" / {:.0} °C", t));
        }
        s.push_str(&format!(" (moyenne de {} images)", self.frames));
        s
    }

    pub fn load(path: &Path) -> Option<Self> {
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        crate::core::config::write_atomic(path, &json, 0o600)
    }

    pub fn remove(path: &Path) {
        let _ = std::fs::remove_file(path);
    }

    /// A dark series succeeded: the reminder goes away if it asked for
    /// these settings. Returns true when it was removed.
    pub fn clear_if_matching(path: &Path, iso: u32, shutter_us: u64) -> bool {
        match Self::load(path) {
            Some(r) if r.matches(iso, shutter_us) => {
                Self::remove(path);
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_frame_no_reminder() {
        assert!(NightStats::new().reminder(None, 0).is_none());
    }

    #[test]
    fn averages_of_the_night() {
        let mut s = NightStats::new();
        s.add(800, 4_000_000, Some(40.0));
        s.add(1600, 8_000_000, Some(45.0));
        s.add(1600, 6_000_400, None);
        let r = s.reminder(Some("2026-01-15_21-00".into()), 42).unwrap();
        assert_eq!(r.frames, 3);
        assert_eq!(r.iso, 1333);
        assert_eq!(r.shutter_us, 6_000_000, "rounded to the millisecond");
        assert_eq!((r.iso_min, r.iso_max), (800, 1600));
        assert_eq!((r.shutter_min_us, r.shutter_max_us), (4_000_000, 8_000_000));
        assert_eq!(r.temp_c, Some(42.5), "frames without temperature are ignored");
        assert_eq!(r.night.as_deref(), Some("2026-01-15_21-00"));
        assert_eq!(r.ended_epoch_ms, 42);
    }

    #[test]
    fn locked_exposure_gives_exact_settings_and_no_temperature_on_pc() {
        let mut s = NightStats::new();
        for _ in 0..100 {
            s.add(1600, 5_000_000, None);
        }
        let r = s.reminder(None, 0).unwrap();
        assert!(r.matches(1600, 5_000_000));
        assert_eq!(r.temp_c, None);
        assert_eq!(r.summary(), "Darks à faire : ISO 1600 / 5.0 s (moyenne de 100 images)");
    }

    #[test]
    fn very_short_exposures_stay_valid() {
        let mut s = NightStats::new();
        s.add(100, 200, None);
        assert_eq!(s.reminder(None, 0).unwrap().shutter_us, 1000);
    }

    #[test]
    fn resumed_night_merges_with_the_first_part() {
        let mut a = NightStats::new();
        for _ in 0..3 {
            a.add(800, 4_000_000, Some(40.0));
        }
        let mut b = NightStats::new();
        b.add(1600, 8_000_000, Some(50.0));
        let night = Some("n".to_string());
        let first = a.reminder(night.clone(), 1).unwrap();
        let merged = b.reminder(night, 2).unwrap().merge(Some(first));
        assert_eq!(merged.frames, 4);
        assert_eq!(merged.iso, 1000);
        assert_eq!(merged.shutter_us, 5_000_000);
        assert_eq!(merged.temp_c, Some(42.5));
        assert_eq!((merged.iso_min, merged.iso_max), (800, 1600));
        assert_eq!(merged.ended_epoch_ms, 2);
    }

    #[test]
    fn another_night_replaces_the_old_reminder() {
        let mut s = NightStats::new();
        s.add(800, 4_000_000, None);
        let old = s.reminder(Some("old".into()), 1).unwrap();
        let new = s.reminder(Some("new".into()), 2).unwrap();
        assert_eq!(new.clone().merge(Some(old)), new);
    }

    #[test]
    fn persistence_and_clearing() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("dark_reminder.json");
        assert!(DarkReminder::load(&p).is_none());
        let mut s = NightStats::new();
        s.add(1600, 5_000_000, Some(38.2));
        let r = s.reminder(Some("n".into()), 7).unwrap();
        r.save(&p).unwrap();
        assert_eq!(DarkReminder::load(&p), Some(r));
        // Darks at other settings: the reminder stays
        assert!(!DarkReminder::clear_if_matching(&p, 800, 5_000_000));
        assert!(p.exists());
        // Darks at the night settings: it goes away
        assert!(DarkReminder::clear_if_matching(&p, 1600, 5_000_000));
        assert!(!p.exists());
        // A corrupt file is ignored
        std::fs::write(&p, b"{broken").unwrap();
        assert!(DarkReminder::load(&p).is_none());
    }
}
