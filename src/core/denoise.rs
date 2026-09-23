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
//! 2. **Stacking with rejection** ([`Stacker`]). Averaging N frames divides
//!    random noise by about √N. For N ≥ 4 the highest and lowest value of
//!    each pixel are discarded before averaging, which removes planes,
//!    satellites and cosmic-ray hits appearing on a single frame. Memory:
//!    4 bytes per sub-pixel (12 MP → 144 MB), whatever N.
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

/// Accumulates frames of identical size and produces a denoised average.
pub struct Stacker {
    width: u32,
    height: u32,
    sum: Vec<u16>,
    min: Vec<u8>,
    max: Vec<u8>,
    count: u32,
}

/// Maximum number of frames (keeps the u16 sum from overflowing: 255 × 255).
pub const MAX_STACK: u32 = 255;

impl Stacker {
    pub fn new(width: u32, height: u32) -> Self {
        let n = (width * height * 3) as usize;
        Self { width, height, sum: vec![0; n], min: vec![u8::MAX; n], max: vec![0; n], count: 0 }
    }

    pub fn count(&self) -> u32 {
        self.count
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Add a frame. Returns false (and ignores it) if the size differs or the
    /// stack is full.
    pub fn add(&mut self, rgb: &[u8]) -> bool {
        if rgb.len() != self.sum.len() || self.count >= MAX_STACK {
            return false;
        }
        for (i, &v) in rgb.iter().enumerate() {
            self.sum[i] += v as u16;
            if v < self.min[i] {
                self.min[i] = v;
            }
            if v > self.max[i] {
                self.max[i] = v;
            }
        }
        self.count += 1;
        true
    }

    /// Average of the frames (min/max rejected when there are at least 4).
    pub fn result(&self) -> Option<Vec<u8>> {
        if self.count == 0 {
            return None;
        }
        let reject = self.count >= 4;
        let n = if reject { self.count - 2 } else { self.count };
        Some(
            (0..self.sum.len())
                .map(|i| {
                    let mut s = self.sum[i] as u32;
                    if reject {
                        s -= self.min[i] as u32 + self.max[i] as u32;
                    }
                    ((s + n / 2) / n) as u8
                })
                .collect(),
        )
    }

    pub fn reset(&mut self) {
        self.sum.iter_mut().for_each(|v| *v = 0);
        self.min.iter_mut().for_each(|v| *v = u8::MAX);
        self.max.iter_mut().for_each(|v| *v = 0);
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
        // 8 frames, 6 kept after rejection: theory √6 ≈ 2.4× less noise
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
        assert!(s.add(&[101u8; 48]));
        assert_eq!(s.result().unwrap()[0], 101, "mean of 100 and 101 rounded");
        s.reset();
        assert_eq!(s.count(), 0);
        assert!(s.result().is_none());
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
