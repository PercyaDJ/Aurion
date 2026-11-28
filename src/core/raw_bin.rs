//! Binning of the raw (CFA) image of a DNG, without demosaicing.
//!
//! Each 2×2 Bayer cell gives one red, two green and one blue sample. The
//! colour planes are binned separately: an output pixel of factor `f` sums
//! the `f × f` cells of a block, i.e. `f²` samples of each colour (the two
//! greens are averaged, so the three channels have the same scale). Values
//! are linear, black level subtracted (per position of the Bayer pattern),
//! 16-bit. Output size: `(width / 2 / f) × (height / 2 / f)`; incomplete
//! blocks at the right and bottom edges are dropped.
//!
//! With `f = 2` (the "2×2 binning" of the audit, IQ-6 and annex C 4.3) each
//! output value sums 4 samples of the same colour: the signal per output
//! pixel is multiplied by 4, the shot noise by 2 (it adds in quadrature),
//! and the read noise, added once per sample, also by 2, so that it weighs
//! less against the signal than on a single sample. How much it helps on
//! the faint aurorae of the HQ camera is to be measured on the hardware.
//! A 12-bit sample minus the black level fits 4096 values, so even `f = 8`
//! (64 samples, 262 080) is clamped only on saturated pixels.
//!
//! The DNG is read by bands of rows ([`crate::core::dng::DngReader`]): memory
//! is one band (`2 × f` rows) plus the output.
//!
//! Used for the gallery preview of a DNG that has no capture-time thumbnail
//! ([`preview_rgb`]). The aurora detection keeps using the JPEG thumbnail
//! (its move to the raw data is a later lot).

use std::io::{Read, Seek};

use crate::core::dng::{DngError, DngReader};

/// Binned colour planes.
#[derive(Debug, Clone, PartialEq)]
pub struct BinnedRaw {
    pub width: u32,
    pub height: u32,
    /// Binning factor (cells per side of a block).
    pub factor: u32,
    /// Red, green (mean of the two greens) and blue, row-major, top row
    /// first, each the sum of `factor²` samples minus their black level.
    pub r: Vec<u16>,
    pub g: Vec<u16>,
    pub b: Vec<u16>,
}

/// Bin the CFA image read by `reader` by `factor` (1 = one pixel per Bayer
/// cell). Refuses patterns that are not a 2×2 R, G, G, B arrangement.
pub fn bin_raw<R: Read + Seek>(reader: &mut DngReader<R>, factor: u32) -> Result<BinnedRaw, DngError> {
    let info = reader.info().clone();
    let unsupported = |m: &str| DngError::Unsupported(m.to_string());
    if factor == 0 || factor > 64 {
        return Err(unsupported("facteur de binning hors de 1 à 64"));
    }
    if info.cfa_repeat != (2, 2) || info.cfa_pattern.len() != 4 {
        return Err(unsupported("matrice de Bayer autre que 2x2"));
    }
    let mut sorted = info.cfa_pattern.clone();
    sorted.sort_unstable();
    if sorted != [0, 1, 1, 2] {
        return Err(unsupported("matrice de Bayer autre que R, G, G, B"));
    }
    // Black level of each of the 4 positions of the cell
    let (bh, bw) = (info.black_level_repeat.0.max(1) as usize, info.black_level_repeat.1.max(1) as usize);
    let black: Vec<f64> = (0..4)
        .map(|k| {
            let (y, x) = (k / 2, k % 2);
            info.black_level.get((y % bh) * bw + x % bw).or(info.black_level.first()).copied().unwrap_or(0.0)
        })
        .collect();
    let block = 2 * factor;
    let (ow, oh) = (info.width / block, info.height / block);
    let (w, f) = (info.width as usize, factor as usize);
    let mut sums = vec![[0f64; 3]; ow as usize];
    let mut band = vec![0u16; block as usize * w];
    let mut out = BinnedRaw { width: ow, height: oh, factor, r: Vec::new(), g: Vec::new(), b: Vec::new() };
    for by in 0..oh {
        reader.read_rows(by * block, block, &mut band)?;
        sums.iter_mut().for_each(|s| *s = [0.0; 3]);
        for row in 0..block as usize {
            let line = &band[row * w..(row + 1) * w];
            for (bx, s) in sums.iter_mut().enumerate() {
                let x0 = bx * 2 * f;
                for (x, &v) in line[x0..x0 + 2 * f].iter().enumerate() {
                    let k = (row % 2) * 2 + x % 2;
                    let c = info.cfa_pattern[k] as usize;
                    s[c] += (v as f64 - black[k]).max(0.0);
                }
            }
        }
        for s in &sums {
            // Each block holds f² red, 2f² green and f² blue samples
            let clamp = |v: f64| v.round().clamp(0.0, u16::MAX as f64) as u16;
            out.r.push(clamp(s[0]));
            out.g.push(clamp(s[1] / 2.0));
            out.b.push(clamp(s[2]));
        }
    }
    Ok(out)
}

/// Smallest factor that brings the binned image within `max_w × max_h`.
pub fn factor_for(width: u32, height: u32, max_w: u32, max_h: u32) -> u32 {
    let need = |side: u32, max: u32| (side / 2).div_ceil(max.max(1));
    need(width, max_w).max(need(height, max_h)).max(1)
}

/// 8-bit sRGB preview of a binned image: linear values scaled so that the
/// 99.5th percentile of all channels becomes white, then the sRGB transfer
/// function. No white balance (raw sensor colours) and no colour matrix: a
/// preview, not a development.
pub fn preview_rgb(img: &BinnedRaw) -> Vec<u8> {
    let mut all: Vec<u16> = img.r.iter().chain(&img.g).chain(&img.b).copied().collect();
    if all.is_empty() {
        return Vec::new();
    }
    let k = ((all.len() as f64 * 0.995) as usize).min(all.len() - 1);
    let white = (*all.select_nth_unstable(k).1).max(1) as f32;
    let encode = |v: u16| crate::core::denoise::encode_linear(v as f32 / white);
    let mut rgb = Vec::with_capacity(img.r.len() * 3);
    for i in 0..img.r.len() {
        rgb.extend_from_slice(&[encode(img.r[i]), encode(img.g[i]), encode(img.b[i])]);
    }
    rgb
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::dng::{test_dng_bytes, TestDngOptions};
    use std::io::Cursor;

    /// CFA image of a flat scene of linear levels (r, g, b) above `black`,
    /// in the pattern `pat` (0 = R, 1 = G, 2 = B).
    fn flat(w: u32, h: u32, pat: [u8; 4], level: [u16; 3], black: u16) -> Vec<u16> {
        (0..h)
            .flat_map(|y| (0..w).map(move |x| black + level[pat[((y % 2) * 2 + x % 2) as usize] as usize]))
            .collect()
    }

    #[test]
    fn bins_each_colour_with_black_subtracted() {
        let (w, h) = (64, 48);
        for (pat, big_endian) in [([0, 1, 1, 2], false), ([2, 1, 1, 0], true), ([1, 0, 2, 1], false)] {
            let px = flat(w, h, pat, [100, 300, 50], 256);
            let opt = TestDngOptions { cfa_pattern: pat, rows_per_strip: 7, big_endian, ..Default::default() };
            let mut r = DngReader::new(Cursor::new(test_dng_bytes(w, h, &px, &opt))).unwrap();
            let b = bin_raw(&mut r, 2).unwrap();
            assert_eq!((b.width, b.height), (16, 12));
            // 4 samples per colour: signal × 4
            assert!(b.r.iter().all(|&v| v == 400), "{:?}", &b.r[..4]);
            assert!(b.g.iter().all(|&v| v == 1200));
            assert!(b.b.iter().all(|&v| v == 200));
            let one = bin_raw(&mut r, 1).unwrap();
            assert_eq!((one.width, one.height, one.r[0], one.g[0], one.b[0]), (32, 24, 100, 300, 50));
        }
    }

    #[test]
    fn binning_by_2_halves_the_relative_noise() {
        // Gaussian read noise of 8 ADU on a faint signal of 20 ADU: the
        // signal-to-noise ratio per output pixel doubles with 4 samples
        let (w, h) = (256, 256);
        let mut rng = crate::core::denoise::XorShift::new(4);
        let px: Vec<u16> = (0..w * h).map(|_| (256.0 + 20.0 + 8.0 * rng.gaussian()).round() as u16).collect();
        let bytes = test_dng_bytes(w, h, &px, &TestDngOptions::default());
        let snr = |v: &[u16]| {
            let m = v.iter().map(|&x| x as f64).sum::<f64>() / v.len() as f64;
            let sd = (v.iter().map(|&x| (x as f64 - m).powi(2)).sum::<f64>() / v.len() as f64).sqrt();
            m / sd
        };
        let mut r = DngReader::new(Cursor::new(bytes)).unwrap();
        let s1 = snr(&bin_raw(&mut r, 1).unwrap().r);
        let s2 = snr(&bin_raw(&mut r, 2).unwrap().r);
        assert!((s2 / s1 - 2.0).abs() < 0.25, "SNR {:.2} → {:.2}", s1, s2);
    }

    #[test]
    fn refuses_bad_factors_and_patterns() {
        let px = vec![300u16; 32 * 32];
        let mut r = DngReader::new(Cursor::new(test_dng_bytes(32, 32, &px, &TestDngOptions::default()))).unwrap();
        assert!(matches!(bin_raw(&mut r, 0), Err(DngError::Unsupported(_))));
        let opt = TestDngOptions { cfa_pattern: [0, 0, 1, 2], ..Default::default() };
        let mut r = DngReader::new(Cursor::new(test_dng_bytes(32, 32, &px, &opt))).unwrap();
        assert!(matches!(bin_raw(&mut r, 2), Err(DngError::Unsupported(_))));
    }

    #[test]
    fn preview_size_and_stretch() {
        assert_eq!(factor_for(4056, 3040, 320, 240), 7);
        assert_eq!(factor_for(64, 48, 320, 240), 1);
        let px = flat(64, 48, [0, 1, 1, 2], [100, 300, 50], 256);
        let mut r = DngReader::new(Cursor::new(test_dng_bytes(64, 48, &px, &TestDngOptions::default()))).unwrap();
        let rgb = preview_rgb(&bin_raw(&mut r, 1).unwrap());
        assert_eq!(rgb.len(), 32 * 24 * 3);
        // Green is the brightest plane: white
        assert_eq!(&rgb[..3], &[crate::core::denoise::encode_linear(1.0 / 3.0), 255, crate::core::denoise::encode_linear(1.0 / 6.0)]);
    }
}
