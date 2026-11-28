//! Star detection on a night-sky frame (pure functions, no I/O), for the
//! alignment of the stack (`core::align`).
//!
//! Steps, all on the **sky only** (the first `roi_top_percent` % of the rows,
//! the same zone as the aurora detection; the ground is never looked at):
//! 1. luminance in linear light (mean of R, G, B after the sRGB transfer
//!    function), reduced by a 2×2 binning of the full-resolution frame (the
//!    frame already decoded for the stack, not the 320×240 analysis
//!    thumbnail, which shows too few stars): noise per binned pixel is about
//!    halved and the work divided by 4;
//! 2. local background and noise: median and MAD (scaled by 1.4826 to read
//!    as a standard deviation) of tiles of [`TILE`]×[`TILE`] binned pixels,
//!    interpolated bilinearly between the tile centres, so that the aurora
//!    and the light pollution gradients are followed;
//! 3. groups of connected pixels (8-connectivity) more than [`LOW_SIGMA`] σ
//!    above the background; each group is one candidate, its brightest
//!    pixel (a local maximum) must be more than [`DETECT_SIGMA`] σ above the
//!    background;
//! 4. rejections:
//!    - *hot pixel*: a sensor defect lights one full-resolution pixel, so
//!      after binning its neighbours stay at the background, while a star
//!      spreads over several pixels. A candidate whose 8 neighbours sum to
//!      less than [`MIN_NEIGHBOUR_RATIO`] of its peak is dropped. This
//!      matters: hot pixels do not move with the sky, and the alignment
//!      would otherwise find "no rotation";
//!    - *saturated*: any sub-pixel of the group at code 255 (bright star,
//!      planet, Moon: its centre is unreliable);
//!    - *extended*: groups larger than [`MAX_AREA`] binned pixels (Moon,
//!      lit clouds, aurora rays, ground lights);
//! 5. centroid weighted by the light above the background on the 3×3 binned
//!    pixels around the peak (sub-pixel), converted to full-resolution
//!    coordinates (pixel centres at integer values);
//! 6. the [`MAX_STARS`] brightest stars (sum of the light above the
//!    background over the group) are kept, brightest first.
//!
//! Memory: 4 bytes + 1 byte per binned pixel, i.e. about 15 MB for the sky
//! of a 4056×3040 frame (2028×1520 binned pixels at most).
//!
//! The thresholds are design choices checked on synthetic star fields
//! (tests below), to be validated on real nights with the 2.7 mm lens: the
//! number of stars found, and whether hot pixels of the JPEG (already
//! spread by the ISP demosaicing) are rejected.

use crate::core::denoise::{encode_linear, to_linear_lut};

/// Tile side of the background estimate, in binned pixels.
pub const TILE: usize = 32;
/// Threshold of the pixels of a group, in σ above the background.
pub const LOW_SIGMA: f32 = 3.0;
/// Threshold of the peak of a group, in σ above the background.
pub const DETECT_SIGMA: f32 = 5.0;
/// Largest group kept, in binned pixels (a 5×5 spot).
pub const MAX_AREA: usize = 25;
/// Minimum light of the 8 neighbours of the peak, as a fraction of the peak.
pub const MIN_NEIGHBOUR_RATIO: f32 = 0.3;
/// Stars kept at most (the alignment uses the brightest ones).
pub const MAX_STARS: usize = 50;

/// One star, in full-resolution pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Star {
    pub x: f64,
    pub y: f64,
    /// Light above the background (linear units × binned pixels).
    pub flux: f32,
}

/// Stars found and candidates rejected, for the session log.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StarField {
    pub stars: Vec<Star>,
    /// Candidates above the threshold, before the rejections.
    pub candidates: usize,
    pub rejected_isolated: usize,
    pub rejected_saturated: usize,
    pub rejected_extended: usize,
}

/// Number of sky rows for a `roi_top_percent` (same rounding as the ROI of
/// the aurora detection, `orchestrator::crop_roi`).
pub fn sky_rows(height: u32, roi_top_percent: u32) -> u32 {
    ((height as f64 * roi_top_percent as f64 / 100.0) as u32).min(height)
}

const ABOVE: u8 = 1;
const VISITED: u8 = 2;
const SATURATED: u8 = 4;

/// 2×2 binned luminance (linear light) of the first `rows` rows, and a
/// saturation flag per binned pixel.
fn binned_luminance(rgb: &[u8], width: usize, rows: usize) -> (Vec<f32>, Vec<u8>, usize, usize) {
    let lut = to_linear_lut();
    let (bw, bh) = (width / 2, rows / 2);
    let mut lum = vec![0f32; bw * bh];
    let mut flags = vec![0u8; bw * bh];
    for by in 0..bh {
        for bx in 0..bw {
            let mut sum = 0.0;
            let mut sat = false;
            for dy in 0..2 {
                let row = (by * 2 + dy) * width;
                for dx in 0..2 {
                    let i = (row + bx * 2 + dx) * 3;
                    let px = &rgb[i..i + 3];
                    sat |= px.contains(&255);
                    sum += lut[px[0] as usize] + lut[px[1] as usize] + lut[px[2] as usize];
                }
            }
            lum[by * bw + bx] = sum / 12.0;
            if sat {
                flags[by * bw + bx] = SATURATED;
            }
        }
    }
    (lum, flags, bw, bh)
}

/// Median and MAD-based σ of each tile.
fn tile_stats(lum: &[f32], bw: usize, bh: usize) -> (Vec<(f32, f32)>, usize, usize) {
    let (tw, th) = (bw.div_ceil(TILE), bh.div_ceil(TILE));
    let mut stats = Vec::with_capacity(tw * th);
    let mut buf = Vec::with_capacity(TILE * TILE);
    for ty in 0..th {
        for tx in 0..tw {
            buf.clear();
            for y in ty * TILE..((ty + 1) * TILE).min(bh) {
                buf.extend_from_slice(&lum[y * bw + tx * TILE..y * bw + ((tx + 1) * TILE).min(bw)]);
            }
            let med = median(&mut buf);
            for v in buf.iter_mut() {
                *v = (*v - med).abs();
            }
            let mad = median(&mut buf);
            // σ floor: a quarter of one sRGB code step at this level, so that
            // a perfectly flat (quantised) sky does not turn noise into stars
            let code = encode_linear(med);
            let lut = to_linear_lut();
            let step = lut[(code as usize + 1).min(255)] - lut[code as usize];
            stats.push((med, (1.4826 * mad).max(step * 0.25)));
        }
    }
    (stats, tw, th)
}

fn median(v: &mut [f32]) -> f32 {
    let mid = v.len() / 2;
    *v.select_nth_unstable_by(mid, |a, b| a.total_cmp(b)).1
}

/// Background and σ at a binned pixel, bilinear between tile centres.
fn background_at(stats: &[(f32, f32)], tw: usize, th: usize, x: usize, y: usize) -> (f32, f32) {
    let fx = ((x as f32 + 0.5) / TILE as f32 - 0.5).clamp(0.0, (tw - 1) as f32);
    let fy = ((y as f32 + 0.5) / TILE as f32 - 0.5).clamp(0.0, (th - 1) as f32);
    let (x0, y0) = (fx as usize, fy as usize);
    let (x1, y1) = ((x0 + 1).min(tw - 1), (y0 + 1).min(th - 1));
    let (ux, uy) = (fx - x0 as f32, fy - y0 as f32);
    let s = |xx: usize, yy: usize| stats[yy * tw + xx];
    let lerp = |a: (f32, f32), b: (f32, f32), u: f32| (a.0 + (b.0 - a.0) * u, a.1 + (b.1 - a.1) * u);
    let top = lerp(s(x0, y0), s(x1, y0), ux);
    let bottom = lerp(s(x0, y1), s(x1, y1), ux);
    lerp(top, bottom, uy)
}

/// Detect the stars of the sky part of a full-resolution RGB frame.
pub fn detect_stars(rgb: &[u8], width: u32, height: u32, roi_top_percent: u32) -> StarField {
    let (w, rows) = (width as usize, sky_rows(height, roi_top_percent) as usize);
    let mut field = StarField::default();
    if rgb.len() < w * height as usize * 3 || w < 2 * TILE || rows < 2 * TILE {
        return field;
    }
    let (mut lum, mut flags, bw, bh) = binned_luminance(rgb, w, rows);
    let (stats, tw, th) = tile_stats(&lum, bw, bh);
    // Light above the background, in place; threshold mask
    let mut sigma = vec![0f32; bw * bh];
    for y in 0..bh {
        for x in 0..bw {
            let i = y * bw + x;
            let (bg, sd) = background_at(&stats, tw, th, x, y);
            lum[i] -= bg;
            sigma[i] = sd;
            if lum[i] > LOW_SIGMA * sd {
                flags[i] |= ABOVE;
            }
        }
    }
    let mut stack = Vec::new();
    let mut stars = Vec::new();
    for start in 0..bw * bh {
        if flags[start] & (ABOVE | VISITED) != ABOVE {
            continue;
        }
        // Flood fill of the group (visited even past MAX_AREA, so that one
        // large object is one rejection, not thousands)
        flags[start] |= VISITED;
        stack.push(start);
        let (mut area, mut peak, mut flux, mut sat) = (0usize, start, 0f32, false);
        while let Some(i) = stack.pop() {
            area += 1;
            flux += lum[i];
            sat |= flags[i] & SATURATED != 0;
            if lum[i] > lum[peak] {
                peak = i;
            }
            let (x, y) = (i % bw, i / bw);
            for ny in y.saturating_sub(1)..=(y + 1).min(bh - 1) {
                for nx in x.saturating_sub(1)..=(x + 1).min(bw - 1) {
                    let j = ny * bw + nx;
                    if flags[j] & (ABOVE | VISITED) == ABOVE {
                        flags[j] |= VISITED;
                        stack.push(j);
                    }
                }
            }
        }
        let (px, py) = (peak % bw, peak / bw);
        if lum[peak] <= DETECT_SIGMA * sigma[peak] || px == 0 || py == 0 || px == bw - 1 || py == bh - 1 {
            continue;
        }
        field.candidates += 1;
        if area > MAX_AREA {
            field.rejected_extended += 1;
            continue;
        }
        if sat {
            field.rejected_saturated += 1;
            continue;
        }
        let (mut neighbours, mut sw, mut sx, mut sy) = (0f32, 0f64, 0f64, 0f64);
        for dy in 0..3 {
            for dx in 0..3 {
                let (x, y) = (px + dx - 1, py + dy - 1);
                let v = lum[y * bw + x].max(0.0);
                if (dx, dy) != (1, 1) {
                    neighbours += v;
                }
                sw += v as f64;
                sx += v as f64 * x as f64;
                sy += v as f64 * y as f64;
            }
        }
        if neighbours < MIN_NEIGHBOUR_RATIO * lum[peak] {
            field.rejected_isolated += 1;
            continue;
        }
        // Binned pixel b covers full pixels 2b and 2b+1: centre at 2b + 0.5
        stars.push(Star { x: 2.0 * sx / sw + 0.5, y: 2.0 * sy / sw + 0.5, flux });
    }
    stars.sort_by(|a, b| b.flux.total_cmp(&a.flux));
    stars.truncate(MAX_STARS);
    field.stars = stars;
    field
}

/// Synthetic star field for the tests and `aurion bench`: dark sky with
/// noise, gaussian stars of `fwhm` full-resolution pixels at the given
/// positions (`(x, y, peak)` with peak in sRGB-code-like units above the
/// background), the rows below `sky_rows` filled with a brighter "ground".
pub fn render_stars(width: u32, height: u32, stars: &[(f64, f64, f64)], fwhm: f64, noise: f64, sky_rows: u32, seed: u64) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let lut = to_linear_lut();
    let bg = lut[18] as f64;
    let mut light = vec![bg; w * h];
    let s = fwhm / 2.3548;
    let r = (3.0 * s).ceil() as i64 + 1;
    for &(x0, y0, peak) in stars {
        let amp = (lut[(18.0 + peak).min(255.0) as usize] as f64 - bg).max(0.0);
        for y in (y0.round() as i64 - r).max(0)..=(y0.round() as i64 + r).min(h as i64 - 1) {
            for x in (x0.round() as i64 - r).max(0)..=(x0.round() as i64 + r).min(w as i64 - 1) {
                let d2 = (x as f64 - x0).powi(2) + (y as f64 - y0).powi(2);
                light[y as usize * w + x as usize] += amp * (-d2 / (2.0 * s * s)).exp();
            }
        }
    }
    let mut rng = crate::core::denoise::XorShift::new(seed);
    let mut rgb = vec![0u8; w * h * 3];
    for (p, &l) in light.iter().enumerate() {
        let ground = p / w >= sky_rows as usize;
        let code = if ground { 60.0 + (p % w) as f64 / w as f64 * 40.0 } else { encode_linear(l as f32) as f64 };
        for c in 0..3 {
            rgb[p * 3 + c] = (code + noise * rng.gaussian()).round().clamp(0.0, 254.0) as u8;
        }
    }
    rgb
}

/// Random star positions `(x, y, peak)` in the first `sky_rows` rows.
pub fn random_stars(width: u32, sky_rows: u32, count: usize, seed: u64) -> Vec<(f64, f64, f64)> {
    let mut rng = crate::core::denoise::XorShift::new(seed);
    let u = |rng: &mut crate::core::denoise::XorShift| rng.next_u32() as f64 / u32::MAX as f64;
    (0..count)
        .map(|_| {
            let x = 20.0 + u(&mut rng) * (width as f64 - 40.0);
            let y = 20.0 + u(&mut rng) * (sky_rows as f64 - 40.0);
            // Many faint stars, few bright ones
            let peak = 25.0 + 180.0 * u(&mut rng).powi(3);
            (x, y, peak)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_synthetic_stars_with_subpixel_centroids() {
        let (w, h) = (800, 600);
        let sky = sky_rows(h, 65);
        let truth = random_stars(w, sky, 40, 5);
        let rgb = render_stars(w, h, &truth, 3.0, 1.5, sky, 1);
        let field = detect_stars(&rgb, w, h, 65);
        assert!(field.stars.len() >= 30, "{} stars of 40 ({:?})", field.stars.len(), field);
        let mut errors = Vec::new();
        for s in &field.stars {
            let d = truth.iter().map(|t| ((t.0 - s.x).powi(2) + (t.1 - s.y).powi(2)).sqrt()).fold(f64::MAX, f64::min);
            errors.push(d);
        }
        errors.sort_by(f64::total_cmp);
        let median = errors[errors.len() / 2];
        assert!(median < 0.5, "median centroid error {:.2} px", median);
        assert!(field.stars.iter().all(|s| s.y < sky as f64), "only the sky is searched");
        assert!(field.stars.windows(2).all(|p| p[0].flux >= p[1].flux), "brightest first");
    }

    #[test]
    fn rejects_hot_pixels_saturated_and_extended_objects() {
        let (w, h) = (640, 480);
        let sky = sky_rows(h, 65);
        let mut rgb = render_stars(w, h, &[(100.0, 100.0, 120.0), (300.0, 150.0, 100.0)], 3.0, 1.0, sky, 2);
        // Hot pixels: one full-resolution pixel, all channels
        for &(x, y) in &[(200usize, 50usize), (401, 201), (77, 260)] {
            let i = (y * w as usize + x) * 3;
            rgb[i..i + 3].copy_from_slice(&[200, 200, 200]);
        }
        // Saturated star
        let i = (120 * w as usize + 500) * 3;
        for dy in 0..3 {
            for dx in 0..3 {
                let j = i + (dy * w as usize + dx) * 3;
                rgb[j..j + 3].copy_from_slice(&[255, 255, 255]);
            }
        }
        // Moon-like disc of radius 15 px
        for y in 230..270usize {
            for x in 480..520usize {
                if (x as f64 - 500.0).powi(2) + (y as f64 - 250.0).powi(2) < 225.0 {
                    let j = (y * w as usize + x) * 3;
                    rgb[j..j + 3].copy_from_slice(&[180, 180, 180]);
                }
            }
        }
        let field = detect_stars(&rgb, w, h, 65);
        assert_eq!(field.stars.len(), 2, "{:?}", field);
        assert!(field.rejected_isolated >= 3, "{:?}", field);
        assert!(field.rejected_saturated >= 1, "{:?}", field);
        assert!(field.rejected_extended >= 1, "{:?}", field);
    }

    #[test]
    fn cloudy_or_tiny_frames_give_no_star() {
        // Uniform grey sky with noise: nothing
        let (w, h) = (400, 300);
        let rgb = render_stars(w, h, &[], 3.0, 2.0, sky_rows(h, 65), 3);
        assert!(detect_stars(&rgb, w, h, 65).stars.is_empty());
        // Too small or too short buffers
        assert_eq!(detect_stars(&rgb, 40, 30, 65), StarField::default());
        assert_eq!(detect_stars(&rgb[..100], w, h, 65), StarField::default());
    }

    #[test]
    fn keeps_at_most_max_stars() {
        let (w, h) = (1200, 900);
        let sky = sky_rows(h, 65);
        let truth = random_stars(w, sky, 150, 9);
        let field = detect_stars(&render_stars(w, h, &truth, 3.0, 1.0, sky, 4), w, h, 65);
        assert_eq!(field.stars.len(), MAX_STARS);
    }
}
