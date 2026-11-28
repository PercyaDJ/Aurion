use crate::core::models::{AuroraColor, DetectionResult};

/// Aurora detection engine — pure logic, operates on image pixel data.
///
/// Multi-color detection (green, red, violet) with:
/// 1. Per-color channel dominance, averaged over a 16×12 grid of cells
/// 2. Sky background (quarter of the cells with the least colour) removed
///    from every cell: the score is the strength of the cells in excess,
///    the area is the share of cells in excess (before 1.18.0 the colour
///    was averaged over the whole ROI, dark sky included, so a faint band
///    low on the horizon stayed below the threshold: D-8 of the audit)
/// 3. Moon mask (bright cells, and saturated white blobs)
/// 4. Hystérésis (on/off thresholds to avoid flickering)
/// 5. Spatial spread check on the cells in excess (anti false-positive)
/// 6. Sun/Moon gate (`core::sky`): thresholds raised, or detection
///    suspended during twilight
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
    gate: DetectionGate,

    /// Hystérésis state
    was_detected: bool,
}

/// What the Sun and the Moon allow for the next frames (see `core::sky`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetectionGate {
    /// The score is divided by this factor before the decision (>= 1).
    pub factor: f64,
    /// No detection at all (bright twilight).
    pub suspended: bool,
}

impl Default for DetectionGate {
    fn default() -> Self {
        Self { factor: 1.0, suspended: false }
    }
}

/// Grid of the surface score (cells over the ROI). On the 320×240
/// thumbnail a cell is 20×13 pixels with the default 65 % ROI.
const GRID_W: u32 = 16;
const GRID_H: u32 = 12;
/// Share of the cells (least coloured first) that gives the sky background.
const BACKGROUND_QUANTILE: f64 = 0.25;
/// Spread: blocks of 4×3 cells (4×4 blocks over the ROI) with at least a
/// quarter of their cells in excess; at least 3 such blocks.
const SPREAD_MIN_BLOCKS: usize = 3;
/// Saturated white pixels (all channels at least this value): the Moon,
/// street lamps. Never an aurora, which saturates one colour only.
const SATURATED: u8 = 250;

/// Colour dominance of a pixel: (green, red, blue) minus the mean of the
/// two other channels.
fn dominance(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    (g - (r + b) / 2.0, r - (g + b) / 2.0, b - (r + g) / 2.0)
}

/// Sky background from the low quantile `q` of the cell values, for a
/// colour whose hold threshold is `hold`. Below half the hold threshold the
/// quantile is the background (light pollution, airglow, colour cast);
/// at the hold threshold and above, even the least coloured quarter of the
/// sky shows the colour: an aurora over the whole sky cannot be told from a
/// cast on one image, the background is neutral (0), as before 1.18.0.
/// Linear in between, so the score never jumps.
pub fn sky_background(q: f64, hold: f64) -> f64 {
    let q = q.max(0.0);
    let (a, b) = (0.5 * hold, hold);
    if q <= a {
        q
    } else if q >= b || b <= a {
        0.0
    } else {
        q * (b - q) / (b - a)
    }
}

/// Value of rank `q` (0..1) of a list (sorted in place).
fn quantile(values: &mut [f64], q: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    values[((values.len() - 1) as f64 * q).floor() as usize]
}

/// Surface measure of one colour on the cell grid.
#[derive(Debug, Clone, Copy, Default)]
struct Surface {
    /// Mean excess of the cells above the hold threshold, over the trigger
    /// threshold (>= 1 for a clear aurora).
    score: f64,
    /// Cells in excess above the trigger / hold thresholds (% of the cells).
    area: f64,
    area_hold: f64,
    background: f64,
    spread: bool,
}

/// Score a colour from its cell values (`None`: cell fully masked).
fn surface(cells: &[Option<f64>], gw: u32, gh: u32, threshold: f64, hold: f64, eps: f64) -> Surface {
    let mut values: Vec<f64> = cells.iter().flatten().copied().collect();
    let n = values.len();
    if n == 0 || threshold <= 0.0 && hold <= 0.0 {
        return Surface::default();
    }
    let background = sky_background(quantile(&mut values, BACKGROUND_QUANTILE), threshold * hold);
    let (mut sum, mut n_hold, mut n_trig) = (0.0, 0usize, 0usize);
    // Spread blocks: 4×4 over the grid
    let mut blocks = [(0u32, 0u32); 16];
    for (i, c) in cells.iter().enumerate() {
        let Some(v) = c else { continue };
        let excess = v - background;
        let (cx, cy) = (i as u32 % gw, i as u32 / gw);
        let blk = ((cy * 4 / gh.max(1)) * 4 + cx * 4 / gw.max(1)) as usize;
        blocks[blk].1 += 1;
        if excess > threshold * hold {
            sum += excess;
            n_hold += 1;
            blocks[blk].0 += 1;
        }
        if excess > threshold {
            n_trig += 1;
        }
    }
    let spread = blocks.iter().filter(|(hit, total)| *total > 0 && *hit * 4 >= *total && *hit > 0).count() >= SPREAD_MIN_BLOCKS;
    Surface {
        score: if n_hold > 0 { sum / n_hold as f64 / (threshold + eps) } else { 0.0 },
        area: n_trig as f64 / n as f64 * 100.0,
        area_hold: n_hold as f64 / n as f64 * 100.0,
        background,
        spread,
    }
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
            gate: DetectionGate::default(),
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

    /// Sun/Moon gate for the next frames (default: none).
    pub fn set_gate(&mut self, gate: DetectionGate) {
        self.gate = DetectionGate { factor: gate.factor.max(1.0), suspended: gate.suspended };
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

        // 2. Build moon mask (if enabled): bright cells, and saturated
        // white blobs (the Moon itself is 1 to 2 pixels on the thumbnail,
        // its halo is what saturates)
        let moon_mask = if self.moon_mask_enabled {
            let mut m = self.compute_moon_mask(roi_pixels, width, roi_height);
            mask_saturated_blobs(roi_pixels, width, roi_height, &mut m);
            m
        } else {
            vec![false; total_roi_pixels]
        };
        let moon_masked = moon_mask.iter().any(|&m| m);

        // 3. Per-colour dominance: global means (logged, as before) and
        // means per cell of the grid (surface score).
        let eps = 1e-3_f64;
        let hold = if self.hysteresis_on > 0.0 { (self.hysteresis_off / self.hysteresis_on).clamp(0.0, 1.0) } else { 1.0 };
        let gw = GRID_W.min(width).max(1);
        let gh = GRID_H.min(roi_height).max(1);
        let n_cells = (gw * gh) as usize;
        // per cell: [green, red, violet] sums and pixel count
        let mut cell_sum = vec![[0.0_f64; 3]; n_cells];
        let mut cell_n = vec![0u32; n_cells];
        let mut green_total = 0.0_f64;
        let mut red_total = 0.0_f64;
        let mut blue_total = 0.0_f64;
        let mut lum_total = 0.0_f64;
        let mut pixel_count = 0u32;

        for i in 0..total_roi_pixels {
            if moon_mask[i] {
                continue;
            }
            let (x, y) = (i as u32 % width, i as u32 / width);
            let cell = ((y * gh / roi_height) * gw + x * gw / width) as usize;
            cell_n[cell] += 1;
            pixel_count += 1;

            let r = roi_pixels[i * 3] as f64;
            let g = roi_pixels[i * 3 + 1] as f64;
            let b = roi_pixels[i * 3 + 2] as f64;
            let lum = (r + g + b) / 3.0;

            // Very dark pixels (noise) count for the area, with no colour
            if lum < self.luminosity_threshold {
                continue;
            }

            let (gs, rs, bs) = dominance(r, g, b);
            green_total += gs.max(0.0);
            red_total += rs.max(0.0);
            blue_total += bs.max(0.0);
            lum_total += lum;
            let c = &mut cell_sum[cell];
            c[0] += gs.max(0.0);
            c[1] += rs.max(0.0);
            // Violet: red AND blue above the green (purple, not the blue
            // of a twilight or moonlit sky)
            c[2] += rs.min(bs).max(0.0);
        }

        if pixel_count == 0 {
            return DetectionResult::negative();
        }

        let pf = pixel_count as f64;
        let green_score = green_total / pf;
        let red_score = red_total / pf;
        let violet_score = blue_total / pf; // blue serves as violet proxy
        let luminosity = lum_total / pf;

        // 4. Surface score per colour, dominant colour. A colour only
        // counts if enough cells show it at the hold threshold.
        let cells = |k: usize| -> Vec<Option<f64>> {
            (0..n_cells).map(|c| (cell_n[c] > 0).then(|| cell_sum[c][k] / cell_n[c] as f64)).collect()
        };
        let candidates = [
            (surface(&cells(0), gw, gh, self.green_threshold, hold, eps), AuroraColor::Green),
            (surface(&cells(1), gw, gh, self.red_threshold, hold, eps), AuroraColor::Red),
            (surface(&cells(2), gw, gh, self.blue_threshold * 0.5, hold, eps), AuroraColor::Violet),
        ];
        let mut best = (Surface::default(), AuroraColor::None);
        for c in candidates {
            if c.0.area_hold > self.area_min_percent && c.0.score > best.0.score {
                best = c;
            }
        }
        let (s, mut aurora_color) = best;
        // Background logged: of the dominant colour, else of the green
        let background = if aurora_color == AuroraColor::None { candidates[0].0.background } else { s.background };
        let aurora_score = s.score;
        let area_percent = s.area;

        // 5. Spatial spread, on the cells in excess of the dominant colour
        let is_spread = aurora_color != AuroraColor::None && s.spread;

        // 6. Sun/Moon gate, then hystérésis decision on the gated score
        let gated = aurora_score / self.gate.factor.max(1.0);
        let detected = if self.gate.suspended {
            false
        } else if self.was_detected {
            // Was detected: stay detected unless score drops below hysteresis_off
            gated >= self.hysteresis_off && is_spread
        } else {
            // Not detected: trigger only on a clear aurora over enough area
            gated >= self.hysteresis_on && area_percent > self.area_min_percent && is_spread
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
            ((gated - self.hysteresis_off) / span).clamp(0.0, 1.0)
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
            background,
            gate_factor: self.gate.factor.max(1.0),
            suspended: self.gate.suspended,
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

    /// Reset hystérésis state (e.g., when entering Watch phase).
    pub fn reset(&mut self) {
        self.was_detected = false;
    }
}


/// Mask the connected blobs of saturated white pixels and a margin around
/// them (3 times their radius plus 2 pixels): the Moon and its halo, street
/// lamps. Used while the orientation of the camera is unknown (no position
/// of the Moon in the image).
fn mask_saturated_blobs(rgb: &[u8], width: u32, height: u32, mask: &mut [bool]) {
    let (w, h) = (width as usize, height as usize);
    let n = w * h;
    let white = |i: usize| rgb[i * 3] >= SATURATED && rgb[i * 3 + 1] >= SATURATED && rgb[i * 3 + 2] >= SATURATED;
    let mut seen = vec![false; n];
    let mut discs: Vec<(f64, f64, f64)> = Vec::new();
    for start in 0..n {
        if seen[start] || !white(start) {
            continue;
        }
        // Flood fill (4-connectivity)
        let (mut sx, mut sy, mut count) = (0.0, 0.0, 0usize);
        let mut stack = vec![start];
        seen[start] = true;
        while let Some(p) = stack.pop() {
            let (x, y) = (p % w, p / w);
            sx += x as f64;
            sy += y as f64;
            count += 1;
            let mut push = |q: usize| {
                if !seen[q] && white(q) {
                    seen[q] = true;
                    stack.push(q);
                }
            };
            if x > 0 { push(p - 1); }
            if x + 1 < w { push(p + 1); }
            if y > 0 { push(p - w); }
            if y + 1 < h { push(p + w); }
        }
        let radius = (count as f64 / std::f64::consts::PI).sqrt();
        discs.push((sx / count as f64, sy / count as f64, 3.0 * radius + 2.0));
    }
    // The largest blobs first, at most 64 (a lit village)
    discs.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    for (cx, cy, r) in discs.into_iter().take(64) {
        let (x0, x1) = ((cx - r).floor().max(0.0) as usize, ((cx + r).ceil() as usize).min(w.saturating_sub(1)));
        let (y0, y1) = ((cy - r).floor().max(0.0) as usize, ((cy + r).ceil() as usize).min(h.saturating_sub(1)));
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (dx, dy) = (x as f64 - cx, y as f64 - cy);
                if dx * dx + dy * dy <= r * r {
                    mask[y * w + x] = true;
                }
            }
        }
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

    /// Neutral sky of luminance `sky`, with rows `y0..y1` (fraction of the
    /// image height) painted `band`.
    fn sky_with_band(w: u32, h: u32, sky: (u8, u8, u8), band: (u8, u8, u8), y0: f64, y1: f64) -> Vec<u8> {
        let mut data = vec![0u8; (w * h * 3) as usize];
        for y in 0..h {
            let f = y as f64 / h as f64;
            let c = if f >= y0 && f < y1 { band } else { sky };
            for x in 0..w {
                let i = ((y * w + x) * 3) as usize;
                data[i] = c.0;
                data[i + 1] = c.1;
                data[i + 2] = c.2;
            }
        }
        data
    }

    #[test]
    fn faint_aurora_low_on_the_horizon_is_detected() {
        // D-8: green dominance 20 (threshold 15) on the lowest 20 % of the
        // 65 % ROI (rows 52 % to 65 %), on a grey sky of luminance 40.
        let (w, h) = (320, 240);
        let img = sky_with_band(w, h, (40, 40, 40), (30, 50, 30), 0.52, 0.65);
        let r = default_detector().analyze(&img, w, h);
        // Mean over the whole ROI: 20 × 20 % = 4, far below 15 (before
        // 1.18.0 the score was this mean over the threshold: 0.27)
        assert!(r.green_score < 5.0, "{:?}", r);
        assert!(r.detected, "{:?}", r);
        assert_eq!(r.aurora_color, AuroraColor::Green);
        assert!((r.aurora_score - 20.0 / 15.0).abs() < 0.05, "{}", r.aurora_score);
        assert!(r.area_percent > 15.0 && r.area_percent < 25.0, "{}", r.area_percent);
        // The same sky without the band: nothing
        let none = default_detector().analyze(&sky_with_band(w, h, (40, 40, 40), (40, 40, 40), 0.0, 0.0), w, h);
        assert!(!none.detected && none.aurora_score == 0.0, "{:?}", none);
    }

    #[test]
    fn a_colour_cast_is_removed_as_sky_background() {
        let (w, h) = (320, 240);
        // Whole sky slightly green (dominance 5: light pollution, airglow)
        let cast = sky_with_band(w, h, (40, 45, 40), (40, 45, 40), 0.0, 0.0);
        let r = default_detector().analyze(&cast, w, h);
        assert!(!r.detected && r.aurora_score == 0.0, "{:?}", r);
        assert!((r.background - 5.0).abs() < 0.01, "background {}", r.background);
        // The same cast with a band 20 above it: the band is measured
        // relative to the background (excess 20, score 1.33)
        let band = sky_with_band(w, h, (40, 45, 40), (33, 58, 33), 0.30, 0.45);
        let r = default_detector().analyze(&band, w, h);
        assert!(r.detected, "{:?}", r);
        assert!((r.aurora_score - 20.0 / 15.0).abs() < 0.05, "{}", r.aurora_score);
    }

    #[test]
    fn background_is_neutral_when_the_whole_sky_is_coloured() {
        // Below half the hold threshold: the quantile; above the hold
        // threshold: neutral; continuous in between.
        assert_eq!(sky_background(3.0, 10.5), 3.0);
        assert_eq!(sky_background(-4.0, 10.5), 0.0);
        assert_eq!(sky_background(12.0, 10.5), 0.0);
        let mid = sky_background(7.875, 10.5);
        assert!(mid > 0.0 && mid < 7.875);
        assert!((sky_background(5.25, 10.5) - sky_background(5.2501, 10.5)).abs() < 1e-3);
    }

    #[test]
    fn a_compact_green_light_is_not_spread() {
        // Green dominance 60 in a 40×30 corner (one block of the 4×4 grid)
        let (w, h) = (320, 240);
        let mut img = sky_with_band(w, h, (40, 40, 40), (40, 40, 40), 0.0, 0.0);
        for y in 10..40 {
            for x in 10..50 {
                let i = ((y * w + x) * 3) as usize;
                img[i] = 20;
                img[i + 1] = 80;
                img[i + 2] = 20;
            }
        }
        let r = default_detector().analyze(&img, w, h);
        assert!(!r.detected, "{:?}", r);
    }

    #[test]
    fn a_saturated_moon_and_its_halo_are_masked() {
        // Moon: 3×3 white pixels; halo: bluish pixels around (moonlit sky
        // under a daylight white balance) that would look violet otherwise
        let (w, h) = (320, 240);
        let mut img = sky_with_band(w, h, (40, 40, 40), (40, 40, 40), 0.0, 0.0);
        for y in 40..60u32 {
            for x in 150..170u32 {
                let i = ((y * w + x) * 3) as usize;
                let (dx, dy) = (x as i32 - 160, y as i32 - 50);
                if dx.abs() <= 1 && dy.abs() <= 1 {
                    img[i..i + 3].copy_from_slice(&[255, 255, 255]);
                } else if dx * dx + dy * dy <= 16 {
                    img[i..i + 3].copy_from_slice(&[120, 60, 140]);
                }
            }
        }
        let r = default_detector().analyze(&img, w, h);
        assert!(r.moon_masked);
        assert_eq!(r.violet_score, 0.0, "halo inside the mask: {:?}", r);
        assert!(!r.detected);
    }

    #[test]
    fn the_gate_raises_the_thresholds_or_suspends_the_detection() {
        // Score 1.33: triggers without gate
        let img = make_uniform_image(100, 100, 40, 60, 40);
        let mut det = default_detector();
        det.set_gate(DetectionGate { factor: 1.5, suspended: false });
        let r = det.analyze(&img, 100, 100);
        assert!(!r.detected, "1.33 / 1.5 < 1: {:?}", r);
        assert!((r.aurora_score - 1.333).abs() < 0.01, "raw score kept for the log");
        assert_eq!(r.gate_factor, 1.5);
        // A strong aurora still triggers under the Moon
        let strong = det.analyze(&make_uniform_image(100, 100, 30, 80, 30), 100, 100);
        assert!(strong.detected);
        // Twilight: nothing, whatever the score
        det.set_gate(DetectionGate { factor: 1.0, suspended: true });
        let r = det.analyze(&make_uniform_image(100, 100, 30, 80, 30), 100, 100);
        assert!(!r.detected && r.suspended && r.aurora_score > 3.0);
        // Gate lifted: back to normal
        det.set_gate(DetectionGate::default());
        assert!(det.analyze(&img, 100, 100).detected);
    }
}
