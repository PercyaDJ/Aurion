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

    /// Consecutive RUN/WATCH frames metered close to the target.
    on_target_frames: u32,
    /// Exposure has reached the target (calibration converged, or a few
    /// frames metered on target): the RUN lock may freeze it from then on.
    settled: bool,
}

/// Highest ISO the HQ Camera (Sony IMX477) reaches with analogue gain only.
/// libcamera's IMX477 helper tops out at about ×22.3; beyond, rpicam-still
/// adds digital gain, which brightens the JPEG used for metering but not
/// the DNG. Capping here keeps the metering honest for the RAW.
/// To confirm on hardware with `--metadata` (AnalogueGain / DigitalGain).
pub const MAX_ANALOGUE_ISO: u32 = 2200;

/// Calibration stops after this many frames even if not converged.
pub const CALIBRATION_MAX_FRAMES: u32 = 6;

/// Change of exposure (ratio) below which the calibration has converged.
const CONVERGED: std::ops::RangeInclusive<f64> = 0.85..=1.18;

/// Metering error (log) under which a frame counts as on target.
const ON_TARGET_LOG_ERR: f64 = 0.15;

/// sRGB 8-bit value to linear light (0..1).
fn srgb_to_linear(v: f64) -> f64 {
    let c = (v / 255.0).clamp(0.0, 1.0);
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
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
        // Start at minimum exposure (conservative); the night calibration
        // starts from `calibration_start()` instead.
        let initial = ExposureSettings::new(iso_min, shutter_min_us);
        // Digital gain adds no information to the RAW (see MAX_ANALOGUE_ISO)
        let iso_max = iso_max.min(MAX_ANALOGUE_ISO).max(iso_min);

        Self {
            iso_min,
            iso_max,
            shutter_min_us,
            shutter_max_us,
            current: initial,
            detect_alpha,
            detect_rate_up,
            detect_rate_down,
            capture_alpha,
            capture_rate_up,
            capture_rate_down,
            target_percentile,
            target_brightness,
            saturation_reject,
            on_target_frames: 0,
            settled: false,
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

    /// Effective ISO ceiling (user maximum capped at MAX_ANALOGUE_ISO).
    pub fn iso_max(&self) -> u32 {
        self.iso_max
    }

    /// True once the exposure has reached the target: calibration
    /// converged, or 3 frames in a row metered on target.
    pub fn settled(&self) -> bool {
        self.settled
    }

    /// Starting point of a calibration: geometric middle of the user range
    /// (same as the preview). Starting from the minimum (before 1.15.0)
    /// left the first hours of the night underexposed.
    pub fn calibration_start(&mut self) -> ExposureSettings {
        let iso = ((self.iso_min as f64) * (self.iso_max as f64)).sqrt() as u32;
        let shutter = ((self.shutter_min_us as f64) * (self.shutter_max_us as f64)).sqrt() as u64;
        self.set(ExposureSettings::new(iso, shutter));
        self.on_target_frames = 0;
        self.settled = false;
        self.current
    }

    /// One calibration frame: jump towards the target. Returns true when
    /// converged (the exposure barely moved); the controller is then settled.
    pub fn calibration_step(&mut self, histogram: &[u32; 256]) -> bool {
        let before = self.current;
        let next = self.jump(histogram);
        let change = (next.shutter_us as f64 * next.iso as f64) / (before.shutter_us as f64 * before.iso as f64);
        let converged = CONVERGED.contains(&change);
        if converged {
            self.settled = true;
        }
        converged
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
        if err.abs() < ON_TARGET_LOG_ERR {
            self.on_target_frames += 1;
            if self.on_target_frames >= 3 {
                self.settled = true;
            }
        } else {
            self.on_target_frames = 0;
        }

        // Move a fraction `alpha` of the way to the target, in log space
        // (exponential smoothing: no overshoot, no oscillation). The step
        // only depends on this frame: carrying the previous step over (as
        // before 1.11.1) made the exposure swing endlessly around the target.
        let delta = alpha * err;

        // Limit the change per frame (rate limiter)
        let clamped_delta = if delta > 0.0 {
            delta.min(rate_up.ln_1p()) // ln(1 + rate_up) ≈ rate_up for small values
        } else {
            delta.max(-(rate_down.ln_1p()))
        };

        self.apply_multiplier(clamped_delta.exp());
        self.current
    }

    /// One-shot metering used by the preview and the calibration: correct
    /// the exposure directly towards the target (no EMA, no rate limit),
    /// bounded to ×8 / ÷8 per call so a single bad frame cannot send it to
    /// an extreme. The ratio is computed in linear light: the JPEG is
    /// gamma-encoded, so a sky at 30 instead of 60 needs about ×4.6 of
    /// light, not ×2 (before 1.15.0, hence 3 to 4 preview shots).
    pub fn jump(&mut self, histogram: &[u32; 256]) -> ExposureSettings {
        let measured = self.compute_percentile(histogram);
        let mult = if measured < 1.0 {
            8.0
        } else {
            (srgb_to_linear(self.target_brightness) / srgb_to_linear(measured)).clamp(0.125, 8.0)
        };
        self.apply_multiplier(mult);
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
        self.on_target_frames = 0;
        self.settled = false;
    }

    /// Set exposure directly (e.g., from config or preset).
    pub fn set(&mut self, settings: ExposureSettings) {
        self.current = ExposureSettings::new(
            settings.iso.clamp(self.iso_min, self.iso_max),
            settings.shutter_us.clamp(self.shutter_min_us, self.shutter_max_us),
        );
    }
}

/// `--gain` argument of rpicam-still for an ISO (ISO 100 = gain 1.0). One
/// formatting for the night, the preview and the darks: darks must match
/// the night frames exactly.
pub fn rpicam_gain(iso: u32) -> String {
    format!("{:.1}", iso as f64 / 100.0)
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

    /// Sky whose brightness follows the exposure (linear sensor): the
    /// target (60) is reached at ISO 800, 8 s.
    fn sky_brightness(e: ExposureSettings) -> u8 {
        let ev = e.shutter_us as f64 * e.iso as f64;
        (60.0 * ev / (8_000_000.0 * 800.0)).round().clamp(0.0, 255.0) as u8
    }

    fn run_night(ctrl: &mut ExposureController, phase: Phase, frames: usize) -> Vec<f64> {
        (0..frames)
            .map(|_| {
                let e = ctrl.update(&make_histogram_for_brightness(sky_brightness(ctrl.current())), phase);
                e.shutter_us as f64 * e.iso as f64
            })
            .collect()
    }

    #[test]
    fn exposure_settles_without_oscillating() {
        for phase in [Phase::Run, Phase::Watch] {
            let mut ctrl = default_controller();
            ctrl.set(ExposureSettings::new(800, 3_000_000)); // under-exposed start
            let evs = run_night(&mut ctrl, phase, 400);
            // Once settled, the exposure stays put (a timelapse must not flicker)
            let tail = &evs[300..];
            let (lo, hi) = tail.iter().fold((f64::MAX, 0.0f64), |(lo, hi), &v| (lo.min(v), hi.max(v)));
            assert!(hi / lo < 1.03, "{:?}: exposure swings by {:.1} % once settled", phase, (hi / lo - 1.0) * 100.0);
            // ... and close to the target (8 s at ISO 800)
            let target = 8_000_000.0 * 800.0;
            assert!((tail[tail.len() - 1] / target - 1.0).abs() < 0.05, "{:?}: settled {:.2}x off target", phase, tail[tail.len() - 1] / target);
        }
    }

    #[test]
    fn well_exposed_sky_does_not_move_the_exposure() {
        let mut ctrl = default_controller();
        ctrl.set(ExposureSettings::new(800, 8_000_000));
        let before = ctrl.current();
        ctrl.update(&make_histogram_for_brightness(60), Phase::Run);
        assert_eq!(ctrl.current(), before, "first update on a correct frame must not jump");

        // Straight from the config too (the first step used to be a +15 % jump)
        let mut fresh = default_controller();
        let start = fresh.current();
        fresh.update(&make_histogram_for_brightness(60), Phase::Watch);
        assert_eq!(fresh.current(), start);
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
        // Frame at half the target value: the JPEG is gamma-encoded, so the
        // light needed is the linear ratio (about ×3.5), not ×2.
        let before = ctrl.current();
        let after = ctrl.jump(&make_histogram_for_brightness(30));
        let ratio = (after.shutter_us as f64 * after.iso as f64)
            / (before.shutter_us as f64 * before.iso as f64);
        let expected = srgb_to_linear(60.0) / srgb_to_linear(30.0);
        assert!((ratio - expected).abs() < 0.05 && ratio > 3.0, "ratio={} expected={}", ratio, expected);
        // Black frame → bounded ×8, never beyond the limits (ISO capped at
        // the analogue maximum, even if the user range goes to 3200).
        for _ in 0..10 { ctrl.jump(&make_histogram_for_brightness(0)); }
        assert_eq!(ctrl.current().shutter_us, 30_000_000);
        assert_eq!(ctrl.current().iso, MAX_ANALOGUE_ISO);
    }

    /// Simulated sky: the P80 brightness of the JPEG grows with the light
    /// collected (ISO × shutter), through the sRGB curve.
    fn sky_histogram(settings: ExposureSettings, sky: f64) -> [u32; 256] {
        let light = sky * settings.iso as f64 * settings.shutter_us as f64 / 1e9;
        let c = light.clamp(0.0, 1.0);
        let srgb = if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
        make_histogram_for_brightness((srgb * 255.0).round() as u8)
    }

    #[test]
    fn calibration_reaches_a_dark_sky_target_in_a_few_frames() {
        let mut ctrl = default_controller();
        // A sky that needs about ISO 1600 / 20 s to reach the target (60)
        let target_light = srgb_to_linear(60.0);
        let sky = target_light / (1600.0 * 20_000_000.0 / 1e9);
        let start = ctrl.calibration_start();
        assert!(start.iso > 100 && start.shutter_us > 1_000_000, "starts mid-range: {:?}", start);
        let mut frames = 0;
        let mut converged = false;
        while frames < CALIBRATION_MAX_FRAMES && !converged {
            converged = ctrl.calibration_step(&sky_histogram(ctrl.current(), sky));
            frames += 1;
        }
        assert!(converged && ctrl.settled(), "converged in {} frames", frames);
        assert!(frames <= 4, "{} frames", frames);
        let light = ctrl.current().iso as f64 * ctrl.current().shutter_us as f64;
        let wanted = 1600.0 * 20_000_000.0;
        assert!((light / wanted - 1.0).abs() < 0.2, "exposure {:?}", ctrl.current());
    }

    #[test]
    fn settles_after_three_frames_on_target_in_run() {
        let mut ctrl = default_controller();
        let on_target = make_histogram_for_brightness(60);
        ctrl.update(&on_target, Phase::Run);
        ctrl.update(&on_target, Phase::Run);
        assert!(!ctrl.settled());
        ctrl.update(&on_target, Phase::Run);
        assert!(ctrl.settled());
    }

    #[test]
    fn iso_is_capped_at_the_analogue_maximum() {
        let ctrl = default_controller(); // user maximum 3200
        assert_eq!(ctrl.iso_max(), MAX_ANALOGUE_ISO);
    }

    #[test]
    fn gain_is_the_same_for_night_preview_and_darks() {
        assert_eq!(rpicam_gain(800), "8.0");
        assert_eq!(rpicam_gain(1234), "12.3");
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
