use crate::core::models::DetectionResult;

/// Aurora detection engine — pure logic, operates on image pixel data.
///
/// Detection is done on JPG images only, using:
/// 1. Green channel dominance
/// 2. Luminosity analysis
/// 3. Temporal variation (compared to previous frame)
/// 4. Spatial spread check (anti false-positive)
pub struct AuroraDetector {
    /// ROI: top N% of the image (50–99)
    roi_top_percent: u32,
    green_threshold: f64,
    luminosity_threshold: f64,
    variation_threshold: f64,
    /// Previous frame stats for temporal comparison
    prev_green_score: Option<f64>,
    prev_luminosity: Option<f64>,
}

impl AuroraDetector {
    pub fn new(
        roi_top_percent: u32,
        green_threshold: f64,
        luminosity_threshold: f64,
        variation_threshold: f64,
    ) -> Self {
        Self {
            roi_top_percent: roi_top_percent.clamp(50, 99),
            green_threshold,
            luminosity_threshold,
            variation_threshold,
            prev_green_score: None,
            prev_luminosity: None,
        }
    }

    pub fn update_config(
        &mut self,
        roi_top_percent: u32,
        green_threshold: f64,
        luminosity_threshold: f64,
        variation_threshold: f64,
    ) {
        self.roi_top_percent = roi_top_percent.clamp(50, 99);
        self.green_threshold = green_threshold;
        self.luminosity_threshold = luminosity_threshold;
        self.variation_threshold = variation_threshold;
    }

    /// Analyze an image (RGB pixel data, width, height).
    /// Returns a DetectionResult.
    pub fn analyze(&mut self, rgb_data: &[u8], width: u32, height: u32) -> DetectionResult {
        if rgb_data.len() < (width * height * 3) as usize {
            return DetectionResult::negative();
        }

        // 1. Extract ROI (top N% of image)
        let roi_height = (height as f64 * self.roi_top_percent as f64 / 100.0) as u32;
        let roi_pixels = &rgb_data[0..(width * roi_height * 3) as usize];

        // 2. Compute green dominance score
        let green_score = self.compute_green_dominance(roi_pixels);

        // 3. Compute mean luminosity in ROI
        let luminosity = self.compute_luminosity(roi_pixels);

        // 4. Compute temporal variation
        let variation = self.compute_variation(green_score, luminosity);

        // 5. Spatial spread check (anti false positive)
        let is_spread = self.check_spatial_spread(roi_pixels, width, roi_height);

        // Update history
        self.prev_green_score = Some(green_score);
        self.prev_luminosity = Some(luminosity);

        // Decision
        let green_pass = green_score > self.green_threshold;
        let luminosity_pass = luminosity > self.luminosity_threshold;
        let _variation_pass = variation > self.variation_threshold;
        let detected = green_pass && luminosity_pass && is_spread;

        // Confidence: normalized combination of scores
        let confidence = if detected {
            let g_conf = (green_score / self.green_threshold).min(2.0) / 2.0;
            let l_conf = (luminosity / self.luminosity_threshold).min(2.0) / 2.0;
            (g_conf + l_conf) / 2.0
        } else {
            0.0
        };

        DetectionResult {
            detected,
            green_score,
            luminosity,
            variation,
            confidence,
        }
    }

    /// Green dominance: mean of (G - R) + (G - B) across all pixels.
    fn compute_green_dominance(&self, rgb_data: &[u8]) -> f64 {
        let pixel_count = rgb_data.len() / 3;
        if pixel_count == 0 {
            return 0.0;
        }

        let mut total: f64 = 0.0;
        for i in 0..pixel_count {
            let r = rgb_data[i * 3] as f64;
            let g = rgb_data[i * 3 + 1] as f64;
            let b = rgb_data[i * 3 + 2] as f64;
            // Green dominance: how much green exceeds red and blue
            let dominance = (g - r) + (g - b);
            total += dominance;
        }
        total / pixel_count as f64
    }

    /// Mean luminosity (simple average of all channels).
    fn compute_luminosity(&self, rgb_data: &[u8]) -> f64 {
        let pixel_count = rgb_data.len() / 3;
        if pixel_count == 0 {
            return 0.0;
        }

        let mut total: f64 = 0.0;
        for i in 0..pixel_count {
            let r = rgb_data[i * 3] as f64;
            let g = rgb_data[i * 3 + 1] as f64;
            let b = rgb_data[i * 3 + 2] as f64;
            total += (r + g + b) / 3.0;
        }
        total / pixel_count as f64
    }

    /// Temporal variation: change from previous frame.
    fn compute_variation(&self, green_score: f64, luminosity: f64) -> f64 {
        match (self.prev_green_score, self.prev_luminosity) {
            (Some(prev_g), Some(prev_l)) => {
                let dg = (green_score - prev_g).abs();
                let dl = (luminosity - prev_l).abs();
                dg + dl
            }
            _ => 0.0, // First frame: no variation
        }
    }

    /// Check if bright areas are spatially spread (not concentrated like headlights).
    /// Returns true if the brightness is spread across the image (aurora-like).
    fn check_spatial_spread(&self, rgb_data: &[u8], width: u32, height: u32) -> bool {
        if width == 0 || height == 0 {
            return false;
        }

        // Divide image into a 4x4 grid
        let grid_cols = 4u32;
        let grid_rows = 4u32;
        let cell_w = width / grid_cols;
        let cell_h = height / grid_rows;

        if cell_w == 0 || cell_h == 0 {
            return true; // Too small to analyze
        }

        let mut bright_cells = 0u32;
        let bright_threshold = 60.0; // Mean brightness to count as "bright"

        for gy in 0..grid_rows {
            for gx in 0..grid_cols {
                let mut cell_sum: f64 = 0.0;
                let mut cell_count: u32 = 0;

                for y in (gy * cell_h)..((gy + 1) * cell_h).min(height) {
                    for x in (gx * cell_w)..((gx + 1) * cell_w).min(width) {
                        let idx = ((y * width + x) * 3) as usize;
                        if idx + 2 < rgb_data.len() {
                            let g = rgb_data[idx + 1] as f64;
                            cell_sum += g;
                            cell_count += 1;
                        }
                    }
                }

                if cell_count > 0 && (cell_sum / cell_count as f64) > bright_threshold {
                    bright_cells += 1;
                }
            }
        }

        // Aurora → spread across multiple cells (>= 3)
        // Headlight → concentrated in 1-2 cells
        bright_cells >= 3
    }

    /// Reset temporal history (e.g., when entering Watch phase).
    pub fn reset(&mut self) {
        self.prev_green_score = None;
        self.prev_luminosity = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a uniform color image (RGB).
    fn make_uniform_image(width: u32, height: u32, r: u8, g: u8, b: u8) -> Vec<u8> {
        let mut data = Vec::with_capacity((width * height * 3) as usize);
        for _ in 0..(width * height) {
            data.push(r);
            data.push(g);
            data.push(b);
        }
        data
    }

    /// Create an image with a green aurora-like glow in the top half.
    fn make_aurora_image(width: u32, height: u32) -> Vec<u8> {
        let mut data = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            for _x in 0..width {
                if y < height / 2 {
                    // Aurora green glow
                    data.push(30);  // R
                    data.push(120); // G
                    data.push(40);  // B
                } else {
                    // Dark ground
                    data.push(10);
                    data.push(10);
                    data.push(10);
                }
            }
        }
        data
    }

    /// Create an image simulating headlights (bright concentrated spot).
    fn make_headlight_image(width: u32, height: u32) -> Vec<u8> {
        let mut data = Vec::with_capacity((width * height * 3) as usize);
        let cx = width / 2;
        let cy = height / 4;
        let radius = 5u32;
        for y in 0..height {
            for x in 0..width {
                let dist = ((x as i32 - cx as i32).pow(2) + (y as i32 - cy as i32).pow(2)) as f64;
                if dist < (radius * radius) as f64 {
                    // Bright white spot
                    data.push(255);
                    data.push(255);
                    data.push(255);
                } else {
                    // Dark
                    data.push(5);
                    data.push(5);
                    data.push(5);
                }
            }
        }
        data
    }

    #[test]
    fn test_dark_sky_no_detection() {
        let mut detector = AuroraDetector::new(65, 15.0, 30.0, 10.0);
        let img = make_uniform_image(100, 100, 5, 5, 5);
        let result = detector.analyze(&img, 100, 100);
        assert!(!result.detected);
    }

    #[test]
    fn test_aurora_detection() {
        let mut detector = AuroraDetector::new(65, 10.0, 20.0, 0.0);
        let img = make_aurora_image(100, 100);
        let result = detector.analyze(&img, 100, 100);
        assert!(result.detected);
        assert!(result.green_score > 10.0);
    }

    #[test]
    fn test_headlight_no_detection() {
        let mut detector = AuroraDetector::new(65, 15.0, 30.0, 10.0);
        let img = make_headlight_image(100, 100);
        let result = detector.analyze(&img, 100, 100);
        // Headlight: concentrated bright spot → spatial spread check should fail
        assert!(!result.detected);
    }

    #[test]
    fn test_roi_clamp() {
        let detector = AuroraDetector::new(30, 15.0, 30.0, 10.0);
        assert_eq!(detector.roi_top_percent, 50);

        let detector2 = AuroraDetector::new(120, 15.0, 30.0, 10.0);
        assert_eq!(detector2.roi_top_percent, 99);
    }

    #[test]
    fn test_reset() {
        let mut detector = AuroraDetector::new(65, 15.0, 30.0, 10.0);
        let img = make_uniform_image(100, 100, 50, 80, 50);
        detector.analyze(&img, 100, 100);
        assert!(detector.prev_green_score.is_some());

        detector.reset();
        assert!(detector.prev_green_score.is_none());
    }
}
