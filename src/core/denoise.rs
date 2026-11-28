//! Noise reduction for night-sky photos (pure functions, no I/O).
//!
//! Two complementary treatments, both preserving stars and aurora shapes:
//!
//! 1. **Hot pixel removal** ([`remove_hot_pixels`]). Long exposures on a
//!    warm sensor produce isolated pixels far brighter than everything
//!    around them. A pixel is corrected only when it exceeds the *brightest*
//!    of its 8 neighbours by `threshold`: a star covers several pixels (its
//!    neighbours are bright too) so it is left untouched. Corrected pixels
//!    take the median of their neighbours.
//!
//! 2. **Stacking with sigma rejection** ([`Stacker`]). Averaging N frames
//!    divides random noise by about √N. The average is taken in linear light
//!    (sRGB codes converted to light, averaged, converted back), not on the
//!    gamma-encoded codes. Mean and variance of each sub-pixel are updated
//!    frame by frame with Welford's method; at the end, the brightest and
//!    then the darkest value of each sub-pixel are discarded when they lie
//!    more than [`SIGMA_REJECT`] standard deviations from the mean of the
//!    other values (from 3 frames on). This removes planes, satellites and
//!    cosmic-ray hits present on a single frame without throwing away good
//!    frames, unlike the former min/max rejection which always dropped 2.
//!    Limit: one high and one low outlier per sub-pixel at most. Memory:
//!    10 bytes per sub-pixel plus 1 byte per pixel (frames covering it):
//!    4056×3040 → 370 MB + 12 MB, whatever N (up to [`MAX_STACK`]).
//!    Frames can be added resampled on a [`Transform`] found from the stars
//!    ([`Stacker::add_warped`], see `core::align`): pixels whose source falls
//!    outside the sky of the frame are left out of the mean of that pixel,
//!    which is why the number of frames is kept per pixel.
//!
//! The sensor ISP denoise (`rpicam-still --denoise cdn_hq`) is configured
//! separately in the camera adapter.

/// Replace isolated bright pixels (per channel) by the median of their
/// 8 neighbours. Returns the number of corrected sub-pixels.
///
/// A sub-pixel is "hot" when it exceeds its brightest neighbour by
/// `threshold` **and** its neighbours are at the level of the local
/// background (median of the outer 5×5 ring). A star is an extended spot:
/// its neighbours are lit above the background, so it is never touched.
/// The 2-pixel border is left untouched.
pub fn remove_hot_pixels(rgb: &mut [u8], width: u32, height: u32, threshold: u8) -> usize {
    let (w, h) = (width as usize, height as usize);
    if w < 5 || h < 5 || rgb.len() < w * h * 3 {
        return 0;
    }
    let src = rgb.to_vec();
    let t = threshold as i16;
    let mut corrected = 0;
    let row = w * 3;
    for y in 2..h - 2 {
        for x in 2..w - 2 {
            for c in 0..3 {
                let i = y * row + x * 3 + c;
                let v = src[i] as i16;
                // Fast path: the dark sky is by far the most common case.
                if v <= t {
                    continue;
                }
                let ring1 = [
                    src[i - row - 3], src[i - row], src[i - row + 3],
                    src[i - 3], src[i + 3],
                    src[i + row - 3], src[i + row], src[i + row + 3],
                ];
                let max1 = *ring1.iter().max().unwrap() as i16;
                if v - max1 <= t {
                    continue;
                }
                // Outer ring (16 pixels at distance 2) = local background
                let mut ring2 = [0u8; 16];
                let mut k = 0;
                for dy in -2i32..=2 {
                    for dx in -2i32..=2 {
                        if dy.abs() == 2 || dx.abs() == 2 {
                            let j = (i as i64 + dy as i64 * row as i64 + dx as i64 * 3) as usize;
                            ring2[k] = src[j];
                            k += 1;
                        }
                    }
                }
                ring2.sort_unstable();
                let background = ((ring2[7] as u16 + ring2[8] as u16) / 2) as i16;
                let mut sorted = ring1;
                sorted.sort_unstable();
                let median1 = ((sorted[3] as u16 + sorted[4] as u16) / 2) as i16;
                // Neighbours lit well above the background → a star, keep it
                if median1 - background > t / 2 {
                    continue;
                }
                rgb[i] = median1 as u8;
                corrected += 1;
            }
        }
    }
    corrected
}

/// Rejection threshold of [`Stacker`], in standard deviations.
pub const SIGMA_REJECT: f32 = 2.5;

/// sRGB transfer function (IEC 61966-2-1), code 0..255 → linear 0..1.
pub(crate) fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb(l: f32) -> f32 {
    if l <= 0.003_130_8 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 }
}

/// Linear light of each 8-bit sRGB code.
pub(crate) fn to_linear_lut() -> &'static [f32; 256] {
    static LUT: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
    LUT.get_or_init(|| std::array::from_fn(|v| srgb_to_linear(v as f32 / 255.0)))
}

/// 8-bit sRGB code of each linear value quantised on 16 bits.
fn to_srgb_lut() -> &'static [u8] {
    static LUT: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    LUT.get_or_init(|| (0..=u16::MAX).map(|i| (linear_to_srgb(i as f32 / 65535.0) * 255.0).round().clamp(0.0, 255.0) as u8).collect())
}

pub(crate) fn encode_linear(l: f32) -> u8 {
    to_srgb_lut()[(l.clamp(0.0, 1.0) * 65535.0).round() as usize]
}

/// Accumulates frames of identical size and produces a denoised average.
pub struct Stacker {
    width: u32,
    height: u32,
    /// Welford running mean (linear light) of each sub-pixel.
    mean: Vec<f32>,
    /// Welford sum of squared deviations from the mean.
    m2: Vec<f32>,
    /// Darkest and brightest sRGB code seen (rejection candidates).
    min: Vec<u8>,
    max: Vec<u8>,
    /// Frames that covered each pixel (all of them without resampling).
    n: Vec<u8>,
    count: u32,
}

/// Maximum number of frames of one stack (the per-pixel count is a `u8`).
pub const MAX_STACK: u32 = 255;

/// Count, sum and sum of squares of `n` values once `x` is taken out, and
/// the standard deviation of the remaining ones (`n >= 3`).
fn without(n: f32, sum: f32, sumsq: f32, x: f32) -> (f32, f32, f32, f32) {
    let (n1, s1, q1) = (n - 1.0, sum - x, sumsq - x * x);
    let mean = s1 / n1;
    let var = ((q1 - n1 * mean * mean) / (n1 - 1.0)).max(0.0);
    (n1, s1, q1, var.sqrt())
}

/// Affine map from the pixels of the stack (reference frame) to the pixels
/// of a new frame: `x' = a·x + b·y + tx`, `y' = c·x + d·y + ty`. Pixel
/// centres are at integer coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub tx: f64,
    pub ty: f64,
}

impl Transform {
    pub const IDENTITY: Transform = Transform { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: 0.0 };

    /// Rotation by `angle` radians around `(cx, cy)`.
    pub fn rotation_about(angle: f64, cx: f64, cy: f64) -> Transform {
        let (s, c) = angle.sin_cos();
        Transform { a: c, b: -s, c: s, d: c, tx: cx - c * cx + s * cy, ty: cy - s * cx - c * cy }
    }

    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (self.a * x + self.b * y + self.tx, self.c * x + self.d * y + self.ty)
    }

    /// Rotation angle (radians) of the linear part.
    pub fn angle(&self) -> f64 {
        (self.c - self.b).atan2(self.a + self.d)
    }

    /// Fixed point of the map (the centre of the rotation), None for a pure
    /// translation or a degenerate map.
    pub fn fixed_point(&self) -> Option<(f64, f64)> {
        // (I − M)·p = t
        let (m11, m12, m21, m22) = (1.0 - self.a, -self.b, -self.c, 1.0 - self.d);
        let det = m11 * m22 - m12 * m21;
        if det.abs() < 1e-9 {
            return None;
        }
        Some(((self.tx * m22 - m12 * self.ty) / det, (m11 * self.ty - m21 * self.tx) / det))
    }
}

impl Stacker {
    pub fn new(width: u32, height: u32) -> Self {
        let px = (width * height) as usize;
        let n = px * 3;
        Self { width, height, mean: vec![0.0; n], m2: vec![0.0; n], min: vec![u8::MAX; n], max: vec![0; n], n: vec![0; px], count: 0 }
    }

    pub fn count(&self) -> u32 {
        self.count
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Welford update of sub-pixel `i` with linear value `x` (code `v`),
    /// `inv_n` = 1 / frames now covering its pixel.
    #[inline]
    fn update(&mut self, i: usize, x: f32, v: u8, inv_n: f32) {
        let d = x - self.mean[i];
        self.mean[i] += d * inv_n;
        self.m2[i] += d * (x - self.mean[i]);
        self.min[i] = self.min[i].min(v);
        self.max[i] = self.max[i].max(v);
    }

    /// Add a frame. Returns false (and ignores it) if the size differs or the
    /// stack is full.
    pub fn add(&mut self, rgb: &[u8]) -> bool {
        if rgb.len() != self.mean.len() || self.count >= MAX_STACK {
            return false;
        }
        let lut = to_linear_lut();
        self.count += 1;
        for p in 0..self.n.len() {
            self.n[p] += 1;
            let inv_n = 1.0 / self.n[p] as f32;
            for c in 0..3 {
                let i = p * 3 + c;
                let v = rgb[i];
                self.update(i, lut[v as usize], v, inv_n);
            }
        }
        true
    }

    /// Add a frame resampled into the geometry of the stack: the stack pixel
    /// `(x, y)` takes the frame at `t.apply(x, y)` (bilinear interpolation,
    /// in linear light). Only the first `sky_rows` rows are resampled, and
    /// only from the sky of the frame; the rows below (the ground, which does
    /// not turn with the sky) are added as they are. Stack pixels whose
    /// source falls outside the sky of the frame are left out for this frame.
    /// Returns the number of pixels left out, None if the size differs or
    /// the stack is full.
    pub fn add_warped(&mut self, rgb: &[u8], t: &Transform, sky_rows: u32) -> Option<usize> {
        if rgb.len() != self.mean.len() || self.count >= MAX_STACK {
            return None;
        }
        let lut = to_linear_lut();
        self.count += 1;
        let (w, h) = (self.width as usize, self.height as usize);
        let sky = (sky_rows as usize).min(h);
        let (xmax, ymax) = (w as f64 - 1.0, sky as f64 - 1.0);
        let mut left_out = 0;
        for y in 0..h {
            let (mut sx, mut sy) = t.apply(0.0, y as f64);
            for x in 0..w {
                let p = y * w + x;
                if y >= sky {
                    self.n[p] += 1;
                    let inv_n = 1.0 / self.n[p] as f32;
                    for c in 0..3 {
                        let v = rgb[p * 3 + c];
                        self.update(p * 3 + c, lut[v as usize], v, inv_n);
                    }
                    continue;
                }
                let (fx, fy) = (sx, sy);
                sx += t.a;
                sy += t.c;
                if !(0.0..=xmax).contains(&fx) || !(0.0..=ymax).contains(&fy) {
                    left_out += 1;
                    continue;
                }
                let (x0, y0) = ((fx as usize).min(w.saturating_sub(2)), (fy as usize).min(sky.saturating_sub(2)));
                let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(sky - 1));
                let (ux, uy) = ((fx - x0 as f64) as f32, (fy - y0 as f64) as f32);
                self.n[p] += 1;
                let inv_n = 1.0 / self.n[p] as f32;
                for c in 0..3 {
                    let s = |xx: usize, yy: usize| lut[rgb[(yy * w + xx) * 3 + c] as usize];
                    let top = s(x0, y0) * (1.0 - ux) + s(x1, y0) * ux;
                    let bottom = s(x0, y1) * (1.0 - ux) + s(x1, y1) * ux;
                    let l = top * (1.0 - uy) + bottom * uy;
                    self.update(p * 3 + c, l, encode_linear(l), inv_n);
                }
            }
        }
        Some(left_out)
    }

    /// Average of the frames in linear light, extreme values rejected when
    /// they are more than [`SIGMA_REJECT`] σ from the others (from 3 frames
    /// covering the pixel). A pixel covered by no frame stays black.
    pub fn result(&self) -> Option<Vec<u8>> {
        if self.count == 0 {
            return None;
        }
        let lut = to_linear_lut();
        Some(
            (0..self.mean.len())
                .map(|i| {
                    let mean = self.mean[i];
                    let count = self.n[i / 3];
                    if count < 3 {
                        return encode_linear(mean);
                    }
                    let (mut n, mut sum) = (count as f32, count as f32 * mean);
                    let mut sumsq = self.m2[i] + n * mean * mean;
                    // Brightest value first (planes, satellites, cosmic rays),
                    // then the darkest one
                    let (hi, lo) = (self.max[i], self.min[i]);
                    let x = lut[hi as usize];
                    let (n1, s1, q1, sd) = without(n, sum, sumsq, x);
                    // σ floor: one code step, so that a flat stack does not
                    // reject a value one code away
                    let floor = x - lut[hi.saturating_sub(1) as usize];
                    if x - s1 / n1 > SIGMA_REJECT * sd.max(floor) {
                        (n, sum, sumsq) = (n1, s1, q1);
                    }
                    if n >= 3.0 && lo < hi {
                        let x = lut[lo as usize];
                        let (n1, s1, _, sd) = without(n, sum, sumsq, x);
                        let floor = lut[(lo as usize + 1).min(255)] - x;
                        if s1 / n1 - x > SIGMA_REJECT * sd.max(floor) {
                            (n, sum) = (n1, s1);
                        }
                    }
                    encode_linear(sum / n)
                })
                .collect(),
        )
    }

    pub fn reset(&mut self) {
        self.mean.iter_mut().for_each(|v| *v = 0.0);
        self.m2.iter_mut().for_each(|v| *v = 0.0);
        self.min.iter_mut().for_each(|v| *v = u8::MAX);
        self.max.iter_mut().for_each(|v| *v = 0);
        self.n.iter_mut().for_each(|v| *v = 0);
        self.count = 0;
    }
}

/// Deterministic pseudo-random generator for tests and benchmarks.
pub struct XorShift(u64);

impl XorShift {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }
    pub fn next_u32(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 32) as u32
    }
    /// Approximately normal value (mean 0, std 1): sum of 12 uniforms − 6.
    pub fn gaussian(&mut self) -> f64 {
        (0..12).map(|_| self.next_u32() as f64 / u32::MAX as f64).sum::<f64>() - 6.0
    }
}

/// Synthetic night sky used by tests and `aurion bench`: dark gradient,
/// green aurora band, a few stars (3×3 blobs), gaussian noise and hot pixels.
/// Returns `(clean, noisy)`.
pub fn synthetic_sky(width: u32, height: u32, noise_std: f64, hot_pixels: usize, seed: u64) -> (Vec<u8>, Vec<u8>) {
    let (w, h) = (width as usize, height as usize);
    let mut clean = vec![0u8; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 3;
            let base = 12.0 + 10.0 * y as f64 / h as f64;
            let band = (-(((y as f64 / h as f64) - 0.3) / 0.08).powi(2)).exp()
                * (0.6 + 0.4 * (x as f64 / 40.0).sin());
            clean[i] = (base + 15.0 * band) as u8;
            clean[i + 1] = (base + 110.0 * band) as u8;
            clean[i + 2] = (base + 25.0 * band) as u8;
        }
    }
    let mut rng = XorShift::new(seed);
    // Stars: 3×3 blobs
    for _ in 0..(w * h / 4000).max(3) {
        let sx = 2 + rng.next_u32() as usize % (w - 4);
        let sy = 2 + rng.next_u32() as usize % (h - 4);
        for dy in 0..3 {
            for dx in 0..3 {
                let v = if dx == 1 && dy == 1 { 240 } else { 170 };
                let i = ((sy + dy - 1) * w + sx + dx - 1) * 3;
                clean[i] = v;
                clean[i + 1] = v;
                clean[i + 2] = v;
            }
        }
    }
    let mut noisy: Vec<u8> = clean
        .iter()
        .map(|&v| (v as f64 + noise_std * rng.gaussian()).round().clamp(0.0, 255.0) as u8)
        .collect();
    for _ in 0..hot_pixels {
        let x = 2 + rng.next_u32() as usize % (w - 4);
        let y = 2 + rng.next_u32() as usize % (h - 4);
        let c = rng.next_u32() as usize % 3;
        noisy[(y * w + x) * 3 + c] = 255;
    }
    (clean, noisy)
}

/// Root mean square error between two images.
pub fn rmse(a: &[u8], b: &[u8]) -> f64 {
    let n = a.len().min(b.len()).max(1);
    (a.iter().zip(b).map(|(&x, &y)| (x as f64 - y as f64).powi(2)).sum::<f64>() / n as f64).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hot_pixels_removed_stars_kept() {
        let (clean, mut img) = synthetic_sky(200, 150, 0.0, 60, 7);
        let before = rmse(&clean, &img);
        let fixed = remove_hot_pixels(&mut img, 200, 150, 40);
        assert!(fixed >= 55, "only {} hot pixels fixed", fixed);
        assert!(rmse(&clean, &img) < before / 5.0);
        // Stars (3×3 blobs, value 240 in the centre) are untouched
        let stars_before = clean.iter().filter(|&&v| v == 240).count();
        let stars_after = img.iter().zip(&clean).filter(|(&a, &c)| c == 240 && a == 240).count();
        assert_eq!(stars_before, stars_after);
    }

    #[test]
    fn hot_pixel_edge_cases() {
        let mut tiny = vec![255u8; 4 * 4 * 3];
        assert_eq!(remove_hot_pixels(&mut tiny, 4, 4, 40), 0);
        let mut short = vec![0u8; 10];
        assert_eq!(remove_hot_pixels(&mut short, 100, 100, 40), 0);
        // Uniform bright image: nothing is "isolated"
        let mut flat = vec![200u8; 20 * 20 * 3];
        assert_eq!(remove_hot_pixels(&mut flat, 20, 20, 40), 0);
    }

    #[test]
    fn stacking_reduces_noise_by_sqrt_n() {
        let (w, h) = (160, 120);
        let (clean, _) = synthetic_sky(w, h, 0.0, 0, 3);
        let mut stack = Stacker::new(w, h);
        let mut single = 0.0;
        for seed in 0..8 {
            let noisy: Vec<u8> = {
                let mut rng = XorShift::new(100 + seed);
                clean.iter().map(|&v| (v as f64 + 8.0 * rng.gaussian()).round().clamp(0.0, 255.0) as u8).collect()
            };
            if seed == 0 {
                single = rmse(&clean, &noisy);
            }
            assert!(stack.add(&noisy));
        }
        let stacked = rmse(&clean, &stack.result().unwrap());
        // 8 frames, almost all kept: theory up to √8 ≈ 2.8× less noise
        assert!(stacked < single / 2.0, "single {:.2} → stacked {:.2}", single, stacked);
    }

    #[test]
    fn stacking_rejects_a_plane_on_one_frame() {
        let (w, h) = (50, 40);
        let base = vec![20u8; (w * h * 3) as usize];
        let mut stack = Stacker::new(w, h);
        for k in 0..5 {
            let mut f = base.clone();
            if k == 2 {
                // Plane / satellite trail across the image
                for x in 0..w as usize {
                    let i = (20 * w as usize + x) * 3;
                    f[i] = 255;
                    f[i + 1] = 255;
                    f[i + 2] = 255;
                }
            }
            stack.add(&f);
        }
        let out = stack.result().unwrap();
        assert!(out.iter().all(|&v| v == 20), "the trail must vanish");
    }

    #[test]
    fn stacker_guards() {
        let mut s = Stacker::new(4, 4);
        assert!(s.result().is_none());
        assert!(!s.add(&[0u8; 10]), "wrong size refused");
        assert!(s.add(&[100u8; 48]));
        assert!(s.add(&[102u8; 48]));
        assert_eq!(s.result().unwrap()[0], 101, "mean of the light of 100 and 102");
        s.reset();
        assert_eq!(s.count(), 0);
        assert!(s.result().is_none());
    }

    #[test]
    fn transfer_function_round_trips_every_code() {
        let lut = to_linear_lut();
        for v in 0..=255u8 {
            assert_eq!(encode_linear(lut[v as usize]), v, "code {}", v);
        }
        // A flat stack gives back exactly the input
        let mut s = Stacker::new(2, 2);
        for _ in 0..6 {
            s.add(&[37u8; 12]);
        }
        assert!(s.result().unwrap().iter().all(|&v| v == 37));
    }

    #[test]
    fn averages_light_not_gamma_codes() {
        // Half the frames black, half at code 255: half the light is code
        // 188 in sRGB, not 128 (the mean of the codes)
        let mut s = Stacker::new(1, 1);
        s.add(&[0, 0, 0]);
        s.add(&[255, 255, 255]);
        assert_eq!(s.result().unwrap(), vec![188, 188, 188]);
    }

    #[test]
    fn sigma_rejection_keeps_every_good_frame() {
        // 4 frames with mild noise: min/max rejection would keep only 2,
        // sigma rejection keeps all 4 (exact mean of their light)
        let frames: [u8; 4] = [60, 62, 61, 63];
        let mut s = Stacker::new(1, 1);
        for v in frames {
            s.add(&[v; 3]);
        }
        let lut = to_linear_lut();
        let mean = frames.iter().map(|&v| lut[v as usize]).sum::<f32>() / 4.0;
        assert_eq!(s.result().unwrap()[0], encode_linear(mean));
        // A dark outlier (cloud edge, failed frame) on one frame of 6 goes
        let mut s = Stacker::new(1, 1);
        for v in [80u8, 81, 79, 80, 3, 80] {
            s.add(&[v; 3]);
        }
        assert_eq!(s.result().unwrap()[0], 80);
        // The outlier comes first: the result does not depend on the order
        let mut s = Stacker::new(1, 1);
        for v in [255u8, 20, 20, 20, 20] {
            s.add(&[v; 3]);
        }
        assert_eq!(s.result().unwrap()[0], 20);
    }

    #[test]
    fn full_chain_improves_quality() {
        // Hot pixels + noise, 4 frames: hot pixel removal then stacking
        let (w, h) = (160, 120);
        let (clean, _) = synthetic_sky(w, h, 0.0, 0, 11);
        let mut stack = Stacker::new(w, h);
        let mut first = None;
        for seed in 0..4 {
            // Same scene and same hot pixels (sensor defect), new noise per frame
            let (_, mut noisy) = synthetic_sky(w, h, 0.0, 40, 11);
            let mut rng = XorShift::new(500 + seed);
            for v in noisy.iter_mut() {
                if *v < 255 {
                    *v = (*v as f64 + 6.0 * rng.gaussian()).round().clamp(0.0, 254.0) as u8;
                }
            }
            if first.is_none() {
                first = Some(rmse(&clean, &noisy));
            }
            remove_hot_pixels(&mut noisy, w, h, 40);
            stack.add(&noisy);
        }
        let out = rmse(&clean, &stack.result().unwrap());
        assert!(out < first.unwrap() * 0.6, "{:.2} → {:.2}", first.unwrap(), out);
    }
}
