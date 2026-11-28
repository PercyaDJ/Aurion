use crate::core::models::{AuroraColor, DetectionResult};

/// Aurora detection engine — pure logic, operates on image pixel data.
///
/// Multi-color detection (green, red, violet) with:
/// 1. Per-color channel dominance scoring
/// 2. Area check (min % of ROI pixels above threshold)
/// 3. Moon mask (bright blob exclusion)
/// 4. Hystérésis (on/off thresholds to avoid flickering)
/// 5. Spatial spread check (anti false-positive)
pub struct AuroraDetector {
    /// ROI: top N% of the image (50–99)
    roi_top_percent: u32,
    green_threshold: f64,
    red_threshold: f64,
    blue_threshold: f64,
    luminosity_threshold: f64,
    area_min_percent: f64,
    hysteresis_on: f64,
    hysteresis_off: f64,
    moon_mask_enabled: bool,
    moon_luminance_threshold: f64,

    /// Hystérésis state
    was_detected: bool,
}

/// Colour dominance of a pixel: (green, red, blue) minus the mean of the
/// two other channels.
fn dominance(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    (g - (r + b) / 2.0, r - (g + b) / 2.0, b - (r + g) / 2.0)
}

impl AuroraDetector {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        roi_top_percent: u32,
        green_threshold: f64,
        red_threshold: f64,
        blue_threshold: f64,
        luminosity_threshold: f64,
        area_min_percent: f64,
        hysteresis_on: f64,
        hysteresis_off: f64,
        moon_mask_enabled: bool,
        moon_luminance_threshold: f64,
    ) -> Self {
        Self {
            roi_top_percent: roi_top_percent.clamp(50, 99),
            green_threshold,
            red_threshold,
            blue_threshold,
            luminosity_threshold,
            area_min_percent,
            hysteresis_on,
            hysteresis_off,
            moon_mask_enabled,
            moon_luminance_threshold,
            was_detected: false,
        }
    }

    /// Create from config (convenience).
    pub fn from_config(cfg: &crate::core::config::DetectionConfig) -> Self {
        Self::new(
            cfg.roi_top_percent,
            cfg.green_threshold,
            cfg.red_threshold,
            cfg.blue_threshold,
            cfg.luminosity_threshold,
            cfg.area_min_percent,
            cfg.hysteresis_on,
            cfg.hysteresis_off,
            cfg.moon_mask_enabled,
            cfg.moon_luminance_threshold,
        )
    }

    pub fn update_config(&mut self, cfg: &crate::core::config::DetectionConfig) {
        self.roi_top_percent = cfg.roi_top_percent.clamp(50, 99);
        self.green_threshold = cfg.green_threshold;
        self.red_threshold = cfg.red_threshold;
        self.blue_threshold = cfg.blue_threshold;
        self.luminosity_threshold = cfg.luminosity_threshold;
        self.area_min_percent = cfg.area_min_percent;
        self.hysteresis_on = cfg.hysteresis_on;
        self.hysteresis_off = cfg.hysteresis_off;
        self.moon_mask_enabled = cfg.moon_mask_enabled;
        self.moon_luminance_threshold = cfg.moon_luminance_threshold;
    }

    /// Analyze an image (RGB pixel data, width, height).
    /// Returns a DetectionResult with multi-color scoring.
    pub fn analyze(&mut self, rgb_data: &[u8], width: u32, height: u32) -> DetectionResult {
        if rgb_data.len() < (width * height * 3) as usize {
            return DetectionResult::negative();
        }

        // 1. Extract ROI (top N% of image)
        let roi_height = (height as f64 * self.roi_top_percent as f64 / 100.0) as u32;
        let roi_pixels = &rgb_data[0..(width * roi_height * 3) as usize];
        let total_roi_pixels = (width * roi_height) as usize;

        if total_roi_pixels == 0 {
            return DetectionResult::negative();
        }

        // 2. Build moon mask (if enabled)
        let moon_mask = if self.moon_mask_enabled {
            self.compute_moon_mask(roi_pixels, width, roi_height)
        } else {
            vec![false; total_roi_pixels]
        };
        let moon_masked = moon_mask.iter().any(|&m| m);

        // 3. Compute per-color scores + area on non-masked pixels.
        // Areas are counted at the trigger threshold and at the hold
        // threshold (trigger × hysteresis_off / hysteresis_on).
        let eps = 1e-3_f64;
        let hold = if self.hysteresis_on > 0.0 { (self.hysteresis_off / self.hysteresis_on).clamp(0.0, 1.0) } else { 1.0 };
        let mut green_total = 0.0_f64;
        let mut red_total = 0.0_f64;
        let mut blue_total = 0.0_f64;
        let mut lum_total = 0.0_f64;
        // [trigger, hold] pixel counts per colour
        let mut green_area = [0u32; 2];
        let mut red_area = [0u32; 2];
        let mut violet_area = [0u32; 2];
        let mut pixel_count = 0u32;

        for i in 0..total_roi_pixels {
            if moon_mask[i] {
                continue;
            }

            let r = roi_pixels[i * 3] as f64;
            let g = roi_pixels[i * 3 + 1] as f64;
            let b = roi_pixels[i * 3 + 2] as f64;
            let lum = (r + g + b) / 3.0;

            // Skip very dark pixels (noise)
            if lum < self.luminosity_threshold {
                pixel_count += 1;
                continue;
            }

            let (gs, rs, bs) = dominance(r, g, b);

            green_total += gs.max(0.0);
            red_total += rs.max(0.0);
            blue_total += bs.max(0.0);
            lum_total += lum;
            pixel_count += 1;

            for (k, f) in [1.0, hold].into_iter().enumerate() {
                if gs > self.green_threshold * f {
                    green_area[k] += 1;
                }
                if rs > self.red_threshold * f {
                    red_area[k] += 1;
                }
                // Violet: moderate red AND blue, both above half their thresholds
                if rs > self.red_threshold * 0.5 * f && bs > self.blue_threshold * 0.5 * f {
                    violet_area[k] += 1;
                }
            }
        }

        if pixel_count == 0 {
            return DetectionResult::negative();
        }

        let pf = pixel_count as f64;
        let green_score = green_total / pf;
        let red_score = red_total / pf;
        let violet_score = blue_total / pf; // blue serves as violet proxy
        let luminosity = lum_total / pf;
        let pct = |n: u32| n as f64 / pf * 100.0;

        // 4. Dominant colour and continuous score: colour strength relative
        // to its threshold (>= 1 for a clear aurora). Before 1.15.0 the score
        // was 0 or > 1, so the hold threshold (hysteresis_off) had no effect.
        // A colour only counts if enough sky shows it at the hold threshold.
        let candidates = [
            (green_score / (self.green_threshold + eps), AuroraColor::Green, pct(green_area[0]), pct(green_area[1])),
            (red_score / (self.red_threshold + eps), AuroraColor::Red, pct(red_area[0]), pct(red_area[1])),
            (violet_score / (self.blue_threshold + eps), AuroraColor::Violet, pct(violet_area[0]), pct(violet_area[1])),
        ];
        let mut best = (0.0_f64, AuroraColor::None, 0.0_f64, 0.0_f64);
        for c in candidates {
            if c.3 > self.area_min_percent && c.0 > best.0 {
                best = c;
            }
        }
        let (aurora_score, mut aurora_color, area_percent, _) = best;

        // 5. Spatial spread check, on the dominant colour (before 1.15.0 it
        // tested raw green brightness > 60, which blocked red auroras)
        let spread_threshold = match aurora_color {
            AuroraColor::Red => self.red_threshold,
            AuroraColor::Violet => self.blue_threshold * 0.5,
            _ => self.green_threshold,
        } * hold;
        let is_spread = aurora_color != AuroraColor::None
            && self.check_spatial_spread(roi_pixels, width, roi_height, aurora_color, spread_threshold);

        // 6. Hystérésis decision
        let detected = if self.was_detected {
            // Was detected: stay detected unless score drops below hysteresis_off
            aurora_score >= self.hysteresis_off && is_spread
        } else {
            // Not detected: trigger only on a clear aurora over enough area
            aurora_score >= self.hysteresis_on && area_percent > self.area_min_percent && is_spread
        };
        self.was_detected = detected;
        // Below the hold threshold the dominant colour is noise, not aurora
        if aurora_score < self.hysteresis_off {
            aurora_color = AuroraColor::None;
        }

        // Confidence: 0 at the hold threshold, 1 at twice the trigger
        // threshold (before 1.15.0 it was always 1.0 when detected).
        let confidence = if detected {
            let span = (2.0 * self.hysteresis_on - self.hysteresis_off).max(1e-6);
            ((aurora_score - self.hysteresis_off) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };

        DetectionResult {
            detected,
            aurora_score,
            aurora_color,
            green_score,
            red_score,
            violet_score,
            area_percent,
            luminosity,
            confidence,
            moon_masked,
        }
    }

    /// Compute a boolean mask for "moon" bright blobs.
    /// Works on a downscaled grid (16×12) for speed, then expands back.
    fn compute_moon_mask(&self, rgb_data: &[u8], width: u32, height: u32) -> Vec<bool> {
        let total = (width * height) as usize;
        let mut mask = vec![false; total];

        let grid_w = 16u32.min(width);
        let grid_h = 12u32.min(height);
        let cell_w = width / grid_w;
        let cell_h = height / grid_h;

        if cell_w == 0 || cell_h == 0 {
            return mask;
        }

        // Compute mean luminance per grid cell
        let mut grid_lum = vec![0.0_f64; (grid_w * grid_h) as usize];
        for gy in 0..grid_h {
            for gx in 0..grid_w {
                let mut sum = 0.0_f64;
                let mut count = 0u32;
                for y in (gy * cell_h)..((gy + 1) * cell_h).min(height) {
                    for x in (gx * cell_w)..((gx + 1) * cell_w).min(width) {
                        let idx = ((y * width + x) * 3) as usize;
                        if idx + 2 < rgb_data.len() {
                            let r = rgb_data[idx] as f64;
                            let g = rgb_data[idx + 1] as f64;
                            let b = rgb_data[idx + 2] as f64;
                            sum += (r + g + b) / 3.0;
                            count += 1;
                        }
                    }
                }
                if count > 0 {
                    grid_lum[(gy * grid_w + gx) as usize] = sum / count as f64;
                }
            }
        }

        // Find bright cells (moon candidates)
        let mut bright_cells: Vec<bool> = grid_lum.iter()
            .map(|&l| l > self.moon_luminance_threshold)
            .collect();

        // Expand: if a cell is bright, also mark its 8 neighbors (dilate)
        let mut expanded = bright_cells.clone();
        for gy in 0..grid_h {
            for gx in 0..grid_w {
                if bright_cells[(gy * grid_w + gx) as usize] {
                    for dy in -1i32..=1 {
                        for dx in -1i32..=1 {
                            let ny = gy as i32 + dy;
                            let nx = gx as i32 + dx;
                            if ny >= 0 && ny < grid_h as i32 && nx >= 0 && nx < grid_w as i32 {
                                expanded[(ny as u32 * grid_w + nx as u32) as usize] = true;
                            }
                        }
                    }
                }
            }
        }
        bright_cells = expanded;

        // Mark mask for all pixels in bright cells (only if at least one bright cell)
        if !bright_cells.iter().any(|&b| b) {
            return mask;
        }

        for gy in 0..grid_h {
            for gx in 0..grid_w {
                if !bright_cells[(gy * grid_w + gx) as usize] {
                    continue;
                }
                for y in (gy * cell_h)..((gy + 1) * cell_h).min(height) {
                    for x in (gx * cell_w)..((gx + 1) * cell_w).min(width) {
                        let idx = (y * width + x) as usize;
                        if idx < total {
                            mask[idx] = true;
                        }
                    }
                }
            }
        }

        mask
    }

    /// Check if bright areas are spatially spread (not concentrated like headlights).
    fn check_spatial_spread(&self, rgb_data: &[u8], width: u32, height: u32, color: AuroraColor, threshold: f64) -> bool {
        if width == 0 || height == 0 {
            return false;
        }

        let grid_cols = 4u32;
        let grid_rows = 4u32;
        let cell_w = width / grid_cols;
        let cell_h = height / grid_rows;

        if cell_w == 0 || cell_h == 0 {
            return true;
        }

        let mut bright_cells = 0u32;

        for gy in 0..grid_rows {
            for gx in 0..grid_cols {
                let mut cell_sum: f64 = 0.0;
                let mut cell_count: u32 = 0;

                for y in (gy * cell_h)..((gy + 1) * cell_h).min(height) {
                    for x in (gx * cell_w)..((gx + 1) * cell_w).min(width) {
                        let idx = ((y * width + x) * 3) as usize;
                        if idx + 2 < rgb_data.len() {
                            let (gs, rs, bs) =
                                dominance(rgb_data[idx] as f64, rgb_data[idx + 1] as f64, rgb_data[idx + 2] as f64);
                            cell_sum += match color {
                                AuroraColor::Red => rs,
                                AuroraColor::Violet => rs.min(bs),
                                _ => gs,
                            };
                            cell_count += 1;
                        }
                    }
                }

                if cell_count > 0 && (cell_sum / cell_count as f64) > threshold {
                    bright_cells += 1;
                }
            }
        }

        // Aurora → spread across multiple cells (>= 3)
        bright_cells >= 3
    }

    /// Reset hystérésis state (e.g., when entering Watch phase).
    pub fn reset(&mut self) {
        self.was_detected = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_detector() -> AuroraDetector {
        AuroraDetector::new(
            65, 15.0, 10.0, 8.0, 30.0,
            1.0, 1.0, 0.7,
            true, 240.0,
        )
    }

    /// Create a uniform color image (RGB).
    fn make_uniform_image(width: u32, height: u32, r: u8, g: u8, b: u8) -> Vec<u8> {
        let pixel_count = (width * height) as usize;
        let mut data = vec![0u8; pixel_count * 3];
        for i in 0..pixel_count {
            data[i * 3] = r;
            data[i * 3 + 1] = g;
            data[i * 3 + 2] = b;
        }
        data
    }

    /// Create an image with a green aurora-like glow in the top half.
    fn make_aurora_image(width: u32, height: u32) -> Vec<u8> {
        let pixel_count = (width * height) as usize;
        let mut data = vec![0u8; pixel_count * 3];
        let half = height / 2;
        for y in 0..height {
            for x in 0..width {
                let i = (y * width + x) as usize;
                if y < half {
                    // Green aurora glow
                    data[i * 3] = 20;     // R
                    data[i * 3 + 1] = 120; // G (dominant)
                    data[i * 3 + 2] = 30;  // B
                } else {
                    // Dark ground
                    data[i * 3] = 10;
                    data[i * 3 + 1] = 10;
                    data[i * 3 + 2] = 10;
                }
            }
        }
        data
    }

    /// Create a red aurora image.
    fn make_red_aurora_image(width: u32, height: u32) -> Vec<u8> {
        let pixel_count = (width * height) as usize;
        let mut data = vec![0u8; pixel_count * 3];
        let half = height / 2;
        for y in 0..height {
            for x in 0..width {
                let i = (y * width + x) as usize;
                if y < half {
                    data[i * 3] = 130;     // R (dominant)
                    data[i * 3 + 1] = 30;  // G
                    data[i * 3 + 2] = 40;  // B
                } else {
                    data[i * 3] = 10;
                    data[i * 3 + 1] = 10;
                    data[i * 3 + 2] = 10;
                }
            }
        }
        data
    }

    /// Create an image with a very bright moon blob.
    fn make_moon_image(width: u32, height: u32) -> Vec<u8> {
        let pixel_count = (width * height) as usize;
        let mut data = vec![0u8; pixel_count * 3];
        // Dark sky with a bright moon in top-left corner
        for y in 0..height {
            for x in 0..width {
                let i = (y * width + x) as usize;
                if x < width / 4 && y < height / 4 {
                    // Bright moon
                    data[i * 3] = 250;
                    data[i * 3 + 1] = 250;
                    data[i * 3 + 2] = 250;
                } else {
                    data[i * 3] = 15;
                    data[i * 3 + 1] = 15;
                    data[i * 3 + 2] = 15;
                }
            }
        }
        data
    }

    #[test]
    fn test_dark_sky_no_detection() {
        let mut det = default_detector();
        let img = make_uniform_image(100, 100, 10, 10, 10);
        let result = det.analyze(&img, 100, 100);
        assert!(!result.detected);
        assert_eq!(result.aurora_color, AuroraColor::None);
    }

    #[test]
    fn test_green_aurora_detection() {
        let mut det = default_detector();
        let img = make_aurora_image(100, 100);
        let result = det.analyze(&img, 100, 100);
        assert!(result.green_score > 0.0);
        assert!(result.aurora_score > 0.0);
        // Green should be the dominant color
        assert!(result.green_score > result.red_score);
    }

    #[test]
    fn test_red_aurora_detection() {
        let mut det = default_detector();
        let img = make_red_aurora_image(100, 100);
        let result = det.analyze(&img, 100, 100);
        assert!(result.red_score > 0.0);
        assert!(result.red_score > result.green_score);
    }

    #[test]
    fn test_moon_mask_excludes_bright_blob() {
        let mut det = default_detector();
        let img = make_moon_image(100, 100);
        let result = det.analyze(&img, 100, 100);
        assert!(result.moon_masked, "Moon should be detected and masked");
    }

    #[test]
    fn test_hysteresis_stickiness() {
        let mut det = default_detector();
        // Clear green aurora (dominance 95 for a threshold of 15): triggers
        let img = make_aurora_image(100, 100);
        let r1 = det.analyze(&img, 100, 100);
        assert!(r1.detected && r1.aurora_score >= 1.0, "{:?}", r1);

        // Fading aurora: dominance 12 (score 0.8, between 0.7 and 1.0)
        let weak = make_uniform_image(100, 100, 50, 62, 50);
        let held = det.analyze(&weak, 100, 100);
        assert!(held.detected, "held by the hysteresis: {:?}", held);
        assert!(held.aurora_score > 0.7 && held.aurora_score < 1.0);
        assert!(held.confidence > 0.0 && held.confidence < r1.confidence);

        // The same weak sky does not trigger from scratch
        let mut fresh = default_detector();
        assert!(!fresh.analyze(&weak, 100, 100).detected, "below the trigger threshold");

        // Gone: dominance 5 (score 0.33) releases the detection
        let gone = det.analyze(&make_uniform_image(100, 100, 50, 55, 50), 100, 100);
        assert!(!gone.detected);
        assert_eq!(gone.aurora_color, AuroraColor::None);
    }

    #[test]
    fn confidence_grows_with_the_aurora_strength() {
        let mut a = default_detector();
        let mut b = default_detector();
        let medium = a.analyze(&make_uniform_image(100, 100, 40, 60, 40), 100, 100); // score 1.33
        let strong = b.analyze(&make_uniform_image(100, 100, 30, 80, 30), 100, 100); // score 3.33
        assert!(medium.detected && strong.detected);
        assert!(medium.confidence < strong.confidence, "{} < {}", medium.confidence, strong.confidence);
        assert_eq!(strong.confidence, 1.0);
    }

    #[test]
    fn red_aurora_is_not_blocked_by_the_green_spread_check() {
        let mut det = default_detector();
        // Red dominance 25 (threshold 10), green far below 60
        let r = det.analyze(&make_uniform_image(100, 100, 70, 45, 45), 100, 100);
        assert!(r.detected, "{:?}", r);
        assert_eq!(r.aurora_color, AuroraColor::Red);
    }

    #[test]
    fn test_roi_clamp() {
        let det = AuroraDetector::new(30, 15.0, 10.0, 8.0, 30.0, 1.0, 1.0, 0.7, true, 240.0);
        assert_eq!(det.roi_top_percent, 50); // Clamped to 50
    }

    #[test]
    fn test_reset() {
        let mut det = default_detector();
        det.was_detected = true;
        det.reset();
        assert!(!det.was_detected);
    }

    #[test]
    fn test_night_without_aurora_never_run() {
        // Dark sky: should never detect aurora
        let mut det = default_detector();
        let img = make_uniform_image(100, 100, 5, 5, 5);
        for _ in 0..10 {
            let result = det.analyze(&img, 100, 100);
            assert!(!result.detected, "Dark sky should never trigger detection");
        }
    }
}
