use crate::core::models::{ExposureSettings, Phase};

/// Exposure control engine — pure logic, no hardware access.
///
/// Uses log-space EMA control with per-phase rate limiting:
/// - **Detect/Calibration**: faster adaptation (alpha=0.08, ±15%/20%)
/// - **Capture/Run**: very stable for timelapse (alpha=0.03, ±3%/5%)
///
/// Metering: percentile P80 on ROI histogram, rejecting saturated pixels.
pub struct ExposureController {
    // User bounds
    iso_min: u32,
    iso_max: u32,
    shutter_min_us: u64,
    shutter_max_us: u64,

    // Current settings
    current: ExposureSettings,

    // Log-space EMA accumulator
    log_state: f64,

    // Per-phase control params
    detect_alpha: f64,
    detect_rate_up: f64,
    detect_rate_down: f64,
    capture_alpha: f64,
    capture_rate_up: f64,
    capture_rate_down: f64,

    // Metering params
    target_percentile: f64,
    target_brightness: f64,
    saturation_reject: f64,
}

impl ExposureController {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        iso_min: u32,
        iso_max: u32,
        shutter_min_us: u64,
        shutter_max_us: u64,
        detect_alpha: f64,
        detect_rate_up: f64,
        detect_rate_down: f64,
        capture_alpha: f64,
        capture_rate_up: f64,
        capture_rate_down: f64,
        target_percentile: f64,
        target_brightness: f64,
        saturation_reject: f64,
    ) -> Self {
        // Start at minimum exposure (conservative)
        let initial = ExposureSettings::new(iso_min, shutter_min_us);
        let log_state = (initial.shutter_us as f64).ln();

        Self {
            iso_min,
            iso_max,
            shutter_min_us,
            shutter_max_us,
            current: initial,
            log_state,
            detect_alpha,
            detect_rate_up,
            detect_rate_down,
            capture_alpha,
            capture_rate_up,
            capture_rate_down,
            target_percentile,
            target_brightness,
            saturation_reject,
        }
    }

    /// Create from config (convenience).
    pub fn from_config(cfg: &crate::core::config::ExposureConfig) -> Self {
        Self::new(
            cfg.iso_min,
            cfg.iso_max,
            cfg.shutter_min_us,
            cfg.shutter_max_us,
            cfg.detect_alpha,
            cfg.detect_rate_up,
            cfg.detect_rate_down,
            cfg.capture_alpha,
            cfg.capture_rate_up,
            cfg.capture_rate_down,
            cfg.target_percentile,
            cfg.target_brightness,
            cfg.saturation_reject,
        )
    }

    pub fn current(&self) -> ExposureSettings {
        self.current
    }

    /// Main entry point: update exposure based on histogram and current phase.
    /// Returns the new exposure settings.
    pub fn update(&mut self, histogram: &[u32; 256], phase: Phase) -> ExposureSettings {
        // Pick alpha and rate limits based on phase
        let (alpha, rate_up, rate_down) = match phase {
            Phase::Run => (self.capture_alpha, self.capture_rate_up, self.capture_rate_down),
            _ => (self.detect_alpha, self.detect_rate_up, self.detect_rate_down),
        };

        // Measure: robust percentile on histogram (rejecting saturated)
        let measured = self.compute_percentile(histogram);
        if measured < 0.01 {
            // Image is essentially black — increase exposure significantly
            let mult = 1.0 + rate_up;
            self.apply_multiplier(mult);
            return self.current;
        }

        // Error in log-space: positive = too dark, negative = too bright
        let err = (self.target_brightness / measured).ln();

        // EMA smooth
        self.log_state += alpha * err;

        // Convert log_state to a multiplier relative to current exposure
        let current_ev = (self.current.shutter_us as f64 * self.current.iso as f64 / 100.0).ln();
        let target_ev = current_ev + self.log_state;

        // Limit the change per frame (rate limiter)
        let delta = target_ev - current_ev;
        let clamped_delta = if delta > 0.0 {
            delta.min(rate_up.ln_1p()) // ln(1 + rate_up) ≈ rate_up for small values
        } else {
            delta.max(-(rate_down.ln_1p()))
        };

        let mult = clamped_delta.exp();
        self.apply_multiplier(mult);

        // Reset log_state to avoid integral windup
        self.log_state = clamped_delta;

        self.current
    }

    /// One-shot metering used by the preview: correct the exposure directly
    /// towards the target (no EMA, no rate limit), bounded to ×8 / ÷8 per
    /// call so a single bad frame cannot send it to an extreme.
    pub fn jump(&mut self, histogram: &[u32; 256]) -> ExposureSettings {
        let measured = self.compute_percentile(histogram);
        let mult = if measured < 1.0 {
            8.0
        } else {
            (self.target_brightness / measured).clamp(0.125, 8.0)
        };
        self.apply_multiplier(mult);
        self.log_state = 0.0;
        self.current
    }

    /// Compute the percentile brightness from histogram, rejecting saturated pixels.
    fn compute_percentile(&self, histogram: &[u32; 256]) -> f64 {
        let sat_limit = self.saturation_reject as usize;
        let sat_limit = sat_limit.min(255);

        // Count non-saturated pixels
        let total: u32 = histogram[..=sat_limit].iter().sum();
        if total == 0 {
            return 0.0;
        }

        let target_count = (total as f64 * self.target_percentile) as u32;
        let mut cumulative = 0u32;

        for (brightness, &count) in histogram[..=sat_limit].iter().enumerate() {
            cumulative += count;
            if cumulative >= target_count {
                return brightness as f64;
            }
        }

        sat_limit as f64
    }

    /// Apply a multiplicative change to exposure (shutter first, then ISO).
    fn apply_multiplier(&mut self, multiplier: f64) {
        // Adjust shutter first
        let new_shutter = (self.current.shutter_us as f64 * multiplier) as u64;
        let clamped_shutter = new_shutter.clamp(self.shutter_min_us, self.shutter_max_us);

        // If shutter hit a limit, compensate with ISO
        let actual_shutter_mult = clamped_shutter as f64 / self.current.shutter_us.max(1) as f64;
        let remaining_mult = multiplier / actual_shutter_mult;

        let new_iso = (self.current.iso as f64 * remaining_mult) as u32;
        let clamped_iso = new_iso.clamp(self.iso_min, self.iso_max);

        self.current = ExposureSettings::new(clamped_iso, clamped_shutter);
    }

    /// Reset exposure to minimum (start of session).
    pub fn reset(&mut self) {
        self.current = ExposureSettings::new(self.iso_min, self.shutter_min_us);
        self.log_state = 0.0;
    }

    /// Set exposure directly (e.g., from config or preset).
    pub fn set(&mut self, settings: ExposureSettings) {
        self.current = ExposureSettings::new(
            settings.iso.clamp(self.iso_min, self.iso_max),
            settings.shutter_us.clamp(self.shutter_min_us, self.shutter_max_us),
        );
        self.log_state = 0.0;
    }
}

/// Compute a 256-bin histogram from RGB image data (using luminance).
/// This takes **ROI-cropped** data — the caller must crop to roi_top_percent first.
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
        hist[brightness as usize] = 1000;
        hist
    }

    fn make_spread_histogram(center: u8, spread: u8) -> [u32; 256] {
        let mut hist = [0u32; 256];
        let lo = center.saturating_sub(spread);
        let hi = center.saturating_add(spread);
        for b in lo..=hi {
            hist[b as usize] = 100;
        }
        hist
    }

    fn default_controller() -> ExposureController {
        ExposureController::new(
            100, 3200,       // ISO range
            1_000_000, 30_000_000, // shutter range
            0.08, 0.15, 0.20,     // detect alpha, rate up/down
            0.03, 0.03, 0.05,     // capture alpha, rate up/down
            0.80, 60.0, 250.0,    // percentile, target, sat reject
        )
    }

    #[test]
    fn test_dark_image_increases_exposure() {
        let mut ctrl = default_controller();
        let hist = make_histogram_for_brightness(10); // Very dark
        let before = ctrl.current();
        let after = ctrl.update(&hist, Phase::Calibration);
        // Should increase exposure
        assert!(after.shutter_us > before.shutter_us || after.iso > before.iso,
            "Exposure should increase for dark image: before={:?}, after={:?}", before, after);
    }

    #[test]
    fn test_bright_image_decreases_exposure() {
        let mut ctrl = default_controller();
        // Set to high exposure first
        ctrl.set(ExposureSettings::new(1600, 15_000_000));
        let hist = make_histogram_for_brightness(200); // Very bright
        let before = ctrl.current();
        let after = ctrl.update(&hist, Phase::Calibration);
        assert!(after.shutter_us < before.shutter_us || after.iso < before.iso,
            "Exposure should decrease for bright image: before={:?}, after={:?}", before, after);
    }

    #[test]
    fn test_correct_exposure_stays_stable() {
        let mut ctrl = default_controller();
        let hist = make_histogram_for_brightness(60); // At target
        let before = ctrl.current();
        let after = ctrl.update(&hist, Phase::Run);
        // Should stay very close
        let delta = (after.shutter_us as f64 - before.shutter_us as f64).abs()
            / before.shutter_us as f64;
        assert!(delta < 0.1, "Exposure should be stable at target: delta={:.3}", delta);
    }

    #[test]
    fn test_capture_phase_limits_change() {
        let mut ctrl = default_controller();
        ctrl.set(ExposureSettings::new(800, 10_000_000));
        let hist = make_histogram_for_brightness(10); // Very dark
        let before = ctrl.current();
        let after = ctrl.update(&hist, Phase::Run);
        // In capture mode, rate_up=3% — change should be small
        let change = after.shutter_us as f64 / before.shutter_us as f64;
        assert!(change < 1.20, "Capture mode should limit change: ratio={:.3}", change);
    }

    #[test]
    fn test_stays_within_bounds() {
        let mut ctrl = default_controller();
        // Push very dark: should not exceed bounds
        let hist = make_histogram_for_brightness(0);
        for _ in 0..100 {
            ctrl.update(&hist, Phase::Calibration);
        }
        assert!(ctrl.current().shutter_us <= 30_000_000);
        assert!(ctrl.current().iso <= 3200);

        // Push very bright: should not go below bounds
        let hist = make_histogram_for_brightness(255);
        for _ in 0..100 {
            ctrl.update(&hist, Phase::Calibration);
        }
        assert!(ctrl.current().shutter_us >= 1_000_000);
        assert!(ctrl.current().iso >= 100);
    }

    #[test]
    fn test_percentile_computation() {
        let ctrl = default_controller();
        let hist = make_spread_histogram(80, 20);
        let p = ctrl.compute_percentile(&hist);
        // P80 of uniform [60..100] should be around 92
        assert!(p > 85.0 && p < 100.0, "Percentile={}", p);
    }

    #[test]
    fn test_reset() {
        let mut ctrl = default_controller();
        ctrl.set(ExposureSettings::new(1600, 15_000_000));
        ctrl.reset();
        assert_eq!(ctrl.current().iso, 100);
        assert_eq!(ctrl.current().shutter_us, 1_000_000);
    }

    #[test]
    fn test_jump_converges_fast() {
        let mut ctrl = default_controller();
        ctrl.set(ExposureSettings::new(400, 4_000_000));
        // Frame twice too dark → exposure doubles in a single step.
        let before = ctrl.current();
        let after = ctrl.jump(&make_histogram_for_brightness(30));
        let ratio = (after.shutter_us as f64 * after.iso as f64)
            / (before.shutter_us as f64 * before.iso as f64);
        assert!((ratio - 2.0).abs() < 0.05, "ratio={}", ratio);
        // Black frame → bounded ×8, never beyond the limits.
        for _ in 0..10 { ctrl.jump(&make_histogram_for_brightness(0)); }
        assert_eq!(ctrl.current().shutter_us, 30_000_000);
        assert_eq!(ctrl.current().iso, 3200);
    }

    #[test]
    fn test_compute_histogram() {
        // Create a simple 2x2 white image
        let pixels = vec![255u8; 12]; // 4 white pixels
        let hist = compute_histogram(&pixels);
        assert_eq!(hist[255], 4);
        assert_eq!(hist[0], 0);
    }
}
