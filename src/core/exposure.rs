use crate::core::models::ExposureSettings;

/// Exposure control engine — pure logic, no hardware access.
///
/// Two modes:
/// 1. **Calibration**: find optimal ISO/shutter within user bounds, prioritizing anti-burn (p99).
/// 2. **RUN adjustment**: smooth ±EV per frame, anti-yoyo.
pub struct ExposureController {
    iso_min: u32,
    iso_max: u32,
    shutter_min_us: u64,
    shutter_max_us: u64,
    ev_step_max: f64,
    current: ExposureSettings,
    /// Momentum for anti-yoyo smoothing
    ev_momentum: f64,
}

impl ExposureController {
    pub fn new(
        iso_min: u32,
        iso_max: u32,
        shutter_min_us: u64,
        shutter_max_us: u64,
        ev_step_max: f64,
    ) -> Self {
        // Start at middle of the range
        let iso = (iso_min + iso_max) / 2;
        let shutter = (shutter_min_us + shutter_max_us) / 2;

        Self {
            iso_min,
            iso_max,
            shutter_min_us,
            shutter_max_us,
            ev_step_max,
            current: ExposureSettings::new(iso, shutter),
            ev_momentum: 0.0,
        }
    }

    pub fn current(&self) -> ExposureSettings {
        self.current
    }

    /// Calibration: analyze histogram and adjust exposure.
    /// `histogram` is a 256-element array of pixel counts.
    /// Returns the adjusted exposure settings.
    pub fn calibrate(&mut self, histogram: &[u32; 256]) -> ExposureSettings {
        let total_pixels: u32 = histogram.iter().sum();
        if total_pixels == 0 {
            return self.current;
        }

        // Compute p99 (99th percentile brightness)
        let p99_target = (total_pixels as f64 * 0.99) as u32;
        let mut cumulative = 0u32;
        let mut p99_value = 0u32;
        for (brightness, &count) in histogram.iter().enumerate() {
            cumulative += count;
            if cumulative >= p99_target {
                p99_value = brightness as u32;
                break;
            }
        }

        // Compute mean brightness
        let mean: f64 = histogram
            .iter()
            .enumerate()
            .map(|(b, &c)| b as f64 * c as f64)
            .sum::<f64>()
            / total_pixels as f64;

        // Strategy:
        // - If p99 > 240: overexposed → reduce significantly
        // - If p99 > 200: slightly overexposed → reduce a bit
        // - If mean < 30: too dark → increase (but respect p99)
        // - If mean < 60: slightly dark → increase gently

        let ev_adjustment: f64 = if p99_value > 240 {
            -1.0 // Reduce exposure significantly
        } else if p99_value > 200 {
            -0.33 // Reduce slightly
        } else if mean < 30.0 && p99_value < 180 {
            0.66 // Increase more
        } else if mean < 60.0 && p99_value < 200 {
            0.33 // Increase gently
        } else {
            0.0 // Good exposure
        };

        if ev_adjustment.abs() > 0.01 {
            self.apply_ev_adjustment(ev_adjustment);
        }

        self.current
    }

    /// RUN-mode per-frame adjustment: smooth, limited change.
    /// `histogram` is the current frame's histogram.
    pub fn adjust_frame(&mut self, histogram: &[u32; 256]) -> ExposureSettings {
        let total_pixels: u32 = histogram.iter().sum();
        if total_pixels == 0 {
            return self.current;
        }

        // Compute p99
        let p99_target = (total_pixels as f64 * 0.99) as u32;
        let mut cumulative = 0u32;
        let mut p99_value = 0u32;
        for (brightness, &count) in histogram.iter().enumerate() {
            cumulative += count;
            if cumulative >= p99_target {
                p99_value = brightness as u32;
                break;
            }
        }

        // Compute mean
        let mean: f64 = histogram
            .iter()
            .enumerate()
            .map(|(b, &c)| b as f64 * c as f64)
            .sum::<f64>()
            / total_pixels as f64;

        // Target: mean around 50-80, p99 under 220
        let raw_adjustment = if p99_value > 220 {
            -0.15
        } else if mean < 40.0 && p99_value < 200 {
            0.15
        } else if mean > 100.0 {
            -0.10
        } else {
            0.0
        };

        // Apply momentum for anti-yoyo
        // Momentum smoothing: new_momentum = 0.7 * old + 0.3 * raw
        self.ev_momentum = self.ev_momentum * 0.7 + raw_adjustment * 0.3;

        // Clamp to max EV step
        let clamped = self.ev_momentum.clamp(-self.ev_step_max, self.ev_step_max);

        if clamped.abs() > 0.01 {
            self.apply_ev_adjustment(clamped);
        }

        self.current
    }

    /// Apply an EV adjustment by modifying shutter first, then ISO.
    fn apply_ev_adjustment(&mut self, ev_delta: f64) {
        // Convert EV delta to a multiplier: 2^ev_delta
        let multiplier = 2.0_f64.powf(ev_delta);

        // Adjust shutter first
        let new_shutter = (self.current.shutter_us as f64 * multiplier) as u64;
        let clamped_shutter = new_shutter.clamp(self.shutter_min_us, self.shutter_max_us);

        // If shutter hit a limit, adjust ISO to compensate
        let actual_shutter_mult = clamped_shutter as f64 / self.current.shutter_us as f64;
        let remaining_mult = multiplier / actual_shutter_mult;

        let new_iso = (self.current.iso as f64 * remaining_mult) as u32;
        let clamped_iso = new_iso.clamp(self.iso_min, self.iso_max);

        self.current = ExposureSettings::new(clamped_iso, clamped_shutter);
    }

    /// Reset exposure to initial values.
    pub fn reset(&mut self) {
        let iso = (self.iso_min + self.iso_max) / 2;
        let shutter = (self.shutter_min_us + self.shutter_max_us) / 2;
        self.current = ExposureSettings::new(iso, shutter);
        self.ev_momentum = 0.0;
    }

    /// Set exposure directly (e.g., from config or preset).
    pub fn set(&mut self, settings: ExposureSettings) {
        self.current = ExposureSettings::new(
            settings.iso.clamp(self.iso_min, self.iso_max),
            settings.shutter_us.clamp(self.shutter_min_us, self.shutter_max_us),
        );
    }
}

/// Compute a 256-bin histogram from RGB image data (using luminance).
pub fn compute_histogram(rgb_data: &[u8]) -> [u32; 256] {
    let mut hist = [0u32; 256];
    let pixel_count = rgb_data.len() / 3;
    for i in 0..pixel_count {
        let r = rgb_data[i * 3] as f64;
        let g = rgb_data[i * 3 + 1] as f64;
        let b = rgb_data[i * 3 + 2] as f64;
        // Standard luminance formula
        let lum = (0.299 * r + 0.587 * g + 0.114 * b) as u8;
        hist[lum as usize] += 1;
    }
    hist
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_histogram_for_brightness(brightness: u8) -> [u32; 256] {
        let mut hist = [0u32; 256];
        hist[brightness as usize] = 10000;
        hist
    }

    fn make_spread_histogram(mean: u8, spread: u8) -> [u32; 256] {
        let mut hist = [0u32; 256];
        let low = mean.saturating_sub(spread);
        let high = mean.saturating_add(spread);
        for b in low..=high {
            hist[b as usize] = 1000;
        }
        hist
    }

    #[test]
    fn test_calibrate_reduces_overexposed() {
        let mut ctrl = ExposureController::new(100, 3200, 1_000_000, 30_000_000, 0.33);
        let initial = ctrl.current();

        // Overexposed histogram (p99 at 250)
        let hist = make_histogram_for_brightness(250);
        ctrl.calibrate(&hist);

        assert!(ctrl.current().ev() < initial.ev(), "Should reduce exposure for overexposed image");
    }

    #[test]
    fn test_calibrate_increases_dark() {
        let mut ctrl = ExposureController::new(100, 3200, 1_000_000, 30_000_000, 0.33);
        let initial = ctrl.current();

        // Very dark histogram
        let hist = make_histogram_for_brightness(10);
        ctrl.calibrate(&hist);

        assert!(ctrl.current().ev() > initial.ev(), "Should increase exposure for dark image");
    }

    #[test]
    fn test_frame_adjustment_limited() {
        let mut ctrl = ExposureController::new(100, 3200, 1_000_000, 30_000_000, 0.33);
        let initial_ev = ctrl.current().ev();

        // Multiple frame adjustments should not cause big jumps
        let hist = make_histogram_for_brightness(20);
        for _ in 0..5 {
            ctrl.adjust_frame(&hist);
        }

        let ev_change = (ctrl.current().ev() - initial_ev).abs();
        assert!(ev_change < 2.0, "Frame adjustment should be gradual, got {} EV change", ev_change);
    }

    #[test]
    fn test_stays_within_bounds() {
        let mut ctrl = ExposureController::new(100, 800, 1_000_000, 10_000_000, 0.33);

        // Push very bright → should reduce to bounds
        let hist = make_histogram_for_brightness(250);
        for _ in 0..20 {
            ctrl.calibrate(&hist);
        }

        assert!(ctrl.current().iso >= 100);
        assert!(ctrl.current().iso <= 800);
        assert!(ctrl.current().shutter_us >= 1_000_000);
        assert!(ctrl.current().shutter_us <= 10_000_000);
    }

    #[test]
    fn test_compute_histogram() {
        // All mid-gray pixels (r=128, g=128, b=128 → luminance ≈ 128)
        let data = vec![128, 128, 128, 128, 128, 128]; // 2 pixels
        let hist = compute_histogram(&data);
        // Luminance = 0.299*128 + 0.587*128 + 0.114*128 ≈ 128
        let lum = (0.299 * 128.0 + 0.587 * 128.0 + 0.114 * 128.0) as u8;
        assert_eq!(hist[lum as usize], 2);
    }

    #[test]
    fn test_reset() {
        let mut ctrl = ExposureController::new(100, 3200, 1_000_000, 30_000_000, 0.33);
        // Calibrate multiple times with bright image to move away from midpoint
        let hist = make_histogram_for_brightness(250);
        for _ in 0..5 {
            ctrl.calibrate(&hist);
        }
        let after_calib = ctrl.current();

        ctrl.reset();
        let after_reset = ctrl.current();

        // After reset, should be back to middle of range
        let mid_iso = (100 + 3200) / 2;
        assert_eq!(after_reset.iso, mid_iso);
        // Calibration should have moved ISO down from the middle
        assert!(after_calib.iso <= mid_iso || after_calib.shutter_us < after_reset.shutter_us,
            "Calibration with bright image should reduce exposure");
    }
}
